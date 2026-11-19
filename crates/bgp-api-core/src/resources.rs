use std::borrow::Cow;

use mnmf_store::Db;
use mnmf_store::borrowed::AllocationRef;
use mnmf_store::types::{Address, TableKind};

use crate::details::route_cidr;
use crate::dto::{allocation_object, route_object};
use crate::ipkey::{ParsedPrefix, ParsedRange, parse_prefix, summary_prefix_key};
use crate::model::{DatasetMetadata, ResponseMeta};
use crate::range_cursor::range_page_entries;
use crate::resource_model::{
    AllocationObject, PrefixDescriptor, PrefixResponse, RangeDescriptor, RangeFacet, RangeKind,
    RangeResponse, RangeSummaryDto, RouteObject, RoutePage,
};
use crate::service::ApiError;

const MAX_OVERLAP_ENTRIES: usize = 2_000_000;

#[derive(Debug, Clone, Copy)]
pub struct Page {
    pub cursor: i64,
    pub limit: usize,
    pub number: usize,
    pub numbered: bool,
}

#[derive(Debug, Clone)]
pub struct RangePage {
    pub cursor: String,
    pub limit: usize,
}

pub(crate) fn lookup_prefix<'a>(
    db: &'a Db,
    meta: &'a DatasetMetadata,
    prefix: &ParsedPrefix,
    page: Page,
) -> Result<PrefixResponse<'a>, ApiError> {
    let allocation = covering_allocation(db, prefix)?;
    let mut ids =
        db.overlap_route_ids(prefix.start, prefix.end, page.cursor, MAX_OVERLAP_ENTRIES)?;
    ids.truncate(page.limit + 1);
    let mut routes = Vec::with_capacity(ids.len());
    for id in ids {
        let rec = db.route_ref(id, false)?;
        let cidr = route_cidr(&rec);
        let mut obj = route_object(id, rec);
        obj.relation = Some(route_relation(&cidr, prefix));
        routes.push(obj);
    }
    let (items, next_cursor) = paginate_routes(routes, page.limit);
    Ok(PrefixResponse {
        meta: Some(ResponseMeta {
            dataset: Some(Cow::Borrowed(meta)),
        }),
        prefix: PrefixDescriptor {
            cidr: prefix.canonical.clone(),
            version: prefix.version,
            prefix_length: prefix.prefix_length,
            start_ip: prefix.start.to_string(),
            end_ip: prefix.end.to_string(),
            address_count: prefix.address_count.clone(),
        },
        allocation,
        routes: RoutePage {
            items,
            next_cursor,
            page: None,
            total_pages: None,
            total_items: None,
        },
    })
}

fn route_relation(cidr: &str, query: &ParsedPrefix) -> &'static str {
    if cidr == query.canonical {
        return "exact";
    }
    if let Some(p) = parse_prefix(cidr)
        && p.version == query.version
        && p.prefix_length <= query.prefix_length
        && p.start <= query.start
        && p.end >= query.end
    {
        return "covering";
    }
    "more_specific"
}

fn covering_allocation<'a>(
    db: &'a Db,
    prefix: &ParsedPrefix,
) -> Result<Option<AllocationObject<'a>>, ApiError> {
    let ids = db.matching_prefix_ids(TableKind::Allocations, prefix.start, 0)?;
    let q_start = Address::from_ip(prefix.start);
    let q_end = Address::from_ip(prefix.end);
    let mut best: Option<(u32, AllocationRef<'a>)> = None;
    for id in ids {
        let rec = db.allocation_ref(id, false)?;
        if rec.start > q_start || rec.end < q_end {
            continue;
        }
        let better = match &best {
            None => true,
            Some((bid, b)) => {
                rec.start > b.start
                    || (rec.start == b.start
                        && (rec.end < b.end || (rec.end == b.end && id < *bid)))
            }
        };
        if better {
            best = Some((id, rec));
        }
    }
    Ok(best.map(|(id, rec)| allocation_object(id, rec)))
}

pub(crate) fn lookup_range<'a>(
    db: &'a Db,
    meta: &'a DatasetMetadata,
    range: &ParsedRange,
    kind: RangeKind,
    page: &RangePage,
) -> Result<RangeResponse<'a>, ApiError> {
    let (entries, next_cursor) = range_page_entries(db, range, kind, page)?;
    let mut resp = RangeResponse {
        meta: Some(ResponseMeta {
            dataset: Some(Cow::Borrowed(meta)),
        }),
        range: range_descriptor(range),
        kind,
        mode: "records",
        summary: None,
        allocations: Vec::new(),
        routes: Vec::new(),
        next_cursor,
    };
    if kind == RangeKind::Allocations {
        for entry in entries {
            resp.allocations.push(allocation_object(
                entry.id,
                db.allocation_ref(entry.id, false)?,
            ));
        }
    } else {
        for entry in entries {
            resp.routes
                .push(route_object(entry.id, db.route_ref(entry.id, false)?));
        }
    }
    Ok(resp)
}

pub(crate) fn lookup_range_summary<'a>(
    db: &'a Db,
    meta: &'a DatasetMetadata,
    range: &ParsedRange,
    kind: RangeKind,
) -> Result<RangeResponse<'a>, ApiError> {
    let Some((v4, len)) = summary_prefix_key(range) else {
        return Err(ApiError::Internal(
            "range is not eligible for generated summary".into(),
        ));
    };
    let item = db.summary(v4, len)?.unwrap_or_default();
    let countries = item
        .countries
        .into_iter()
        .map(|f| RangeFacet {
            value: f.value,
            record_count: f.count as i64,
        })
        .collect();
    let asns = item
        .asns
        .into_iter()
        .map(|f| RangeFacet {
            value: f.value,
            record_count: f.count as i64,
        })
        .collect();
    Ok(RangeResponse {
        meta: Some(ResponseMeta {
            dataset: Some(Cow::Borrowed(meta)),
        }),
        range: range_descriptor(range),
        kind,
        mode: "summary",
        summary: Some(RangeSummaryDto {
            aggregation: "overlapping_source_records",
            bucket_prefix_length: len,
            buckets: 1,
            allocation_records: item.allocation_records as i64,
            route_records: item.routes as i64,
            countries,
            asns,
        }),
        allocations: Vec::new(),
        routes: Vec::new(),
        next_cursor: None,
    })
}

fn range_descriptor(v: &ParsedRange) -> RangeDescriptor {
    RangeDescriptor {
        start_ip: v.start.to_string(),
        end_ip: v.end.to_string(),
        version: v.version,
        address_count: v.address_count.clone(),
    }
}

pub(crate) fn paginate_routes<'a>(
    mut items: Vec<RouteObject<'a>>,
    limit: usize,
) -> (Vec<RouteObject<'a>>, Option<String>) {
    if items.len() <= limit {
        return (items, None);
    }
    items.truncate(limit);
    let next = items.last().map(|r| r.id.to_string());
    (items, next)
}
