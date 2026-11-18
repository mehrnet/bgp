use std::collections::HashMap;

use crate::types::{Address, Allocation, Autnum, Geofeed, RangeSummary, Route};

const MAX_POOL_STRINGS: usize = 1 << 20;

#[derive(Debug)]
pub struct StringPoolWriter {
    index: HashMap<String, u32>,
    pub(crate) offsets: Vec<u64>,
    pub(crate) data: Vec<u8>,
}

impl Default for StringPoolWriter {
    fn default() -> Self {
        Self::new()
    }
}

impl StringPoolWriter {
    #[must_use]
    pub fn new() -> Self {
        Self {
            index: HashMap::new(),
            offsets: vec![0],
            data: Vec::new(),
        }
    }

    pub fn clear_index(&mut self) {
        self.index.clear();
        self.index.shrink_to_fit();
    }

    pub fn encode_pooled(&mut self, dst: &mut Vec<u8>, val: &str) {
        if val.is_empty() {
            dst.push(0);
            return;
        }
        if let Some(&id) = self.index.get(val) {
            append_uvarint(dst, u64::from(id) + 2);
            return;
        }
        if self.index.len() < MAX_POOL_STRINGS {
            let id = (self.offsets.len() - 1) as u32;
            self.data.extend_from_slice(val.as_bytes());
            self.offsets.push(self.data.len() as u64);
            self.index.insert(val.to_owned(), id);
            append_uvarint(dst, u64::from(id) + 2);
            return;
        }
        dst.push(1);
        encode_inline(dst, val);
    }
}

pub fn append_uvarint(dst: &mut Vec<u8>, mut v: u64) {
    while v >= 0x80 {
        dst.push((v as u8) | 0x80);
        v >>= 7;
    }
    dst.push(v as u8);
}

pub fn encode_inline(dst: &mut Vec<u8>, val: &str) {
    append_uvarint(dst, val.len() as u64);
    dst.extend_from_slice(val.as_bytes());
}

fn encode_compact_addr(dst: &mut Vec<u8>, version: u8, addr: &Address) {
    dst.extend_from_slice(addr.version_slice(version));
}

pub fn append_allocation(dst: &mut Vec<u8>, pool: &mut StringPoolWriter, v: &Allocation) {
    dst.push(v.version);
    encode_compact_addr(dst, v.version, &v.start);
    encode_compact_addr(dst, v.version, &v.end);
    pool.encode_pooled(dst, &v.registry);
    pool.encode_pooled(dst, &v.country);
    encode_inline(dst, &v.name);
    pool.encode_pooled(dst, &v.allocation_date);
    pool.encode_pooled(dst, &v.status);
    pool.encode_pooled(dst, &v.abuse_contact);
    pool.encode_pooled(dst, &v.source);
    pool.encode_pooled(dst, &v.maintainers);
    pool.encode_pooled(dst, &v.organization);
    encode_inline(dst, &v.created);
    encode_inline(dst, &v.last_modified);
    encode_inline(dst, &v.description);
}

pub fn append_route(dst: &mut Vec<u8>, pool: &mut StringPoolWriter, v: &Route) {
    dst.push(v.version);
    dst.push(v.prefix_length);
    encode_compact_addr(dst, v.version, &v.network);
    append_uvarint(dst, u64::from(v.as_number));
    pool.encode_pooled(dst, &v.origin_asn);
    pool.encode_pooled(dst, &v.abuse_contact);
    pool.encode_pooled(dst, &v.registry);
    pool.encode_pooled(dst, &v.source);
    pool.encode_pooled(dst, &v.maintainers);
    pool.encode_pooled(dst, &v.organization);
    encode_inline(dst, &v.description);
}

pub fn append_geofeed(dst: &mut Vec<u8>, pool: &mut StringPoolWriter, v: &Geofeed) {
    dst.push(v.version);
    encode_compact_addr(dst, v.version, &v.start);
    encode_compact_addr(dst, v.version, &v.end);
    pool.encode_pooled(dst, &v.country);
    pool.encode_pooled(dst, &v.region);
    pool.encode_pooled(dst, &v.city);
    let flags = u8::from(v.latitude.is_some())
        | (u8::from(v.longitude.is_some()) << 1)
        | (u8::from(v.is_fallback) << 2);
    dst.push(flags);
    if let Some(lat) = v.latitude {
        dst.extend_from_slice(&lat.to_be_bytes());
    }
    if let Some(lon) = v.longitude {
        dst.extend_from_slice(&lon.to_be_bytes());
    }
}

pub fn append_autnum(dst: &mut Vec<u8>, pool: &mut StringPoolWriter, v: &Autnum) {
    append_uvarint(dst, u64::from(v.as_number));
    encode_inline(dst, &v.name);
    pool.encode_pooled(dst, &v.organization);
    pool.encode_pooled(dst, &v.asn);
    pool.encode_pooled(dst, &v.registry);
    pool.encode_pooled(dst, &v.country);
    pool.encode_pooled(dst, &v.status);
    pool.encode_pooled(dst, &v.source);
    pool.encode_pooled(dst, &v.maintainers);
    pool.encode_pooled(dst, &v.abuse_contact);
    encode_inline(dst, &v.created);
    encode_inline(dst, &v.last_modified);
    encode_inline(dst, &v.description);
}

pub fn append_summary(dst: &mut Vec<u8>, v: &RangeSummary) {
    dst.push(v.prefix_length);
    append_uvarint(dst, v.allocation_records);
    append_uvarint(dst, v.routes);
    for facets in [&v.countries, &v.asns] {
        append_uvarint(dst, facets.len() as u64);
        for f in facets {
            encode_inline(dst, &f.value);
            append_uvarint(dst, f.count);
        }
    }
}

pub fn append_prefix_entry(dst: &mut Vec<u8>, addr: &Address, version: u8, bits: u8, id: u32) {
    encode_compact_addr(dst, version, addr);
    dst.push(bits);
    dst.extend_from_slice(&id.to_be_bytes());
}

pub fn append_range_entry(dst: &mut Vec<u8>, start: &Address, end: &Address, version: u8, id: u32) {
    encode_compact_addr(dst, version, start);
    encode_compact_addr(dst, version, end);
    dst.extend_from_slice(&id.to_be_bytes());
}

pub fn sort_fixed_records(data: &mut [u8], stride: usize) {
    let count = data.len() / stride;
    if count <= 1 {
        return;
    }
    let mut indices: Vec<usize> = (0..count).collect();
    indices.sort_unstable_by(|&a, &b| {
        let sa = data.get(a * stride..(a + 1) * stride).unwrap_or(&[]);
        let sb = data.get(b * stride..(b + 1) * stride).unwrap_or(&[]);
        sa.cmp(sb)
    });
    let mut sorted = vec![0u8; data.len()];
    for (dst_i, &src_i) in indices.iter().enumerate() {
        if let (Some(dst), Some(src)) = (
            sorted.get_mut(dst_i * stride..(dst_i + 1) * stride),
            data.get(src_i * stride..(src_i + 1) * stride),
        ) {
            dst.copy_from_slice(src);
        }
    }
    data.copy_from_slice(&sorted);
}
