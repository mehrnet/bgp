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
            "{},{},4,AU,QLD,Brisbane\n",
            sort_v4("1.1.1.0")?,
            sort_v4("1.1.1.255")?
        ),
    )?;
    fs::write(
        input.join("geolocations_fallback.csv"),
        format!(
            "{},{},4,DE,Berlin,Berlin,52.52,13.405\n{},{},4,IR,Tehran,Tehran (District 7),35.7425,51.4402\n",
            sort_v4("1.1.1.128")?,
            sort_v4("1.1.1.255")?,
            sort_v4("2.2.2.0")?,
            sort_v4("2.2.2.255")?
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
        let ip_rfc = parse_runtime_ip("1.1.1.200").ok_or("bad ip")?;
        let r_rfc = lookup_ip(&db, &ip_rfc, full).map_err(|e| format!("{e:?}"))?;
        assert_eq!(r_rfc.location.country_code.as_deref(), Some("AU"));
        assert_eq!(r_rfc.location.city.as_deref(), Some("Brisbane"));
        assert_eq!(r_rfc.location.latitude, None);
        assert_eq!(r_rfc.location.longitude, None);

        let ip_fb = parse_runtime_ip("2.2.2.10").ok_or("bad ip")?;
        let r_fb = lookup_ip(&db, &ip_fb, full).map_err(|e| format!("{e:?}"))?;
        assert_eq!(r_fb.location.country_code.as_deref(), Some("IR"));
        assert_eq!(r_fb.location.region.as_deref(), Some("Tehran"));
        assert_eq!(r_fb.location.city.as_deref(), Some("Tehran (District 7)"));
        assert_eq!(r_fb.location.latitude, Some(35.7425));
        assert_eq!(r_fb.location.longitude, Some(51.4402));
    }

    let _ = fs::remove_dir_all(&dir);
    Ok(())
}
