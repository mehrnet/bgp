use std::fs::{self, File};
use std::io::Write;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use bgp_tools::rir::parse_rir_dir;
use flate2::Compression;
use flate2::write::GzEncoder;

fn temp_dir(tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    std::env::temp_dir().join(format!("tools-test-{tag}-{nanos}"))
}

#[test]
fn parse_rir_handles_rpsl_gzip_latin1_iana_and_delegated() -> Result<(), Box<dyn std::error::Error>>
{
    let dir = temp_dir("rir");
    let raw = dir.join("raw_data");
    fs::create_dir_all(&raw)?;

    let mut rpsl =
        b"inetnum: 0.0.0.0 - 255.255.255.255\nnetname: IANA-BLK\ncountry: EU\n\n".to_vec();
    rpsl.extend_from_slice(
        b"inetnum: 2.2.2.0 - 2.2.2.255\nnetname: WILHELM\ncountry: de\norg: Org\xdf\nremarks: geofeed https://geo.example.test/feed.csv\n\n",
    );
    rpsl.extend_from_slice(b"route: 2.2.2.0/24\norigin: AS64500\nsource: RIPE # comment\n\n");
    rpsl.extend_from_slice(
        b"aut-num: AS64500\nas-name: EXAMPLE\ncountry: DE\nstatus: ASSIGNED\n\n",
    );

    let mut gz = GzEncoder::new(File::create(raw.join("ripe.db.gz"))?, Compression::fast());
    gz.write_all(&rpsl)?;
    gz.finish()?;

    fs::write(
        raw.join("delegated-arin-extended-latest"),
        "arin|US|ipv4|1.1.1.0|256|20110811|allocated\n",
    )?;

    parse_rir_dir(&raw, &dir, "ripe")?;

    let allocs = fs::read_to_string(dir.join("allocations_ripe.csv"))?;
    assert!(!allocs.contains("IANA-BLK"));
    assert!(allocs.contains("WILHELM"));
    assert!(allocs.contains("20110811"));

    let routes = fs::read_to_string(dir.join("routes_ripe.csv"))?;
    assert!(routes.contains("2.2.2.0/24,AS64500"));

    let autnums = fs::read_to_string(dir.join("autnums_ripe.csv"))?;
    assert!(autnums.contains("AS64500"));

    let geos = fs::read_to_string(dir.join("geofeeds_ripe.txt"))?;
    assert_eq!(geos.trim(), "https://geo.example.test/feed.csv");

    let _ = fs::remove_dir_all(&dir);
    Ok(())
}
