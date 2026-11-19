use std::borrow::Cow;

use bytes::Bytes;

use crate::asn::lookup_asn;
use crate::ipkey::{RuntimeIp, parse_ip_only, parse_prefix, parse_range, summary_prefix_key};
use crate::model::{HealthResponse, RuntimeInfo};
use crate::params::{
    parse_asn_page, parse_asn_query, parse_page, parse_range_kind, parse_range_page, query_param,
};
use crate::resources::{lookup_prefix, lookup_range, lookup_range_summary};
use crate::service::{ApiError, lookup_ip_with_meta};
use crate::{ApiRequest, ApiResponse, ApiState, encode_json, json_error};

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
    match req.path {
        "/" => encode_json(&serde_json::json!({ "ok": true, "service": "bgp-api", "version": 1 })),
        "/v1/health" => handle_health(state),
        "/v1/ip" => handle_ip(state, req.query),
        "/v1/prefix" => handle_prefix(state, req.query).await,
        "/v1/range" => handle_range(state, req.query).await,
        "/v1/asn" => handle_asn(state, req.query),
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
        build: Some(Cow::Borrowed(&state.config.build)),
        dataset: Some(Cow::Borrowed(&state.dataset_meta)),
        runtime: state.config.runtime_cache_control.then_some(RuntimeInfo {
            cache_control: true,
        }),
    };
    encode_json(&resp)
}

fn handle_ip(state: &ApiState, query: &str) -> Result<Bytes, ApiError> {
    let raw_ip = query_param(query, "query");
    let ip =
        parse_ip_only(raw_ip.as_deref().unwrap_or("")).ok_or_else(|| ApiError::BadRequest {
            code: "INVALID_IP",
            message: "query must be a valid IPv4 or IPv6 address".into(),
        })?;
    let details_val = query_param(query, "details");
    let full = match details_val.as_deref().map(str::trim).unwrap_or("") {
        "" => false,
        "full" => true,
        _ => {
            return Err(ApiError::BadRequest {
                code: "INVALID_DETAILS",
                message: "details must be full when present".into(),
            });
        }
    };
    if !full && let Some(cached) = state.caches.compact.get(&ip) {
        return Ok(cached);
    }
    let runtime_ip = RuntimeIp::from_ip(ip);
    let body = encode_json(&lookup_ip_with_meta(
        &state.db,
        &runtime_ip,
        Some(&state.dataset_meta),
        full,
    )?)?;
    if !full {
        state.caches.compact.insert(ip, body.clone());
    }
    Ok(body)
}

async fn handle_prefix(state: &ApiState, query: &str) -> Result<Bytes, ApiError> {
    let raw = query_param(query, "prefix");
    let prefix =
        parse_prefix(raw.as_deref().unwrap_or("")).ok_or_else(|| ApiError::BadRequest {
            code: "INVALID_PREFIX",
            message: "prefix must be a valid IPv4 or IPv6 CIDR".into(),
        })?;
    let page = parse_page(query)?;
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
    let body = encode_json(
        &lookup_prefix(&state.db, &state.dataset_meta, &prefix, page).map_err(|e| {
            too_broad_msg(
                e,
                "prefix matches too many source records; request a narrower prefix",
            )
        })?,
    )?;
    state.caches.resource.insert(key, body.clone());
    Ok(body)
}

async fn handle_range(state: &ApiState, query: &str) -> Result<Bytes, ApiError> {
    let start = query_param(query, "start");
    let end = query_param(query, "end");
    let range = parse_range(
        start.as_deref().unwrap_or(""),
        end.as_deref().unwrap_or(""),
    )
    .ok_or_else(|| ApiError::BadRequest {
        code: "INVALID_RANGE",
        message: "start and end must be valid addresses in the same IP version, with start not greater than end".into(),
    })?;
    let kind = parse_range_kind(query)?;
    let page = parse_range_page(query)?;
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
        lookup_range_summary(&state.db, &state.dataset_meta, &range, kind)?
    } else {
        let _permit = state
            .overlap_slots
            .acquire()
            .await
            .map_err(|e| ApiError::Internal(e.to_string()))?;
        lookup_range(&state.db, &state.dataset_meta, &range, kind, &page).map_err(|e| {
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

fn handle_asn(state: &ApiState, query: &str) -> Result<Bytes, ApiError> {
    let raw = query_param(query, "query");
    let asn = parse_asn_query(raw.as_deref().unwrap_or(""))?;
    let page = parse_asn_page(query)?;
    let mode = if page.numbered {
        format!("page\0{}", page.number)
    } else {
        format!("cursor\0{}", page.cursor)
    };
    let key = format!("asn\0{asn}\0{mode}\0{}", page.limit);
    if let Some(cached) = state.caches.resource.get(&key) {
        return Ok(cached);
    }
    let Some(resp) = lookup_asn(&state.db, &state.dataset_meta, asn, page)? else {
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
