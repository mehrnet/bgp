use std::io::Write;

use flate2::{Compression, write::DeflateEncoder};

use crate::sweep::{Segment, put_le_key, write_uvarint};

pub(crate) const ASN_CHUNK_SIZE: usize = 256;

#[derive(Debug, Clone, Copy)]
pub(crate) struct SegTableSpec {
    pub(crate) bits: u8,
    pub(crate) block_size: usize,
    pub(crate) delta_shift: u8,
}

#[must_use]
pub(crate) const fn delta_shift(is_v6: bool, bits: u8) -> u8 {
    if is_v6 && bits == 64 {
        16
    } else if !is_v6 && bits == 32 {
        8
    } else {
        0
    }
}

pub(crate) fn write_uvarint128(dst: &mut Vec<u8>, mut val: u128) {
    while val >= 0x80 {
        dst.push((val as u8) | 0x80);
        val >>= 7;
    }
    dst.push(val as u8);
}

fn write_shifted_delta(dst: &mut Vec<u8>, delta: u64, shift: u8) {
    if shift == 0 {
        write_uvarint(dst, delta);
        return;
    }
    let mask = (1u64 << shift) - 1;
    let enc = if (delta & mask) == 0 {
        u128::from(delta >> shift) << 1
    } else {
        (u128::from(delta) << 1) | 1
    };
    write_uvarint128(dst, enc);
}

pub(crate) fn encode_seg_table_shifted(
    buf: &mut Vec<u8>,
    segs: &[Segment],
    spec: SegTableSpec,
    mut write_payload: impl FnMut(&mut Vec<u8>, &Segment),
) {
    let kb = usize::from(spec.bits).div_ceil(8);
    let mut infos: Vec<(u64, u32)> = Vec::with_capacity(segs.len().div_ceil(spec.block_size));
    let mut data: Vec<u8> = Vec::new();
    let mut prev = 0u64;
    for (i, seg) in segs.iter().enumerate() {
        if i % spec.block_size == 0 {
            infos.push((seg.start, data.len() as u32));
            prev = seg.start;
        }
        write_shifted_delta(&mut data, seg.start - prev, spec.delta_shift);
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

pub(crate) fn write_chunked_records<F>(
    buf: &mut Vec<u8>,
    count: usize,
    chunk_size: usize,
    mut encode_item: F,
) -> Result<(), String>
where
    F: FnMut(usize, &mut Vec<u8>),
{
    buf.extend_from_slice(&(chunk_size as u16).to_le_bytes());
    let num_chunks = count.div_ceil(chunk_size);
    let mut chunk_offsets = Vec::with_capacity(num_chunks + 1);
    let mut compressed = Vec::new();
    for chunk_idx in 0..num_chunks {
        chunk_offsets.push(compressed.len() as u32);
        let start = chunk_idx * chunk_size;
        let end = (start + chunk_size).min(count);
        let bytes = compress_chunk(start, end, &mut encode_item)?;
        compressed.extend_from_slice(&bytes);
    }
    chunk_offsets.push(compressed.len() as u32);
    for off in chunk_offsets {
        buf.extend_from_slice(&off.to_le_bytes());
    }
    buf.extend_from_slice(&compressed);
    Ok(())
}

fn compress_chunk<F>(start: usize, end: usize, encode_item: &mut F) -> Result<Vec<u8>, String>
where
    F: FnMut(usize, &mut Vec<u8>),
{
    let n = end - start;
    let base = (n + 1) * 4;
    let mut offsets = Vec::with_capacity(n + 1);
    let mut body = Vec::new();
    for idx in start..end {
        offsets.push((base + body.len()) as u32);
        encode_item(idx, &mut body);
    }
    offsets.push((base + body.len()) as u32);

    let mut enc = DeflateEncoder::new(Vec::new(), Compression::best());
    for off in offsets {
        enc.write_all(&off.to_le_bytes())
            .map_err(|e| format!("deflate chunk offsets: {e}"))?;
    }
    enc.write_all(&body)
        .map_err(|e| format!("deflate chunk body: {e}"))?;
    enc.finish().map_err(|e| format!("finish chunk: {e}"))
}
