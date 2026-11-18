use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use bgp_compact::city_coords::{CityCoordIndex, parse_coord_str};

use rusqlite::types::Value;
use rusqlite::{Connection, params};

const BATCH_SIZE: usize = 10_000;

struct TableSpec<'a> {
    name: &'a str,
    file: &'a str,
    fields: usize,
    required: &'a [usize],
    report_every: usize,
    create_sql: &'a str,
    insert_sql: &'a str,
}

const SPECS: &[TableSpec<'static>] = &[
    TableSpec {
        name: "allocations",
        file: "allocations.csv",
        fields: 15,
        required: &[0, 1, 2],
        report_every: 1_000_000,
        create_sql: "CREATE TABLE allocations (
            start_ip_sort TEXT NOT NULL, end_ip_sort TEXT NOT NULL, ip_version INTEGER NOT NULL,
            registry TEXT, country TEXT, netname TEXT, status TEXT, allocation_date TEXT,
            created TEXT, last_modified TEXT, source TEXT, mnt_by TEXT, org TEXT,
            abuse_contact TEXT, descr TEXT);",
        insert_sql: "INSERT INTO allocations (start_ip_sort, end_ip_sort, ip_version, registry, country, netname, status, allocation_date, created, last_modified, source, mnt_by, org, abuse_contact, descr) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)",
    },
    TableSpec {
        name: "routes",
        file: "routes.csv",
        fields: 11,
        required: &[0, 1, 2, 3, 4],
        report_every: 500_000,
        create_sql: "CREATE TABLE routes (
            start_ip_sort TEXT NOT NULL, end_ip_sort TEXT NOT NULL, ip_version INTEGER NOT NULL,
            cidr TEXT NOT NULL, asn TEXT NOT NULL, registry TEXT, source TEXT, mnt_by TEXT,
            org TEXT, abuse_contact TEXT, descr TEXT);",
        insert_sql: "INSERT INTO routes (start_ip_sort, end_ip_sort, ip_version, cidr, asn, registry, source, mnt_by, org, abuse_contact, descr) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
    },
    TableSpec {
        name: "autnums",
        file: "autnums.csv",
        fields: 12,
        required: &[0],
        report_every: 100_000,
        create_sql: "CREATE TABLE autnums (
            asn TEXT NOT NULL, registry TEXT, country TEXT, as_name TEXT, org TEXT,
            status TEXT, created TEXT, last_modified TEXT, source TEXT, mnt_by TEXT,
            abuse_contact TEXT, descr TEXT);",
        insert_sql: "INSERT INTO autnums (asn, registry, country, as_name, org, status, created, last_modified, source, mnt_by, abuse_contact, descr) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
    },
    TableSpec {
        name: "geolocations",
        file: "geolocations.csv",
        fields: 8,
        required: &[0, 1, 2],
        report_every: 500_000,
        create_sql: "CREATE TABLE geolocations (
            start_ip_sort TEXT NOT NULL, end_ip_sort TEXT NOT NULL, ip_version INTEGER NOT NULL,
            country TEXT, region TEXT, city TEXT,
            latitude REAL, longitude REAL, is_fallback INTEGER NOT NULL DEFAULT 0);",
        insert_sql: "INSERT INTO geolocations (start_ip_sort, end_ip_sort, ip_version, country, region, city, latitude, longitude, is_fallback) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
    },
];

pub fn build_sqlite_dataset(
    input_dir: &Path,
    output_path: &Path,
    release_tag: &str,
    built_at: &str,
    source_commit: &str,
) -> Result<(), String> {
    if output_path.exists() {
        std::fs::remove_file(output_path)
            .map_err(|e| format!("remove {}: {e}", output_path.display()))?;
    }
    let mut conn = Connection::open(output_path).map_err(|e| format!("open sqlite: {e}"))?;
    conn.execute_batch(
        "PRAGMA journal_mode = OFF;
         PRAGMA synchronous = OFF;
         PRAGMA temp_store = MEMORY;
         PRAGMA locking_mode = EXCLUSIVE;
         PRAGMA cache_size = -524288;",
    )
    .map_err(|e| format!("pragmas: {e}"))?;
    create_metadata(&conn, release_tag, built_at, source_commit)?;
    for spec in SPECS {
        conn.execute_batch(spec.create_sql)
            .map_err(|e| format!("create {}: {e}", spec.name))?;
        let is_geo = spec.name == "geolocations";
        let fb = input_dir.join("geolocations_fallback.csv");
        let city_coords = is_geo
            .then(|| CityCoordIndex::from_fallback_csv_if_exists(&fb))
            .transpose()?;
        let mut count = import_csv(
            &mut conn,
            &input_dir.join(spec.file),
            spec,
            is_geo.then_some(0),
            city_coords.as_ref(),
        )?;
        if is_geo && fb.is_file() {
            count += import_csv(&mut conn, &fb, spec, Some(1), None)?;
        }
        println!("{}: {} rows", spec.name, count);
    }
    conn.execute_batch("ANALYZE; PRAGMA optimize;")
        .map_err(|e| format!("optimize: {e}"))
}

fn create_metadata(conn: &Connection, tag: &str, built: &str, commit: &str) -> Result<(), String> {
    conn.execute_batch(
        "CREATE TABLE dataset_metadata (key TEXT PRIMARY KEY, value TEXT NOT NULL);
         CREATE TABLE bgp_api_dataset (
            singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
            release_tag TEXT NOT NULL, built_at TEXT NOT NULL,
            activated_at TEXT, source_commit TEXT);",
    )
    .map_err(|e| format!("create metadata tables: {e}"))?;
    for (k, v) in [
        ("release_tag", tag),
        ("built_at", built),
        ("source_commit", commit),
    ] {
        conn.execute(
            "INSERT INTO dataset_metadata (key, value) VALUES (?1, ?2)",
            params![k, v],
        )
        .map_err(|e| format!("insert dataset_metadata: {e}"))?;
    }
    conn.execute(
        "INSERT INTO bgp_api_dataset (singleton, release_tag, built_at, activated_at, source_commit) VALUES (1, ?1, ?2, NULL, ?3)",
        params![tag, built, commit],
    )
    .map_err(|e| format!("insert bgp_api_dataset: {e}"))?;
    Ok(())
}

fn import_csv(
    conn: &mut Connection,
    path: &Path,
    spec: &TableSpec<'_>,
    geo_fallback: Option<i64>,
    city_coords: Option<&CityCoordIndex>,
) -> Result<usize, String> {
    let file = File::open(path).map_err(|e| format!("open {}: {e}", path.display()))?;
    let mut rdr = csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .from_reader(BufReader::with_capacity(1 << 20, file));
    let mut rec = csv::ByteRecord::new();
    let mut batch: Vec<Vec<Value>> = Vec::with_capacity(BATCH_SIZE);
    let mut count = 0usize;
    while rdr
        .read_byte_record(&mut rec)
        .map_err(|e| format!("read {}: {e}", path.display()))?
    {
        if !valid_row(&rec, spec.required) {
            continue;
        }
        batch.push(row_values(&rec, spec.fields, geo_fallback, city_coords));
        count += 1;
        if batch.len() >= BATCH_SIZE {
            flush_batch(conn, spec.insert_sql, &mut batch)?;
        }
        if spec.report_every > 0 && count.is_multiple_of(spec.report_every) {
            println!("{}: imported {} rows", spec.name, count);
        }
    }
    if !batch.is_empty() {
        flush_batch(conn, spec.insert_sql, &mut batch)?;
    }
    if count == 0 {
        return Err(format!("{} is empty", path.display()));
    }
    Ok(count)
}

fn flush_batch(
    conn: &mut Connection,
    sql: &str,
    batch: &mut Vec<Vec<Value>>,
) -> Result<(), String> {
    let tx = conn.transaction().map_err(|e| format!("begin tx: {e}"))?;
    {
        let mut stmt = tx.prepare(sql).map_err(|e| format!("prepare: {e}"))?;
        for row in batch.iter() {
            stmt.execute(rusqlite::params_from_iter(row.iter()))
                .map_err(|e| format!("insert: {e}"))?;
        }
    }
    tx.commit().map_err(|e| format!("commit: {e}"))?;
    batch.clear();
    Ok(())
}

fn valid_row(rec: &csv::ByteRecord, required: &[usize]) -> bool {
    required.iter().all(|&i| {
        rec.get(i)
            .is_some_and(|b| !String::from_utf8_lossy(b).trim().is_empty())
    })
}

fn field_text(rec: &csv::ByteRecord, idx: usize) -> String {
    rec.get(idx)
        .map(|b| String::from_utf8_lossy(b).trim().to_owned())
        .unwrap_or_default()
}

fn row_values(
    rec: &csv::ByteRecord,
    fields: usize,
    geo_fallback: Option<i64>,
    city_coords: Option<&CityCoordIndex>,
) -> Vec<Value> {
    let coords = geo_fallback.map(|_| {
        let (lat, lon) = (
            parse_coord_str(&field_text(rec, 6)),
            parse_coord_str(&field_text(rec, 7)),
        );
        city_coords.map_or((lat, lon), |idx| {
            idx.resolve(lat, lon, &field_text(rec, 3), &field_text(rec, 5))
        })
    });
    let mut vals: Vec<Value> = (0..fields)
        .map(|i| match (coords, i) {
            (Some((lat, _)), 6) => lat.map_or(Value::Null, Value::Real),
            (Some((_, lon)), 7) => lon.map_or(Value::Null, Value::Real),
            _ => match field_text(rec, i) {
                s if s.is_empty() => Value::Null,
                s => Value::Text(s),
            },
        })
        .collect();
    if let Some(fb) = geo_fallback {
        vals.push(Value::Integer(fb));
    }
    vals
}
