use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

const V4_MAPPED_OFFSET: u128 = 0xffffu128 << 32;

#[must_use]
pub fn unmap_ip(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(v6) => v6.to_ipv4_mapped().map_or(IpAddr::V6(v6), IpAddr::V4),
        IpAddr::V4(_) => ip,
    }
}

#[must_use]
pub fn pad_sort_key_ip(ip: IpAddr) -> String {
    match ip {
        IpAddr::V4(v4) => format!("{:039}", V4_MAPPED_OFFSET | u128::from(u32::from(v4))),
        IpAddr::V6(v6) => format!("{:039}", u128::from(v6)),
    }
}

#[must_use]
pub fn pad_sort_key_str(raw: &str) -> String {
    raw.trim()
        .parse::<IpAddr>()
        .map_or_else(|_| String::new(), pad_sort_key_ip)
}

#[must_use]
pub fn addr_from_sort_key(raw: &str, version: &str) -> Option<IpAddr> {
    let val: u128 = raw.trim().parse().ok()?;
    match version.trim() {
        "4" => Some(IpAddr::V4(Ipv4Addr::from((val & 0xffff_ffff) as u32))),
        "6" => Some(IpAddr::V6(Ipv6Addr::from(val))),
        _ => None,
    }
}

#[must_use]
pub fn cidr_to_bounds(raw: &str) -> Option<(u8, u128, u128, IpAddr, u8)> {
    let (ip_s, len_s) = raw.trim().split_once('/')?;
    let ip: IpAddr = ip_s.trim().parse().ok()?;
    let len: u8 = len_s.trim().parse().ok()?;
    match ip {
        IpAddr::V4(v4) if len <= 32 => {
            let mask = if len == 0 { 0u32 } else { !0u32 << (32 - len) };
            let net = u32::from(v4) & mask;
            let end = net | !mask;
            Some((
                4,
                u128::from(net),
                u128::from(end),
                IpAddr::V4(Ipv4Addr::from(net)),
                len,
            ))
        }
        IpAddr::V6(v6) if len <= 128 => {
            let mask = if len == 0 {
                0u128
            } else {
                !0u128 << (128 - len)
            };
            let net = u128::from(v6) & mask;
            let end = net | !mask;
            Some((6, net, end, IpAddr::V6(Ipv6Addr::from(net)), len))
        }
        _ => None,
    }
}

#[must_use]
pub fn cidr_to_sort_keys(raw: &str) -> Option<(String, String, u8)> {
    let (ver, start, end, net_ip, _) = cidr_to_bounds(raw)?;
    let end_ip = u128_to_ip(end, ver == 4);
    let start_key = if ver == 4 {
        format!("{:039}", V4_MAPPED_OFFSET | start)
    } else {
        pad_sort_key_ip(net_ip)
    };
    Some((start_key, pad_sort_key_ip(end_ip), ver))
}

#[must_use]
pub fn u128_to_ip(val: u128, is_v4: bool) -> IpAddr {
    if is_v4 {
        IpAddr::V4(Ipv4Addr::from(val as u32))
    } else {
        IpAddr::V6(Ipv6Addr::from(val))
    }
}

#[must_use]
pub fn slot_range(start: IpAddr, end: IpAddr, v4_bits: u8, v6_bits: u8) -> (u8, u64, u64, bool) {
    let (start, end) = (unmap_ip(start), unmap_ip(end));
    match (start, end) {
        (IpAddr::V4(a), IpAddr::V4(b)) if a <= b => {
            let shift = 32 - u32::from(v4_bits);
            let mask = if shift == 32 {
                u64::MAX
            } else {
                (1u64 << shift) - 1
            };
            let (s, e) = (u64::from(u32::from(a)), u64::from(u32::from(b)));
            if (s & mask) != 0 || (e & mask) != mask {
                (4, 0, 0, false)
            } else {
                (4, s >> shift, e >> shift, true)
            }
        }
        (IpAddr::V6(a), IpAddr::V6(b)) if a <= b => {
            let v = u128::from(a);
            let w = u128::from(b);
            let (s_hi, s_lo) = ((v >> 64) as u64, v as u64);
            let (e_hi, e_lo) = ((w >> 64) as u64, w as u64);
            let shift = 64 - u32::from(v6_bits);
            let mask = if shift == 64 {
                u64::MAX
            } else {
                (1u64 << shift) - 1
            };
            if s_lo != 0 || e_lo != u64::MAX || (s_hi & mask) != 0 || (e_hi & mask) != mask {
                (6, 0, 0, false)
            } else {
                (6, s_hi >> shift, e_hi >> shift, true)
            }
        }
        _ => (0, 0, 0, false),
    }
}

pub fn range_to_prefixes(
    start_key: &str,
    end_key: &str,
    version: u8,
) -> Result<Vec<(String, u8)>, String> {
    let bits: u32 = if version == 4 { 32 } else { 128 };
    let mask = if bits == 128 {
        u128::MAX
    } else {
        (1u128 << bits) - 1
    };
    let s = start_key
        .trim()
        .parse::<u128>()
        .map_err(|_| "invalid range start".to_owned())?
        & mask;
    let e = end_key
        .trim()
        .parse::<u128>()
        .map_err(|_| "invalid range end".to_owned())?
        & mask;
    if s > e {
        return Err("range start is greater than end".into());
    }
    Ok(range_to_cidr_bounds(s, e, bits)
        .into_iter()
        .map(|(net, len)| {
            let full = if version == 4 {
                V4_MAPPED_OFFSET | net
            } else {
                net
            };
            (format!("{version}:{full:039}/{len}"), len)
        })
        .collect())
}

#[must_use]
pub fn range_to_cidr_bounds(start: u128, end: u128, bits: u32) -> Vec<(u128, u8)> {
    let mut cur = start;
    let mut out = Vec::with_capacity(8);
    loop {
        let max_pow = if cur == 0 {
            bits
        } else {
            cur.trailing_zeros().min(bits)
        };
        let rem = end - cur;
        let rem_pow = if rem == u128::MAX {
            128
        } else {
            127 - (rem + 1).leading_zeros()
        };
        let step_pow = max_pow.min(rem_pow);
        let len = (bits - step_pow) as u8;
        out.push((cur, len));
        if step_pow == 128 || cur + ((1u128 << step_pow) - 1) == end {
            break;
        }
        cur += 1u128 << step_pow;
    }
    out
}

#[must_use]
pub fn parse_asn(raw: &str) -> u32 {
    let s = raw.trim().to_ascii_uppercase();
    let num = s.strip_prefix("AS").unwrap_or(&s);
    num.parse::<u32>().unwrap_or(0)
}

#[must_use]
pub fn normalize_country(raw: &str) -> Option<String> {
    let s = raw.trim().to_ascii_uppercase();
    if s.len() == 2 && s.bytes().all(|b| b.is_ascii_uppercase()) {
        Some(s)
    } else {
        None
    }
}
