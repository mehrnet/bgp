use std::net::{IpAddr, Ipv4Addr};

use crate::Db;
use crate::borrowed::{AllocationRef, AutnumRef, GeofeedRef, RouteRef};
use crate::codec::{be_u32, be_u64};
use crate::format::{prefix_section, record_slice, slice_asn_routes};
use crate::index::{
    ASN_ROUTE_INDEX_STRIDE, AUTNUM_INDEX_STRIDE, find_exact_u32, find_summary_entry,
    matching_prefix_ids, overlap_route_ids,
};
use crate::range_index::direct_range_entries;
use crate::records::{
    decode_allocation_ref, decode_autnum_ref, decode_geofeed_ref, decode_route_ref, decode_summary,
};
use crate::types::{
    Allocation, Autnum, Geofeed, RangeIndexEntry, RangeQuery, RangeSummary, Route, Section,
    StoreError, TableKind,
};

impl Db {
    pub fn allocation_ref(
        &self,
        id: u32,
        lookup_only: bool,
    ) -> Result<AllocationRef<'_>, StoreError> {
        let raw = record_slice(
            self.sec(Section::AllocationOffsets),
            self.sec(Section::AllocationData),
            id,
        )?;
        decode_allocation_ref(raw, &self.pool(), lookup_only)
    }

    pub fn allocation(&self, id: u32, lookup_only: bool) -> Result<Allocation, StoreError> {
        self.allocation_ref(id, lookup_only)
            .map(AllocationRef::into_owned)
    }

    pub fn route_ref(&self, id: u32, lookup_only: bool) -> Result<RouteRef<'_>, StoreError> {
        let raw = record_slice(
            self.sec(Section::RouteOffsets),
            self.sec(Section::RouteData),
            id,
        )?;
        decode_route_ref(raw, &self.pool(), lookup_only)
    }

    pub fn route(&self, id: u32, lookup_only: bool) -> Result<Route, StoreError> {
        self.route_ref(id, lookup_only).map(RouteRef::into_owned)
    }

    pub fn geofeed_ref(&self, id: u32) -> Result<GeofeedRef<'_>, StoreError> {
        let raw = record_slice(
            self.sec(Section::GeofeedOffsets),
            self.sec(Section::GeofeedData),
            id,
        )?;
        decode_geofeed_ref(raw, &self.pool())
    }

    pub fn geofeed(&self, id: u32) -> Result<Geofeed, StoreError> {
        self.geofeed_ref(id).map(GeofeedRef::into_owned)
    }

    pub fn autnum_ref(
        &self,
        asn: u32,
        identity_only: bool,
    ) -> Result<Option<AutnumRef<'_>>, StoreError> {
        let Some(row) = find_exact_u32(self.sec(Section::AutnumIndex), AUTNUM_INDEX_STRIDE, asn)?
        else {
            return Ok(None);
        };
        let off = be_u64(row.get(4..12).unwrap_or(&[]))? as usize;
        let len = be_u32(row.get(12..16).unwrap_or(&[]))? as usize;
        let raw = self
            .sec(Section::AutnumData)
            .get(off..off.saturating_add(len))
            .ok_or_else(|| StoreError::InvalidFormat("autnum record out of bounds".into()))?;
        Ok(Some(decode_autnum_ref(raw, &self.pool(), identity_only)?))
    }

    pub fn autnum(&self, asn: u32, identity_only: bool) -> Result<Option<Autnum>, StoreError> {
        Ok(self
            .autnum_ref(asn, identity_only)?
            .map(AutnumRef::into_owned))
    }

    pub fn asn_route_slice(
        &self,
        asn: u32,
        numbered: bool,
        page_number: usize,
        cursor: i64,
        limit: usize,
    ) -> Result<(i64, Vec<u32>), StoreError> {
        let Some(row) = find_exact_u32(
            self.sec(Section::AsnRouteIndex),
            ASN_ROUTE_INDEX_STRIDE,
            asn,
        )?
        else {
            return Ok((0, Vec::new()));
        };
        let first_idx = be_u64(row.get(4..12).unwrap_or(&[]))? as usize;
        let count = be_u32(row.get(12..16).unwrap_or(&[]))? as usize;
        let slice = self
            .sec(Section::AsnRouteIds)
            .get(first_idx * 4..first_idx.saturating_add(count) * 4)
            .ok_or_else(|| StoreError::InvalidFormat("ASN route IDs out of bounds".into()))?;
        slice_asn_routes(slice, count, numbered, page_number, cursor, limit)
    }

    pub fn summary(
        &self,
        addr: Ipv4Addr,
        prefix_len: u8,
    ) -> Result<Option<RangeSummary>, StoreError> {
        let Some((off, len)) =
            find_summary_entry(self.sec(Section::SummaryIndex), addr, prefix_len)?
        else {
            return Ok(None);
        };
        let raw = self
            .sec(Section::SummaryData)
            .get(off as usize..(off as usize).saturating_add(len as usize))
            .ok_or_else(|| StoreError::InvalidFormat("summary slice out of bounds".into()))?;
        Ok(Some(decode_summary(raw)?))
    }

    pub fn matching_prefix_ids(
        &self,
        kind: TableKind,
        ip: IpAddr,
        limit: usize,
    ) -> Result<Vec<u32>, StoreError> {
        matching_prefix_ids(self.sec(prefix_section(kind, ip.is_ipv4())), ip, limit)
    }

    pub fn overlap_route_ids(
        &self,
        start: IpAddr,
        end: IpAddr,
        after: i64,
        scan_limit: usize,
    ) -> Result<Vec<u32>, StoreError> {
        overlap_route_ids(
            self.sec(prefix_section(TableKind::Routes, start.is_ipv4())),
            start,
            end,
            after,
            scan_limit,
        )
    }

    pub fn direct_range_entries(
        &self,
        q: RangeQuery<'_>,
    ) -> Result<(Vec<RangeIndexEntry>, bool), StoreError> {
        let sec = match (q.kind, q.version == 4) {
            (TableKind::Allocations, true) => Section::AllocRangeV4,
            (TableKind::Allocations, false) => Section::AllocRangeV6,
            (_, true) => Section::RouteRangeV4,
            (_, false) => Section::RouteRangeV6,
        };
        direct_range_entries(self.sec(sec), q.version, q.start, q.end, q.after, q.maximum)
    }
}
