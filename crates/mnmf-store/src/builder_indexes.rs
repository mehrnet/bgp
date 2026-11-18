use std::collections::BTreeMap;

use crate::builder::{BuildStats, MmapWriter};
use crate::codec::be_u32;
use crate::encode::{append_prefix_entry, append_range_entry, sort_fixed_records};
use crate::index::{ASN_ROUTE_INDEX_STRIDE, PREFIX_V4_STRIDE, PREFIX_V6_STRIDE};
use crate::lpm_build::SelectionLpmBuilder;
use crate::range_index::{RANGE_V4_STRIDE, RANGE_V6_STRIDE};
use crate::route_lpm_build::RouteLpmBuilder;
use crate::types::{Address, Section};
use bgp_compact::sortkey::{range_to_cidr_bounds, u128_to_ip};

pub(crate) struct TableSections {
    pub(crate) lpm4: Section,
    pub(crate) lpm6: Section,
    pub(crate) range4: Option<Section>,
    pub(crate) range6: Option<Section>,
    pub(crate) pfx4: Section,
    pub(crate) pfx6: Section,
}

impl TableSections {
    pub(crate) const ALLOC: Self = Self {
        lpm4: Section::AllocLpmV4,
        lpm6: Section::AllocLpmV6,
        range4: Some(Section::AllocRangeV4),
        range6: Some(Section::AllocRangeV6),
        pfx4: Section::AllocPrefixV4,
        pfx6: Section::AllocPrefixV6,
    };
    pub(crate) const GEO: Self = Self {
        lpm4: Section::GeoLpmV4,
        lpm6: Section::GeoLpmV6,
        range4: None,
        range6: None,
        pfx4: Section::GeofeedPrefixV4,
        pfx6: Section::GeofeedPrefixV6,
    };
}

#[derive(Default)]
pub(crate) struct RangePrefixPair {
    pub(crate) r4: Vec<u8>,
    pub(crate) r6: Vec<u8>,
    pub(crate) p4: Vec<u8>,
    pub(crate) p6: Vec<u8>,
}

impl RangePrefixPair {
    pub(crate) fn push_range_and_prefixes(
        &mut self,
        ver: u8,
        s: Address,
        e: Address,
        id: u32,
    ) -> u64 {
        let bits = if ver == 4 { 32 } else { 128 };
        if ver == 4 {
            append_range_entry(&mut self.r4, &s, &e, 4, id);
        } else {
            append_range_entry(&mut self.r6, &s, &e, 6, id);
        }
        let s_val = addr_to_u128(&s, ver);
        let e_val = addr_to_u128(&e, ver);
        let covers = range_to_cidr_bounds(s_val, e_val, bits);
        for &(net, len) in &covers {
            let addr = Address::from_ip(u128_to_ip(net, ver == 4));
            if ver == 4 {
                append_prefix_entry(&mut self.p4, &addr, 4, len, id);
            } else {
                append_prefix_entry(&mut self.p6, &addr, 6, len, id);
            }
        }
        covers.len() as u64
    }
}

pub(crate) fn write_sel_and_indexes(
    w: &mut MmapWriter,
    idx: &mut RangePrefixPair,
    sec: TableSections,
    stats: &mut BuildStats,
    is_alloc: bool,
) -> Result<(), String> {
    write_sel_lpm(w, 4, sec.lpm4, &idx.r4, stats, is_alloc)?;
    write_sel_lpm(w, 6, sec.lpm6, &idx.r6, stats, is_alloc)?;
    if let (Some(sr4), Some(sr6)) = (sec.range4, sec.range6) {
        sort_fixed_records(&mut idx.r4, RANGE_V4_STRIDE);
        w.write_section(sr4, &idx.r4)?;
        sort_fixed_records(&mut idx.r6, RANGE_V6_STRIDE);
        w.write_section(sr6, &idx.r6)?;
    }
    sort_fixed_records(&mut idx.p4, PREFIX_V4_STRIDE);
    w.write_section(sec.pfx4, &idx.p4)?;
    sort_fixed_records(&mut idx.p6, PREFIX_V6_STRIDE);
    w.write_section(sec.pfx6, &idx.p6)
}

pub(crate) fn write_route_indexes(
    w: &mut MmapWriter,
    idx: &mut RangePrefixPair,
    stats: &mut BuildStats,
) -> Result<(), String> {
    sort_fixed_records(&mut idx.p4, PREFIX_V4_STRIDE);
    write_route_lpm(w, 4, Section::RouteLpmV4, &idx.p4, PREFIX_V4_STRIDE, stats)?;
    w.write_section(Section::RoutePrefixV4, &idx.p4)?;
    sort_fixed_records(&mut idx.p6, PREFIX_V6_STRIDE);
    write_route_lpm(w, 6, Section::RouteLpmV6, &idx.p6, PREFIX_V6_STRIDE, stats)?;
    w.write_section(Section::RoutePrefixV6, &idx.p6)?;
    sort_fixed_records(&mut idx.r4, RANGE_V4_STRIDE);
    w.write_section(Section::RouteRangeV4, &idx.r4)?;
    sort_fixed_records(&mut idx.r6, RANGE_V6_STRIDE);
    w.write_section(Section::RouteRangeV6, &idx.r6)
}

pub(crate) fn write_asn_routes(
    w: &mut MmapWriter,
    asn_routes: &BTreeMap<u32, Vec<u32>>,
) -> Result<(), String> {
    let ids_start = w.begin_section()?;
    let mut index = Vec::with_capacity(asn_routes.len() * ASN_ROUTE_INDEX_STRIDE);
    let mut first_idx = 0u64;
    for (&asn, ids) in asn_routes {
        index.extend_from_slice(&asn.to_be_bytes());
        index.extend_from_slice(&first_idx.to_be_bytes());
        index.extend_from_slice(&(ids.len() as u32).to_be_bytes());
        for &id in ids {
            w.write_all(&id.to_be_bytes())?;
        }
        first_idx += ids.len() as u64;
    }
    w.end_section(Section::AsnRouteIds, ids_start);
    w.write_section(Section::AsnRouteIndex, &index)
}

fn write_sel_lpm(
    w: &mut MmapWriter,
    ver: u8,
    sec: Section,
    range_data: &[u8],
    stats: &mut BuildStats,
    is_alloc: bool,
) -> Result<(), String> {
    let (stride, ab, bits) = if ver == 4 {
        (RANGE_V4_STRIDE, 4usize, 32u32)
    } else {
        (RANGE_V6_STRIDE, 16usize, 128u32)
    };
    let mut sel = SelectionLpmBuilder::new(ver);
    for chunk in range_data.chunks_exact(stride) {
        let s = addr_from_bytes(chunk.get(..ab).unwrap_or(&[]), ver);
        let e = addr_from_bytes(chunk.get(ab..2 * ab).unwrap_or(&[]), ver);
        let id = be_u32(chunk.get(2 * ab..stride).unwrap_or(&[])).map_err(|err| err.to_string())?;
        for (net, len) in range_to_cidr_bounds(addr_to_u128(&s, ver), addr_to_u128(&e, ver), bits) {
            sel.insert(Address::from_ip(u128_to_ip(net, ver == 4)), len, id, s, e);
        }
    }
    let (enc, nodes) = sel.encode();
    w.write_section(sec, &enc)?;
    if is_alloc {
        stats.allocation_lpm_nodes += nodes;
        stats.allocation_lpm_bytes += enc.len() as u64;
    } else {
        stats.geofeed_lpm_nodes += nodes;
        stats.geofeed_lpm_bytes += enc.len() as u64;
    }
    Ok(())
}

fn write_route_lpm(
    w: &mut MmapWriter,
    ver: u8,
    sec: Section,
    pfx_data: &[u8],
    stride: usize,
    stats: &mut BuildStats,
) -> Result<(), String> {
    let ab = if ver == 4 { 4usize } else { 16usize };
    let key_len = ab + 1;
    let mut lpm = RouteLpmBuilder::new(ver);
    let mut off = 0usize;
    while let Some(key) = pfx_data.get(off..off + key_len) {
        let mut ids = Vec::new();
        while let Some(rec) = pfx_data.get(off..off + stride) {
            if rec.get(..key_len) != Some(key) {
                break;
            }
            if let Some(id_b) = rec.get(key_len..stride) {
                ids.push(be_u32(id_b).map_err(|err| err.to_string())?);
            }
            off += stride;
        }
        let bits = key.get(ab).copied().unwrap_or(0);
        lpm.insert(
            addr_from_bytes(key.get(..ab).unwrap_or(&[]), ver),
            bits,
            ids,
        );
    }
    let (enc, nodes, id_cnt) = lpm.encode();
    w.write_section(sec, &enc)?;
    stats.route_lpm_nodes += nodes;
    stats.route_lpm_ids += id_cnt;
    stats.route_lpm_bytes += enc.len() as u64;
    Ok(())
}

fn addr_to_u128(addr: &Address, ver: u8) -> u128 {
    if ver == 4 {
        u128::from(be_u32(addr.0.get(12..16).unwrap_or(&[])).unwrap_or(0))
    } else {
        u128::from_be_bytes(addr.0)
    }
}

fn addr_from_bytes(src: &[u8], ver: u8) -> Address {
    let mut out = [0u8; 16];
    if ver == 4 {
        if let (Some(d), Some(s)) = (out.get_mut(12..16), src.get(..4)) {
            d.copy_from_slice(s);
        }
    } else if let Some(s) = src.get(..16) {
        out.copy_from_slice(s);
    }
    Address(out)
}
