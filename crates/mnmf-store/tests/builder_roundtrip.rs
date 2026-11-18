use std::fs;
use std::net::IpAddr;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use bgp_compact::sortkey::pad_sort_key_str;
use mnmf_store::Db;
use mnmf_store::builder::{BuildOptions, build_mmap_dataset};
use mnmf_store::types::TableKind;

fn temp_dir(tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    std::env::temp_dir().join(format!("mnmf-build-test-{tag}-{nanos}"))
}

fn write_fixture_csvs(input: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    fs::create_dir_all(input)?;
    let mut allocs = format!(
        "{},{},4,APNIC,AU,APNIC-LABS,ALLOCATED,20110811,,,APNIC,,,abuse@example.test,\n",
        pad_sort_key_str("1.1.1.0"),
        pad_sort_key_str("1.1.1.255")
    )
    .into_bytes();
    allocs.extend_from_slice(
        format!(
            "{},{},4,ripe,DE,WILHELM,ALLOCATED,20110811,,,ripe,,Org",
            pad_sort_key_str("2.2.2.0"),
            pad_sort_key_str("2.2.2.255")
        )
        .as_bytes(),
    );
    allocs.extend_from_slice(b"\xdf,,Heidbergstra\xdfe\n");
    fs::write(input.join("allocations.csv"), allocs)?;
    let routes = format!(
        "{},{},4,1.1.1.0/24,AS13335,APNIC,APNIC,,,abuse@example.test,\n",
        pad_sort_key_str("1.1.1.0"),
        pad_sort_key_str("1.1.1.255")
    );
    fs::write(input.join("routes.csv"), routes)?;
    let geos = format!(
        "{},{},4,AU,Queensland,South Brisbane\n",
        pad_sort_key_str("1.1.1.0"),
        pad_sort_key_str("1.1.1.255")
    );
    fs::write(input.join("geolocations.csv"), geos)?;
    let fb_geos = format!(
        "{},{},4,DE,Berlin,Berlin,52.52,13.405\n{},{},4,DE,Hamburg,Hamburg,53.5511,9.9937\n",
        pad_sort_key_str("1.1.1.128"),
        pad_sort_key_str("1.1.1.255"),
        pad_sort_key_str("2.2.2.0"),
        pad_sort_key_str("2.2.2.255")
    );
    fs::write(input.join("geolocations_fallback.csv"), fb_geos)?;
    fs::write(
        input.join("autnums.csv"),
        "AS13335,APNIC,US,CLOUDFLARENET,ORG-CLOUD,ASSIGNED,,,APNIC,,abuse@example.test,\n",
    )?;
    Ok(())
}

#[test]
fn builds_mmap_dataset_readable_by_store() -> Result<(), Box<dyn std::error::Error>> {
    let dir = temp_dir("ok");
    let input = dir.join("csv");
    write_fixture_csvs(&input)?;

    let out = dir.join("test.mmap");
    let stats = build_mmap_dataset(&BuildOptions {
        input_dir: input,
        output_path: out.clone(),
        release_tag: "db-rs-test".into(),
        built_at: "2026-11-18T00:00:00Z".into(),
        source_commit: "abc123".into(),
    })?;
    assert_eq!(stats.allocations, 2);
    assert_eq!(stats.routes, 1);

    let db = Db::open(&out)?;
    assert_eq!(db.metadata().release_tag, "db-rs-test");
    let ip1: IpAddr = "1.1.1.1".parse()?;
    let alloc_id = db.lookup_allocation_lpm(ip1)?;
    assert_ne!(alloc_id, 0);
    assert_eq!(db.allocation(alloc_id, false)?.name, "APNIC-LABS");
    assert_eq!(db.lookup_route_lpm(ip1)?.len(), 1);
    assert_eq!(
        db.geofeed(db.lookup_geofeed_lpm(ip1)?)?.city,
        "South Brisbane"
    );
    verify_geo_fallback(&db)?;
    assert_eq!(
        db.autnum(13335, false)?.map(|a| a.name),
        Some("CLOUDFLARENET".into())
    );
    assert_eq!(
        db.matching_prefix_ids(TableKind::Allocations, "2.2.2.1".parse()?, 8)?
            .len(),
        1
    );
    let sum = db
        .summary("1.0.0.0".parse()?, 8)?
        .ok_or("missing /8 summary")?;
    assert_eq!(sum.allocation_records, 1);
    assert_eq!(sum.routes, 1);
    let _ = fs::remove_dir_all(&dir);
    Ok(())
}

fn verify_geo_fallback(db: &Db) -> Result<(), Box<dyn std::error::Error>> {
    let rfc_wins = db.geofeed(db.lookup_geofeed_lpm("1.1.1.200".parse()?)?)?;
    assert_eq!(rfc_wins.city, "South Brisbane");
    assert!(!rfc_wins.is_fallback);
    assert_eq!(rfc_wins.latitude, None);

    let fb = db.geofeed(db.lookup_geofeed_lpm("2.2.2.42".parse()?)?)?;
    assert_eq!(fb.city, "Hamburg");
    assert!(fb.is_fallback);
    assert_eq!(fb.latitude, Some(53.5511));
    assert_eq!(fb.longitude, Some(9.9937));
    Ok(())
}
