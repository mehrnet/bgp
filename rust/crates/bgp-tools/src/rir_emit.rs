use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter};
use std::net::Ipv4Addr;
use std::path::Path;

use bgp_compact::sortkey::{cidr_to_sort_keys, pad_sort_key_ip};

use crate::rir::RirWriters;

#[derive(Debug)]
pub struct RpslMeta {
    pub country: String,
    pub status: String,
    pub created: String,
    pub last_modified: String,
    pub source: String,
    pub mnt_by: String,
    pub org: String,
    pub abuse_contact: String,
    pub description: String,
}

pub(crate) fn write_alloc(
    csv_w: &mut csv::Writer<BufWriter<File>>,
    bounds: (&str, &str, u8),
    reg: &str,
    m: &RpslMeta,
    netname: &str,
    alloc_date: &str,
) {
    let (start, end, ver) = bounds;
    if start.is_empty() || end.is_empty() || (ver == 4 && is_iana_placeholder(start, end, netname))
    {
        return;
    }
    let ver_s = ver.to_string();
    let _ = csv_w.write_record([
        start,
        end,
        &ver_s,
        reg,
        &m.country,
        netname,
        &m.status,
        alloc_date,
        &m.created,
        &m.last_modified,
        &m.source,
        &m.mnt_by,
        &m.org,
        &m.abuse_contact,
        &m.description,
    ]);
}

pub fn is_iana_placeholder(start: &str, end: &str, netname: &str) -> bool {
    netname.trim().to_ascii_lowercase().starts_with("iana-")
        && start == pad_sort_key_ip(Ipv4Addr::UNSPECIFIED.into())
        && end == pad_sort_key_ip(Ipv4Addr::BROADCAST.into())
}

pub(crate) fn write_route(
    csv_w: &mut csv::Writer<BufWriter<File>>,
    cidr: &str,
    ver: u8,
    asn: &str,
    reg: &str,
    m: &RpslMeta,
) {
    if asn.is_empty() {
        return;
    }
    let Some((start, end, _)) = cidr_to_sort_keys(cidr) else {
        return;
    };
    let ver_s = ver.to_string();
    let _ = csv_w.write_record([
        &start,
        &end,
        &ver_s,
        cidr,
        &asn.to_ascii_uppercase(),
        reg,
        &m.source,
        &m.mnt_by,
        &m.org,
        &m.abuse_contact,
        &m.description,
    ]);
}

pub fn parse_delegated(path: &Path, registry: &str, w: &mut RirWriters) {
    let Ok(file) = File::open(path) else { return };
    for line in BufReader::with_capacity(256 * 1024, file)
        .lines()
        .map_while(Result::ok)
    {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let p: Vec<&str> = line.split('|').collect();
        let (Some(&ip_type), Some(&status)) = (p.get(2), p.get(6)) else {
            continue;
        };
        if status != "allocated" && status != "assigned" {
            continue;
        }
        let (cc, s_raw, val_s, date) = (
            p.get(1).copied().unwrap_or(""),
            p.get(3).copied().unwrap_or(""),
            p.get(4).copied().unwrap_or(""),
            p.get(5).copied().unwrap_or(""),
        );
        let bounds = match ip_type {
            "ipv4" => delegated_v4(s_raw, val_s).map(|(s, e)| (s, e, 4u8)),
            "ipv6" => cidr_to_sort_keys(&format!("{s_raw}/{val_s}")).map(|(s, e, _)| (s, e, 6u8)),
            _ => None,
        };
        if let Some((start, end, ver)) = bounds {
            let meta = RpslMeta {
                country: cc.to_ascii_uppercase(),
                status: status.to_owned(),
                created: String::new(),
                last_modified: String::new(),
                source: String::new(),
                mnt_by: String::new(),
                org: String::new(),
                abuse_contact: String::new(),
                description: String::new(),
            };
            write_alloc(
                &mut w.allocations,
                (&start, &end, ver),
                registry,
                &meta,
                "",
                date,
            );
        }
    }
}

fn delegated_v4(start_s: &str, count_s: &str) -> Option<(String, String)> {
    let ip: Ipv4Addr = start_s.parse().ok()?;
    let count: u32 = count_s.parse().ok().filter(|&c| c > 0)?;
    let end = Ipv4Addr::from(u32::from(ip).checked_add(count - 1)?);
    Some((pad_sort_key_ip(ip.into()), pad_sort_key_ip(end.into())))
}
