use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

const FULL_V6_COUNT: &str = "340282366920938463463374607431768211456";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeIp {
    pub ip: IpAddr,
    pub canonical: String,
    pub version: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedPrefix {
    pub canonical: String,
    pub version: u8,
    pub prefix_length: u8,
    pub start: IpAddr,
    pub end: IpAddr,
    pub address_count: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedRange {
    pub start: IpAddr,
    pub end: IpAddr,
    pub version: u8,
    pub address_count: String,
}

#[must_use]
pub fn parse_runtime_ip(input: &str) -> Option<RuntimeIp> {
    let trimmed = input.trim();
    if trimmed.contains('%') {
        return None;
    }
    let ip: IpAddr = trimmed.parse().ok()?;
    let version = if ip.is_ipv4() { 4 } else { 6 };
    Some(RuntimeIp {
        canonical: ip.to_string(),
        ip,
        version,
    })
}

#[must_use]
pub fn parse_prefix(input: &str) -> Option<ParsedPrefix> {
    let trimmed = input.trim();
    let (addr_str, len_str) = trimmed.split_once('/')?;
    if addr_str.contains('%') || len_str.is_empty() || !len_str.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    let ip: IpAddr = addr_str.parse().ok()?;
    let bits: u8 = len_str.parse().ok()?;
    let max_bits = if ip.is_ipv4() { 32u8 } else { 128u8 };
    if bits > max_bits {
        return None;
    }
    let (start, end, count_str) = masked_bounds(ip, bits);
    let version = if ip.is_ipv4() { 4 } else { 6 };
    Some(ParsedPrefix {
        canonical: format!("{start}/{bits}"),
        version,
        prefix_length: bits,
        start,
        end,
        address_count: count_str,
    })
}

#[must_use]
pub fn parse_range(start_input: &str, end_input: &str) -> Option<ParsedRange> {
    let start = parse_runtime_ip(start_input)?;
    let end = parse_runtime_ip(end_input)?;
    if start.version != end.version {
        return None;
    }
    let s_num = ip_to_u128(start.ip);
    let e_num = ip_to_u128(end.ip);
    if s_num > e_num {
        return None;
    }
    let diff = e_num - s_num;
    let address_count = match diff.checked_add(1) {
        Some(n) => n.to_string(),
        None => FULL_V6_COUNT.to_owned(),
    };
    Some(ParsedRange {
        start: start.ip,
        end: end.ip,
        version: start.version,
        address_count,
    })
}

#[must_use]
pub fn summary_prefix_key(range: &ParsedRange) -> Option<(Ipv4Addr, u8)> {
    if range.version != 4 {
        return None;
    }
    let prefix = prefix_for_range(range)?;
    if prefix.prefix_length > 16 {
        return None;
    }
    match prefix.start {
        IpAddr::V4(v4) => Some((v4, prefix.prefix_length)),
        IpAddr::V6(_) => None,
    }
}

#[must_use]
pub fn prefix_for_range(range: &ParsedRange) -> Option<ParsedPrefix> {
    let max_bits = if range.version == 4 { 32u32 } else { 128u32 };
    let s = ip_to_u128(range.start);
    let e = ip_to_u128(range.end);
    let diff = e - s;
    let Some(count) = diff.checked_add(1) else {
        return if s == 0 {
            parse_prefix(&format!("{}/0", range.start))
        } else {
            None
        };
    };
    if !count.is_power_of_two() {
        return None;
    }
    let mask_bits = count.trailing_zeros();
    let prefix_len = max_bits.checked_sub(mask_bits)?;
    if s & (count - 1) != 0 {
        return None;
    }
    parse_prefix(&format!("{}/{}", range.start, prefix_len))
}

fn masked_bounds(ip: IpAddr, bits: u8) -> (IpAddr, IpAddr, String) {
    match ip {
        IpAddr::V4(v4) => {
            let host_bits = 32 - u32::from(bits);
            let mask = if bits == 0 {
                0u32
            } else {
                u32::MAX << host_bits
            };
            let start = u32::from(v4) & mask;
            let end = start | !mask;
            let count = 1u64 << host_bits;
            (
                IpAddr::V4(Ipv4Addr::from(start)),
                IpAddr::V4(Ipv4Addr::from(end)),
                count.to_string(),
            )
        }
        IpAddr::V6(v6) => {
            let host_bits = 128 - u32::from(bits);
            let mask = if bits == 0 {
                0u128
            } else {
                u128::MAX << host_bits
            };
            let start = u128::from(v6) & mask;
            let end = start | !mask;
            let count = if bits == 0 {
                FULL_V6_COUNT.to_owned()
            } else {
                (1u128 << host_bits).to_string()
            };
            (
                IpAddr::V6(Ipv6Addr::from(start)),
                IpAddr::V6(Ipv6Addr::from(end)),
                count,
            )
        }
    }
}

fn ip_to_u128(ip: IpAddr) -> u128 {
    match ip {
        IpAddr::V4(v4) => u128::from(u32::from(v4)),
        IpAddr::V6(v6) => u128::from(v6),
    }
}
