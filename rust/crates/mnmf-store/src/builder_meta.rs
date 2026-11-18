use std::collections::{HashMap, HashSet};
use std::path::Path;

use bgp_compact::sortkey::parse_asn;

use crate::builder::{BuildStats, MmapWriter};
use crate::builder_indexes::{RangePrefixPair, TableSections, write_sel_and_indexes};
use crate::builder_tables::{field, for_each_csv_row, next_id, parse_range_row};
use crate::encode::{StringPoolWriter, append_autnum, append_geofeed};
use crate::index::AUTNUM_INDEX_STRIDE;
use crate::types::{Autnum, Geofeed, Section};

pub(crate) fn build_geofeeds(
    w: &mut MmapWriter,
    pool: &mut StringPoolWriter,
    stats: &mut BuildStats,
    path: &Path,
) -> Result<(), String> {
    let data_start = w.begin_section()?;
    let (mut offsets, mut idx, mut buf, mut data_pos) =
        (vec![0u64], RangePrefixPair::default(), Vec::new(), 0u64);
    for_each_csv_row(path, |row| {
        let Some((ver, s, e)) = (row.len() >= 6).then(|| parse_range_row(row)).flatten() else {
            return Ok(());
        };
        let id = next_id(stats.geofeeds)?;
        let rec = Geofeed {
            version: ver,
            start: s,
            end: e,
            country: field(row, 3),
            region: field(row, 4),
            city: field(row, 5),
        };
        buf.clear();
        append_geofeed(&mut buf, pool, &rec);
        w.write_all(&buf)?;
        data_pos += buf.len() as u64;
        offsets.push(data_pos);
        stats.geofeed_index += idx.push_range_and_prefixes(ver, s, e, id);
        stats.geofeeds = u64::from(id);
        Ok(())
    })?;
    w.end_section(Section::GeofeedData, data_start);
    w.write_u64_section(Section::GeofeedOffsets, &offsets)?;
    write_sel_and_indexes(w, &mut idx, TableSections::GEO, stats, false)
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
