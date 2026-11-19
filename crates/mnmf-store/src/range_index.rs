use std::cmp::Ordering;
use std::collections::BTreeSet;

use crate::codec::be_u32;
use crate::index::lower_bound_stride;
use crate::types::{Address, RANGE_KEY_SIZE, RangeIndexEntry, StoreError};

pub(crate) const RANGE_V4_STRIDE: usize = 12;
pub(crate) const RANGE_V6_STRIDE: usize = 36;

pub(crate) fn direct_range_entries(
    table: &[u8],
    version: u8,
    start: Address,
    end: Address,
    after: Option<&[u8; RANGE_KEY_SIZE]>,
    maximum: usize,
) -> Result<(Vec<RangeIndexEntry>, bool), StoreError> {
    let max_items = maximum.max(1);
    let (addr_bytes, stride) = if version == 4 {
        (4, RANGE_V4_STRIDE)
    } else {
        (16, RANGE_V6_STRIDE)
    };
    let (seek_start, seek_end, seek_id) = range_seek_bounds(version, &start, after);
    let limit_end = end.version_slice(version);
    let n = table.len() / stride;

    let mut pos = lower_bound_stride(table, stride, |row| {
        compare_range_row(row, addr_bytes, seek_start, seek_end, seek_id)
    })?;

    let mut entries = Vec::with_capacity(max_items);
    while pos < n {
        let row = table
            .get(pos * stride..(pos + 1) * stride)
            .ok_or_else(|| StoreError::InvalidFormat("range row truncated".into()))?;
        let r_start = row.get(..addr_bytes).unwrap_or(&[]);
        if r_start > limit_end {
            break;
        }
        let r_end = row.get(addr_bytes..2 * addr_bytes).unwrap_or(&[]);
        let id = be_u32(row.get(2 * addr_bytes..stride).unwrap_or(&[]))?;
        let mut key = [0u8; RANGE_KEY_SIZE];
        put_range_key(
            &mut key,
            version,
            addr_from_slice(r_start, version),
            addr_from_slice(r_end, version),
            id,
        );
        entries.push(RangeIndexEntry { id, key });
        if entries.len() == max_items {
            return Ok((entries, true));
        }
        pos += 1;
    }
    Ok((entries, false))
}

fn range_seek_bounds<'a>(
    version: u8,
    start: &'a Address,
    after: Option<&'a [u8; RANGE_KEY_SIZE]>,
) -> (&'a [u8], Option<&'a [u8]>, u32) {
    match after {
        Some(key) if version == 4 => (
            key.get(13..17).unwrap_or(&[]),
            Some(key.get(29..33).unwrap_or(&[])),
            be_u32(key.get(33..37).unwrap_or(&[])).unwrap_or(0),
        ),
        Some(key) => (
            key.get(1..17).unwrap_or(&[]),
            Some(key.get(17..33).unwrap_or(&[])),
            be_u32(key.get(33..37).unwrap_or(&[])).unwrap_or(0),
        ),
        None => (start.version_slice(version), None, 0),
    }
}

fn compare_range_row(
    row: &[u8],
    addr_bytes: usize,
    seek_start: &[u8],
    seek_end: Option<&[u8]>,
    seek_id: u32,
) -> Result<Ordering, StoreError> {
    let r_start = row.get(..addr_bytes).unwrap_or(&[]);
    let cmp_start = r_start.cmp(seek_start);
    if cmp_start != Ordering::Equal {
        return Ok(cmp_start);
    }
    let Some(end_bound) = seek_end else {
        return Ok(Ordering::Equal);
    };
    let r_end = row.get(addr_bytes..2 * addr_bytes).unwrap_or(&[]);
    let cmp_end = r_end.cmp(end_bound);
    if cmp_end != Ordering::Equal {
        return Ok(cmp_end);
    }
    let r_id = be_u32(row.get(2 * addr_bytes..2 * addr_bytes + 4).unwrap_or(&[]))?;
    Ok(if r_id > seek_id {
        Ordering::Greater
    } else {
        Ordering::Less
    })
}

pub fn put_range_key(
    key: &mut [u8; RANGE_KEY_SIZE],
    version: u8,
    start: Address,
    end: Address,
    id: u32,
) {
    if let Some(v) = key.get_mut(0) {
        *v = version;
    }
    if let Some(s) = key.get_mut(1..17) {
        s.copy_from_slice(&start.0);
    }
    if let Some(e) = key.get_mut(17..33) {
        e.copy_from_slice(&end.0);
    }
    if let Some(id_dst) = key.get_mut(33..37) {
        id_dst.copy_from_slice(&id.to_be_bytes());
    }
}

pub fn range_key_start_key(key: &mut [u8; RANGE_KEY_SIZE], version: u8, start: Address) {
    if let Some(v) = key.get_mut(0) {
        *v = version;
    }
    if let Some(s) = key.get_mut(1..17) {
        s.copy_from_slice(&start.0);
    }
    if let Some(rest) = key.get_mut(17..37) {
        rest.fill(0);
    }
}

fn addr_from_slice(src: &[u8], version: u8) -> Address {
    let mut out = [0u8; 16];
    if version == 4 {
        if let (Some(dst), Some(s)) = (out.get_mut(12..16), src.get(..4)) {
            dst.copy_from_slice(s);
        }
    } else if let Some(s) = src.get(..16) {
        out.copy_from_slice(s);
    }
    Address(out)
}

pub(crate) struct OverlapCollector {
    ids: BTreeSet<u32>,
    scanned: usize,
    scan_limit: usize,
    after: i64,
}

impl OverlapCollector {
    pub(crate) fn new(scan_limit: usize, after: i64) -> Self {
        Self {
            ids: BTreeSet::new(),
            scanned: 0,
            scan_limit,
            after,
        }
    }

    pub(crate) fn push_id(&mut self, id: u32) -> Result<(), StoreError> {
        self.scanned += 1;
        if self.scanned > self.scan_limit {
            return Err(StoreError::QueryTooBroad);
        }
        if i64::from(id) > self.after {
            self.ids.insert(id);
        }
        Ok(())
    }

    pub(crate) fn collect_key(
        &mut self,
        table: &[u8],
        mut pos: usize,
        key: &[u8],
    ) -> Result<(), StoreError> {
        let key_len = key.len();
        let stride = key_len + 4;
        let n = table.len() / stride;
        while pos < n {
            let row = table
                .get(pos * stride..(pos + 1) * stride)
                .ok_or_else(|| StoreError::InvalidFormat("prefix row truncated".into()))?;
            if row.get(..key_len) != Some(key) {
                break;
            }
            self.push_id(be_u32(row.get(key_len..stride).unwrap_or(&[]))?)?;
            pos += 1;
        }
        Ok(())
    }

    pub(crate) fn into_vec(self) -> Vec<u32> {
        self.ids.into_iter().collect()
    }
}
