use std::cmp::Ordering;
use std::collections::BTreeSet;
use std::net::{IpAddr, Ipv4Addr};

use crate::codec::{be_u32, be_u64};
use crate::types::StoreError;

pub(crate) const AUTNUM_INDEX_STRIDE: usize = 16;
pub(crate) const ASN_ROUTE_INDEX_STRIDE: usize = 16;
pub(crate) const SUMMARY_INDEX_STRIDE: usize = 17;
pub(crate) const PREFIX_V4_STRIDE: usize = 9;
pub(crate) const PREFIX_V6_STRIDE: usize = 21;

pub(crate) fn lower_bound_stride<'a, F>(
    table: &'a [u8],
    stride: usize,
    mut cmp: F,
) -> Result<usize, StoreError>
where
    F: FnMut(&'a [u8]) -> Result<Ordering, StoreError>,
{
    let n = table.len() / stride;
    let (mut lo, mut hi) = (0usize, n);
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        let row = table
            .get(mid * stride..(mid + 1) * stride)
            .ok_or_else(|| StoreError::InvalidFormat("index entry out of bounds".into()))?;
        if cmp(row)? == Ordering::Less {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    Ok(lo)
}

pub(crate) fn find_exact_u32(
    table: &[u8],
    stride: usize,
    key: u32,
) -> Result<Option<&[u8]>, StoreError> {
    let pos = lower_bound_stride(table, stride, |row| {
        let candidate = be_u32(
            row.get(..4)
                .ok_or_else(|| StoreError::InvalidFormat("index key truncated".into()))?,
        )?;
        Ok(candidate.cmp(&key))
    })?;
    let Some(row) = table.get(pos * stride..(pos + 1) * stride) else {
        return Ok(None);
    };
    let candidate = be_u32(
        row.get(..4)
            .ok_or_else(|| StoreError::InvalidFormat("index key truncated".into()))?,
    )?;
    if candidate == key {
        Ok(Some(row))
    } else {
        Ok(None)
    }
}

pub(crate) fn find_summary_entry(
    table: &[u8],
    addr: Ipv4Addr,
    prefix_len: u8,
) -> Result<Option<(u64, u32)>, StoreError> {
    let octets = addr.octets();
    let target = [octets[0], octets[1], octets[2], octets[3], prefix_len];
    let pos = lower_bound_stride(table, SUMMARY_INDEX_STRIDE, |row| {
        let key = row
            .get(..5)
            .ok_or_else(|| StoreError::InvalidFormat("summary key truncated".into()))?;
        Ok(key.cmp(&target[..]))
    })?;
    let Some(row) = table.get(pos * SUMMARY_INDEX_STRIDE..(pos + 1) * SUMMARY_INDEX_STRIDE) else {
        return Ok(None);
    };
    if row.get(..5) != Some(&target[..]) {
        return Ok(None);
    }
    let off = be_u64(
        row.get(5..13)
            .ok_or_else(|| StoreError::InvalidFormat("summary offset truncated".into()))?,
    )?;
    let len = be_u32(
        row.get(13..17)
            .ok_or_else(|| StoreError::InvalidFormat("summary len truncated".into()))?,
    )?;
    Ok(Some((off, len)))
}

pub(crate) fn matching_prefix_ids(
    table: &[u8],
    ip: IpAddr,
    limit: usize,
) -> Result<Vec<u32>, StoreError> {
    let (addr_bytes, stride, max_bits) = prefix_layout(ip.is_ipv4());
    let key_len = addr_bytes + 1;
    let n = table.len() / stride;
    let mut ids = Vec::new();
    let mut target = [0u8; 17];

    for len in (0..=max_bits).rev() {
        if limit > 0 && ids.len() >= limit {
            break;
        }
        build_prefix_key(&mut target, addr_bytes, ip, len);
        let key = target.get(..key_len).unwrap_or(&[]);
        let mut pos = lower_bound_stride(table, stride, |row| {
            Ok(row.get(..key_len).unwrap_or(&[]).cmp(key))
        })?;
        while pos < n && (limit == 0 || ids.len() < limit) {
            let row = table
                .get(pos * stride..(pos + 1) * stride)
                .ok_or_else(|| StoreError::InvalidFormat("prefix row truncated".into()))?;
            if row.get(..key_len) != Some(key) {
                break;
            }
            ids.push(be_u32(row.get(key_len..stride).unwrap_or(&[]))?);
            pos += 1;
        }
    }
    Ok(ids)
}

pub(crate) fn overlap_route_ids(
    table: &[u8],
    start_ip: IpAddr,
    end_ip: IpAddr,
    after: i64,
    scan_limit: usize,
) -> Result<Vec<u32>, StoreError> {
    let (addr_bytes, stride, max_bits) = prefix_layout(start_ip.is_ipv4());
    let key_len = addr_bytes + 1;
    let mut collector = OverlapCollector {
        ids: BTreeSet::new(),
        scanned: 0,
        scan_limit,
        after,
    };
    let mut target = [0u8; 17];

    for len in (0..=max_bits).rev() {
        build_prefix_key(&mut target, addr_bytes, start_ip, len);
        let key = target.get(..key_len).unwrap_or(&[]);
        let pos = lower_bound_stride(table, stride, |row| {
            Ok(row.get(..key_len).unwrap_or(&[]).cmp(key))
        })?;
        collector.collect_key(table, pos, key)?;
    }

    let (mut start_bytes, mut end_bytes) = ([0u8; 16], [0u8; 16]);
    write_ip_bytes(start_bytes.get_mut(..addr_bytes), start_ip);
    write_ip_bytes(end_bytes.get_mut(..addr_bytes), end_ip);
    let start_s = start_bytes.get(..addr_bytes).unwrap_or(&[]);
    let end_s = end_bytes.get(..addr_bytes).unwrap_or(&[]);
    let mut pos = lower_bound_stride(table, stride, |row| {
        Ok(row.get(..addr_bytes).unwrap_or(&[]).cmp(start_s))
    })?;
    let n = table.len() / stride;
    while pos < n {
        let row = table
            .get(pos * stride..(pos + 1) * stride)
            .ok_or_else(|| StoreError::InvalidFormat("overlap row truncated".into()))?;
        if row.get(..addr_bytes).unwrap_or(&[]) > end_s {
            break;
        }
        collector.push_id(be_u32(row.get(key_len..stride).unwrap_or(&[]))?)?;
        pos += 1;
    }
    Ok(collector.ids.into_iter().collect())
}

struct OverlapCollector {
    ids: BTreeSet<u32>,
    scanned: usize,
    scan_limit: usize,
    after: i64,
}

impl OverlapCollector {
    fn push_id(&mut self, id: u32) -> Result<(), StoreError> {
        self.scanned += 1;
        if self.scanned > self.scan_limit {
            return Err(StoreError::QueryTooBroad);
        }
        if i64::from(id) > self.after {
            self.ids.insert(id);
        }
        Ok(())
    }

    fn collect_key(&mut self, table: &[u8], mut pos: usize, key: &[u8]) -> Result<(), StoreError> {
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
}

fn prefix_layout(is_v4: bool) -> (usize, usize, usize) {
    if is_v4 {
        (4, PREFIX_V4_STRIDE, 32)
    } else {
        (16, PREFIX_V6_STRIDE, 128)
    }
}

fn write_ip_bytes(dst: Option<&mut [u8]>, ip: IpAddr) {
    let Some(buf) = dst else { return };
    match ip {
        IpAddr::V4(v4) => buf.copy_from_slice(&v4.octets()),
        IpAddr::V6(v6) => buf.copy_from_slice(&v6.octets()),
    }
}

fn build_prefix_key(target: &mut [u8; 17], addr_bytes: usize, ip: IpAddr, bits_len: usize) {
    write_ip_bytes(target.get_mut(..addr_bytes), ip);
    if let Some(len_slot) = target.get_mut(addr_bytes) {
        *len_slot = bits_len as u8;
    }
    let total_bits = addr_bytes * 8;
    if bits_len >= total_bits {
        return;
    }
    let full = bits_len / 8;
    let rem = bits_len % 8;
    if let Some(slot) = target.get_mut(full) {
        if rem != 0 {
            *slot &= 0xffu8 << (8 - rem);
            if let Some(rest) = target.get_mut(full + 1..addr_bytes) {
                rest.fill(0);
            }
        } else if let Some(rest) = target.get_mut(full..addr_bytes) {
            rest.fill(0);
        }
    }
}
