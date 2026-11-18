use std::net::IpAddr;

use crate::country_db::{FORMAT_VERSION, MAGIC, MAX_V6_BITS};
use crate::sortkey::unmap_ip;
use crate::sweep::{get_le_key, read_uvarint};

#[derive(Debug)]
pub struct CountryDb<'a> {
    v4_bits: u8,
    v6_bits: u8,
    countries: &'a [u8],
    v4: FamilyView<'a>,
    v6: FamilyView<'a>,
}

#[derive(Debug)]
struct FamilyView<'a> {
    bits: u8,
    index: &'a [u8],
    data: &'a [u8],
}

impl<'a> CountryDb<'a> {
    pub fn open(data: &'a [u8]) -> Result<Self, &'static str> {
        if data.get(..4) != Some(MAGIC) || data.get(4) != Some(&FORMAT_VERSION) {
            return Err("invalid MNCC header");
        }
        let v4_bits = *data.get(5).ok_or("truncated header")?;
        let v6_bits = *data.get(6).ok_or("truncated header")?;
        let bs = *data.get(7).ok_or("truncated header")?;
        if !(1..=32).contains(&v4_bits) || !(1..=MAX_V6_BITS).contains(&v6_bits) || bs == 0 {
            return Err("invalid MNCC parameters");
        }
        let n = u16::from_le_bytes([
            *data.get(8).ok_or("truncated")?,
            *data.get(9).ok_or("truncated")?,
        ]) as usize;
        let countries = data.get(10..10 + 2 * n).ok_or("truncated countries")?;
        let (v4, pos) = open_family(data, 10 + 2 * n, v4_bits, usize::from(bs))?;
        let (v6, _) = open_family(data, pos, v6_bits, usize::from(bs))?;
        Ok(Self {
            v4_bits,
            v6_bits,
            countries,
            v4,
            v6,
        })
    }

    #[must_use]
    pub fn lookup(&self, addr: IpAddr) -> Option<&str> {
        match unmap_ip(addr) {
            IpAddr::V4(v4) => {
                let key = u64::from(u32::from(v4)) >> (32 - u32::from(self.v4_bits));
                self.lookup_family(&self.v4, key)
            }
            IpAddr::V6(v6) => {
                let hi = (u128::from(v6) >> 64) as u64;
                let key = hi >> (64 - u32::from(self.v6_bits));
                self.lookup_family(&self.v6, key)
            }
        }
    }

    fn lookup_family(&self, f: &FamilyView<'a>, key: u64) -> Option<&str> {
        let kb = usize::from(f.bits).div_ceil(8);
        let stride = kb + 4;
        let blocks = f.index.len() / stride;
        let b = find_block(f.index, stride, kb, blocks, key)?;
        let base = b * stride;
        let mut start = get_le_key(f.index.get(base..base + kb)?);
        let mut off = read_u32_le(f.index.get(base + kb..base + stride)?)? as usize;
        let end = if b + 1 < blocks {
            let next = (b + 1) * stride + kb;
            read_u32_le(f.index.get(next..next + 4)?)? as usize
        } else {
            f.data.len()
        };
        let mut country = 0u64;
        while off < end {
            let (delta, n) = read_uvarint(f.data.get(off..end)?)?;
            let (c, cn) = read_uvarint(f.data.get(off + n..end)?)?;
            start += delta;
            if start > key {
                break;
            }
            country = c;
            off += n + cn;
        }
        let idx = usize::try_from(country).ok()?.checked_sub(1)? * 2;
        std::str::from_utf8(self.countries.get(idx..idx + 2)?).ok()
    }
}

fn open_family(
    data: &[u8],
    pos: usize,
    bits: u8,
    bs: usize,
) -> Result<(FamilyView<'_>, usize), &'static str> {
    let entries =
        read_u32_le(data.get(pos..pos + 4).ok_or("truncated")?).ok_or("truncated")? as usize;
    let data_len =
        read_u32_le(data.get(pos + 4..pos + 8).ok_or("truncated")?).ok_or("truncated")? as usize;
    if entries == 0 {
        return Err("empty family");
    }
    let idx_len = entries.div_ceil(bs) * (usize::from(bits).div_ceil(8) + 4);
    let idx_start = pos + 8;
    let index = data
        .get(idx_start..idx_start + idx_len)
        .ok_or("truncated index")?;
    let fdata = data
        .get(idx_start + idx_len..idx_start + idx_len + data_len)
        .ok_or("truncated data")?;
    Ok((
        FamilyView {
            bits,
            index,
            data: fdata,
        },
        idx_start + idx_len + data_len,
    ))
}

fn find_block(index: &[u8], stride: usize, kb: usize, blocks: usize, key: u64) -> Option<usize> {
    let (mut lo, mut hi) = (0usize, blocks);
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        if index
            .get(mid * stride..mid * stride + kb)
            .is_some_and(|s| get_le_key(s) <= key)
        {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    lo.checked_sub(1)
}

fn read_u32_le(slice: &[u8]) -> Option<u32> {
    Some(u32::from_le_bytes(slice.get(..4)?.try_into().ok()?))
}
