use crate::borrowed::{AllocationRef, AutnumRef, GeofeedRef, RouteRef};
use crate::codec::{Reader, StringPool};
use crate::types::{Facet, RangeSummary, StoreError};

pub(crate) fn decode_allocation_ref<'a>(
    raw: &'a [u8],
    pool: &StringPool<'a>,
    lookup_only: bool,
) -> Result<AllocationRef<'a>, StoreError> {
    let mut r = Reader::new(raw);
    let version = r.byte()?;
    let mut out = AllocationRef {
        version,
        start: r.address(version)?,
        end: r.address(version)?,
        registry: r.pooled_cow(pool)?,
        country: r.pooled_cow(pool)?,
        name: r.inline_cow()?,
        allocation_date: r.pooled_cow(pool)?,
        status: r.pooled_cow(pool)?,
        abuse_contact: r.pooled_cow(pool)?,
        ..AllocationRef::default()
    };
    if lookup_only {
        return Ok(out);
    }
    out.source = r.pooled_cow(pool)?;
    out.maintainers = r.pooled_cow(pool)?;
    out.organization = r.pooled_cow(pool)?;
    out.created = r.inline_cow()?;
    out.last_modified = r.inline_cow()?;
    out.description = r.inline_cow()?;
    r.done()?;
    Ok(out)
}

pub(crate) fn decode_route_ref<'a>(
    raw: &'a [u8],
    pool: &StringPool<'a>,
    lookup_only: bool,
) -> Result<RouteRef<'a>, StoreError> {
    let mut r = Reader::new(raw);
    let version = r.byte()?;
    let prefix_length = r.byte()?;
    let network = r.address(version)?;
    let as_number = u32::try_from(r.uvarint()?)
        .map_err(|_| StoreError::InvalidFormat("ASN overflow".into()))?;
    let mut out = RouteRef {
        version,
        prefix_length,
        network,
        as_number,
        origin_asn: r.pooled_cow(pool)?,
        abuse_contact: r.pooled_cow(pool)?,
        ..RouteRef::default()
    };
    if lookup_only {
        return Ok(out);
    }
    out.registry = r.pooled_cow(pool)?;
    out.source = r.pooled_cow(pool)?;
    out.maintainers = r.pooled_cow(pool)?;
    out.organization = r.pooled_cow(pool)?;
    out.description = r.inline_cow()?;
    r.done()?;
    Ok(out)
}

pub(crate) fn decode_geofeed_ref<'a>(
    raw: &'a [u8],
    pool: &StringPool<'a>,
) -> Result<GeofeedRef<'a>, StoreError> {
    let mut r = Reader::new(raw);
    let version = r.byte()?;
    let start = r.address(version)?;
    let end = r.address(version)?;
    let country = r.pooled_cow(pool)?;
    let region = r.pooled_cow(pool)?;
    let city = r.pooled_cow(pool)?;
    let flags = r.byte()?;
    let latitude = (flags & 1 != 0).then(|| r.f64_be()).transpose()?;
    let longitude = (flags & 2 != 0).then(|| r.f64_be()).transpose()?;
    let out = GeofeedRef {
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

pub(crate) fn decode_autnum_ref<'a>(
    raw: &'a [u8],
    pool: &StringPool<'a>,
    identity_only: bool,
) -> Result<AutnumRef<'a>, StoreError> {
    let mut r = Reader::new(raw);
    let as_number = u32::try_from(r.uvarint()?)
        .map_err(|_| StoreError::InvalidFormat("ASN overflow".into()))?;
    let mut out = AutnumRef {
        as_number,
        name: r.inline_cow()?,
        organization: r.pooled_cow(pool)?,
        ..AutnumRef::default()
    };
    if identity_only {
        return Ok(out);
    }
    out.asn = r.pooled_cow(pool)?;
    out.registry = r.pooled_cow(pool)?;
    out.country = r.pooled_cow(pool)?;
    out.status = r.pooled_cow(pool)?;
    out.source = r.pooled_cow(pool)?;
    out.maintainers = r.pooled_cow(pool)?;
    out.abuse_contact = r.pooled_cow(pool)?;
    out.created = r.inline_cow()?;
    out.last_modified = r.inline_cow()?;
    out.description = r.inline_cow()?;
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
