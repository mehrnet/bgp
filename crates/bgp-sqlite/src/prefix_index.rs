use std::path::Path;

use bgp_compact::sortkey::range_to_prefixes;
use rusqlite::types::Value;
use rusqlite::{Connection, params};

const BATCH_SIZE: i64 = 1000;

struct SourceDef {
    name: &'static str,
    query: &'static str,
}

const SOURCES: &[SourceDef] = &[
    SourceDef {
        name: "allocation",
        query: "SELECT rowid, start_ip_sort, end_ip_sort, ip_version, registry, country, netname, NULL, NULL, NULL, NULL, status, allocation_date, created, last_modified, source, mnt_by, org, abuse_contact, descr, NULL, NULL, NULL FROM allocations WHERE rowid > ?1 ORDER BY rowid LIMIT ?2",
    },
    SourceDef {
        name: "route",
        query: "SELECT rowid, start_ip_sort, end_ip_sort, ip_version, registry, NULL, NULL, cidr, asn, NULL, NULL, NULL, NULL, NULL, NULL, source, mnt_by, org, abuse_contact, descr, NULL, NULL, NULL FROM routes WHERE rowid > ?1 ORDER BY rowid LIMIT ?2",
    },
    SourceDef {
        name: "geofeed",
        query: "SELECT rowid, start_ip_sort, end_ip_sort, ip_version, NULL, country, NULL, NULL, NULL, region, city, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, latitude, longitude, is_fallback FROM geolocations WHERE is_fallback = 0 AND rowid > ?1 ORDER BY rowid LIMIT ?2",
    },
];

struct SourceRow {
    row_id: i64,
    start: String,
    end: String,
    version: i64,
    cols: Vec<Value>,
}

pub fn build_prefix_index(db_path: &Path) -> Result<(), String> {
    let mut conn = Connection::open(db_path).map_err(|e| format!("open sqlite: {e}"))?;
    conn.execute_batch(
        "PRAGMA synchronous = OFF;
         PRAGMA journal_mode = MEMORY;
         DROP TABLE IF EXISTS lookup_prefixes;
         CREATE TABLE lookup_prefixes (
            source TEXT NOT NULL, prefix_key TEXT NOT NULL, prefix_length INTEGER NOT NULL,
            start_ip_sort TEXT NOT NULL, end_ip_sort TEXT NOT NULL, ip_version INTEGER NOT NULL,
            registry TEXT, country TEXT, netname TEXT, cidr TEXT, asn TEXT, region TEXT, city TEXT,
            status TEXT, allocation_date TEXT, created TEXT, last_modified TEXT,
            record_source TEXT, mnt_by TEXT, org TEXT, abuse_contact TEXT, description TEXT,
            latitude REAL, longitude REAL, is_fallback INTEGER);",
    )
    .map_err(|e| format!("init lookup_prefixes: {e}"))?;

    for def in SOURCES {
        materialize(&mut conn, def)?;
    }
    conn.execute_batch(
        "DROP INDEX IF EXISTS idx_alloc; DROP INDEX IF EXISTS idx_routes; DROP INDEX IF EXISTS idx_geo;
         CREATE INDEX IF NOT EXISTS idx_alloc_by_start ON allocations(ip_version, start_ip_sort);
         CREATE INDEX IF NOT EXISTS idx_alloc_by_end ON allocations(ip_version, end_ip_sort);
         CREATE INDEX IF NOT EXISTS idx_routes_by_start ON routes(ip_version, start_ip_sort);
         CREATE INDEX IF NOT EXISTS idx_routes_by_end ON routes(ip_version, end_ip_sort);
         CREATE INDEX IF NOT EXISTS idx_routes_by_asn ON routes(asn, cidr);
         CREATE INDEX IF NOT EXISTS idx_autnums_by_asn ON autnums(asn);
         CREATE INDEX IF NOT EXISTS idx_geo_by_start ON geolocations(ip_version, start_ip_sort);
         CREATE INDEX IF NOT EXISTS idx_geo_by_end ON geolocations(ip_version, end_ip_sort);
         CREATE INDEX idx_lookup_prefix ON lookup_prefixes(source, prefix_key);
         ANALYZE;",
    )
    .map_err(|e| format!("create indexes: {e}"))
}

fn materialize(conn: &mut Connection, def: &SourceDef) -> Result<(), String> {
    let (mut last_id, mut records, mut prefixes) = (0i64, 0usize, 0usize);
    loop {
        let batch = fetch_batch(conn, def.query, last_id)?;
        if batch.is_empty() {
            return Ok(());
        }
        let batch_len = batch.len();
        if let Some(last) = batch.last() {
            last_id = last.row_id;
        }
        prefixes += insert_batch(conn, def.name, &batch)?;
        records += batch_len;
        if records.is_multiple_of(100_000) || batch_len < BATCH_SIZE as usize {
            println!(
                "{}: {} records, {} prefix rows",
                def.name, records, prefixes
            );
        }
    }
}

fn fetch_batch(conn: &Connection, sql: &str, last_id: i64) -> Result<Vec<SourceRow>, String> {
    let mut stmt = conn
        .prepare(sql)
        .map_err(|e| format!("prepare fetch: {e}"))?;
    let rows = stmt
        .query_map(params![last_id, BATCH_SIZE], |r| {
            let cols = (4..23)
                .map(|i| r.get::<_, Value>(i))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(SourceRow {
                row_id: r.get(0)?,
                start: r.get(1)?,
                end: r.get(2)?,
                version: r.get(3)?,
                cols,
            })
        })
        .map_err(|e| format!("query fetch: {e}"))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("scan row: {e}"))
}

fn insert_batch(conn: &mut Connection, source: &str, batch: &[SourceRow]) -> Result<usize, String> {
    let tx = conn.transaction().map_err(|e| format!("begin tx: {e}"))?;
    let mut inserted = 0usize;
    {
        let mut stmt = tx
            .prepare(
                "INSERT INTO lookup_prefixes (source, prefix_key, prefix_length, start_ip_sort, end_ip_sort, ip_version, registry, country, netname, cidr, asn, region, city, status, allocation_date, created, last_modified, record_source, mnt_by, org, abuse_contact, description, latitude, longitude, is_fallback) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,?24,?25)",
            )
            .map_err(|e| format!("prepare insert: {e}"))?;
        for row in batch {
            let ver = u8::try_from(row.version).map_err(|_| "invalid version".to_owned())?;
            for (key, len) in range_to_prefixes(&row.start, &row.end, ver)? {
                let prefix_params = [
                    Value::Text(source.to_owned()),
                    Value::Text(key),
                    Value::Integer(i64::from(len)),
                    Value::Text(row.start.clone()),
                    Value::Text(row.end.clone()),
                    Value::Integer(row.version),
                ];
                stmt.execute(rusqlite::params_from_iter(
                    prefix_params.iter().chain(row.cols.iter()),
                ))
                .map_err(|e| format!("insert prefix: {e}"))?;
                inserted += 1;
            }
        }
    }
    tx.commit().map_err(|e| format!("commit: {e}"))?;
    Ok(inserted)
}
