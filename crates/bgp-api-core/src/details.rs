use std::borrow::Cow;

use mnmf_store::borrowed::{AllocationRef, GeofeedRef, RouteRef};
use mnmf_store::types::Address;

use crate::model::{AsnIdentity, LookupDetailRecord, LookupDetails, LookupResponse};

pub(crate) fn opt_trimmed(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_owned())
    }
}

pub(crate) fn cow_trimmed<'a>(value: Cow<'a, str>) -> Option<Cow<'a, str>> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(match value {
        Cow::Borrowed(s) => Cow::Borrowed(s.trim()),
        Cow::Owned(s) => Cow::Owned(s.trim().to_owned()),
    })
}

pub(crate) fn cow_lower<'a>(value: Cow<'a, str>) -> Option<Cow<'a, str>> {
    let c = cow_trimmed(value)?;
    if c.bytes().all(|b| !b.is_ascii_uppercase()) {
        Some(c)
    } else {
        Some(Cow::Owned(c.to_lowercase()))
    }
}

pub(crate) fn cow_upper<'a>(value: Cow<'a, str>) -> Option<Cow<'a, str>> {
    let c = cow_trimmed(value)?;
    if c.bytes().all(|b| !b.is_ascii_lowercase()) {
        Some(c)
    } else {
        Some(Cow::Owned(c.to_uppercase()))
    }
}

pub(crate) fn cow_country_code<'a>(raw_upper: &Option<Cow<'a, str>>) -> Option<Cow<'a, str>> {
    let code = raw_upper.as_ref()?;
    if code.len() == 2 && code.bytes().all(|b| b.is_ascii_uppercase()) {
        Some(code.clone())
    } else {
        None
    }
}

pub(crate) fn as_number_from_asn(asn: Option<&str>) -> Option<u32> {
    let num_str = asn?.strip_prefix("AS")?;
    let n: u32 = num_str.parse().ok()?;
    if n > 0 { Some(n) } else { None }
}

pub(crate) fn route_cidr(route: &RouteRef<'_>) -> String {
    format!(
        "{}/{}",
        route.network.to_ip(route.version),
        route.prefix_length
    )
}

pub(crate) fn route_bounds(route: &RouteRef<'_>) -> (Address, Address) {
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

pub(crate) fn build_details<'a>(
    allocs: Vec<AllocationRef<'a>>,
    routes: Vec<RouteRef<'a>>,
    geofeeds: Vec<GeofeedRef<'a>>,
) -> LookupDetails<'a> {
    LookupDetails {
        allocations: allocs.into_iter().map(alloc_detail).collect(),
        routes: routes.into_iter().map(route_detail).collect(),
        geofeeds: geofeeds.into_iter().map(geofeed_detail).collect(),
    }
}

fn alloc_detail(v: AllocationRef<'_>) -> LookupDetailRecord<'_> {
    let c_raw = cow_upper(v.country);
    LookupDetailRecord {
        start_ip: v.start.to_ip(v.version).to_string(),
        end_ip: v.end.to_ip(v.version).to_string(),
        version: v.version,
        registry: cow_lower(v.registry),
        country_code: cow_country_code(&c_raw),
        country_raw: c_raw,
        name: cow_trimmed(v.name),
        allocation_date: cow_trimmed(v.allocation_date),
        status: cow_trimmed(v.status),
        abuse_contact: cow_trimmed(v.abuse_contact),
        source: cow_trimmed(v.source),
        maintainers: cow_trimmed(v.maintainers),
        organization: cow_trimmed(v.organization),
        created: cow_trimmed(v.created),
        last_modified: cow_trimmed(v.last_modified),
        description: cow_trimmed(v.description),
        ..LookupDetailRecord::default()
    }
}

fn route_detail(v: RouteRef<'_>) -> LookupDetailRecord<'_> {
    let (start, end) = route_bounds(&v);
    let cidr = route_cidr(&v);
    let asn = cow_upper(v.origin_asn);
    LookupDetailRecord {
        start_ip: start.to_ip(v.version).to_string(),
        end_ip: end.to_ip(v.version).to_string(),
        version: v.version,
        cidr: Some(cidr),
        as_number: as_number_from_asn(asn.as_deref()),
        asn,
        registry: cow_lower(v.registry),
        source: cow_trimmed(v.source),
        maintainers: cow_trimmed(v.maintainers),
        organization: cow_trimmed(v.organization),
        abuse_contact: cow_trimmed(v.abuse_contact),
        description: cow_trimmed(v.description),
        ..LookupDetailRecord::default()
    }
}

fn geofeed_detail(v: GeofeedRef<'_>) -> LookupDetailRecord<'_> {
    let c_raw = cow_upper(v.country);
    LookupDetailRecord {
        start_ip: v.start.to_ip(v.version).to_string(),
        end_ip: v.end.to_ip(v.version).to_string(),
        version: v.version,
        country_code: cow_country_code(&c_raw),
        country_raw: c_raw,
        region: cow_trimmed(v.region),
        city: cow_trimmed(v.city),
        latitude: v.latitude,
        longitude: v.longitude,
        ..LookupDetailRecord::default()
    }
}

pub(crate) fn populate_network<'a>(
    resp: &mut LookupResponse<'a>,
    routes: &[RouteRef<'a>],
    asns: Vec<String>,
    origins: Vec<AsnIdentity<'a>>,
) {
    resp.network.asns = asns;
    resp.network.origins = origins;
    if resp.network.asns.len() == 1
        && let Some(single) = resp.network.asns.first()
    {
        resp.network.asn = Some(single.clone());
        resp.network.as_number = as_number_from_asn(Some(single));
    }
    if let Some(route) = routes.first() {
        resp.network.cidr = Some(route_cidr(route));
        let (start, end) = route_bounds(route);
        resp.network.start_ip = Some(start.to_ip(route.version).to_string());
        resp.network.end_ip = Some(end.to_ip(route.version).to_string());
        resp.network.abuse_contact = cow_trimmed(route.abuse_contact.clone());
    }
}

pub(crate) fn populate_allocation<'a>(
    resp: &mut LookupResponse<'a>,
    allocation: Option<AllocationRef<'a>>,
) {
    let Some(alloc) = allocation else { return };
    resp.allocation.start_ip = Some(alloc.start.to_ip(alloc.version).to_string());
    resp.allocation.end_ip = Some(alloc.end.to_ip(alloc.version).to_string());
    resp.allocation.registry = cow_lower(alloc.registry);
    resp.allocation.country_raw = cow_upper(alloc.country);
    resp.allocation.country_code = cow_country_code(&resp.allocation.country_raw);
    resp.allocation.name = cow_trimmed(alloc.name);
    resp.allocation.allocation_date = cow_trimmed(alloc.allocation_date);
    resp.allocation.status = cow_trimmed(alloc.status);
    resp.allocation.abuse_contact = cow_trimmed(alloc.abuse_contact);
    resp.registry = resp.allocation.registry.clone();
    resp.allocation_date = resp.allocation.allocation_date.clone();
    resp.allocation_status = resp.allocation.status.clone();
}

pub(crate) fn populate_location<'a>(
    resp: &mut LookupResponse<'a>,
    geofeed: Option<GeofeedRef<'a>>,
) {
    if let Some(geo) = geofeed {
        let country_raw = cow_upper(geo.country);
        resp.location.country_code = cow_country_code(&country_raw);
        resp.location.region = cow_trimmed(geo.region);
        resp.location.city = cow_trimmed(geo.city);
        resp.location.latitude = geo.latitude;
        resp.location.longitude = geo.longitude;
    }
    if resp.location.country_code.is_none() {
        resp.location.country_code = resp.allocation.country_code.clone();
    }
}
