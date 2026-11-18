use std::collections::HashMap;
use std::net::IpAddr;

use crate::lite_chunk::{
    ASN_CHUNK_SIZE, SegTableSpec, delta_shift, encode_seg_table_shifted, write_chunked_records,
};
use crate::lite_db::LiteDbBuilder;
use crate::lite_routes::CompactDicts;
use crate::lite_types::{
    AsnRoute, DEFAULT_BLOCK_SIZE, DEFAULT_ROUTE_V4_BITS, DEFAULT_ROUTE_V6_BITS, EncodeInput,
    GeoLocKey, LiteBuildOptions,
};
use crate::sortkey::{normalize_country, slot_range};
use crate::sweep::{Interval, Segment, write_uvarint};

pub(crate) const FALLBACK_PRIORITY_BIAS: i64 = 1i64 << 40;

pub(crate) fn push_unique(list: &mut Vec<u32>, val: u32) {
    if !list.contains(&val) {
        list.push(val);
    }
}

pub(crate) fn push_interval(
    b: &mut LiteDbBuilder,
    fam: u8,
    bounds: (u64, u64),
    val: u32,
    is_alloc: bool,
    priority_bias: i64,
) {
    let (lo, hi) = bounds;
    let width = (hi - lo).min(1 << 30) as i64;
    let iv = Interval {
        start: lo,
        end: hi,
        val,
        aux: 0,
        priority: -width - priority_bias,
        seq: b.seq,
    };
    match (fam, is_alloc) {
        (4, true) => b.alloc_v4.push(iv),
        (6, true) => b.alloc_v6.push(iv),
        (4, false) => b.geo_v4.push(iv),
        _ => b.geo_v6.push(iv),
    }
}

pub(crate) fn insert_geofeed(
    b: &mut LiteDbBuilder,
    bounds: (IpAddr, IpAddr),
    fields: (&str, &str, &str),
    priority_bias: i64,
) -> bool {
    let cc = normalize_country(fields.0).unwrap_or_default();
    let (reg, cty) = (fields.1.trim().to_owned(), fields.2.trim().to_owned());
    if cc.is_empty() && reg.is_empty() && cty.is_empty() {
        return false;
    }
    let (fam, lo, hi, ok) = slot_range(bounds.0, bounds.1, b.opts.geo_v4_bits, b.opts.geo_v6_bits);
    if !ok {
        return false;
    }
    let key = GeoLocKey {
        cc_id: b.countries.id(&cc),
        reg_id: b.regions.id(&reg),
        city_id: b.cities.id(&cty),
    };
    let loc_id = if let Some(&id) = b.geo_loc_map.get(&key) {
        if let Some(slot) = b.geo_loc_freq.get_mut(id as usize) {
            *slot += 1;
        }
        id
    } else {
        let id = b.geo_loc_list.len() as u32;
        b.geo_loc_map.insert(key, id);
        b.geo_loc_list.push(key);
        b.geo_loc_freq.push(1);
        id
    };
    b.seq += 1;
    push_interval(b, fam, (lo, hi), loc_id, false, priority_bias);
    true
}

pub(crate) fn push_asn_route(b: &mut LiteDbBuilder, asn: u32, net_ip: IpAddr, bits: u8) {
    if !b.opts.include_asn_routes {
        return;
    }
    let rec = match net_ip {
        IpAddr::V4(v4) => {
            let mut raw = [0u8; 16];
            raw[..4].copy_from_slice(&v4.octets());
            AsnRoute {
                is_v6: false,
                prefix: raw,
                bits,
                key_hi: u64::from(u32::from(v4)),
                key_lo: 0,
            }
        }
        IpAddr::V6(v6) => {
            let val = u128::from(v6);
            AsnRoute {
                is_v6: true,
                prefix: v6.octets(),
                bits,
                key_hi: (val >> 64) as u64,
                key_lo: val as u64,
            }
        }
    };
    b.asn_routes.entry(asn).or_default().push(rec);
}

pub(crate) fn write_dicts(buf: &mut Vec<u8>, d: &CompactDicts) {
    buf.extend_from_slice(&(d.countries.len() as u16).to_le_bytes());
    for cc in &d.countries {
        buf.extend_from_slice(cc.as_bytes());
    }
    for dict in [
        &d.registries,
        &d.statuses,
        &d.sources,
        &d.regions,
        &d.cities,
    ] {
        write_uvarint(buf, dict.len() as u64);
        for s in dict {
            write_uvarint(buf, s.len() as u64);
            buf.extend_from_slice(s.as_bytes());
        }
    }
}

pub(crate) fn write_seg_sections(
    buf: &mut Vec<u8>,
    b: &crate::lite_db::LiteDbBuilder,
    input: &EncodeInput<'_>,
) {
    let bs = usize::from(DEFAULT_BLOCK_SIZE);
    let spec = |bits: u8, is_v6: bool| SegTableSpec {
        bits,
        block_size: bs,
        delta_shift: delta_shift(is_v6, bits),
    };
    let w_alloc = |d: &mut Vec<u8>, s: &Segment| {
        write_uvarint(d, u64::from(s.val >> 16));
        write_uvarint(d, u64::from(s.val & 0xffff));
    };
    encode_seg_table_shifted(
        buf,
        input.alloc_s4,
        spec(b.opts.alloc_v4_bits, false),
        w_alloc,
    );
    encode_seg_table_shifted(
        buf,
        input.alloc_s6,
        spec(b.opts.alloc_v6_bits, true),
        w_alloc,
    );
    let w_geo = |d: &mut Vec<u8>, s: &Segment| write_uvarint(d, u64::from(s.val));
    encode_seg_table_shifted(buf, input.geo_s4, spec(b.opts.geo_v4_bits, false), w_geo);
    encode_seg_table_shifted(buf, input.geo_s6, spec(b.opts.geo_v6_bits, true), w_geo);
    let w_route = |d: &mut Vec<u8>, s: &Segment| {
        d.push(s.aux);
        if s.aux > 0 {
            write_uvarint(d, u64::from(s.val));
        }
    };
    encode_seg_table_shifted(
        buf,
        &input.routes.segs4,
        spec(DEFAULT_ROUTE_V4_BITS, false),
        w_route,
    );
    encode_seg_table_shifted(
        buf,
        &input.routes.segs6,
        spec(DEFAULT_ROUTE_V6_BITS, true),
        w_route,
    );
}

pub(crate) fn write_asn_routes(
    buf: &mut Vec<u8>,
    opts: &LiteBuildOptions,
    asns: &[u32],
    asn_routes: &HashMap<u32, Vec<AsnRoute>>,
) -> Result<usize, String> {
    if !opts.include_asn_routes {
        return Ok(0);
    }
    let mut total = 0usize;
    write_chunked_records(buf, asns.len(), ASN_CHUNK_SIZE, |idx, data| {
        let Some(&asn) = asns.get(idx) else {
            return;
        };
        let dedup = dedup_sort_routes(asn_routes.get(&asn).map_or(&[], Vec::as_slice));
        total += dedup.len();
        for r in dedup {
            let nbytes = usize::from(r.bits).div_ceil(8);
            data.push(if r.is_v6 { 128 + r.bits } else { r.bits });
            if let Some(slice) = r.prefix.get(..nbytes) {
                data.extend_from_slice(slice);
            }
        }
    })?;
    Ok(total)
}

pub(crate) fn write_offset_data(buf: &mut Vec<u8>, offsets: &[u32], data: &[u8]) {
    for &off in offsets {
        buf.extend_from_slice(&off.to_le_bytes());
    }
    buf.extend_from_slice(data);
}

fn dedup_sort_routes(raw: &[AsnRoute]) -> Vec<AsnRoute> {
    let mut v = raw.to_vec();
    v.sort_by(|a, b| {
        a.is_v6
            .cmp(&b.is_v6)
            .then_with(|| a.key_hi.cmp(&b.key_hi))
            .then_with(|| a.key_lo.cmp(&b.key_lo))
            .then_with(|| a.bits.cmp(&b.bits))
    });
    v.dedup_by(|a, b| {
        a.is_v6 == b.is_v6 && a.key_hi == b.key_hi && a.key_lo == b.key_lo && a.bits == b.bits
    });
    v
}
