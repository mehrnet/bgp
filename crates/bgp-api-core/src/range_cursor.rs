use std::collections::BTreeSet;
use std::net::IpAddr;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use mnmf_store::Db;
use mnmf_store::range_index::{put_range_key, range_key_start_key};
use mnmf_store::types::{Address, RANGE_KEY_SIZE, RangeIndexEntry, RangeQuery, TableKind};

use crate::details::route_bounds;
use crate::ipkey::ParsedRange;
use crate::resource_model::RangeKind;
use crate::resources::RangePage;
use crate::service::ApiError;

const RANGE_CURSOR_COVERING: u8 = 1;
const RANGE_CURSOR_DIRECT: u8 = 2;
const RANGE_CURSOR_SIZE: usize = 1 + 1 + 16 + 16 + RANGE_KEY_SIZE;

#[derive(Debug)]
struct RangeCursor {
    phase: u8,
    kind: RangeKind,
    query_start: Address,
    query_end: Address,
    key: [u8; RANGE_KEY_SIZE],
}

pub(crate) fn range_page_entries(
    db: &Db,
    value: &ParsedRange,
    kind: RangeKind,
    page: &RangePage,
) -> Result<(Vec<RangeIndexEntry>, Option<String>), ApiError> {
    let cursor = decode_range_cursor(&page.cursor)?;
    let start = Address::from_ip(value.start);
    let end = Address::from_ip(value.end);
    if let Some(c) = &cursor
        && (c.kind != kind
            || c.query_start != start
            || c.query_end != end
            || c.key.first().copied().unwrap_or(0) != value.version)
    {
        return Err(invalid_cursor());
    }
    let table_kind = if kind == RangeKind::Allocations {
        TableKind::Allocations
    } else {
        TableKind::Routes
    };
    let mut entries: Vec<RangeIndexEntry> = Vec::with_capacity(page.limit);
    let phase = cursor.as_ref().map_or(0, |c| c.phase);
    if phase != RANGE_CURSOR_DIRECT {
        for entry in covering_range_entries(db, table_kind, kind, value.start)? {
            if cursor
                .as_ref()
                .is_some_and(|c| c.phase == RANGE_CURSOR_COVERING && entry.key <= c.key)
            {
                continue;
            }
            if entries.len() == page.limit {
                let tok = entries
                    .last()
                    .map(|e| encode_cursor(RANGE_CURSOR_COVERING, kind, start, end, &e.key));
                return Ok((entries, tok));
            }
            entries.push(entry);
        }
    }
    append_direct_entries(db, value, kind, cursor.as_ref(), page.limit, &mut entries)
}

fn append_direct_entries(
    db: &Db,
    value: &ParsedRange,
    kind: RangeKind,
    cursor: Option<&RangeCursor>,
    limit: usize,
    entries: &mut Vec<RangeIndexEntry>,
) -> Result<(Vec<RangeIndexEntry>, Option<String>), ApiError> {
    let start = Address::from_ip(value.start);
    let end = Address::from_ip(value.end);
    let table_kind = if kind == RangeKind::Allocations {
        TableKind::Allocations
    } else {
        TableKind::Routes
    };
    let after = cursor
        .filter(|c| c.phase == RANGE_CURSOR_DIRECT)
        .map(|c| &c.key);
    let (mut direct, mut more) = db.direct_range_entries(RangeQuery {
        kind: table_kind,
        version: value.version,
        start,
        end,
        after,
        maximum: limit - entries.len() + 1,
    })?;
    if entries.len() == limit {
        if direct.is_empty() {
            return Ok((std::mem::take(entries), None));
        }
        let mut first = [0u8; RANGE_KEY_SIZE];
        range_key_start_key(&mut first, value.version, start);
        return Ok((
            std::mem::take(entries),
            Some(encode_cursor(RANGE_CURSOR_DIRECT, kind, start, end, &first)),
        ));
    }
    let cap = limit - entries.len();
    if direct.len() > cap {
        direct.truncate(cap);
        more = true;
    }
    entries.extend(direct);
    let next = if more {
        entries
            .last()
            .map(|e| encode_cursor(RANGE_CURSOR_DIRECT, kind, start, end, &e.key))
    } else {
        None
    };
    Ok((std::mem::take(entries), next))
}

fn covering_range_entries(
    db: &Db,
    table_kind: TableKind,
    kind: RangeKind,
    query_start_ip: IpAddr,
) -> Result<Vec<RangeIndexEntry>, ApiError> {
    let query_start = Address::from_ip(query_start_ip);
    let ids = db.matching_prefix_ids(table_kind, query_start_ip, 0)?;
    let mut seen = BTreeSet::new();
    let mut entries = Vec::with_capacity(ids.len());
    for id in ids {
        if !seen.insert(id) {
            continue;
        }
        let (start, end, ver) = if kind == RangeKind::Allocations {
            let r = db.allocation_ref(id, false)?;
            (r.start, r.end, r.version)
        } else {
            let r = db.route_ref(id, false)?;
            let (s, e) = route_bounds(&r);
            (s, e, r.version)
        };
        if start < query_start && end >= query_start {
            let mut key = [0u8; RANGE_KEY_SIZE];
            put_range_key(&mut key, ver, start, end, id);
            entries.push(RangeIndexEntry { id, key });
        }
    }
    entries.sort_by_key(|a| a.key);
    Ok(entries)
}

fn encode_cursor(
    phase: u8,
    kind: RangeKind,
    start: Address,
    end: Address,
    key: &[u8; RANGE_KEY_SIZE],
) -> String {
    let mut payload = [0u8; RANGE_CURSOR_SIZE];
    payload[0] = phase;
    payload[1] = if kind == RangeKind::Allocations { 1 } else { 2 };
    payload[2..18].copy_from_slice(&start.0);
    payload[18..34].copy_from_slice(&end.0);
    payload[34..].copy_from_slice(key);
    URL_SAFE_NO_PAD.encode(payload)
}

fn decode_range_cursor(raw: &str) -> Result<Option<RangeCursor>, ApiError> {
    if raw.is_empty() {
        return Ok(None);
    }
    let bytes = URL_SAFE_NO_PAD.decode(raw).map_err(|_| invalid_cursor())?;
    if bytes.len() != RANGE_CURSOR_SIZE {
        return Err(invalid_cursor());
    }
    let phase = bytes.first().copied().unwrap_or(0);
    if phase != RANGE_CURSOR_COVERING && phase != RANGE_CURSOR_DIRECT {
        return Err(invalid_cursor());
    }
    let kind = match bytes.get(1).copied() {
        Some(1) => RangeKind::Allocations,
        Some(2) => RangeKind::Routes,
        _ => return Err(invalid_cursor()),
    };
    let mut qs = [0u8; 16];
    let mut qe = [0u8; 16];
    let mut key = [0u8; RANGE_KEY_SIZE];
    qs.copy_from_slice(bytes.get(2..18).ok_or_else(invalid_cursor)?);
    qe.copy_from_slice(bytes.get(18..34).ok_or_else(invalid_cursor)?);
    key.copy_from_slice(bytes.get(34..).ok_or_else(invalid_cursor)?);
    Ok(Some(RangeCursor {
        phase,
        kind,
        query_start: Address(qs),
        query_end: Address(qe),
        key,
    }))
}

fn invalid_cursor() -> ApiError {
    ApiError::BadRequest {
        code: "INVALID_CURSOR",
        message: "cursor is invalid for this range lookup".into(),
    }
}
