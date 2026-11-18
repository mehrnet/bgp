use std::cmp::Ordering;

use crate::lpm::addr_bit;
use crate::route_lpm_build::{common_prefix, mask_addr};
use crate::types::Address;

const SELECTION_MAGIC: &[u8; 4] = b"SLP2";

#[derive(Debug, Clone, Copy)]
struct SelCand {
    id: u32,
    width: Address,
}

#[derive(Debug, Clone)]
struct SelNode {
    prefix: Address,
    prefix_len: u8,
    left: u32,
    right: u32,
    cand: SelCand,
}

impl SelNode {
    fn child(&self, branch: usize) -> u32 {
        if branch == 0 { self.left } else { self.right }
    }

    fn set_child(&mut self, branch: usize, id: u32) {
        if branch == 0 {
            self.left = id;
        } else {
            self.right = id;
        }
    }
}

#[derive(Debug)]
pub struct SelectionLpmBuilder {
    version: u8,
    nodes: Vec<SelNode>,
}

impl SelectionLpmBuilder {
    #[must_use]
    pub fn new(version: u8) -> Self {
        Self {
            version,
            nodes: vec![SelNode {
                prefix: Address::default(),
                prefix_len: 0,
                left: 0,
                right: 0,
                cand: SelCand {
                    id: 0,
                    width: Address::default(),
                },
            }],
        }
    }

    pub fn insert(&mut self, net: Address, bits: u8, id: u32, start: Address, end: Address) {
        let cand = SelCand {
            id,
            width: range_width(&start, &end),
        };
        let _ = self.insert_at(1, net, bits, cand);
    }

    fn insert_at(&mut self, r: u32, pfx: Address, len: u8, cand: SelCand) -> u32 {
        if r == 0 {
            self.nodes.push(SelNode {
                prefix: mask_addr(pfx, self.version, len),
                prefix_len: len,
                left: 0,
                right: 0,
                cand,
            });
            return self.nodes.len() as u32;
        }
        let idx = (r - 1) as usize;
        let Some((npfx, nlen, left, right, ncand)) = self
            .nodes
            .get(idx)
            .map(|n| (n.prefix, n.prefix_len, n.left, n.right, n.cand))
        else {
            return r;
        };
        let common = common_prefix(&npfx.0, nlen, &pfx.0, len, self.version);
        if common == nlen {
            if common == len {
                if (ncand.id == 0 || cand_less(&cand, &ncand))
                    && let Some(slot) = self.nodes.get_mut(idx)
                {
                    slot.cand = cand;
                }
                return r;
            }
            let branch = addr_bit(&pfx.0, self.version, usize::from(common));
            let child_ref = if branch == 0 { left } else { right };
            let child = self.insert_at(child_ref, pfx, len, cand);
            if let Some(slot) = self.nodes.get_mut(idx) {
                slot.set_child(branch, child);
            }
            return r;
        }
        self.split(r, npfx, pfx, (len, common), cand)
    }

    fn split(
        &mut self,
        existing_ref: u32,
        npfx: Address,
        pfx: Address,
        lens: (u8, u8),
        cand: SelCand,
    ) -> u32 {
        let (len, common) = lens;
        self.nodes.push(SelNode {
            prefix: mask_addr(pfx, self.version, common),
            prefix_len: common,
            left: 0,
            right: 0,
            cand: SelCand {
                id: 0,
                width: Address::default(),
            },
        });
        let p_ref = self.nodes.len() as u32;
        let p_idx = (p_ref - 1) as usize;
        let ex_br = addr_bit(&npfx.0, self.version, usize::from(common));
        if let Some(p) = self.nodes.get_mut(p_idx) {
            p.set_child(ex_br, existing_ref);
        }
        if common == len {
            if let Some(p) = self.nodes.get_mut(p_idx) {
                p.cand = cand;
            }
            return p_ref;
        }
        let new_br = addr_bit(&pfx.0, self.version, usize::from(common));
        let child = self.insert_at(0, pfx, len, cand);
        if let Some(p) = self.nodes.get_mut(p_idx) {
            p.set_child(new_br, child);
        }
        p_ref
    }

    #[must_use]
    pub fn encode(&self) -> (Vec<u8>, u64) {
        let node_sz = if self.version == 4 { 21usize } else { 45usize };
        let mut out = Vec::with_capacity(16 + self.nodes.len() * node_sz);
        out.extend_from_slice(SELECTION_MAGIC);
        out.extend_from_slice(&[self.version, 0, 0, 0]);
        out.extend_from_slice(&(self.nodes.len() as u32).to_be_bytes());
        out.extend_from_slice(&0u32.to_be_bytes());
        for node in &self.nodes {
            out.extend_from_slice(node.prefix.version_slice(self.version));
            out.push(node.prefix_len);
            out.extend_from_slice(&node.child(0).to_be_bytes());
            out.extend_from_slice(&node.child(1).to_be_bytes());
            out.extend_from_slice(&node.cand.id.to_be_bytes());
            out.extend_from_slice(node.cand.width.version_slice(self.version));
        }
        (out, self.nodes.len() as u64)
    }
}

fn cand_less(a: &SelCand, b: &SelCand) -> bool {
    match a.width.0.cmp(&b.width.0) {
        Ordering::Less => true,
        Ordering::Greater => false,
        Ordering::Equal => a.id < b.id,
    }
}

fn range_width(start: &Address, end: &Address) -> Address {
    let s = u128::from_be_bytes(start.0);
    let e = u128::from_be_bytes(end.0);
    Address(e.saturating_sub(s).to_be_bytes())
}
