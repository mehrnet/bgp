use crate::codec::{Reader, StringPool};
use crate::types::{Allocation, Autnum, Facet, Geofeed, RangeSummary, Route, StoreError};

pub(crate) fn decode_allocation(
    raw: &[u8],
    pool: &StringPool<'_>,
    lookup_only: bool,
) -> Result<Allocation, StoreError> {
    let mut r = Reader::new(raw);
    let version = r.byte()?;
    let mut out = Allocation {
        version,
        start: r.address(version)?,
        end: r.address(version)?,
        registry: r.pooled_str(pool)?,
        country: r.pooled_str(pool)?,
        name: r.inline_str()?,
        allocation_date: r.pooled_str(pool)?,
        status: r.pooled_str(pool)?,
        abuse_contact: r.pooled_str(pool)?,
        ..Allocation::default()
    };
    if lookup_only {
        return Ok(out);
    }
    out.source = r.pooled_str(pool)?;
    out.maintainers = r.pooled_str(pool)?;
    out.organization = r.pooled_str(pool)?;
    out.created = r.inline_str()?;
    out.last_modified = r.inline_str()?;
    out.description = r.inline_str()?;
    r.done()?;
    Ok(out)
}

pub(crate) fn decode_route(
    raw: &[u8],
    pool: &StringPool<'_>,
    lookup_only: bool,
) -> Result<Route, StoreError> {
    let mut r = Reader::new(raw);
    let version = r.byte()?;
    let prefix_length = r.byte()?;
    let network = r.address(version)?;
    let as_number = u32::try_from(r.uvarint()?)
        .map_err(|_| StoreError::InvalidFormat("ASN overflow".into()))?;
    let mut out = Route {
        version,
        prefix_length,
        network,
        as_number,
        origin_asn: r.pooled_str(pool)?,
        abuse_contact: r.pooled_str(pool)?,
        ..Route::default()
    };
    if lookup_only {
        return Ok(out);
    }
    out.registry = r.pooled_str(pool)?;
    out.source = r.pooled_str(pool)?;
    out.maintainers = r.pooled_str(pool)?;
    out.organization = r.pooled_str(pool)?;
    out.description = r.inline_str()?;
    r.done()?;
    Ok(out)
}

pub(crate) fn decode_geofeed(raw: &[u8], pool: &StringPool<'_>) -> Result<Geofeed, StoreError> {
    let mut r = Reader::new(raw);
    let version = r.byte()?;
    let start = r.address(version)?;
    let end = r.address(version)?;
    let country = r.pooled_str(pool)?;
    let region = r.pooled_str(pool)?;
    let city = r.pooled_str(pool)?;
    let flags = r.byte()?;
    let latitude = (flags & 1 != 0).then(|| r.f64_be()).transpose()?;
    let longitude = (flags & 2 != 0).then(|| r.f64_be()).transpose()?;
    let out = Geofeed {
        version,
        start,
        end,
        country,
        region,
        city,
        latitude,
        longitude,
        is_fallback: (flags & 4) != 0,
    };
    r.done()?;
    Ok(out)
}

pub(crate) fn decode_autnum(
    raw: &[u8],
    pool: &StringPool<'_>,
    identity_only: bool,
) -> Result<Autnum, StoreError> {
    let mut r = Reader::new(raw);
    let as_number = u32::try_from(r.uvarint()?)
        .map_err(|_| StoreError::InvalidFormat("ASN overflow".into()))?;
    let mut out = Autnum {
        as_number,
        name: r.inline_str()?,
        organization: r.pooled_str(pool)?,
        ..Autnum::default()
    };
    if identity_only {
        return Ok(out);
    }
    out.asn = r.pooled_str(pool)?;
    out.registry = r.pooled_str(pool)?;
    out.country = r.pooled_str(pool)?;
    out.status = r.pooled_str(pool)?;
    out.source = r.pooled_str(pool)?;
    out.maintainers = r.pooled_str(pool)?;
    out.abuse_contact = r.pooled_str(pool)?;
    out.created = r.inline_str()?;
    out.last_modified = r.inline_str()?;
    out.description = r.inline_str()?;
    r.done()?;
    Ok(out)
}

pub(crate) fn decode_summary(raw: &[u8]) -> Result<RangeSummary, StoreError> {
    let mut r = Reader::new(raw);
    let prefix_length = r.byte()?;
    let allocation_records = r.uvarint()?;
    let routes = r.uvarint()?;
    let countries = decode_facets(&mut r)?;
    let asns = decode_facets(&mut r)?;
    r.done()?;
    Ok(RangeSummary {
        prefix_length,
        allocation_records,
        routes,
        countries,
        asns,
    })
}

fn decode_facets(r: &mut Reader<'_>) -> Result<Vec<Facet>, StoreError> {
    let count = r.uvarint()?;
    if count > 1_000_000 {
        return Err(StoreError::InvalidFormat("invalid facet count".into()));
    }
    let mut items = Vec::with_capacity(count as usize);
    for _ in 0..count {
        items.push(Facet {
            value: r.inline_str()?,
            count: r.uvarint()?,
        });
    }
    Ok(items)
}
