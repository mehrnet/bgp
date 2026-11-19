use std::borrow::Cow;
use std::collections::BTreeMap;

use mnmf_store::Db;
use mnmf_store::borrowed::{AllocationRef, GeofeedRef, RouteRef};
use mnmf_store::types::{Address, StoreError, TableKind};

use crate::details::{
    build_details, cow_trimmed, cow_upper, opt_trimmed, populate_allocation, populate_location,
    populate_network,
};
use crate::ipkey::RuntimeIp;
use crate::model::{AsnIdentity, DatasetMetadata, LookupResponse, ResponseMeta};

const MAX_CANDIDATES: usize = 64;

#[derive(Debug)]
pub enum ApiError {
    BadRequest { code: &'static str, message: String },
    Unauthorized,
    NotFound { code: &'static str, message: String },
    TooBroad(String),
    Internal(String),
}

impl From<StoreError> for ApiError {
    fn from(err: StoreError) -> Self {
        match err {
            StoreError::QueryTooBroad => Self::TooBroad("query matched too many records".into()),
            other => Self::Internal(other.to_string()),
        }
    }
}

#[must_use]
pub fn dataset_meta_from_db(db: &Db) -> DatasetMetadata {
    let m = db.metadata();
    DatasetMetadata {
        release_tag: opt_trimmed(&m.release_tag),
        built_at: opt_trimmed(&m.built_at),
        activated_at: None,
        source_commit: opt_trimmed(&m.source_commit),
    }
}

pub fn lookup_ip<'a>(
    db: &'a Db,
    ip: &RuntimeIp,
    full_details: bool,
) -> Result<LookupResponse<'a>, ApiError> {
    lookup_ip_with_meta(db, ip, None, full_details)
}

pub(crate) fn lookup_ip_with_meta<'a>(
    db: &'a Db,
    ip: &RuntimeIp,
    meta: Option<&'a DatasetMetadata>,
    full_details: bool,
) -> Result<LookupResponse<'a>, ApiError> {
    if full_details {
        lookup_ip_full(db, ip, meta)
    } else {
        lookup_ip_compact(db, ip, meta)
    }
}

fn lookup_ip_compact<'a>(
    db: &'a Db,
    ip: &RuntimeIp,
    meta: Option<&'a DatasetMetadata>,
) -> Result<LookupResponse<'a>, ApiError> {
    let alloc_id = db.lookup_allocation_lpm(ip.ip)?;
    let allocation = if alloc_id != 0 {
        Some(db.allocation_ref(alloc_id, true)?)
    } else {
        None
    };
    let route_ids = db.lookup_route_lpm(ip.ip)?;
    let mut routes = Vec::with_capacity(route_ids.len());
    for id in route_ids {
        routes.push(db.route_ref(id, true)?);
    }
    let origins = origin_identities(db, &routes)?;
    let geo_id = db.lookup_geofeed_lpm(ip.ip)?;
    let geofeed = if geo_id != 0 {
        Some(db.geofeed_ref(geo_id)?)
    } else {
        None
    };
    Ok(build_lookup_response(
        db,
        meta,
        ip,
        LookupParts {
            allocation,
            routes: &routes,
            origins,
            geofeed,
        },
    ))
}

fn lookup_ip_full<'a>(
    db: &'a Db,
    ip: &RuntimeIp,
    meta: Option<&'a DatasetMetadata>,
) -> Result<LookupResponse<'a>, ApiError> {
    let alloc_ids = db.matching_prefix_ids(TableKind::Allocations, ip.ip, MAX_CANDIDATES)?;
    let mut allocations = Vec::with_capacity(alloc_ids.len());
    for id in alloc_ids {
        allocations.push(db.allocation_ref(id, false)?);
    }
    let route_ids = db.matching_prefix_ids(TableKind::Routes, ip.ip, MAX_CANDIDATES)?;
    let mut routes = Vec::with_capacity(route_ids.len());
    for id in route_ids {
        routes.push(db.route_ref(id, false)?);
    }
    let selected_routes = narrowest_routes(&routes);
    let origins = origin_identities(db, &selected_routes)?;
    let geo_ids = db.matching_prefix_ids(TableKind::Geofeeds, ip.ip, MAX_CANDIDATES)?;
    let mut geofeeds = Vec::with_capacity(geo_ids.len());
    for id in geo_ids {
        geofeeds.push(db.geofeed_ref(id)?);
    }
    let best_alloc = allocations
        .iter()
        .min_by_key(|a| range_width(a.start, a.end))
        .cloned();
    let best_geo = geofeeds
        .iter()
        .min_by_key(|g| (g.is_fallback, range_width(g.start, g.end)))
        .cloned();
    let mut response = build_lookup_response(
        db,
        meta,
        ip,
        LookupParts {
            allocation: best_alloc,
            routes: &selected_routes,
            origins,
            geofeed: best_geo,
        },
    );
    response.details = Some(build_details(allocations, routes, geofeeds));
    Ok(response)
}

struct LookupParts<'a, 'b> {
    allocation: Option<AllocationRef<'a>>,
    routes: &'b [RouteRef<'a>],
    origins: Vec<AsnIdentity<'a>>,
    geofeed: Option<GeofeedRef<'a>>,
}

fn build_lookup_response<'a>(
    db: &'a Db,
    meta: Option<&'a DatasetMetadata>,
    ip: &RuntimeIp,
    parts: LookupParts<'a, '_>,
) -> LookupResponse<'a> {
    let dataset = meta.map_or_else(|| Cow::Owned(dataset_meta_from_db(db)), Cow::Borrowed);
    let has_alloc = parts.allocation.is_some();
    let has_geo = parts.geofeed.is_some();
    let has_route = !parts.routes.is_empty();
    let mut resp = LookupResponse {
        meta: Some(ResponseMeta {
            dataset: Some(dataset),
        }),
        ip: ip.canonical.clone(),
        version: ip.version,
        registry: None,
        allocation_date: None,
        allocation_status: None,
        network: Default::default(),
        allocation: Default::default(),
        location: Default::default(),
        sources: Default::default(),
        details: None,
    };
    populate_network(
        &mut resp,
        parts.routes,
        sorted_route_asns(parts.routes),
        parts.origins,
    );
    populate_allocation(&mut resp, parts.allocation);
    populate_location(&mut resp, parts.geofeed);
    resp.sources.allocation = has_alloc;
    resp.sources.route = has_route;
    resp.sources.geofeed = has_geo;
    resp
}

fn sorted_route_asns(routes: &[RouteRef<'_>]) -> Vec<String> {
    let mut map: BTreeMap<(u32, String), ()> = BTreeMap::new();
    for r in routes {
        if let Some(name) = cow_upper(r.origin_asn.clone()) {
            map.insert((r.as_number, name.into_owned()), ());
        }
    }
    map.into_keys().map(|(_, name)| name).collect()
}

fn origin_identities<'a>(
    db: &'a Db,
    routes: &[RouteRef<'a>],
) -> Result<Vec<AsnIdentity<'a>>, ApiError> {
    let mut map: BTreeMap<(u32, String), u32> = BTreeMap::new();
    for r in routes {
        if let Some(name) = cow_upper(r.origin_asn.clone()) {
            map.entry((r.as_number, name.into_owned()))
                .or_insert(r.as_number);
        }
    }
    let mut origins = Vec::with_capacity(map.len());
    for ((_, asn), num) in map {
        let mut identity = AsnIdentity {
            asn,
            as_number: num,
            name: None,
            organization: None,
        };
        if num > 0
            && let Some(autnum) = db.autnum_ref(num, true)?
        {
            identity.name = cow_trimmed(autnum.name);
            identity.organization = cow_trimmed(autnum.organization);
        }
        origins.push(identity);
    }
    Ok(origins)
}

fn narrowest_routes<'a>(routes: &[RouteRef<'a>]) -> Vec<RouteRef<'a>> {
    let Some(best_len) = routes.iter().map(|r| r.prefix_length).max() else {
        return Vec::new();
    };
    routes
        .iter()
        .filter(|r| r.prefix_length == best_len)
        .cloned()
        .collect()
}

fn range_width(start: Address, end: Address) -> u128 {
    u128::from_be_bytes(end.0).saturating_sub(u128::from_be_bytes(start.0))
}
