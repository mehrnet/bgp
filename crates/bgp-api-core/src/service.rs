use std::collections::BTreeMap;

use mnmf_store::Db;
use mnmf_store::types::{Address, Allocation, Geofeed, Route, StoreError, TableKind};

use crate::details::{
    as_number_from_asn, build_details, country_code, opt_lower, opt_trimmed, opt_upper,
    route_bounds, route_cidr,
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

pub fn lookup_ip(db: &Db, ip: &RuntimeIp, full_details: bool) -> Result<LookupResponse, ApiError> {
    if full_details {
        lookup_ip_full(db, ip)
    } else {
        lookup_ip_compact(db, ip)
    }
}

fn lookup_ip_compact(db: &Db, ip: &RuntimeIp) -> Result<LookupResponse, ApiError> {
    let alloc_id = db.lookup_allocation_lpm(ip.ip)?;
    let allocation = if alloc_id != 0 {
        Some(db.allocation(alloc_id, true)?)
    } else {
        None
    };
    let route_ids = db.lookup_route_lpm(ip.ip)?;
    let mut routes = Vec::with_capacity(route_ids.len());
    for id in route_ids {
        routes.push(db.route(id, true)?);
    }
    let origins = origin_identities(db, &routes)?;
    let geo_id = db.lookup_geofeed_lpm(ip.ip)?;
    let geofeed = if geo_id != 0 {
        Some(db.geofeed(geo_id)?)
    } else {
        None
    };
    Ok(build_lookup_response(
        db,
        ip,
        allocation.as_ref(),
        &routes,
        origins,
        geofeed.as_ref(),
    ))
}

fn lookup_ip_full(db: &Db, ip: &RuntimeIp) -> Result<LookupResponse, ApiError> {
    let alloc_ids = db.matching_prefix_ids(TableKind::Allocations, ip.ip, MAX_CANDIDATES)?;
    let mut allocations = Vec::with_capacity(alloc_ids.len());
    for id in alloc_ids {
        allocations.push(db.allocation(id, false)?);
    }
    let route_ids = db.matching_prefix_ids(TableKind::Routes, ip.ip, MAX_CANDIDATES)?;
    let mut routes = Vec::with_capacity(route_ids.len());
    for id in route_ids {
        routes.push(db.route(id, false)?);
    }
    let selected_routes = narrowest_routes(&routes);
    let origins = origin_identities(db, &selected_routes)?;
    let geo_ids = db.matching_prefix_ids(TableKind::Geofeeds, ip.ip, MAX_CANDIDATES)?;
    let mut geofeeds = Vec::with_capacity(geo_ids.len());
    for id in geo_ids {
        geofeeds.push(db.geofeed(id)?);
    }
    let mut response = build_lookup_response(
        db,
        ip,
        allocations
            .iter()
            .min_by_key(|a| range_width(a.start, a.end)),
        &selected_routes,
        origins,
        geofeeds
            .iter()
            .min_by_key(|g| (g.is_fallback, range_width(g.start, g.end))),
    );
    response.details = Some(build_details(&allocations, &routes, &geofeeds));
    Ok(response)
}

fn build_lookup_response(
    db: &Db,
    ip: &RuntimeIp,
    allocation: Option<&Allocation>,
    routes: &[Route],
    origins: Vec<AsnIdentity>,
    geofeed: Option<&Geofeed>,
) -> LookupResponse {
    let mut resp = LookupResponse {
        meta: Some(ResponseMeta {
            dataset: Some(dataset_meta_from_db(db)),
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
    populate_network(&mut resp, routes, origins);
    populate_allocation(&mut resp, allocation);
    populate_location(&mut resp, geofeed);
    resp.sources.allocation = allocation.is_some();
    resp.sources.route = !routes.is_empty();
    resp.sources.geofeed = geofeed.is_some();
    resp
}

fn populate_network(resp: &mut LookupResponse, routes: &[Route], origins: Vec<AsnIdentity>) {
    resp.network.asns = sorted_route_asns(routes);
    resp.network.origins = origins;
    if resp.network.asns.len() == 1
        && let Some(single) = resp.network.asns.first()
    {
        resp.network.asn = Some(single.clone());
        resp.network.as_number = as_number_from_asn(Some(single));
    }
    if let Some(route) = routes.first() {
        resp.network.cidr = opt_trimmed(&route_cidr(route));
        let (start, end) = route_bounds(route);
        resp.network.start_ip = Some(start.to_ip(route.version).to_string());
        resp.network.end_ip = Some(end.to_ip(route.version).to_string());
        resp.network.abuse_contact = opt_trimmed(&route.abuse_contact);
    }
}

fn populate_allocation(resp: &mut LookupResponse, allocation: Option<&Allocation>) {
    let Some(alloc) = allocation else { return };
    resp.allocation.start_ip = Some(alloc.start.to_ip(alloc.version).to_string());
    resp.allocation.end_ip = Some(alloc.end.to_ip(alloc.version).to_string());
    resp.allocation.registry = opt_lower(&alloc.registry);
    resp.allocation.country_raw = opt_upper(&alloc.country);
    resp.allocation.country_code = country_code(resp.allocation.country_raw.clone());
    resp.allocation.name = opt_trimmed(&alloc.name);
    resp.allocation.allocation_date = opt_trimmed(&alloc.allocation_date);
    resp.allocation.status = opt_trimmed(&alloc.status);
    resp.allocation.abuse_contact = opt_trimmed(&alloc.abuse_contact);
    resp.registry = resp.allocation.registry.clone();
    resp.allocation_date = resp.allocation.allocation_date.clone();
    resp.allocation_status = resp.allocation.status.clone();
}

fn populate_location(resp: &mut LookupResponse, geofeed: Option<&Geofeed>) {
    if let Some(geo) = geofeed {
        resp.location.country_code = country_code(opt_upper(&geo.country));
        resp.location.region = opt_trimmed(&geo.region);
        resp.location.city = opt_trimmed(&geo.city);
        resp.location.latitude = geo.latitude;
        resp.location.longitude = geo.longitude;
    }
    if resp.location.country_code.is_none() {
        resp.location.country_code = resp.allocation.country_code.clone();
    }
}

fn sorted_route_asns(routes: &[Route]) -> Vec<String> {
    let mut map: BTreeMap<(u32, String), ()> = BTreeMap::new();
    for r in routes {
        if let Some(name) = opt_upper(&r.origin_asn) {
            map.insert((r.as_number, name), ());
        }
    }
    map.into_keys().map(|(_, name)| name).collect()
}

fn origin_identities(db: &Db, routes: &[Route]) -> Result<Vec<AsnIdentity>, ApiError> {
    let mut map: BTreeMap<(u32, String), u32> = BTreeMap::new();
    for r in routes {
        if let Some(name) = opt_upper(&r.origin_asn) {
            map.entry((r.as_number, name)).or_insert(r.as_number);
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
            && let Some(autnum) = db.autnum(num, true)?
        {
            identity.name = opt_trimmed(&autnum.name);
            identity.organization = opt_trimmed(&autnum.organization);
        }
        origins.push(identity);
    }
    Ok(origins)
}

fn narrowest_routes(routes: &[Route]) -> Vec<Route> {
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
