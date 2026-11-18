use std::cmp::Ordering;
use std::collections::BinaryHeap;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct Interval {
    pub start: u64,
    pub end: u64,
    pub val: u32,
    pub aux: u8,
    pub priority: i64,
    pub seq: usize,
}

impl Ord for Interval {
    fn cmp(&self, other: &Self) -> Ordering {
        self.priority
            .cmp(&other.priority)
            .then_with(|| (other.end - other.start).cmp(&(self.end - self.start)))
            .then_with(|| self.seq.cmp(&other.seq))
    }
}

impl PartialOrd for Interval {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Segment {
    pub start: u64,
    pub val: u32,
    pub aux: u8,
}

fn same_segment(
    a: &Segment,
    b: &Segment,
    can_merge: Option<&dyn Fn(u64, u64, u8) -> bool>,
) -> bool {
    if a.val != b.val || a.aux != b.aux {
        return false;
    }
    if a.val == 0 {
        return true;
    }
    can_merge.is_none_or(|f| f(a.start, b.start, a.aux))
}

fn emit_segment(
    out: &mut Vec<Segment>,
    limit: Option<u64>,
    cand: Segment,
    can_merge: Option<&dyn Fn(u64, u64, u8) -> bool>,
) {
    if limit.is_some_and(|l| cand.start >= l) {
        return;
    }
    let Some(last) = out.last().copied() else {
        out.push(cand);
        return;
    };
    if same_segment(&last, &cand, can_merge) {
        return;
    }
    if last.start == cand.start {
        if let Some(slot) = out.last_mut() {
            *slot = cand;
        }
        let len = out.len();
        if len > 1
            && out
                .get(len - 2)
                .is_some_and(|prev| same_segment(prev, &cand, can_merge))
        {
            out.pop();
        }
        return;
    }
    out.push(cand);
}

#[must_use]
pub fn flatten_intervals(
    mut ivs: Vec<Interval>,
    bits: u8,
    can_merge: Option<&dyn Fn(u64, u64, u8) -> bool>,
) -> Vec<Segment> {
    let limit = (bits < 64).then(|| 1u64 << bits);
    ivs.sort_by(|a, b| a.start.cmp(&b.start).then_with(|| a.seq.cmp(&b.seq)));
    let mut out = vec![Segment {
        start: 0,
        val: 0,
        aux: 0,
    }];
    let mut active: BinaryHeap<Interval> = BinaryHeap::new();
    let mut i = 0usize;
    let mut p = 0u64;
    while i < ivs.len() || !active.is_empty() {
        p = advance_position(&ivs, &mut i, &mut active, p);
        let Some(&top) = active.peek() else {
            emit_segment(
                &mut out,
                limit,
                Segment {
                    start: p,
                    val: 0,
                    aux: 0,
                },
                can_merge,
            );
            continue;
        };
        emit_segment(
            &mut out,
            limit,
            Segment {
                start: p,
                val: top.val,
                aux: top.aux,
            },
            can_merge,
        );
        let next = next_sweep_pos(&top, ivs.get(i).map(|x| x.start));
        let Some(np) = next.filter(|&n| limit.is_none_or(|l| n < l)) else {
            break;
        };
        p = np;
    }
    out
}

fn advance_position(
    ivs: &[Interval],
    i: &mut usize,
    active: &mut BinaryHeap<Interval>,
    mut p: u64,
) -> u64 {
    if active.is_empty()
        && let Some(iv) = ivs.get(*i)
    {
        p = iv.start;
    }
    while let Some(iv) = ivs.get(*i).filter(|iv| iv.start <= p) {
        active.push(*iv);
        *i += 1;
    }
    while active.peek().is_some_and(|t| t.end < p) {
        active.pop();
    }
    p
}

fn next_sweep_pos(top: &Interval, next_start: Option<u64>) -> Option<u64> {
    if top.end == u64::MAX {
        return next_start;
    }
    let after = top.end + 1;
    Some(next_start.map_or(after, |ns| ns.min(after)))
}

#[must_use]
pub fn same_cidr_block(bits: u8) -> Box<dyn Fn(u64, u64, u8) -> bool> {
    Box::new(move |prev_start, curr_start, prefix_len| {
        if prefix_len == 0 {
            return true;
        }
        let shift = u32::from(bits.saturating_sub(prefix_len));
        if shift >= 64 {
            return true;
        }
        (prev_start >> shift) == (curr_start >> shift)
    })
}

pub fn write_uvarint(dst: &mut Vec<u8>, mut val: u64) {
    while val >= 0x80 {
        dst.push((val as u8) | 0x80);
        val >>= 7;
    }
    dst.push(val as u8);
}

#[must_use]
pub fn read_uvarint(src: &[u8]) -> Option<(u64, usize)> {
    let mut x = 0u64;
    let mut s = 0u32;
    for (i, &b) in src.iter().take(10).enumerate() {
        if b < 0x80 {
            if i == 9 && b > 1 {
                return None;
            }
            return Some((x | (u64::from(b) << s), i + 1));
        }
        x |= u64::from(b & 0x7f) << s;
        s += 7;
    }
    None
}

pub fn put_le_key(dst: &mut Vec<u8>, val: u64, kb: usize) {
    for j in 0..kb {
        dst.push((val >> (8 * j)) as u8);
    }
}

#[must_use]
pub fn get_le_key(src: &[u8]) -> u64 {
    let mut v = 0u64;
    for &b in src.iter().rev() {
        v = (v << 8) | u64::from(b);
    }
    v
}

pub fn encode_seg_table(
    buf: &mut Vec<u8>,
    segs: &[Segment],
    bits: u8,
    block_size: usize,
    mut write_payload: impl FnMut(&mut Vec<u8>, &Segment),
) {
    let kb = usize::from(bits).div_ceil(8);
    let mut infos: Vec<(u64, u32)> = Vec::with_capacity(segs.len().div_ceil(block_size));
    let mut data: Vec<u8> = Vec::new();
    let mut prev = 0u64;
    for (i, seg) in segs.iter().enumerate() {
        if i % block_size == 0 {
            infos.push((seg.start, data.len() as u32));
            prev = seg.start;
        }
        write_uvarint(&mut data, seg.start - prev);
        write_payload(&mut data, seg);
        prev = seg.start;
    }
    buf.extend_from_slice(&(segs.len() as u32).to_le_bytes());
    buf.extend_from_slice(&(data.len() as u32).to_le_bytes());
    for (key, off) in infos {
        put_le_key(buf, key, kb);
        buf.extend_from_slice(&off.to_le_bytes());
    }
    buf.extend_from_slice(&data);
}
