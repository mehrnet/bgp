use std::cmp::Ordering;
use std::net::IpAddr;

use crate::codec::be_u32;
use crate::types::{Address, StoreError};

const SELECTION_HEADER_SIZE: usize = 16;
const SELECTION_MAGIC: &[u8; 4] = b"SLP2";

fn selection_layout(version: u8) -> Result<(usize, usize), StoreError> {
    match version {
        4 => Ok((21, 4)),
        6 => Ok((45, 16)),
        _ => Err(StoreError::InvalidFormat(
            "invalid selection LPM version".into(),
        )),
    }
}

pub(crate) fn validate_selection_lpm(value: &[u8], version: u8) -> Result<(), StoreError> {
    let (node_size, _) = selection_layout(version)?;
    let header_magic = value
        .get(..4)
        .ok_or_else(|| StoreError::InvalidFormat("selection LPM header too small".into()))?;
    let hdr_ver = value
        .get(4)
        .copied()
        .ok_or_else(|| StoreError::InvalidFormat("selection LPM version missing".into()))?;
    if value.len() < SELECTION_HEADER_SIZE || header_magic != SELECTION_MAGIC || hdr_ver != version
    {
        return Err(StoreError::InvalidFormat(
            "invalid selection LPM header".into(),
        ));
    }
    let node_count = be_u32(
        value
            .get(8..12)
            .ok_or_else(|| StoreError::InvalidFormat("selection LPM node count missing".into()))?,
    )? as usize;
    if node_count == 0 {
        return Err(StoreError::InvalidFormat(
            "selection LPM has no root node".into(),
        ));
    }
    let expected = SELECTION_HEADER_SIZE.saturating_add(node_count.saturating_mul(node_size));
    if expected != value.len() {
        return Err(StoreError::InvalidFormat(
            "invalid selection LPM length".into(),
        ));
    }
    Ok(())
}

struct BestSelection {
    id: u32,
    prefix_len: u8,
    width: [u8; 16],
}

impl BestSelection {
    fn update(&mut self, width: &[u8], id: u32, prefix_len: u8, version: u8) {
        if !self.is_better(width, id, prefix_len, version) {
            return;
        }
        self.id = id;
        self.prefix_len = prefix_len;
        copy_addr_bytes(&mut self.width, width, version);
    }

    fn is_better(&self, width: &[u8], id: u32, prefix_len: u8, version: u8) -> bool {
        if self.id == 0 {
            return true;
        }
        let best_slice = if version == 4 {
            self.width.get(12..16).unwrap_or(&[])
        } else {
            &self.width[..]
        };
        match width.cmp(best_slice) {
            Ordering::Less => true,
            Ordering::Greater => false,
            Ordering::Equal if prefix_len != self.prefix_len => prefix_len > self.prefix_len,
            Ordering::Equal => id < self.id,
        }
    }
}

pub(crate) fn lookup_selection_lpm(value: &[u8], ip: IpAddr) -> Result<u32, StoreError> {
    let version = if ip.is_ipv4() { 4u8 } else { 6u8 };
    let (node_size, addr_bytes) = selection_layout(version)?;
    let node_count = be_u32(
        value
            .get(8..12)
            .ok_or_else(|| StoreError::InvalidFormat("selection LPM header missing".into()))?,
    )?;
    let query = Address::from_ip(ip).0;
    let max_bits = if version == 4 { 32 } else { 128 };
    let mut current = 0u32;
    let mut best = BestSelection {
        id: 0,
        prefix_len: 0,
        width: [0u8; 16],
    };

    let ctx = SelCtx {
        value,
        node_count,
        node_size,
        addr_bytes,
        version,
        max_bits,
        query: &query,
    };
    while let Some(next_node) = step_selection(&ctx, current, &mut best)? {
        current = next_node;
    }
    Ok(best.id)
}

struct SelCtx<'a> {
    value: &'a [u8],
    node_count: u32,
    node_size: usize,
    addr_bytes: usize,
    version: u8,
    max_bits: usize,
    query: &'a [u8; 16],
}

fn step_selection(
    c: &SelCtx<'_>,
    current: u32,
    best: &mut BestSelection,
) -> Result<Option<u32>, StoreError> {
    if current >= c.node_count {
        return Err(StoreError::InvalidFormat(
            "selection LPM child out of range".into(),
        ));
    }
    let off = SELECTION_HEADER_SIZE + (current as usize) * c.node_size;
    let node = c
        .value
        .get(off..off + c.node_size)
        .ok_or_else(|| StoreError::InvalidFormat("selection node truncated".into()))?;
    let prefix_len = *node
        .get(c.addr_bytes)
        .ok_or_else(|| StoreError::InvalidFormat("selection prefix len missing".into()))?;
    if usize::from(prefix_len) > c.max_bits
        || !prefix_matches(
            c.query,
            node.get(..c.addr_bytes).unwrap_or(&[]),
            c.version,
            usize::from(prefix_len),
        )
    {
        return Ok(None);
    }
    let cand_off = c.addr_bytes + 1 + 8;
    let cand_id = be_u32(node.get(cand_off..cand_off + 4).unwrap_or(&[]))?;
    if cand_id != 0 {
        let w = node
            .get(cand_off + 4..cand_off + 4 + c.addr_bytes)
            .unwrap_or(&[]);
        best.update(w, cand_id, prefix_len, c.version);
    }
    if usize::from(prefix_len) >= c.max_bits {
        return Ok(None);
    }
    let branch = addr_bit(c.query, c.version, usize::from(prefix_len));
    let child_off = c.addr_bytes + 1 + branch * 4;
    let next = be_u32(node.get(child_off..child_off + 4).unwrap_or(&[]))?;
    Ok((next != 0).then(|| next - 1))
}

pub(crate) fn addr_bit(query: &[u8; 16], version: u8, bit: usize) -> usize {
    let offset = if version == 4 { 12 } else { 0 };
    let byte = query.get(offset + bit / 8).copied().unwrap_or(0);
    usize::from((byte >> (7 - (bit % 8))) & 1)
}

pub(crate) fn prefix_matches(query: &[u8; 16], prefix: &[u8], version: u8, len: usize) -> bool {
    let offset = if version == 4 { 12 } else { 0 };
    let full = len / 8;
    let rem = len % 8;
    if query.get(offset..offset + full) != prefix.get(..full) {
        return false;
    }
    if rem == 0 {
        return true;
    }
    let mask = 0xffu8 << (8 - rem);
    let q_byte = query.get(offset + full).copied().unwrap_or(0);
    let p_byte = prefix.get(full).copied().unwrap_or(0);
    (q_byte & mask) == (p_byte & mask)
}

fn copy_addr_bytes(dst: &mut [u8; 16], src: &[u8], version: u8) {
    if version == 4 {
        if let (Some(d), Some(s)) = (dst.get_mut(12..16), src.get(..4)) {
            d.copy_from_slice(s);
        }
    } else if let Some(s) = src.get(..16) {
        dst.copy_from_slice(s);
    }
}
