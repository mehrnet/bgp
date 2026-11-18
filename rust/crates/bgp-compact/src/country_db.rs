use std::collections::HashMap;
use std::io::Write;
use std::net::IpAddr;

use crate::sortkey::{normalize_country, slot_range};
use crate::sweep::{Interval, Segment, encode_seg_table, flatten_intervals, write_uvarint};

pub const DEFAULT_V4_BITS: u8 = 24;
pub const DEFAULT_V6_BITS: u8 = 48;
pub const DEFAULT_BLOCK_SIZE: u8 = 64;
pub(crate) const MAX_V6_BITS: u8 = 62;
pub(crate) const MAGIC: &[u8; 4] = b"MNCC";
pub(crate) const FORMAT_VERSION: u8 = 1;
pub const LAYER_ALLOCATION: i64 = 1;
pub const LAYER_GEOFEED: i64 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddResult {
    Added,
    SkippedCountry,
    SkippedGranularity,
    SkippedAddress,
}

#[derive(Debug, Clone, Copy)]
pub struct CountryStats {
    pub v4_entries: usize,
    pub v6_entries: usize,
    pub countries: usize,
    pub bytes: usize,
}

#[derive(Debug)]
pub struct CountryDbBuilder {
    v4_bits: u8,
    v6_bits: u8,
    block_size: u8,
    v4: Vec<Interval>,
    v6: Vec<Interval>,
    seq: usize,
}

impl CountryDbBuilder {
    pub fn new(v4_bits: u8, v6_bits: u8) -> Result<Self, String> {
        if !(1..=32).contains(&v4_bits) {
            return Err(format!("v4 prefix must be 1..32, got {v4_bits}"));
        }
        if !(1..=MAX_V6_BITS).contains(&v6_bits) {
            return Err(format!("v6 prefix must be 1..{MAX_V6_BITS}, got {v6_bits}"));
        }
        Ok(Self {
            v4_bits,
            v6_bits,
            block_size: DEFAULT_BLOCK_SIZE,
            v4: Vec::new(),
            v6: Vec::new(),
            seq: 0,
        })
    }

    pub fn add(&mut self, start: IpAddr, end: IpAddr, country: &str, layer: i64) -> AddResult {
        let Some(cc) = pack_country(country) else {
            return AddResult::SkippedCountry;
        };
        let (family, lo, hi, ok) = slot_range(start, end, self.v4_bits, self.v6_bits);
        if family == 0 {
            return AddResult::SkippedAddress;
        }
        if !ok {
            return AddResult::SkippedGranularity;
        }
        self.seq += 1;
        let iv = Interval {
            start: lo,
            end: hi,
            val: u32::from(cc),
            aux: 0,
            priority: layer,
            seq: self.seq,
        };
        if family == 4 {
            self.v4.push(iv);
        } else {
            self.v6.push(iv);
        }
        AddResult::Added
    }

    pub fn encode(self, w: &mut impl Write) -> Result<CountryStats, std::io::Error> {
        let segs4 = flatten_intervals(self.v4, self.v4_bits, None);
        let segs6 = flatten_intervals(self.v6, self.v6_bits, None);
        let (codes, index) = rank_countries(&segs4, &segs6);
        let mut buf = Vec::new();
        buf.extend_from_slice(MAGIC);
        buf.extend_from_slice(&[FORMAT_VERSION, self.v4_bits, self.v6_bits, self.block_size]);
        buf.extend_from_slice(&(codes.len() as u16).to_le_bytes());
        for code in &codes {
            buf.push((code >> 8) as u8);
            buf.push(*code as u8);
        }
        let bs = usize::from(self.block_size);
        let write_cc = |d: &mut Vec<u8>, s: &Segment| {
            let id = index.get(&(s.val as u16)).copied().unwrap_or(0);
            write_uvarint(d, u64::from(id));
        };
        encode_seg_table(&mut buf, &segs4, self.v4_bits, bs, write_cc);
        encode_seg_table(&mut buf, &segs6, self.v6_bits, bs, write_cc);
        w.write_all(&buf)?;
        Ok(CountryStats {
            v4_entries: segs4.len(),
            v6_entries: segs6.len(),
            countries: codes.len(),
            bytes: buf.len(),
        })
    }
}

fn pack_country(raw: &str) -> Option<u16> {
    let s = normalize_country(raw)?;
    let b = s.as_bytes();
    Some((u16::from(*b.first()?) << 8) | u16::from(*b.get(1)?))
}

fn rank_countries(segs4: &[Segment], segs6: &[Segment]) -> (Vec<u16>, HashMap<u16, u16>) {
    let mut freq: HashMap<u16, usize> = HashMap::new();
    for s in segs4.iter().chain(segs6.iter()).filter(|s| s.val != 0) {
        *freq.entry(s.val as u16).or_insert(0) += 1;
    }
    let mut codes: Vec<u16> = freq.keys().copied().collect();
    codes.sort_by(|a, b| freq.get(b).cmp(&freq.get(a)).then_with(|| a.cmp(b)));
    let mut index = HashMap::with_capacity(codes.len() + 1);
    index.insert(0, 0);
    for (i, &code) in codes.iter().enumerate() {
        index.insert(code, (i + 1) as u16);
    }
    (codes, index)
}
