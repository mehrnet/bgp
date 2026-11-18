use std::net::IpAddr;

use crate::lite_chunk::{SegTableSpec, delta_shift, encode_seg_table_shifted};
use crate::lite_db::LiteDbBuilder;
use crate::lite_routes::CompactDicts;
use crate::lite_types::{
    AsnRoute, DEFAULT_BLOCK_SIZE, DEFAULT_ROUTE_V4_BITS, DEFAULT_ROUTE_V6_BITS, EncodeInput,
    GeoLocKey,
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

pub(crate) fn encode_coords(coords: Option<(f64, f64)>) -> Option<(i32, i32)> {
    let (lat, lon) = coords?;
    if !lat.is_finite()
        || !lon.is_finite()
        || !(-90.0..=90.0).contains(&lat)
        || !(-180.0..=180.0).contains(&lon)
    {
        return None;
    }
    Some((
        (lat * 1_000_000.0).round() as i32,
        (lon * 1_000_000.0).round() as i32,
    ))
}

pub(crate) fn insert_geofeed(
    b: &mut LiteDbBuilder,
    bounds: (IpAddr, IpAddr),
    fields: (&str, &str, &str),
    coords: Option<(f64, f64)>,
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
        coords: encode_coords(coords),
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
        buf.extend_from_slice(&(dict.len() as u32).to_le_bytes());
        let mut data = Vec::new();
        let mut offsets = Vec::with_capacity(dict.len() + 1);
        for s in dict {
            offsets.push(data.len() as u32);
            data.extend_from_slice(s.as_bytes());
        }
        offsets.push(data.len() as u32);
        write_offset_data(buf, &offsets, &data);
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

pub(crate) fn write_offset_data(buf: &mut Vec<u8>, offsets: &[u32], data: &[u8]) {
    for &off in offsets {
        buf.extend_from_slice(&off.to_le_bytes());
    }
    buf.extend_from_slice(data);
}
