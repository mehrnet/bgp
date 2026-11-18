use std::net::IpAddr;

use crate::codec::be_u32;
use crate::lpm::{addr_bit, prefix_matches};
use crate::types::{Address, StoreError};

const ROUTE_HEADER_SIZE: usize = 16;
const ROUTE_NODE_SIZE: usize = 33;
const ROUTE_MAGIC: &[u8; 4] = b"MLP2";

pub(crate) fn validate_route_lpm(value: &[u8], version: u8) -> Result<(), StoreError> {
    let header_magic = value
        .get(..4)
        .ok_or_else(|| StoreError::InvalidFormat("route LPM header too small".into()))?;
    let hdr_ver = value
        .get(4)
        .copied()
        .ok_or_else(|| StoreError::InvalidFormat("route LPM version missing".into()))?;
    if value.len() < ROUTE_HEADER_SIZE || header_magic != ROUTE_MAGIC || hdr_ver != version {
        return Err(StoreError::InvalidFormat("invalid route LPM header".into()));
    }
    let node_count = be_u32(
        value
            .get(8..12)
            .ok_or_else(|| StoreError::InvalidFormat("route LPM node count missing".into()))?,
    )? as usize;
    let id_count = be_u32(
        value
            .get(12..16)
            .ok_or_else(|| StoreError::InvalidFormat("route LPM ID count missing".into()))?,
    )? as usize;
    if node_count == 0 {
        return Err(StoreError::InvalidFormat(
            "route LPM has no root node".into(),
        ));
    }
    let expected = ROUTE_HEADER_SIZE
        .saturating_add(node_count.saturating_mul(ROUTE_NODE_SIZE))
        .saturating_add(id_count.saturating_mul(4));
    if expected != value.len() {
        return Err(StoreError::InvalidFormat("invalid route LPM length".into()));
    }
    Ok(())
}

pub(crate) fn lookup_route_lpm(value: &[u8], ip: IpAddr) -> Result<Vec<u32>, StoreError> {
    let version = if ip.is_ipv4() { 4u8 } else { 6u8 };
    let node_count = be_u32(
        value
            .get(8..12)
            .ok_or_else(|| StoreError::InvalidFormat("route LPM node count missing".into()))?,
    )?;
    let id_count = be_u32(
        value
            .get(12..16)
            .ok_or_else(|| StoreError::InvalidFormat("route LPM ID count missing".into()))?,
    )?;
    let query = Address::from_ip(ip).0;
    let max_bits = if version == 4 { 32 } else { 128 };
    let mut best = (0u32, 0u32);
    let mut current = 0u32;

    let ctx = RouteCtx {
        value,
        node_count,
        version,
        max_bits,
        query: &query,
    };
    while let Some(next) = step_route(&ctx, current, &mut best)? {
        current = next;
    }
    extract_route_ids(value, node_count, id_count, best.0, best.1)
}

struct RouteCtx<'a> {
    value: &'a [u8],
    node_count: u32,
    version: u8,
    max_bits: usize,
    query: &'a [u8; 16],
}

fn step_route(
    c: &RouteCtx<'_>,
    current: u32,
    best: &mut (u32, u32),
) -> Result<Option<u32>, StoreError> {
    if current >= c.node_count {
        return Err(StoreError::InvalidFormat(
            "route LPM child out of range".into(),
        ));
    }
    let off = ROUTE_HEADER_SIZE + (current as usize) * ROUTE_NODE_SIZE;
    let node = c
        .value
        .get(off..off + ROUTE_NODE_SIZE)
        .ok_or_else(|| StoreError::InvalidFormat("route LPM node truncated".into()))?;
    let prefix_len = *node
        .get(16)
        .ok_or_else(|| StoreError::InvalidFormat("route LPM prefix len missing".into()))?;
    let pfx = node
        .get(if c.version == 4 { 12..16 } else { 0..16 })
        .unwrap_or(&[]);
    if usize::from(prefix_len) > c.max_bits
        || !prefix_matches(c.query, pfx, c.version, usize::from(prefix_len))
    {
        return Ok(None);
    }
    let count = be_u32(node.get(29..33).unwrap_or(&[]))?;
    if count > 0 {
        *best = (be_u32(node.get(25..29).unwrap_or(&[]))?, count);
    }
    if usize::from(prefix_len) >= c.max_bits {
        return Ok(None);
    }
    let branch = addr_bit(c.query, c.version, usize::from(prefix_len));
    let next = be_u32(node.get(17 + branch * 4..21 + branch * 4).unwrap_or(&[]))?;
    Ok((next != 0).then(|| next - 1))
}

fn extract_route_ids(
    value: &[u8],
    node_count: u32,
    id_count: u32,
    best_off: u32,
    best_cnt: u32,
) -> Result<Vec<u32>, StoreError> {
    if best_cnt == 0 {
        return Ok(Vec::new());
    }
    if u64::from(best_off) + u64::from(best_cnt) > u64::from(id_count) {
        return Err(StoreError::InvalidFormat(
            "route LPM ID slice out of bounds".into(),
        ));
    }
    let ids_base =
        ROUTE_HEADER_SIZE + (node_count as usize) * ROUTE_NODE_SIZE + (best_off as usize) * 4;
    let mut out = Vec::with_capacity(best_cnt as usize);
    for idx in 0..best_cnt as usize {
        let pos = ids_base + idx * 4;
        out.push(be_u32(value.get(pos..pos + 4).unwrap_or(&[]))?);
    }
    Ok(out)
}
