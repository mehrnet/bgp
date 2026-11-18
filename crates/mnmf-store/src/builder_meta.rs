use std::collections::{HashMap, HashSet};
use std::path::Path;

use bgp_compact::sortkey::parse_asn;

use crate::builder::{BuildStats, MmapWriter};
use crate::builder_indexes::{RangePrefixPair, write_geo_indexes};
use crate::builder_tables::{field, for_each_csv_row, next_id, parse_range_row};
use crate::encode::{StringPoolWriter, append_autnum, append_geofeed};
use crate::index::AUTNUM_INDEX_STRIDE;
use crate::types::{Autnum, Geofeed, Section};

pub(crate) fn build_geofeeds(
    w: &mut MmapWriter,
    pool: &mut StringPoolWriter,
    stats: &mut BuildStats,
    input_dir: &Path,
) -> Result<(), String> {
    let data_start = w.begin_section()?;
    let (mut offsets, mut buf, mut data_pos) = (vec![0u64], Vec::new(), 0u64);
    let mut rfc_idx = RangePrefixPair::default();
    let mut fb_idx = RangePrefixPair::default();

    let ctx_primary = GeoWriteCtx {
        w,
        pool,
        stats,
        offsets: &mut offsets,
        idx: &mut rfc_idx,
        buf: &mut buf,
        data_pos: &mut data_pos,
    };
    load_geofeed_file(&input_dir.join("geolocations.csv"), false, ctx_primary)?;

    let fallback_path = input_dir.join("geolocations_fallback.csv");
    if fallback_path.is_file() {
        let ctx_fb = GeoWriteCtx {
            w,
            pool,
            stats,
            offsets: &mut offsets,
            idx: &mut fb_idx,
            buf: &mut buf,
            data_pos: &mut data_pos,
        };
        load_geofeed_file(&fallback_path, true, ctx_fb)?;
    }

    w.end_section(Section::GeofeedData, data_start);
    w.write_u64_section(Section::GeofeedOffsets, &offsets)?;
    write_geo_indexes(w, &mut rfc_idx, &mut fb_idx, stats)
}

struct GeoWriteCtx<'a> {
    w: &'a mut MmapWriter,
    pool: &'a mut StringPoolWriter,
    stats: &'a mut BuildStats,
    offsets: &'a mut Vec<u64>,
    idx: &'a mut RangePrefixPair,
    buf: &'a mut Vec<u8>,
    data_pos: &'a mut u64,
}

fn load_geofeed_file(path: &Path, is_fallback: bool, c: GeoWriteCtx<'_>) -> Result<(), String> {
    for_each_csv_row(path, |row| {
        let Some((ver, s, e)) = (row.len() >= 6).then(|| parse_range_row(row)).flatten() else {
            return Ok(());
        };
        let id = next_id(c.stats.geofeeds)?;
        let rec = Geofeed {
            version: ver,
            start: s,
            end: e,
            country: field(row, 3),
            region: field(row, 4),
            city: field(row, 5),
            latitude: parse_coord(row, 6),
            longitude: parse_coord(row, 7),
            is_fallback,
        };
        c.buf.clear();
        append_geofeed(c.buf, c.pool, &rec);
        c.w.write_all(c.buf)?;
        *c.data_pos += c.buf.len() as u64;
        c.offsets.push(*c.data_pos);
        c.stats.geofeed_index += c.idx.push_range_and_prefixes(ver, s, e, id);
        c.stats.geofeeds = u64::from(id);
        Ok(())
    })
}

fn parse_coord(row: &csv::ByteRecord, idx: usize) -> Option<f64> {
    field(row, idx)
        .parse::<f64>()
        .ok()
        .filter(|v| v.is_finite())
}

pub(crate) fn build_autnums(
    w: &mut MmapWriter,
    pool: &mut StringPoolWriter,
    stats: &mut BuildStats,
    path: &Path,
) -> Result<(), String> {
    let mut seen: HashSet<u32> = HashSet::new();
    let mut items: Vec<(u32, Vec<u8>)> = Vec::new();
    for_each_csv_row(path, |row| {
        if row.len() < 12 {
            return Ok(());
        }
        let raw_asn = field(row, 0).to_ascii_uppercase();
        let asn = parse_asn(&raw_asn);
        if asn == 0 || !seen.insert(asn) {
            return Ok(());
        }
        let rec = Autnum {
            as_number: asn,
            asn: raw_asn,
            registry: field(row, 1),
            country: field(row, 2),
            name: field(row, 3),
            organization: field(row, 4),
            status: field(row, 5),
            created: field(row, 6),
            last_modified: field(row, 7),
            source: field(row, 8),
            maintainers: field(row, 9),
            abuse_contact: field(row, 10),
            description: field(row, 11),
        };
        let mut data = Vec::new();
        append_autnum(&mut data, pool, &rec);
        items.push((asn, data));
        stats.autnums += 1;
        Ok(())
    })?;
    items.sort_unstable_by_key(|(asn, _)| *asn);
    let data_start = w.begin_section()?;
    let (mut index, mut off) = (Vec::with_capacity(items.len() * AUTNUM_INDEX_STRIDE), 0u64);
    for (asn, data) in items {
        w.write_all(&data)?;
        index.extend_from_slice(&asn.to_be_bytes());
        index.extend_from_slice(&off.to_be_bytes());
        index.extend_from_slice(&(data.len() as u32).to_be_bytes());
        off += data.len() as u64;
    }
    w.end_section(Section::AutnumData, data_start);
    w.write_section(Section::AutnumIndex, &index)
}

fn _unused(_: HashMap<u32, u32>) {}
