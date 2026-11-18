use std::collections::HashMap;

use crate::resource_model::RangeKind;
use crate::resources::{Page, RangePage};
use crate::service::ApiError;

pub(crate) fn parse_query_map(query: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for pair in query.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        map.entry(percent_decode(k))
            .or_insert_with(|| percent_decode(v));
    }
    map
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
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
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

pub(crate) fn parse_page(q: &HashMap<String, String>) -> Result<Page, ApiError> {
    let mut page = Page {
        cursor: 0,
        limit: 50,
        number: 0,
        numbered: false,
    };
    if let Some(raw) = q.get("limit").filter(|s| !s.is_empty()) {
        let limit: usize = raw
            .parse()
            .ok()
            .filter(|&v| (1..=100).contains(&v))
            .ok_or_else(|| ApiError::BadRequest {
                code: "INVALID_LIMIT",
                message: "limit must be between 1 and 100".into(),
            })?;
        page.limit = limit;
    }
    if let Some(raw) = q.get("cursor").filter(|s| !s.is_empty()) {
        let cursor: i64 =
            raw.parse()
                .ok()
                .filter(|&v| v >= 0)
                .ok_or_else(|| ApiError::BadRequest {
                    code: "INVALID_CURSOR",
                    message: "cursor must be a non-negative record cursor".into(),
                })?;
        page.cursor = cursor;
    }
    Ok(page)
}

pub(crate) fn parse_asn_page(q: &HashMap<String, String>) -> Result<Page, ApiError> {
    let mut page = parse_page(q)?;
    let Some(raw_page) = q.get("page").filter(|s| !s.is_empty()) else {
        return Ok(page);
    };
    if q.get("cursor").is_some_and(|s| !s.is_empty()) {
        return Err(ApiError::BadRequest {
            code: "INVALID_PAGINATION",
            message: "page and cursor cannot be used together".into(),
        });
    }
    let number: usize = raw_page
        .parse()
        .ok()
        .filter(|&v| (1..=100_000).contains(&v))
        .ok_or_else(|| ApiError::BadRequest {
            code: "INVALID_PAGE",
            message: "page must be between 1 and 100000".into(),
        })?;
    page.number = number;
    page.numbered = true;
    Ok(page)
}

pub(crate) fn parse_range_page(q: &HashMap<String, String>) -> Result<RangePage, ApiError> {
    let mut limit = 50usize;
    if let Some(raw) = q.get("limit").filter(|s| !s.is_empty()) {
        limit = raw
            .parse()
            .ok()
            .filter(|&v| (1..=100).contains(&v))
            .ok_or_else(|| ApiError::BadRequest {
                code: "INVALID_LIMIT",
                message: "limit must be between 1 and 100".into(),
            })?;
    }
    let cursor = q.get("cursor").cloned().unwrap_or_default();
    if cursor.len() > 256 {
        return Err(ApiError::BadRequest {
            code: "INVALID_CURSOR",
            message: "cursor is invalid".into(),
        });
    }
    Ok(RangePage { cursor, limit })
}

pub(crate) fn parse_range_kind(q: &HashMap<String, String>) -> Result<RangeKind, ApiError> {
    match q.get("kind").map(String::as_str).unwrap_or("") {
        "" | "allocations" => Ok(RangeKind::Allocations),
        "routes" => Ok(RangeKind::Routes),
        _ => Err(ApiError::BadRequest {
            code: "INVALID_RANGE_KIND",
            message: "kind must be allocations or routes".into(),
        }),
    }
}

pub(crate) fn parse_asn_query(input: &str) -> Result<u32, ApiError> {
    let trimmed = input.trim().to_ascii_uppercase();
    let num_str = trimmed.strip_prefix("AS").unwrap_or(&trimmed);
    num_str
        .parse::<u32>()
        .ok()
        .filter(|&n| n > 0)
        .ok_or_else(|| ApiError::BadRequest {
            code: "INVALID_ASN",
            message: "ASN must be a positive AS number, with or without the AS prefix".into(),
        })
}
