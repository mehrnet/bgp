use serde::Serialize;

use crate::sweep::Segment;

pub const DEFAULT_BLOCK_SIZE: u8 = 64;
pub const DEFAULT_ALLOC_V4_BITS: u8 = 32;
pub const DEFAULT_ALLOC_V6_BITS: u8 = 64;
pub const DEFAULT_ROUTE_V4_BITS: u8 = 32;
pub const DEFAULT_ROUTE_V6_BITS: u8 = 64;

#[derive(Debug, Clone, Default, Serialize)]
pub struct LiteMetadata {
    #[serde(skip_serializing_if = "String::is_empty")]
    pub release_tag: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub built_at: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub source_commit: String,
}

#[derive(Debug, Clone, Default)]
pub struct AutnumInput {
    pub as_number: u32,
    pub registry: String,
    pub country: String,
    pub name: String,
    pub organization: String,
    pub status: String,
    pub source: String,
    pub maintainers: String,
    pub abuse_contact: String,
    pub description: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct GeoLocKey {
    pub(crate) cc_id: u32,
    pub(crate) reg_id: u32,
    pub(crate) city_id: u32,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) struct AsnRoute {
    pub(crate) is_v6: bool,
    pub(crate) prefix: [u8; 16],
    pub(crate) bits: u8,
    pub(crate) key_hi: u64,
    pub(crate) key_lo: u64,
}

#[derive(Debug, Clone, Default)]
pub struct LiteStats {
    pub bytes: usize,
    pub countries: usize,
    pub registries: usize,
    pub regions: usize,
    pub cities: usize,
    pub geo_locations: usize,
    pub autnums: usize,
    pub alloc_v4_segs: usize,
    pub alloc_v6_segs: usize,
    pub geo_v4_segs: usize,
    pub geo_v6_segs: usize,
    pub route_v4_segs: usize,
    pub route_v6_segs: usize,
    pub multi_asn_sets: usize,
    pub asn_route_items: usize,
}

#[derive(Debug, Clone)]
pub struct LiteBuildOptions {
    pub metadata: LiteMetadata,
    pub include_asn_routes: bool,
    pub alloc_v4_bits: u8,
    pub alloc_v6_bits: u8,
}

pub(crate) struct RouteTables {
    pub(crate) segs4: Vec<Segment>,
    pub(crate) segs6: Vec<Segment>,
    pub(crate) multi_sets: Vec<Vec<u32>>,
}

pub(crate) struct EncodeInput<'a> {
    pub(crate) builder: &'a crate::lite_db::LiteDbBuilder,
    pub(crate) dicts: &'a crate::lite_routes::CompactDicts,
    pub(crate) routes: &'a RouteTables,
    pub(crate) alloc_s4: &'a [Segment],
    pub(crate) alloc_s6: &'a [Segment],
    pub(crate) geo_s4: &'a [Segment],
    pub(crate) geo_s6: &'a [Segment],
}
