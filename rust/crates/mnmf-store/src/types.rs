use std::fmt;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use serde::{Deserialize, Serialize};

pub const SCHEMA_VERSION: u32 = 1;
pub const FORMAT_VERSION: u32 = 1;
pub const HEADER_FIXED_SIZE: usize = 16;
pub const SECTION_ENTRY_SIZE: usize = 16;
pub const SECTION_COUNT: usize = 31;
pub const HEADER_SIZE: usize = HEADER_FIXED_SIZE + SECTION_COUNT * SECTION_ENTRY_SIZE;
pub const RANGE_KEY_SIZE: usize = 37;
pub const MAGIC: [u8; 4] = *b"MNMF";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Metadata = 0,
    StringPoolOffsets = 1,
    StringPoolData = 2,
    AllocationOffsets = 3,
    AllocationData = 4,
    RouteOffsets = 5,
    RouteData = 6,
    GeofeedOffsets = 7,
    GeofeedData = 8,
    AutnumIndex = 9,
    AutnumData = 10,
    AsnRouteIndex = 11,
    AsnRouteIds = 12,
    AllocLpmV4 = 13,
    AllocLpmV6 = 14,
    GeoLpmV4 = 15,
    GeoLpmV6 = 16,
    RouteLpmV4 = 17,
    RouteLpmV6 = 18,
    AllocPrefixV4 = 19,
    AllocPrefixV6 = 20,
    RoutePrefixV4 = 21,
    RoutePrefixV6 = 22,
    GeofeedPrefixV4 = 23,
    GeofeedPrefixV6 = 24,
    AllocRangeV4 = 25,
    AllocRangeV6 = 26,
    RouteRangeV4 = 27,
    RouteRangeV6 = 28,
    SummaryIndex = 29,
    SummaryData = 30,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct Address(pub [u8; 16]);

impl Address {
    #[must_use]
    pub fn from_ip(ip: IpAddr) -> Self {
        match ip {
            IpAddr::V4(v4) => {
                let mut bytes = [0u8; 16];
                bytes[12..16].copy_from_slice(&v4.octets());
                Self(bytes)
            }
            IpAddr::V6(v6) => Self(v6.octets()),
        }
    }

    #[must_use]
    pub fn to_ip(self, version: u8) -> IpAddr {
        if version == 4 {
            IpAddr::V4(Ipv4Addr::new(
                self.0[12], self.0[13], self.0[14], self.0[15],
            ))
        } else {
            IpAddr::V6(Ipv6Addr::from(self.0))
        }
    }

    #[must_use]
    pub fn version_slice(&self, version: u8) -> &[u8] {
        if version == 4 {
            &self.0[12..16]
        } else {
            &self.0[..]
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Allocation {
    pub version: u8,
    pub start: Address,
    pub end: Address,
    pub registry: String,
    pub country: String,
    pub name: String,
    pub allocation_date: String,
    pub status: String,
    pub abuse_contact: String,
    pub source: String,
    pub maintainers: String,
    pub organization: String,
    pub created: String,
    pub last_modified: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Route {
    pub version: u8,
    pub prefix_length: u8,
    pub network: Address,
    pub origin_asn: String,
    pub as_number: u32,
    pub registry: String,
    pub source: String,
    pub maintainers: String,
    pub organization: String,
    pub abuse_contact: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Geofeed {
    pub version: u8,
    pub start: Address,
    pub end: Address,
    pub country: String,
    pub region: String,
    pub city: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Autnum {
    pub as_number: u32,
    pub asn: String,
    pub registry: String,
    pub country: String,
    pub name: String,
    pub organization: String,
    pub status: String,
    pub created: String,
    pub last_modified: String,
    pub source: String,
    pub maintainers: String,
    pub abuse_contact: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Facet {
    pub value: String,
    pub count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RangeSummary {
    pub prefix_length: u8,
    pub allocation_records: u64,
    pub routes: u64,
    pub countries: Vec<Facet>,
    pub asns: Vec<Facet>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Metadata {
    pub schema_version: u32,
    #[serde(default)]
    pub release_tag: String,
    #[serde(default)]
    pub built_at: String,
    #[serde(default)]
    pub source_commit: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableKind {
    Allocations,
    Routes,
    Geofeeds,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RangeIndexEntry {
    pub id: u32,
    pub key: [u8; RANGE_KEY_SIZE],
}

#[derive(Debug)]
pub enum StoreError {
    Io(std::io::Error),
    InvalidFormat(String),
    QueryTooBroad,
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "mmap store I/O error: {err}"),
            Self::InvalidFormat(msg) => write!(f, "invalid mmap dataset: {msg}"),
            Self::QueryTooBroad => f.write_str("overlap scan limit exceeded"),
        }
    }
}

impl std::error::Error for StoreError {}

impl From<std::io::Error> for StoreError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct RangeQuery<'a> {
    pub kind: TableKind,
    pub version: u8,
    pub start: Address,
    pub end: Address,
    pub after: Option<&'a [u8; RANGE_KEY_SIZE]>,
    pub maximum: usize,
}
