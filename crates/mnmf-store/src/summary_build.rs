use std::collections::HashMap;

use crate::codec::be_u32;
use crate::encode::{append_summary, sort_fixed_records};
use crate::index::SUMMARY_INDEX_STRIDE;
use crate::types::{Address, Facet, RangeSummary};

#[derive(Debug, Default)]
pub struct SummaryAcc {
    pub allocations: u64,
    pub routes: u64,
    pub countries: HashMap<String, u64>,
    pub asns: HashMap<String, u64>,
}

#[derive(Debug, Default)]
pub struct SummaryCollector {
    pub map: HashMap<(u32, u8), SummaryAcc>,
}

impl SummaryCollector {
    #[must_use]
    pub fn new() -> Self {
        Self {
            map: HashMap::with_capacity(65_793),
        }
    }

    pub fn collect(
        &mut self,
        start: Address,
        end: Address,
        version: u8,
        is_route: bool,
        val: &str,
    ) {
        if version != 4 {
            return;
        }
        let s = be_u32(start.0.get(12..16).unwrap_or(&[])).unwrap_or(0);
        let e = be_u32(end.0.get(12..16).unwrap_or(&[])).unwrap_or(0);
        let norm = val.trim().to_ascii_uppercase();
        for len in [0u8, 8, 16] {
            let shift = 32 - u32::from(len);
            let first = u64::from(s) >> shift;
            let last = u64::from(e) >> shift;
            for bucket in first..=last {
                let acc = self.map.entry((bucket as u32, len)).or_default();
                if is_route {
                    acc.routes += 1;
                    if !norm.is_empty() {
                        *acc.asns.entry(norm.clone()).or_insert(0) += 1;
                    }
                } else {
                    acc.allocations += 1;
                    if !norm.is_empty() {
                        *acc.countries.entry(norm.clone()).or_insert(0) += 1;
                    }
                }
            }
        }
    }

    #[must_use]
    pub fn build_sections(mut self) -> (Vec<u8>, Vec<u8>, u64) {
        let total = (1usize << 17) - 1;
        let mut index = Vec::with_capacity(total * SUMMARY_INDEX_STRIDE);
        let mut data = Vec::new();
        let mut count = 0u64;
        emit_recursive(&self.map, 0, 0, &mut data, &mut index, &mut count);
        self.map.clear();
        sort_fixed_records(&mut index, SUMMARY_INDEX_STRIDE);
        (data, index, count)
    }
}

fn emit_recursive(
    map: &HashMap<(u32, u8), SummaryAcc>,
    net: u32,
    len: u8,
    data: &mut Vec<u8>,
    index: &mut Vec<u8>,
    count: &mut u64,
) {
    let summary = summary_for_prefix(map, net, len);
    let off = data.len() as u64;
    append_summary(data, &summary);
    let rec_len = (data.len() as u64 - off) as u32;
    let addr = if len == 0 {
        0
    } else {
        net << (32 - u32::from(len))
    };
    index.extend_from_slice(&addr.to_be_bytes());
    index.push(len);
    index.extend_from_slice(&off.to_be_bytes());
    index.extend_from_slice(&rec_len.to_be_bytes());
    *count += 1;
    if len < 16 {
        let child = net << 1;
        emit_recursive(map, child, len + 1, data, index, count);
        emit_recursive(map, child | 1, len + 1, data, index, count);
    }
}

fn summary_for_prefix(map: &HashMap<(u32, u8), SummaryAcc>, net: u32, len: u8) -> RangeSummary {
    if matches!(len, 0 | 8 | 16) {
        return to_range_summary(map.get(&(net, len)), len);
    }
    let child_len = if len > 8 { 16u8 } else { 8u8 };
    let diff = u32::from(child_len - len);
    let first = net << diff;
    let cnt = 1u32 << diff;
    let mut merged = SummaryAcc::default();
    for child in first..first + cnt {
        if let Some(sub) = map.get(&(child, child_len)) {
            merge_acc(&mut merged, sub);
        }
    }
    to_range_summary(Some(&merged), len)
}

fn merge_acc(dst: &mut SummaryAcc, src: &SummaryAcc) {
    dst.allocations += src.allocations;
    dst.routes += src.routes;
    for (k, &v) in &src.countries {
        *dst.countries.entry(k.clone()).or_insert(0) += v;
    }
    for (k, &v) in &src.asns {
        *dst.asns.entry(k.clone()).or_insert(0) += v;
    }
}

fn to_range_summary(acc: Option<&SummaryAcc>, len: u8) -> RangeSummary {
    let Some(a) = acc else {
        return RangeSummary {
            prefix_length: len,
            ..Default::default()
        };
    };
    RangeSummary {
        prefix_length: len,
        allocation_records: a.allocations,
        routes: a.routes,
        countries: ranked_facets(&a.countries),
        asns: ranked_facets(&a.asns),
    }
}

fn ranked_facets(map: &HashMap<String, u64>) -> Vec<Facet> {
    let mut items: Vec<Facet> = map
        .iter()
        .map(|(k, &v)| Facet {
            value: k.clone(),
            count: v,
        })
        .collect();
    items.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.value.cmp(&b.value)));
    items.truncate(10);
    items
}
