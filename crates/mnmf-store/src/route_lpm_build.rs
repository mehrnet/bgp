use crate::lpm::addr_bit;
use crate::types::Address;

const ROUTE_MAGIC: &[u8; 4] = b"MLP2";

#[derive(Debug, Clone)]
struct RouteNode {
    prefix: Address,
    prefix_len: u8,
    left: u32,
    right: u32,
    ids: Vec<u32>,
}

impl RouteNode {
    fn set_child(&mut self, branch: usize, id: u32) {
        if branch == 0 {
            self.left = id;
        } else {
            self.right = id;
        }
    }
}

#[derive(Debug)]
pub struct RouteLpmBuilder {
    version: u8,
    nodes: Vec<RouteNode>,
}

impl RouteLpmBuilder {
    #[must_use]
    pub fn new(version: u8) -> Self {
        Self {
            version,
            nodes: vec![RouteNode {
                prefix: Address::default(),
                prefix_len: 0,
                left: 0,
                right: 0,
                ids: Vec::new(),
            }],
        }
    }

    pub fn insert(&mut self, net: Address, bits: u8, ids: Vec<u32>) {
        if !ids.is_empty() {
            let _ = self.insert_at(1, net, bits, ids);
        }
    }

    fn insert_at(&mut self, r: u32, pfx: Address, len: u8, ids: Vec<u32>) -> u32 {
        if r == 0 {
            self.nodes.push(RouteNode {
                prefix: mask_addr(pfx, self.version, len),
                prefix_len: len,
                left: 0,
                right: 0,
                ids,
            });
            return self.nodes.len() as u32;
        }
        let idx = (r - 1) as usize;
        let Some((npfx, nlen, left, right)) = self
            .nodes
            .get(idx)
            .map(|n| (n.prefix, n.prefix_len, n.left, n.right))
        else {
            return r;
        };
        let common = common_prefix(&npfx.0, nlen, &pfx.0, len, self.version);
        if common == nlen {
            if common == len {
                if let Some(slot) = self.nodes.get_mut(idx) {
                    slot.ids = ids;
                }
                return r;
            }
            let branch = addr_bit(&pfx.0, self.version, usize::from(common));
            let child = self.insert_at(if branch == 0 { left } else { right }, pfx, len, ids);
            if let Some(slot) = self.nodes.get_mut(idx) {
                slot.set_child(branch, child);
            }
            return r;
        }
        self.split(r, npfx, pfx, (len, common), ids)
    }

    fn split(
        &mut self,
        existing_ref: u32,
        npfx: Address,
        pfx: Address,
        lens: (u8, u8),
        ids: Vec<u32>,
    ) -> u32 {
        let (len, common) = lens;
        self.nodes.push(RouteNode {
            prefix: mask_addr(pfx, self.version, common),
            prefix_len: common,
            left: 0,
            right: 0,
            ids: Vec::new(),
        });
        let p_ref = self.nodes.len() as u32;
        let p_idx = (p_ref - 1) as usize;
        let ex_br = addr_bit(&npfx.0, self.version, usize::from(common));
        if let Some(p) = self.nodes.get_mut(p_idx) {
            p.set_child(ex_br, existing_ref);
        }
        if common == len {
            if let Some(p) = self.nodes.get_mut(p_idx) {
                p.ids = ids;
            }
            return p_ref;
        }
        let new_br = addr_bit(&pfx.0, self.version, usize::from(common));
        let child = self.insert_at(0, pfx, len, ids);
        if let Some(p) = self.nodes.get_mut(p_idx) {
            p.set_child(new_br, child);
        }
        p_ref
    }

    #[must_use]
    pub fn encode(&self) -> (Vec<u8>, u64, u64) {
        let total_ids: u64 = self.nodes.iter().map(|n| n.ids.len() as u64).sum();
        let mut out = Vec::with_capacity(16 + self.nodes.len() * 33 + (total_ids as usize) * 4);
        out.extend_from_slice(ROUTE_MAGIC);
        out.extend_from_slice(&[self.version, 0, 0, 0]);
        out.extend_from_slice(&(self.nodes.len() as u32).to_be_bytes());
        out.extend_from_slice(&(total_ids as u32).to_be_bytes());
        let mut id_off = 0u32;
        for node in &self.nodes {
            out.extend_from_slice(&node.prefix.0);
            out.push(node.prefix_len);
            out.extend_from_slice(&node.left.to_be_bytes());
            out.extend_from_slice(&node.right.to_be_bytes());
            out.extend_from_slice(&id_off.to_be_bytes());
            out.extend_from_slice(&(node.ids.len() as u32).to_be_bytes());
            id_off += node.ids.len() as u32;
        }
        for node in &self.nodes {
            for &id in &node.ids {
                out.extend_from_slice(&id.to_be_bytes());
            }
        }
        (out, self.nodes.len() as u64, total_ids)
    }
}

pub(crate) fn common_prefix(l: &[u8; 16], l_len: u8, r: &[u8; 16], r_len: u8, version: u8) -> u8 {
    let limit = usize::from(l_len.min(r_len));
    let off = if version == 4 { 12 } else { 0 };
    let full = limit / 8;
    for i in 0..full {
        let diff = l.get(off + i).copied().unwrap_or(0) ^ r.get(off + i).copied().unwrap_or(0);
        if diff != 0 {
            return (i * 8 + diff.leading_zeros() as usize) as u8;
        }
    }
    for bit in full * 8..limit {
        if addr_bit(l, version, bit) != addr_bit(r, version, bit) {
            return bit as u8;
        }
    }
    limit as u8
}

pub(crate) fn mask_addr(mut val: Address, version: u8, len: u8) -> Address {
    let (bits, off) = if version == 4 {
        (32usize, 12usize)
    } else {
        (128usize, 0usize)
    };
    let l = usize::from(len);
    if l >= bits {
        return val;
    }
    let mut full = l / 8;
    let rem = l % 8;
    if rem != 0 {
        if let Some(slot) = val.0.get_mut(off + full) {
            *slot &= 0xffu8 << (8 - rem);
        }
        full += 1;
    }
    for i in off + full..off + bits / 8 {
        if let Some(slot) = val.0.get_mut(i) {
            *slot = 0;
        }
    }
    val
}
