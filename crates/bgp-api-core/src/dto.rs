use mnmf_store::borrowed::{AllocationRef, AutnumRef, RouteRef};

use crate::details::{cow_country_code, cow_lower, cow_trimmed, cow_upper, route_cidr};
use crate::resource_model::{AllocationObject, AutnumObject, RouteObject};

pub(crate) fn allocation_object<'a>(id: u32, v: AllocationRef<'a>) -> AllocationObject<'a> {
    let c_raw = cow_upper(v.country);
    AllocationObject {
        id: i64::from(id),
        start_ip: v.start.to_ip(v.version).to_string(),
        end_ip: v.end.to_ip(v.version).to_string(),
        version: v.version,
        registry: cow_lower(v.registry),
        country_code: cow_country_code(&c_raw),
        country_raw: c_raw,
        name: cow_trimmed(v.name),
        status: cow_trimmed(v.status),
        allocation_date: cow_trimmed(v.allocation_date),
        created: cow_trimmed(v.created),
        last_modified: cow_trimmed(v.last_modified),
        source: cow_trimmed(v.source),
        maintainers: cow_trimmed(v.maintainers),
        organization: cow_trimmed(v.organization),
        abuse_contact: cow_trimmed(v.abuse_contact),
        description: cow_trimmed(v.description),
    }
}

pub(crate) fn route_object<'a>(id: u32, v: RouteRef<'a>) -> RouteObject<'a> {
    RouteObject {
        id: i64::from(id),
        prefix: route_cidr(&v),
        version: v.version,
        origin_asn: v.origin_asn,
        as_number: (v.as_number > 0).then_some(v.as_number),
        relation: None,
        registry: cow_lower(v.registry),
        source: cow_trimmed(v.source),
        maintainers: cow_trimmed(v.maintainers),
        organization: cow_trimmed(v.organization),
        abuse_contact: cow_trimmed(v.abuse_contact),
        description: cow_trimmed(v.description),
    }
}

pub(crate) fn autnum_object<'a>(v: AutnumRef<'a>) -> AutnumObject<'a> {
    let c_raw = cow_upper(v.country);
    AutnumObject {
        asn: v.asn,
        as_number: v.as_number,
        registry: cow_lower(v.registry),
        country_code: cow_country_code(&c_raw),
        country_raw: c_raw,
        name: cow_trimmed(v.name),
        organization: cow_trimmed(v.organization),
        status: cow_trimmed(v.status),
        created: cow_trimmed(v.created),
        last_modified: cow_trimmed(v.last_modified),
        source: cow_trimmed(v.source),
        maintainers: cow_trimmed(v.maintainers),
        abuse_contact: cow_trimmed(v.abuse_contact),
        description: cow_trimmed(v.description),
    }
}
