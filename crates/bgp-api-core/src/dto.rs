use mnmf_store::types::{Allocation, Autnum, Route};

use crate::details::{country_code, opt_lower, opt_trimmed, opt_upper, route_cidr};
use crate::resource_model::{AllocationObject, AutnumObject, RouteObject};

pub(crate) fn allocation_object(id: u32, v: Allocation) -> AllocationObject {
    let c_raw = opt_upper(&v.country);
    AllocationObject {
        id: i64::from(id),
        start_ip: v.start.to_ip(v.version).to_string(),
        end_ip: v.end.to_ip(v.version).to_string(),
        version: v.version,
        registry: opt_lower(&v.registry),
        country_code: country_code(c_raw.clone()),
        country_raw: c_raw,
        name: opt_trimmed(&v.name),
        status: opt_trimmed(&v.status),
        allocation_date: opt_trimmed(&v.allocation_date),
        created: opt_trimmed(&v.created),
        last_modified: opt_trimmed(&v.last_modified),
        source: opt_trimmed(&v.source),
        maintainers: opt_trimmed(&v.maintainers),
        organization: opt_trimmed(&v.organization),
        abuse_contact: opt_trimmed(&v.abuse_contact),
        description: opt_trimmed(&v.description),
    }
}

pub(crate) fn route_object(id: u32, v: Route) -> RouteObject {
    RouteObject {
        id: i64::from(id),
        prefix: route_cidr(&v),
        version: v.version,
        origin_asn: v.origin_asn,
        as_number: (v.as_number > 0).then_some(v.as_number),
        relation: None,
        registry: opt_lower(&v.registry),
        source: opt_trimmed(&v.source),
        maintainers: opt_trimmed(&v.maintainers),
        organization: opt_trimmed(&v.organization),
        abuse_contact: opt_trimmed(&v.abuse_contact),
        description: opt_trimmed(&v.description),
    }
}

pub(crate) fn autnum_object(v: Autnum) -> AutnumObject {
    let c_raw = opt_upper(&v.country);
    AutnumObject {
        asn: v.asn,
        as_number: v.as_number,
        registry: opt_lower(&v.registry),
        country_code: country_code(c_raw.clone()),
        country_raw: c_raw,
        name: opt_trimmed(&v.name),
        organization: opt_trimmed(&v.organization),
        status: opt_trimmed(&v.status),
        created: opt_trimmed(&v.created),
        last_modified: opt_trimmed(&v.last_modified),
        source: opt_trimmed(&v.source),
        maintainers: opt_trimmed(&v.maintainers),
        abuse_contact: opt_trimmed(&v.abuse_contact),
        description: opt_trimmed(&v.description),
    }
}
