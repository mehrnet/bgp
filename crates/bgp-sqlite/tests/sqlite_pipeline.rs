use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use bgp_compact::sortkey::pad_sort_key_str;
use bgp_sqlite::build::build_sqlite_dataset;
use bgp_sqlite::prefix_index::build_prefix_index;
use bgp_sqlite::range_summaries::build_range_summaries;
use bgp_sqlite::validate::validate_sqlite_db;
use rusqlite::Connection;

fn temp_dir(tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    std::env::temp_dir().join(format!("sqlite-test-{tag}-{nanos}"))
}

#[test]
fn builds_indexes_summarizes_and_validates_sqlite_db() -> Result<(), Box<dyn std::error::Error>> {
    let dir = temp_dir("ok");
    let input = dir.join("final_data");
    fs::create_dir_all(&input)?;

    let (s, e) = (pad_sort_key_str("1.1.1.0"), pad_sort_key_str("1.1.1.255"));
    fs::write(
        input.join("allocations.csv"),
        format!("{s},{e},4,APNIC,AU,APNIC-LABS,ALLOCATED,20110811,,,APNIC,,,abuse@example.test,\n"),
    )?;
    fs::write(
        input.join("routes.csv"),
        format!("{s},{e},4,1.1.1.0/24,AS13335,APNIC,APNIC,,,abuse@example.test,\n"),
    )?;
    fs::write(
        input.join("geolocations.csv"),
        format!("{s},{e},4,AU,QLD,Brisbane\n"),
    )?;
    fs::write(
        input.join("autnums.csv"),
        "AS13335,APNIC,US,CLOUDFLARENET,ORG-CLOUD,ASSIGNED,,,APNIC,,abuse@example.test,\n",
    )?;

    let db_path = dir.join("mehrnet_bgp.db");
    build_sqlite_dataset(
        &input,
        &db_path,
        "db-sql-test",
        "2026-11-18T00:00:00Z",
        "deadbeef",
    )?;
    build_prefix_index(&db_path)?;
    let sum_rows = build_range_summaries(&db_path)?;
    assert_eq!(sum_rows, 3);
    validate_sqlite_db(&db_path, true)?;

    let conn = Connection::open(&db_path)?;
    let pfx_rows: i64 = conn.query_row("SELECT count(*) FROM lookup_prefixes", [], |r| r.get(0))?;
    assert_eq!(pfx_rows, 3);
    let tag: String =
        conn.query_row("SELECT release_tag FROM bgp_api_dataset", [], |r| r.get(0))?;
    assert_eq!(tag, "db-sql-test");
    let _ = fs::remove_dir_all(&dir);
    Ok(())
}
