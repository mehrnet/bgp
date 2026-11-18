use std::fs::File;
use std::io::Read;
use std::path::Path;

use crate::codec::{StringPool, be_u32, be_u64};
use crate::index::{
    ASN_ROUTE_INDEX_STRIDE, AUTNUM_INDEX_STRIDE, PREFIX_V4_STRIDE, PREFIX_V6_STRIDE,
    SUMMARY_INDEX_STRIDE, lower_bound_stride,
};
use crate::lpm::validate_selection_lpm;
use crate::range_index::{RANGE_V4_STRIDE, RANGE_V6_STRIDE};
use crate::route_lpm::validate_route_lpm;
use crate::types::{
    FORMAT_VERSION, HEADER_FIXED_SIZE, HEADER_SIZE, MAGIC, Metadata, SCHEMA_VERSION, SECTION_COUNT,
    SECTION_ENTRY_SIZE, Section, StoreError, TableKind,
};

pub(crate) fn selector_sections() -> [Section; 6] {
    [
        Section::AllocLpmV4,
        Section::AllocLpmV6,
        Section::GeoLpmV4,
        Section::GeoLpmV6,
        Section::RouteLpmV4,
        Section::RouteLpmV6,
    ]
}

pub(crate) fn prefix_section(kind: TableKind, is_v4: bool) -> Section {
    match (kind, is_v4) {
        (TableKind::Allocations, true) => Section::AllocPrefixV4,
        (TableKind::Allocations, false) => Section::AllocPrefixV6,
        (TableKind::Routes, true) => Section::RoutePrefixV4,
        (TableKind::Routes, false) => Section::RoutePrefixV6,
        (TableKind::Geofeeds, true) => Section::GeofeedPrefixV4,
        (TableKind::Geofeeds, false) => Section::GeofeedPrefixV6,
    }
}

pub(crate) fn record_slice<'a>(
    offsets: &[u8],
    data: &'a [u8],
    id: u32,
) -> Result<&'a [u8], StoreError> {
    if id == 0 || offsets.len() < 8 || !offsets.len().is_multiple_of(8) {
        return Err(StoreError::InvalidFormat(format!("invalid record ID {id}")));
    }
    let count = offsets.len() / 8 - 1;
    let idx = (id - 1) as usize;
    if idx >= count {
        return Err(StoreError::InvalidFormat(format!(
            "record ID {id} out of range"
        )));
    }
    let start = be_u64(offsets.get(idx * 8..idx * 8 + 8).unwrap_or(&[]))? as usize;
    let end = be_u64(offsets.get((idx + 1) * 8..(idx + 2) * 8).unwrap_or(&[]))? as usize;
    data.get(start..end)
        .ok_or_else(|| StoreError::InvalidFormat(format!("record {id} slice out of bounds")))
}

pub(crate) fn parse_section_table(
    data: &[u8],
) -> Result<[(usize, usize); SECTION_COUNT], StoreError> {
    if data.len() < HEADER_SIZE || data.get(..4) != Some(&MAGIC[..]) {
        return Err(StoreError::InvalidFormat(
            "invalid mmap database magic".into(),
        ));
    }
    if be_u32(data.get(4..8).unwrap_or(&[]))? != FORMAT_VERSION
        || be_u32(data.get(8..12).unwrap_or(&[]))? != SECTION_COUNT as u32
    {
        return Err(StoreError::InvalidFormat(
            "unsupported mmap format version".into(),
        ));
    }
    let mut table = [(0usize, 0usize); SECTION_COUNT];
    for (idx, slot) in table.iter_mut().enumerate() {
        let pos = HEADER_FIXED_SIZE + idx * SECTION_ENTRY_SIZE;
        let off = usize::try_from(be_u64(data.get(pos..pos + 8).unwrap_or(&[]))?)
            .map_err(|_| StoreError::InvalidFormat("section offset overflow".into()))?;
        let sz = usize::try_from(be_u64(data.get(pos + 8..pos + 16).unwrap_or(&[]))?)
            .map_err(|_| StoreError::InvalidFormat("section size overflow".into()))?;
        if off > data.len() || sz > data.len() - off {
            return Err(StoreError::InvalidFormat(format!(
                "section {idx} out of bounds"
            )));
        }
        *slot = (off, sz);
    }
    Ok(table)
}

pub(crate) fn validate_dataset(
    data: &[u8],
    sections: &[(usize, usize); SECTION_COUNT],
) -> Result<Metadata, StoreError> {
    let slice = |s: Section| -> &[u8] {
        let (o, l) = sections.get(s as usize).copied().unwrap_or((0, 0));
        data.get(o..o + l).unwrap_or(&[])
    };
    let meta: Metadata = serde_json::from_slice(slice(Section::Metadata))
        .map_err(|e| StoreError::InvalidFormat(format!("decode metadata: {e}")))?;
    if meta.schema_version != SCHEMA_VERSION {
        return Err(StoreError::InvalidFormat(
            "unsupported schema version".into(),
        ));
    }
    StringPool::new(
        slice(Section::StringPoolOffsets),
        slice(Section::StringPoolData),
    )?;
    for (offs, dat) in [
        (Section::AllocationOffsets, Section::AllocationData),
        (Section::RouteOffsets, Section::RouteData),
        (Section::GeofeedOffsets, Section::GeofeedData),
    ] {
        let o = slice(offs);
        if o.len() < 8
            || !o.len().is_multiple_of(8)
            || be_u64(o.get(o.len() - 8..).unwrap_or(&[]))? != slice(dat).len() as u64
        {
            return Err(StoreError::InvalidFormat("offset table mismatch".into()));
        }
    }
    validate_strides(slice)?;
    validate_lpms(slice)?;
    Ok(meta)
}

fn validate_lpms<'a>(slice: impl Fn(Section) -> &'a [u8]) -> Result<(), StoreError> {
    validate_selection_lpm(slice(Section::AllocLpmV4), 4)?;
    validate_selection_lpm(slice(Section::AllocLpmV6), 6)?;
    validate_selection_lpm(slice(Section::GeoLpmV4), 4)?;
    validate_selection_lpm(slice(Section::GeoLpmV6), 6)?;
    validate_route_lpm(slice(Section::RouteLpmV4), 4)?;
    validate_route_lpm(slice(Section::RouteLpmV6), 6)
}

fn validate_strides<'a>(slice: impl Fn(Section) -> &'a [u8]) -> Result<(), StoreError> {
    for (sec, stride) in [
        (Section::AutnumIndex, AUTNUM_INDEX_STRIDE),
        (Section::AsnRouteIndex, ASN_ROUTE_INDEX_STRIDE),
        (Section::AsnRouteIds, 4),
        (Section::AllocPrefixV4, PREFIX_V4_STRIDE),
        (Section::AllocPrefixV6, PREFIX_V6_STRIDE),
        (Section::RoutePrefixV4, PREFIX_V4_STRIDE),
        (Section::RoutePrefixV6, PREFIX_V6_STRIDE),
        (Section::GeofeedPrefixV4, PREFIX_V4_STRIDE),
        (Section::GeofeedPrefixV6, PREFIX_V6_STRIDE),
        (Section::AllocRangeV4, RANGE_V4_STRIDE),
        (Section::AllocRangeV6, RANGE_V6_STRIDE),
        (Section::RouteRangeV4, RANGE_V4_STRIDE),
        (Section::RouteRangeV6, RANGE_V6_STRIDE),
        (Section::SummaryIndex, SUMMARY_INDEX_STRIDE),
    ] {
        if !slice(sec).len().is_multiple_of(stride) {
            return Err(StoreError::InvalidFormat(format!(
                "section {sec:?} misaligned"
            )));
        }
    }
    Ok(())
}

pub(crate) fn warm_selector_slices<'a>(slices: impl Iterator<Item = &'a [u8]>) -> u64 {
    let mut sink = 0u8;
    let mut total = 0u64;
    for slice in slices {
        total += slice.len() as u64;
        for chunk in slice.chunks(4096) {
            if let Some(first) = chunk.first() {
                sink ^= *first;
            }
        }
        if let Some(last) = slice.last() {
            sink ^= *last;
        }
    }
    std::hint::black_box(sink);
    total
}

pub(crate) fn warm_file_page_cache(path: &Path) -> Result<u64, StoreError> {
    let mut file = File::open(path)?;
    let mut buf = vec![0u8; 4 << 20];
    let mut total = 0u64;
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            return Ok(total);
        }
        total += n as u64;
    }
}

pub(crate) fn slice_asn_routes(
    slice: &[u8],
    count: usize,
    numbered: bool,
    page_number: usize,
    cursor: i64,
    limit: usize,
) -> Result<(i64, Vec<u32>), StoreError> {
    let start = if numbered {
        page_number
            .saturating_sub(1)
            .saturating_mul(limit)
            .min(count)
    } else if cursor > 0 {
        lower_bound_stride(slice, 4, |row| {
            Ok(i64::from(be_u32(row)?).cmp(&(cursor + 1)))
        })?
    } else {
        0
    };
    let end = start
        .saturating_add(limit + usize::from(!numbered))
        .min(count);
    let mut ids = Vec::with_capacity(end.saturating_sub(start));
    for idx in start..end {
        ids.push(be_u32(slice.get(idx * 4..idx * 4 + 4).unwrap_or(&[]))?);
    }
    Ok((count as i64, ids))
}
