use std::borrow::Cow;

use crate::types::{Address, Allocation, Autnum, Geofeed, Route};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AllocationRef<'a> {
    pub version: u8,
    pub start: Address,
    pub end: Address,
    pub registry: Cow<'a, str>,
    pub country: Cow<'a, str>,
    pub name: Cow<'a, str>,
    pub allocation_date: Cow<'a, str>,
    pub status: Cow<'a, str>,
    pub abuse_contact: Cow<'a, str>,
    pub source: Cow<'a, str>,
    pub maintainers: Cow<'a, str>,
    pub organization: Cow<'a, str>,
    pub created: Cow<'a, str>,
    pub last_modified: Cow<'a, str>,
    pub description: Cow<'a, str>,
}

impl AllocationRef<'_> {
    #[must_use]
    pub fn into_owned(self) -> Allocation {
        Allocation {
            version: self.version,
            start: self.start,
            end: self.end,
            registry: self.registry.into_owned(),
            country: self.country.into_owned(),
            name: self.name.into_owned(),
            allocation_date: self.allocation_date.into_owned(),
            status: self.status.into_owned(),
            abuse_contact: self.abuse_contact.into_owned(),
            source: self.source.into_owned(),
            maintainers: self.maintainers.into_owned(),
            organization: self.organization.into_owned(),
            created: self.created.into_owned(),
            last_modified: self.last_modified.into_owned(),
            description: self.description.into_owned(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RouteRef<'a> {
    pub version: u8,
    pub prefix_length: u8,
    pub network: Address,
    pub origin_asn: Cow<'a, str>,
    pub as_number: u32,
    pub registry: Cow<'a, str>,
    pub source: Cow<'a, str>,
    pub maintainers: Cow<'a, str>,
    pub organization: Cow<'a, str>,
    pub abuse_contact: Cow<'a, str>,
    pub description: Cow<'a, str>,
}

impl RouteRef<'_> {
    #[must_use]
    pub fn into_owned(self) -> Route {
        Route {
            version: self.version,
            prefix_length: self.prefix_length,
            network: self.network,
            origin_asn: self.origin_asn.into_owned(),
            as_number: self.as_number,
            registry: self.registry.into_owned(),
            source: self.source.into_owned(),
            maintainers: self.maintainers.into_owned(),
            organization: self.organization.into_owned(),
            abuse_contact: self.abuse_contact.into_owned(),
            description: self.description.into_owned(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct GeofeedRef<'a> {
    pub version: u8,
    pub start: Address,
    pub end: Address,
    pub country: Cow<'a, str>,
    pub region: Cow<'a, str>,
    pub city: Cow<'a, str>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub is_fallback: bool,
}

impl GeofeedRef<'_> {
    #[must_use]
    pub fn into_owned(self) -> Geofeed {
        Geofeed {
            version: self.version,
            start: self.start,
            end: self.end,
            country: self.country.into_owned(),
            region: self.region.into_owned(),
            city: self.city.into_owned(),
            latitude: self.latitude,
            longitude: self.longitude,
            is_fallback: self.is_fallback,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AutnumRef<'a> {
    pub as_number: u32,
    pub asn: Cow<'a, str>,
    pub registry: Cow<'a, str>,
    pub country: Cow<'a, str>,
    pub name: Cow<'a, str>,
    pub organization: Cow<'a, str>,
    pub status: Cow<'a, str>,
    pub created: Cow<'a, str>,
    pub last_modified: Cow<'a, str>,
    pub source: Cow<'a, str>,
    pub maintainers: Cow<'a, str>,
    pub abuse_contact: Cow<'a, str>,
    pub description: Cow<'a, str>,
}

impl AutnumRef<'_> {
    #[must_use]
    pub fn into_owned(self) -> Autnum {
        Autnum {
            as_number: self.as_number,
            asn: self.asn.into_owned(),
            registry: self.registry.into_owned(),
            country: self.country.into_owned(),
            name: self.name.into_owned(),
            organization: self.organization.into_owned(),
            status: self.status.into_owned(),
            created: self.created.into_owned(),
            last_modified: self.last_modified.into_owned(),
            source: self.source.into_owned(),
            maintainers: self.maintainers.into_owned(),
            abuse_contact: self.abuse_contact.into_owned(),
            description: self.description.into_owned(),
        }
    }
}
