use std::collections::BTreeMap;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use bgp_compact::sortkey::{addr_from_sort_key, cidr_to_bounds, parse_asn, u128_to_ip};

use crate::builder::{BuildStats, MmapWriter};
use crate::builder_indexes::{
    RangePrefixPair, TableSections, write_asn_routes, write_route_indexes, write_sel_and_indexes,
};
use crate::encode::{
    StringPoolWriter, append_allocation, append_prefix_entry, append_range_entry, append_route,
};
use crate::summary_build::SummaryCollector;
use crate::types::{Address, Allocation, Route, Section};

pub(crate) fn for_each_csv_row(
    path: &Path,
    mut cb: impl FnMut(&csv::ByteRecord) -> Result<(), String>,
) -> Result<(), String> {
    let file = File::open(path).map_err(|e| format!("open {}: {e}", path.display()))?;
    let mut rdr = csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .from_reader(BufReader::with_capacity(1 << 20, file));
    let mut rec = csv::ByteRecord::new();
    while rdr
        .read_byte_record(&mut rec)
        .map_err(|e| format!("read {}: {e}", path.display()))?
    {
        cb(&rec)?;
    }
    Ok(())
}

pub(crate) fn field(rec: &csv::ByteRecord, idx: usize) -> String {
    rec.get(idx)
        .map(|b| String::from_utf8_lossy(b).trim().to_owned())
        .unwrap_or_default()
}

pub(crate) fn parse_range_row(rec: &csv::ByteRecord) -> Option<(u8, Address, Address)> {
    let ver_s = field(rec, 2);
    let version = match ver_s.as_str() {
        "4" => 4u8,
        "6" => 6u8,
        _ => return None,
    };
    let s = Address::from_ip(addr_from_sort_key(&field(rec, 0), &ver_s)?);
    let e = Address::from_ip(addr_from_sort_key(&field(rec, 1), &ver_s)?);
    (s <= e).then_some((version, s, e))
}

pub(crate) fn build_allocations(
    w: &mut MmapWriter,
    pool: &mut StringPoolWriter,
    summaries: &mut SummaryCollector,
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
        let id = next_id(stats.allocations)?;
        let rec = Allocation {
            version: ver,
            start: s,
            end: e,
            registry: field(row, 3),
            country: field(row, 4),
            name: field(row, 5),
            status: field(row, 6),
            allocation_date: field(row, 7),
            created: field(row, 8),
            last_modified: field(row, 9),
            source: field(row, 10),
            maintainers: field(row, 11),
            organization: field(row, 12),
            abuse_contact: field(row, 13),
            description: field(row, 14),
        };
        buf.clear();
        append_allocation(&mut buf, pool, &rec);
        w.write_all(&buf)?;
        data_pos += buf.len() as u64;
        offsets.push(data_pos);
        stats.allocation_range_index += 1;
        stats.allocation_index += idx.push_range_and_prefixes(ver, s, e, id);
        summaries.collect(s, e, ver, false, &rec.country);
        stats.allocations = u64::from(id);
        Ok(())
    })?;
    w.end_section(Section::AllocationData, data_start);
    w.write_u64_section(Section::AllocationOffsets, &offsets)?;
    write_sel_and_indexes(w, &mut idx, TableSections::ALLOC, stats, true)
}

pub(crate) fn build_routes(
    w: &mut MmapWriter,
    pool: &mut StringPoolWriter,
    summaries: &mut SummaryCollector,
    stats: &mut BuildStats,
    path: &Path,
) -> Result<(), String> {
    let data_start = w.begin_section()?;
    let mut offsets = vec![0u64];
    let mut asn_routes: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    let (mut idx, mut buf, mut data_pos) = (RangePrefixPair::default(), Vec::new(), 0u64);
    for_each_csv_row(path, |row| {
        if row.len() < 5 {
            return Ok(());
        }
        let origin = field(row, 4).to_ascii_uppercase();
        let Some((ver, s_u, e_u, net_ip, bits)) = (!origin.is_empty())
            .then(|| cidr_to_bounds(&field(row, 3)))
            .flatten()
        else {
            return Ok(());
        };
        let id = next_id(stats.routes)?;
        let asn = parse_asn(&origin);
        let net = Address::from_ip(net_ip);
        let rec = parse_route_row(row, net, ver, bits, origin, asn);
        buf.clear();
        append_route(&mut buf, pool, &rec);
        w.write_all(&buf)?;
        data_pos += buf.len() as u64;
        offsets.push(data_pos);
        if asn != 0 {
            asn_routes.entry(asn).or_default().push(id);
        }
        let s = Address::from_ip(u128_to_ip(s_u, ver == 4));
        let e = Address::from_ip(u128_to_ip(e_u, ver == 4));
        push_route_index(&mut idx, &rec, s, e, id);
        summaries.collect(s, e, ver, true, &rec.origin_asn);
        stats.route_index += 1;
        stats.route_range_index += 1;
        stats.routes = u64::from(id);
        Ok(())
    })?;
    w.end_section(Section::RouteData, data_start);
    w.write_u64_section(Section::RouteOffsets, &offsets)?;
    write_route_indexes(w, &mut idx, stats)?;
    write_asn_routes(w, &asn_routes)
}

fn parse_route_row(
    row: &csv::ByteRecord,
    net: Address,
    ver: u8,
    bits: u8,
    origin: String,
    asn: u32,
) -> Route {
    Route {
        network: net,
        version: ver,
        prefix_length: bits,
        as_number: asn,
        origin_asn: origin,
        registry: field(row, 5),
        source: field(row, 6),
        maintainers: field(row, 7),
        organization: field(row, 8),
        abuse_contact: field(row, 9),
        description: field(row, 10),
    }
}

fn push_route_index(idx: &mut RangePrefixPair, rec: &Route, s: Address, e: Address, id: u32) {
    if rec.version == 4 {
        append_range_entry(&mut idx.r4, &s, &e, 4, id);
        append_prefix_entry(&mut idx.p4, &rec.network, 4, rec.prefix_length, id);
    } else {
        append_range_entry(&mut idx.r6, &s, &e, 6, id);
        append_prefix_entry(&mut idx.p6, &rec.network, 6, rec.prefix_length, id);
    }
}

pub(crate) fn next_id(cur: u64) -> Result<u32, String> {
    if cur >= u64::from(u32::MAX) {
        return Err("source record count exceeds u32".into());
    }
    Ok((cur + 1) as u32)
}
