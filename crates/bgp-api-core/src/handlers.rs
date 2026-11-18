use std::collections::HashMap;

use bytes::Bytes;
use serde::Serialize;

use crate::asn::lookup_asn;
use crate::ipkey::{parse_prefix, parse_range, parse_runtime_ip, summary_prefix_key};
use crate::model::{ErrorBody, ErrorResponse, HealthResponse, RuntimeInfo};
use crate::params::{
    parse_asn_page, parse_asn_query, parse_page, parse_query_map, parse_range_kind,
    parse_range_page,
};
use crate::resources::{lookup_prefix, lookup_range, lookup_range_summary};
use crate::service::{ApiError, dataset_meta_from_db, lookup_ip};
use crate::{ApiRequest, ApiResponse, ApiState};

pub(crate) async fn dispatch(state: &ApiState, req: &ApiRequest<'_>) -> ApiResponse {
    if let Some(expected) = &state.config.origin_auth_token
        && req.origin_token != Some(expected.as_str())
    {
        return json_error(ApiError::Unauthorized, None);
    }
    let cors_origin = req
        .origin
        .filter(|orig| state.config.allowed_origins.contains(*orig))
        .map(str::to_owned);
    if req.method == "OPTIONS" {
        return ApiResponse {
            status: 204,
            cors_origin,
            is_options: true,
            body: Bytes::new(),
        };
    }
    if req.method != "GET" {
        return json_error(
            ApiError::NotFound {
                code: "NOT_FOUND",
                message: "route not found".into(),
            },
            cors_origin,
        );
    }
    let result = route_get(state, req).await;
    match result {
        Ok(body) => ApiResponse {
            status: 200,
            cors_origin,
            is_options: false,
            body,
        },
        Err(err) => json_error(err, cors_origin),
    }
}

async fn route_get(state: &ApiState, req: &ApiRequest<'_>) -> Result<Bytes, ApiError> {
    let q = parse_query_map(req.query);
    match req.path {
        "/" => encode_json(&serde_json::json!({ "ok": true, "service": "bgp-api", "version": 1 })),
        "/v1/health" => handle_health(state),
        "/v1/ip" => handle_ip(state, &q),
        "/v1/prefix" => handle_prefix(state, &q).await,
        "/v1/range" => handle_range(state, &q).await,
        "/v1/asn" => handle_asn(state, &q),
        _ => Err(ApiError::NotFound {
            code: "NOT_FOUND",
            message: "route not found".into(),
        }),
    }
}

fn handle_health(state: &ApiState) -> Result<Bytes, ApiError> {
    let resp = HealthResponse {
        ok: true,
        service: "bgp-api",
        version: 1,
        database: "mmap",
        build: Some(state.config.build.clone()),
        dataset: Some(dataset_meta_from_db(&state.db)),
        runtime: state.config.runtime_cache_control.then_some(RuntimeInfo {
            cache_control: true,
        }),
    };
    encode_json(&resp)
}

fn handle_ip(state: &ApiState, q: &HashMap<String, String>) -> Result<Bytes, ApiError> {
    let raw_ip = q.get("query").map(String::as_str).unwrap_or("");
    let ip = parse_runtime_ip(raw_ip).ok_or_else(|| ApiError::BadRequest {
        code: "INVALID_IP",
        message: "query must be a valid IPv4 or IPv6 address".into(),
    })?;
    let details = q.get("details").map(|s| s.trim()).unwrap_or("");
    let full = match details {
        "" => false,
        "full" => true,
        _ => {
            return Err(ApiError::BadRequest {
                code: "INVALID_DETAILS",
                message: "details must be full when present".into(),
            });
        }
    };
    if !full && let Some(cached) = state.caches.compact.get(&ip.canonical) {
        return Ok(cached);
    }
    let body = encode_json(&lookup_ip(&state.db, &ip, full)?)?;
    if !full {
        state.caches.compact.insert(ip.canonical, body.clone());
    }
    Ok(body)
}

async fn handle_prefix(state: &ApiState, q: &HashMap<String, String>) -> Result<Bytes, ApiError> {
    let raw = q.get("prefix").map(String::as_str).unwrap_or("");
    let prefix = parse_prefix(raw).ok_or_else(|| ApiError::BadRequest {
        code: "INVALID_PREFIX",
        message: "prefix must be a valid IPv4 or IPv6 CIDR".into(),
    })?;
    let page = parse_page(q)?;
    let key = format!(
        "prefix\0{}\0{}\0{}",
        prefix.canonical, page.limit, page.cursor
    );
    if let Some(cached) = state.caches.resource.get(&key) {
        return Ok(cached);
    }
    let _permit = state
        .overlap_slots
        .acquire()
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;
    let body = encode_json(&lookup_prefix(&state.db, &prefix, page).map_err(|e| {
        too_broad_msg(
            e,
            "prefix matches too many source records; request a narrower prefix",
        )
    })?)?;
    state.caches.resource.insert(key, body.clone());
    Ok(body)
}

async fn handle_range(state: &ApiState, q: &HashMap<String, String>) -> Result<Bytes, ApiError> {
    let start = q.get("start").map(String::as_str).unwrap_or("");
    let end = q.get("end").map(String::as_str).unwrap_or("");
    let range = parse_range(start, end).ok_or_else(|| ApiError::BadRequest {
        code: "INVALID_RANGE",
        message: "start and end must be valid addresses in the same IP version, with start not greater than end".into(),
    })?;
    let kind = parse_range_kind(q)?;
    let page = parse_range_page(q)?;
    let broad = summary_prefix_key(&range).is_some();
    let kind_str = if matches!(kind, crate::resource_model::RangeKind::Allocations) {
        "allocations"
    } else {
        "routes"
    };
    let key = if broad {
        format!("range\0{}\0{}\0{kind_str}", range.start, range.end)
    } else {
        format!(
            "range\0{}\0{}\0{kind_str}\0{}\0{}",
            range.start, range.end, page.limit, page.cursor
        )
    };
    if let Some(cached) = state.caches.resource.get(&key) {
        return Ok(cached);
    }
    let resp = if broad {
        lookup_range_summary(&state.db, &range, kind)?
    } else {
        let _permit = state
            .overlap_slots
            .acquire()
            .await
            .map_err(|e| ApiError::Internal(e.to_string()))?;
        lookup_range(&state.db, &range, kind, &page).map_err(|e| {
            too_broad_msg(
                e,
                "range matches too many source records; use a canonical IPv4 CIDR from /0 through /16 for a generated summary, or request a narrower range",
            )
        })?
    };
    let body = encode_json(&resp)?;
    state.caches.resource.insert(key, body.clone());
    Ok(body)
}

fn handle_asn(state: &ApiState, q: &HashMap<String, String>) -> Result<Bytes, ApiError> {
    let asn = parse_asn_query(q.get("query").map(String::as_str).unwrap_or(""))?;
    let page = parse_asn_page(q)?;
    let mode = if page.numbered {
        format!("page\0{}", page.number)
    } else {
        format!("cursor\0{}", page.cursor)
    };
    let key = format!("asn\0{asn}\0{mode}\0{}", page.limit);
    if let Some(cached) = state.caches.resource.get(&key) {
        return Ok(cached);
    }
    let Some(resp) = lookup_asn(&state.db, asn, page)? else {
        return Err(ApiError::NotFound {
            code: "ASN_NOT_FOUND",
            message: "no route or aut-num object matched this ASN".into(),
        });
    };
    let body = encode_json(&resp)?;
    state.caches.resource.insert(key, body.clone());
    Ok(body)
}

fn too_broad_msg(err: ApiError, msg: &str) -> ApiError {
    match err {
        ApiError::TooBroad(_) => ApiError::TooBroad(msg.to_owned()),
        other => other,
    }
}

fn encode_json<T: Serialize>(value: &T) -> Result<Bytes, ApiError> {
    let mut vec = serde_json::to_vec(value).map_err(|e| ApiError::Internal(e.to_string()))?;
    vec.push(b'\n');
    Ok(Bytes::from(vec))
}

fn json_error(err: ApiError, cors_origin: Option<String>) -> ApiResponse {
    let (status, code, message) = match err {
        ApiError::BadRequest { code, message } => (400, code, message),
        ApiError::Unauthorized => (401, "UNAUTHORIZED", "origin authorization required".into()),
        ApiError::NotFound { code, message } => (404, code, message),
        ApiError::TooBroad(msg) => (422, "QUERY_TOO_BROAD", msg),
        ApiError::Internal(detail) => {
            eprintln!("internal lookup error: {detail}");
            (500, "INTERNAL_ERROR", "unexpected lookup failure".into())
        }
    };
    let body = encode_json(&ErrorResponse {
        error: ErrorBody { code, message },
    })
    .unwrap_or_else(|_| Bytes::from_static(b"{\"error\":{\"code\":\"INTERNAL_ERROR\"}}\n"));
    ApiResponse {
        status,
        cors_origin,
        is_options: false,
        body,
    }
}
