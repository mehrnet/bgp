use std::borrow::Cow;

use serde::Serialize;

use crate::model::ResponseMeta;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RangeKind {
    Allocations,
    Routes,
}

#[derive(Debug, Clone, Serialize)]
pub struct PrefixDescriptor {
    pub cidr: String,
    pub version: u8,
    pub prefix_length: u8,
    pub start_ip: String,
    pub end_ip: String,
    pub address_count: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct RangeDescriptor {
    pub start_ip: String,
    pub end_ip: String,
    pub version: u8,
    pub address_count: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct RouteObject<'a> {
    #[serde(skip)]
    pub id: i64,
    pub prefix: String,
    pub version: u8,
    pub origin_asn: Cow<'a, str>,
    pub as_number: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub relation: Option<&'static str>,
    pub registry: Option<Cow<'a, str>>,
    pub source: Option<Cow<'a, str>>,
    pub maintainers: Option<Cow<'a, str>>,
    pub organization: Option<Cow<'a, str>>,
    pub abuse_contact: Option<Cow<'a, str>>,
    pub description: Option<Cow<'a, str>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AllocationObject<'a> {
    #[serde(skip)]
    pub id: i64,
    pub start_ip: String,
    pub end_ip: String,
    pub version: u8,
    pub registry: Option<Cow<'a, str>>,
    pub country_code: Option<Cow<'a, str>>,
    pub country_raw: Option<Cow<'a, str>>,
    pub name: Option<Cow<'a, str>>,
    pub status: Option<Cow<'a, str>>,
    pub allocation_date: Option<Cow<'a, str>>,
    pub created: Option<Cow<'a, str>>,
    pub last_modified: Option<Cow<'a, str>>,
    pub source: Option<Cow<'a, str>>,
    pub maintainers: Option<Cow<'a, str>>,
    pub organization: Option<Cow<'a, str>>,
    pub abuse_contact: Option<Cow<'a, str>>,
    pub description: Option<Cow<'a, str>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AutnumObject<'a> {
    pub asn: Cow<'a, str>,
    pub as_number: u32,
    pub registry: Option<Cow<'a, str>>,
    pub country_code: Option<Cow<'a, str>>,
    pub country_raw: Option<Cow<'a, str>>,
    pub name: Option<Cow<'a, str>>,
    pub organization: Option<Cow<'a, str>>,
    pub status: Option<Cow<'a, str>>,
    pub created: Option<Cow<'a, str>>,
    pub last_modified: Option<Cow<'a, str>>,
    pub source: Option<Cow<'a, str>>,
    pub maintainers: Option<Cow<'a, str>>,
    pub abuse_contact: Option<Cow<'a, str>>,
    pub description: Option<Cow<'a, str>>,
}

fn is_zero_usize(v: &Option<usize>) -> bool {
    matches!(v, None | Some(0))
}

fn is_zero_i64(v: &Option<i64>) -> bool {
    matches!(v, None | Some(0))
}

#[derive(Debug, Clone, Serialize)]
pub struct RoutePage<'a> {
    pub items: Vec<RouteObject<'a>>,
    pub next_cursor: Option<String>,
    #[serde(skip_serializing_if = "is_zero_usize")]
    pub page: Option<usize>,
    #[serde(skip_serializing_if = "is_zero_usize")]
    pub total_pages: Option<usize>,
    #[serde(skip_serializing_if = "is_zero_i64")]
    pub total_items: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PrefixResponse<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta: Option<ResponseMeta<'a>>,
    pub prefix: PrefixDescriptor,
    pub allocation: Option<AllocationObject<'a>>,
    pub routes: RoutePage<'a>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RangeFacet {
    pub value: String,
    pub record_count: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct RangeSummaryDto {
    pub aggregation: &'static str,
    pub bucket_prefix_length: u8,
    pub buckets: usize,
    pub allocation_records: i64,
    pub route_records: i64,
    pub countries: Vec<RangeFacet>,
    pub asns: Vec<RangeFacet>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RangeResponse<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta: Option<ResponseMeta<'a>>,
    pub range: RangeDescriptor,
    pub kind: RangeKind,
    pub mode: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<RangeSummaryDto>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub allocations: Vec<AllocationObject<'a>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub routes: Vec<RouteObject<'a>>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AsnResponse<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta: Option<ResponseMeta<'a>>,
    pub asn: String,
    pub as_number: u32,
    pub autnum: Option<AutnumObject<'a>>,
    pub routes: RoutePage<'a>,
}
