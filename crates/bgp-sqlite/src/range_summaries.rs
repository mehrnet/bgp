use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use bgp_compact::sortkey::{normalize_country, parse_asn};
use rusqlite::{Connection, params};
use serde::Serialize;

const SUMMARY_LENGTHS: &[u8] = &[0, 8, 16];

#[derive(Default)]
struct SummaryAcc {
    allocations: i64,
    routes: i64,
    countries: HashMap<String, i64>,
    asns: HashMap<String, i64>,
}

#[derive(Serialize)]
struct FacetJson {
    value: String,
    record_count: i64,
}

pub fn build_range_summaries(db_path: &Path) -> Result<usize, String> {
    let mut conn = Connection::open(db_path).map_err(|e| format!("open sqlite: {e}"))?;
    conn.execute_batch("PRAGMA synchronous = OFF; PRAGMA journal_mode = MEMORY;")
        .map_err(|e| format!("pragmas: {e}"))?;
    let mut map: BTreeMap<(u8, u32), SummaryAcc> = BTreeMap::new();
    collect(
        &conn,
        "SELECT start_ip_sort, end_ip_sort, country FROM allocations WHERE ip_version = 4",
        &mut map,
        |item, val| {
            item.allocations += 1;
            if let Some(cc) = normalize_country(val) {
                *item.countries.entry(cc).or_insert(0) += 1;
            }
        },
    )?;
    collect(
        &conn,
        "SELECT start_ip_sort, end_ip_sort, asn FROM routes WHERE ip_version = 4",
        &mut map,
        |item, val| {
            item.routes += 1;
            let num = parse_asn(val);
            if num > 0 {
                *item.asns.entry(format!("AS{num}")).or_insert(0) += 1;
            }
        },
    )?;
    persist(&mut conn, &map)?;
    Ok(map.len())
}

fn collect(
    conn: &Connection,
    sql: &str,
    map: &mut BTreeMap<(u8, u32), SummaryAcc>,
    mut add: impl FnMut(&mut SummaryAcc, &str),
) -> Result<(), String> {
    let mut stmt = conn.prepare(sql).map_err(|e| format!("prepare: {e}"))?;
    let mut rows = stmt.query([]).map_err(|e| format!("query: {e}"))?;
    let mut count = 0usize;
    while let Some(row) = rows.next().map_err(|e| format!("row: {e}"))? {
        let s_raw: String = row.get(0).map_err(|e| format!("col 0: {e}"))?;
        let e_raw: String = row.get(1).map_err(|e| format!("col 1: {e}"))?;
        let val: Option<String> = row.get(2).map_err(|e| format!("col 2: {e}"))?;
        let s = ipv4_val(&s_raw)?;
        let e = ipv4_val(&e_raw)?;
        if s > e {
            return Err(format!("invalid IPv4 range {s_raw}-{e_raw}"));
        }
        let v_str = val.as_deref().unwrap_or("");
        for &len in SUMMARY_LENGTHS {
            let shift = 32 - u32::from(len);
            let (first, last) = (
                (u64::from(s) >> shift) as u32,
                (u64::from(e) >> shift) as u32,
            );
            for bucket in first..=last {
                add(map.entry((len, bucket)).or_default(), v_str);
            }
        }
        count += 1;
        if count.is_multiple_of(1_000_000) {
            println!("range summaries: processed {} records", count);
        }
    }
    Ok(())
}

fn persist(conn: &mut Connection, map: &BTreeMap<(u8, u32), SummaryAcc>) -> Result<(), String> {
    conn.execute_batch(
        "DROP TABLE IF EXISTS range_summaries;
         CREATE TABLE range_summaries (
            cidr TEXT PRIMARY KEY, ip_version INTEGER NOT NULL, prefix_length INTEGER NOT NULL,
            allocation_records INTEGER NOT NULL, route_records INTEGER NOT NULL,
            countries TEXT NOT NULL, asns TEXT NOT NULL);",
    )
    .map_err(|e| format!("init range_summaries: {e}"))?;

    let tx = conn.transaction().map_err(|e| format!("begin tx: {e}"))?;
    {
        let mut stmt = tx
            .prepare("INSERT INTO range_summaries (cidr, ip_version, prefix_length, allocation_records, route_records, countries, asns) VALUES (?1, 4, ?2, ?3, ?4, ?5, ?6)")
            .map_err(|e| format!("prepare insert: {e}"))?;
        for (&(len, bucket), acc) in map {
            let countries = serde_json::to_string(&top_facets(&acc.countries))
                .map_err(|e| format!("json: {e}"))?;
            let asns =
                serde_json::to_string(&top_facets(&acc.asns)).map_err(|e| format!("json: {e}"))?;
            stmt.execute(params![
                cidr_str(len, bucket),
                i64::from(len),
                acc.allocations,
                acc.routes,
                countries,
                asns
            ])
            .map_err(|e| format!("insert summary: {e}"))?;
        }
    }
    tx.execute_batch("ANALYZE range_summaries;")
        .map_err(|e| format!("analyze: {e}"))?;
    tx.commit().map_err(|e| format!("commit: {e}"))
}

fn ipv4_val(sort_key: &str) -> Result<u32, String> {
    let v: u128 = sort_key
        .trim()
        .parse()
        .map_err(|e| format!("parse v4 sort key: {e}"))?;
    Ok((v & 0xffff_ffff) as u32)
}

fn top_facets(map: &HashMap<String, i64>) -> Vec<FacetJson> {
    let mut items: Vec<FacetJson> = map
        .iter()
        .map(|(k, &v)| FacetJson {
            value: k.clone(),
            record_count: v,
        })
        .collect();
    items.sort_by(|a, b| {
        b.record_count
            .cmp(&a.record_count)
            .then_with(|| a.value.cmp(&b.value))
    });
    items.truncate(10);
    items
}

fn cidr_str(len: u8, bucket: u32) -> String {
    let v = if len == 0 {
        0
    } else {
        bucket << (32 - u32::from(len))
    };
    format!(
        "{}.{}.{}.{}/{}",
        v >> 24,
        (v >> 16) & 255,
        (v >> 8) & 255,
        v & 255,
        len
    )
}
