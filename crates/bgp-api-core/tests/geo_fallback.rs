use std::fs;
use std::net::Ipv4Addr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use bgp_api_core::ipkey::parse_runtime_ip;
use bgp_api_core::service::lookup_ip;
use mnmf_store::Db;
use mnmf_store::builder::{BuildOptions, build_mmap_dataset};

fn temp_dir(tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    std::env::temp_dir().join(format!("core-geo-test-{tag}-{nanos}"))
}

fn sort_v4(ip: &str) -> Result<String, std::net::AddrParseError> {
    let addr: Ipv4Addr = ip.parse()?;
    let val = (0xffffu128 << 32) | u128::from(u32::from(addr));
    Ok(format!("{val:039}"))
}

fn write_geo_fixtures(input: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    fs::create_dir_all(input)?;
    fs::write(
        input.join("allocations.csv"),
        format!(
            "{},{},4,APNIC,AU,APNIC-LABS,,,,,,,,,\n",
            sort_v4("1.1.1.0")?,
            sort_v4("1.1.1.255")?
        ),
    )?;
    fs::write(input.join("routes.csv"), "")?;
    fs::write(input.join("autnums.csv"), "")?;
    fs::write(
        input.join("geolocations.csv"),
        format!(
            "{},{},4,AU,QLD,Brisbane\n{},{},4,DE,DE-HE,Eschborn\n",
            sort_v4("1.1.1.0")?,
            sort_v4("1.1.1.255")?,
            sort_v4("3.3.3.0")?,
            sort_v4("3.3.3.255")?
        ),
    )?;
    fs::write(
        input.join("geolocations_fallback.csv"),
        format!(
            "{},{},4,DE,Berlin,Berlin,52.52,13.405\n{},{},4,IR,Tehran,Tehran (District 7),35.7425,51.4402\n{},{},4,DE,Hesse,eschborn,50.1501,8.58033\n",
            sort_v4("1.1.1.128")?,
            sort_v4("1.1.1.255")?,
            sort_v4("2.2.2.0")?,
            sort_v4("2.2.2.255")?,
            sort_v4("2.2.3.0")?,
            sort_v4("2.2.3.255")?
        ),
    )?;
    Ok(())
}

#[test]
fn lookup_ip_enforces_rfc8805_over_fallback_and_exposes_coords()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = temp_dir("ok");
    let input = dir.join("csv");
    write_geo_fixtures(&input)?;

    let out = dir.join("test.mmap");
    build_mmap_dataset(&BuildOptions {
        input_dir: input,
        output_path: out.clone(),
        release_tag: "db-rs-geo".into(),
        built_at: "2026-11-18T00:00:00Z".into(),
        source_commit: "deadbeef".into(),
    })?;
    let db = Arc::new(Db::open(&out)?);

    for full in [false, true] {
        assert_geo(&db, "1.1.1.200", full, ("AU", None, "Brisbane"), None)?;
        assert_geo(
            &db,
            "2.2.2.10",
            full,
            ("IR", Some("Tehran"), "Tehran (District 7)"),
            Some((35.7425, 51.4402)),
        )?;
        assert_geo(
            &db,
            "3.3.3.10",
            full,
            ("DE", Some("DE-HE"), "Eschborn"),
            Some((50.1501, 8.58033)),
        )?;
    }

    let _ = fs::remove_dir_all(&dir);
    Ok(())
}

fn assert_geo(
    db: &Arc<Db>,
    ip: &str,
    full: bool,
    expected: (&str, Option<&str>, &str),
    coords: Option<(f64, f64)>,
) -> Result<(), Box<dyn std::error::Error>> {
    let parsed = parse_runtime_ip(ip).ok_or("bad ip")?;
    let resp = lookup_ip(db, &parsed, full).map_err(|e| format!("{e:?}"))?;
    assert_eq!(resp.location.country_code.as_deref(), Some(expected.0));
    if expected.1.is_some() {
        assert_eq!(resp.location.region.as_deref(), expected.1);
    }
    assert_eq!(resp.location.city.as_deref(), Some(expected.2));
    assert_eq!(resp.location.latitude, coords.map(|c| c.0));
    assert_eq!(resp.location.longitude, coords.map(|c| c.1));
    Ok(())
}
