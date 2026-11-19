use std::borrow::Cow;

use crate::resource_model::RangeKind;
use crate::resources::{Page, RangePage};
use crate::service::ApiError;

pub(crate) fn query_param<'a>(query: &'a str, target: &str) -> Option<Cow<'a, str>> {
    for pair in query.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        if percent_decode(k).as_ref() == target {
            return Some(percent_decode(v));
        }
    }
    None
}

fn percent_decode(input: &str) -> Cow<'_, str> {
    let bytes = input.as_bytes();
    if !bytes.iter().any(|&b| b == b'%' || b == b'+') {
        return Cow::Borrowed(input);
    }
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while let Some(&b) = bytes.get(i) {
        if b == b'+' {
            out.push(b' ');
            i += 1;
        } else if b == b'%'
            && let (Some(&hi), Some(&lo)) = (bytes.get(i + 1), bytes.get(i + 2))
            && let (Some(h), Some(l)) = (hex_val(hi), hex_val(lo))
        {
            out.push((h << 4) | l);
            i += 3;
        } else {
            out.push(b);
            i += 1;
        }
    }
    Cow::Owned(String::from_utf8_lossy(&out).into_owned())
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn parse_limit_str(raw: &str) -> Result<usize, ApiError> {
    raw.parse()
        .ok()
        .filter(|&v| (1..=100).contains(&v))
        .ok_or_else(|| ApiError::BadRequest {
            code: "INVALID_LIMIT",
            message: "limit must be between 1 and 100".into(),
        })
}

pub(crate) fn parse_page(query: &str) -> Result<Page, ApiError> {
    let mut page = Page {
        cursor: 0,
        limit: 50,
        number: 0,
        numbered: false,
    };
    if let Some(raw) = query_param(query, "limit").filter(|s| !s.is_empty()) {
        page.limit = parse_limit_str(&raw)?;
    }
    if let Some(raw) = query_param(query, "cursor").filter(|s| !s.is_empty()) {
        page.cursor = raw
            .parse()
            .ok()
            .filter(|&v| v >= 0)
            .ok_or_else(|| ApiError::BadRequest {
                code: "INVALID_CURSOR",
                message: "cursor must be a non-negative record cursor".into(),
            })?;
    }
    Ok(page)
}

pub(crate) fn parse_asn_page(query: &str) -> Result<Page, ApiError> {
    let mut page = parse_page(query)?;
    let Some(raw_page) = query_param(query, "page").filter(|s| !s.is_empty()) else {
        return Ok(page);
    };
    if query_param(query, "cursor").is_some_and(|s| !s.is_empty()) {
        return Err(ApiError::BadRequest {
            code: "INVALID_PAGINATION",
            message: "page and cursor cannot be used together".into(),
        });
    }
    page.number = raw_page
        .parse()
        .ok()
        .filter(|&v| (1..=100_000).contains(&v))
        .ok_or_else(|| ApiError::BadRequest {
            code: "INVALID_PAGE",
            message: "page must be between 1 and 100000".into(),
        })?;
    page.numbered = true;
    Ok(page)
}

pub(crate) fn parse_range_page(query: &str) -> Result<RangePage, ApiError> {
    let limit = match query_param(query, "limit").filter(|s| !s.is_empty()) {
        Some(raw) => parse_limit_str(&raw)?,
        None => 50,
    };
    let cursor = query_param(query, "cursor")
        .map(Cow::into_owned)
        .unwrap_or_default();
    if cursor.len() > 256 {
        return Err(ApiError::BadRequest {
            code: "INVALID_CURSOR",
            message: "cursor is invalid".into(),
        });
    }
    Ok(RangePage { cursor, limit })
}

pub(crate) fn parse_range_kind(query: &str) -> Result<RangeKind, ApiError> {
    match query_param(query, "kind").as_deref().unwrap_or("") {
        "" | "allocations" => Ok(RangeKind::Allocations),
        "routes" => Ok(RangeKind::Routes),
        _ => Err(ApiError::BadRequest {
            code: "INVALID_RANGE_KIND",
            message: "kind must be allocations or routes".into(),
        }),
    }
}

pub(crate) fn parse_asn_query(input: &str) -> Result<u32, ApiError> {
    let trimmed = input.trim();
    let num_str = trimmed
        .strip_prefix("AS")
        .or_else(|| trimmed.strip_prefix("as"))
        .or_else(|| trimmed.strip_prefix("As"))
        .or_else(|| trimmed.strip_prefix("aS"))
        .unwrap_or(trimmed);
    num_str
        .parse::<u32>()
        .ok()
        .filter(|&n| n > 0)
        .ok_or_else(|| ApiError::BadRequest {
            code: "INVALID_ASN",
            message: "ASN must be a positive AS number, with or without the AS prefix".into(),
        })
}
