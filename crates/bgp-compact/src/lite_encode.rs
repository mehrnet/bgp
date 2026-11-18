use std::io::Write;

use crate::lite_chunk::{ASN_CHUNK_SIZE, write_chunked_records};
use crate::lite_db::LiteDbBuilder;
use crate::lite_routes::CompactDicts;
use crate::lite_tables::{write_asn_routes, write_dicts, write_offset_data, write_seg_sections};
use crate::lite_types::{
    AutnumInput, DEFAULT_BLOCK_SIZE, DEFAULT_ROUTE_V4_BITS, DEFAULT_ROUTE_V6_BITS, EncodeInput,
    GeoLocKey, LiteBuildOptions, LiteStats,
};
use crate::sweep::write_uvarint;

const MAGIC: &[u8; 4] = b"MNL1";
const FORMAT_VERSION: u8 = 5;
const FLAG_HAS_ASN_ROUTES: u8 = 1 << 0;
const FLAG_SHIFTED_DELTAS: u8 = 1 << 1;
const FLAG_CHUNKED_DEFLATE: u8 = 1 << 2;
const FLAG_ROUTE_FREQ: u8 = 1 << 3;

pub(crate) fn encode_mnl1(
    input: &EncodeInput<'_>,
    w: &mut impl Write,
) -> Result<LiteStats, String> {
    let b = input.builder;
    let dicts = input.dicts;
    let meta = serde_json::to_vec(&b.opts.metadata).map_err(|e| format!("meta json: {e}"))?;
    if meta.len() > u16::MAX as usize {
        return Err("litedb: metadata exceeds 64KiB".into());
    }
    let mut buf = Vec::new();
    write_header(&mut buf, &b.opts, &meta);
    write_dicts(&mut buf, dicts);
    write_geolocs(&mut buf, &dicts.compact_geolocs);
    write_autnums(&mut buf, b, dicts)?;
    write_multi_sets(&mut buf, &input.routes.multi_sets);
    write_route_val_table(&mut buf, &input.routes.route_val_table);
    write_seg_sections(&mut buf, b, input);
    let asn_route_items = write_asn_routes(&mut buf, &b.opts, &dicts.asns, &b.asn_routes)?;
    w.write_all(&buf).map_err(|e| format!("write MNL1: {e}"))?;
    Ok(build_stats(buf.len(), b, input, asn_route_items))
}

fn write_header(buf: &mut Vec<u8>, opts: &LiteBuildOptions, meta: &[u8]) {
    let flags = (u8::from(opts.include_asn_routes) * FLAG_HAS_ASN_ROUTES)
        | FLAG_SHIFTED_DELTAS
        | FLAG_CHUNKED_DEFLATE
        | FLAG_ROUTE_FREQ;
    buf.extend_from_slice(MAGIC);
    buf.extend_from_slice(&[
        FORMAT_VERSION,
        flags,
        DEFAULT_BLOCK_SIZE,
        opts.alloc_v4_bits,
        opts.alloc_v6_bits,
        DEFAULT_ROUTE_V4_BITS,
        DEFAULT_ROUTE_V6_BITS,
        opts.geo_v4_bits,
        opts.geo_v6_bits,
    ]);
    buf.extend_from_slice(&(meta.len() as u16).to_le_bytes());
    buf.extend_from_slice(meta);
}

fn write_geolocs(buf: &mut Vec<u8>, locs: &[GeoLocKey]) {
    buf.extend_from_slice(&(locs.len() as u32).to_le_bytes());
    let mut data = Vec::new();
    let mut offsets = Vec::with_capacity(locs.len() + 1);
    for loc in locs {
        offsets.push(data.len() as u32);
        write_uvarint(&mut data, u64::from(loc.cc_id));
        write_uvarint(&mut data, u64::from(loc.reg_id));
        write_uvarint(&mut data, u64::from(loc.city_id));
    }
    offsets.push(data.len() as u32);
    write_offset_data(buf, &offsets, &data);
}

fn write_autnums(buf: &mut Vec<u8>, b: &LiteDbBuilder, d: &CompactDicts) -> Result<(), String> {
    buf.extend_from_slice(&(d.asns.len() as u32).to_le_bytes());
    for &asn in &d.asns {
        buf.extend_from_slice(&asn.to_le_bytes());
    }
    write_chunked_records(buf, d.asns.len(), ASN_CHUNK_SIZE, |idx, data| {
        if let Some(&asn) = d.asns.get(idx)
            && let Some(rec) = b.autnums.get(&asn)
        {
            encode_autnum_rec(data, rec, b, d);
        } else {
            data.push(0);
        }
    })
}

fn encode_autnum_rec(data: &mut Vec<u8>, r: &AutnumInput, b: &LiteDbBuilder, d: &CompactDicts) {
    data.push(1);
    let reg = d
        .registry_map
        .get(b.registries.lookup_id(&r.registry) as usize)
        .copied()
        .unwrap_or(0);
    let cc = d
        .country_map
        .get(b.countries.lookup_id(&r.country) as usize)
        .copied()
        .unwrap_or(0);
    let st = d
        .status_map
        .get(b.statuses.lookup_id(&r.status) as usize)
        .copied()
        .unwrap_or(0);
    let src = d
        .source_map
        .get(b.sources.lookup_id(&r.source) as usize)
        .copied()
        .unwrap_or(0);
    for id in [reg, cc, st, src] {
        write_uvarint(data, u64::from(id));
    }
    for text in [
        &r.name,
        &r.organization,
        &r.maintainers,
        &r.abuse_contact,
        &r.description,
    ] {
        write_uvarint(data, text.len() as u64);
        data.extend_from_slice(text.as_bytes());
    }
}

fn write_multi_sets(buf: &mut Vec<u8>, sets: &[Vec<u32>]) {
    buf.extend_from_slice(&(sets.len() as u32).to_le_bytes());
    let mut data = Vec::new();
    let mut offsets = Vec::with_capacity(sets.len() + 1);
    for set in sets {
        offsets.push(data.len() as u32);
        write_uvarint(&mut data, set.len() as u64);
        for &idx in set {
            write_uvarint(&mut data, u64::from(idx));
        }
    }
    offsets.push(data.len() as u32);
    write_offset_data(buf, &offsets, &data);
}

fn write_route_val_table(buf: &mut Vec<u8>, table: &[u32]) {
    buf.extend_from_slice(&(table.len() as u32).to_le_bytes());
    for &val in table {
        buf.extend_from_slice(&val.to_le_bytes());
    }
}

fn build_stats(
    bytes: usize,
    b: &LiteDbBuilder,
    input: &EncodeInput<'_>,
    asn_route_items: usize,
) -> LiteStats {
    LiteStats {
        bytes,
        countries: input.dicts.countries.len(),
        registries: input.dicts.registries.len(),
        regions: input.dicts.regions.len(),
        cities: input.dicts.cities.len(),
        geo_locations: input.dicts.compact_geolocs.len(),
        autnums: b.autnums.len(),
        alloc_v4_segs: input.alloc_s4.len(),
        alloc_v6_segs: input.alloc_s6.len(),
        geo_v4_segs: input.geo_s4.len(),
        geo_v6_segs: input.geo_s6.len(),
        route_v4_segs: input.routes.segs4.len(),
        route_v6_segs: input.routes.segs6.len(),
        multi_asn_sets: input.routes.multi_sets.len(),
        asn_route_items,
    }
}
