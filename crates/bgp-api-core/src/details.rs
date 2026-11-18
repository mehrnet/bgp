use mnmf_store::types::{Address, Allocation, Geofeed, Route};

use crate::model::{LookupDetailRecord, LookupDetails};

pub(crate) fn opt_trimmed(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_owned())
    }
}

pub(crate) fn opt_lower(value: &str) -> Option<String> {
    opt_trimmed(value).map(|s| s.to_lowercase())
}

pub(crate) fn opt_upper(value: &str) -> Option<String> {
    opt_trimmed(value).map(|s| s.to_uppercase())
}

pub(crate) fn country_code(raw_upper: Option<String>) -> Option<String> {
    let code = raw_upper?;
    if code.len() == 2 && code.bytes().all(|b| b.is_ascii_uppercase()) {
        Some(code)
    } else {
        None
    }
}

pub(crate) fn as_number_from_asn(asn: Option<&str>) -> Option<u32> {
    let num_str = asn?.strip_prefix("AS")?;
    let n: u32 = num_str.parse().ok()?;
    if n > 0 { Some(n) } else { None }
}

pub(crate) fn route_cidr(route: &Route) -> String {
    format!(
        "{}/{}",
        route.network.to_ip(route.version),
        route.prefix_length
    )
}

pub(crate) fn route_bounds(route: &Route) -> (Address, Address) {
    let start = route.network;
    let mut end = start.0;
    let (bits, offset) = if route.version == 4 {
        (32usize, 12usize)
    } else {
        (128usize, 0usize)
    };
    for bit in usize::from(route.prefix_length)..bits {
        if let Some(slot) = end.get_mut(offset + bit / 8) {
            *slot |= 1u8 << (7 - (bit % 8));
        }
    }
    (start, Address(end))
}

pub(crate) fn build_details(
    allocs: &[Allocation],
    routes: &[Route],
    geofeeds: &[Geofeed],
) -> LookupDetails {
    LookupDetails {
        allocations: allocs.iter().map(alloc_detail).collect(),
        routes: routes.iter().map(route_detail).collect(),
        geofeeds: geofeeds.iter().map(geofeed_detail).collect(),
    }
}

fn alloc_detail(v: &Allocation) -> LookupDetailRecord {
    let c_raw = opt_upper(&v.country);
    LookupDetailRecord {
        start_ip: v.start.to_ip(v.version).to_string(),
        end_ip: v.end.to_ip(v.version).to_string(),
        version: v.version,
        registry: opt_lower(&v.registry),
        country_code: country_code(c_raw.clone()),
        country_raw: c_raw,
        name: opt_trimmed(&v.name),
        allocation_date: opt_trimmed(&v.allocation_date),
        status: opt_trimmed(&v.status),
        abuse_contact: opt_trimmed(&v.abuse_contact),
        source: opt_trimmed(&v.source),
        maintainers: opt_trimmed(&v.maintainers),
        organization: opt_trimmed(&v.organization),
        created: opt_trimmed(&v.created),
        last_modified: opt_trimmed(&v.last_modified),
        description: opt_trimmed(&v.description),
        ..LookupDetailRecord::default()
    }
}

fn route_detail(v: &Route) -> LookupDetailRecord {
    let (start, end) = route_bounds(v);
    let asn = opt_upper(&v.origin_asn);
    LookupDetailRecord {
        start_ip: start.to_ip(v.version).to_string(),
        end_ip: end.to_ip(v.version).to_string(),
        version: v.version,
        cidr: opt_trimmed(&route_cidr(v)),
        as_number: as_number_from_asn(asn.as_deref()),
        asn,
        registry: opt_lower(&v.registry),
        source: opt_trimmed(&v.source),
        maintainers: opt_trimmed(&v.maintainers),
        organization: opt_trimmed(&v.organization),
        abuse_contact: opt_trimmed(&v.abuse_contact),
        description: opt_trimmed(&v.description),
        ..LookupDetailRecord::default()
    }
}

fn geofeed_detail(v: &Geofeed) -> LookupDetailRecord {
    let c_raw = opt_upper(&v.country);
    LookupDetailRecord {
        start_ip: v.start.to_ip(v.version).to_string(),
        end_ip: v.end.to_ip(v.version).to_string(),
        version: v.version,
        country_code: country_code(c_raw.clone()),
        country_raw: c_raw,
        region: opt_trimmed(&v.region),
        city: opt_trimmed(&v.city),
        ..LookupDetailRecord::default()
    }
}
