use std::collections::HashSet;
use std::sync::Arc;

use bgp_api_core::ipkey::{parse_prefix, parse_range, parse_runtime_ip, summary_prefix_key};
use bgp_api_core::{ApiConfig, ApiRequest, ApiState, BuildInfo};
use mnmf_store::Db;
use mnmf_store::types::{
    FORMAT_VERSION, HEADER_FIXED_SIZE, HEADER_SIZE, MAGIC, SCHEMA_VERSION, SECTION_COUNT,
    SECTION_ENTRY_SIZE, Section,
};

fn minimal_db() -> Result<Arc<Db>, Box<dyn std::error::Error>> {
    let mut table: [Vec<u8>; SECTION_COUNT] = std::array::from_fn(|_| Vec::new());
    let meta = format!(r#"{{"schema_version":{SCHEMA_VERSION},"release_tag":"db-rs-test"}}"#);
    if let Some(slot) = table.get_mut(Section::Metadata as usize) {
        *slot = meta.into_bytes();
    }
    for s in [
        Section::StringPoolOffsets,
        Section::AllocationOffsets,
        Section::RouteOffsets,
        Section::GeofeedOffsets,
    ] {
        if let Some(slot) = table.get_mut(s as usize) {
            *slot = 0u64.to_be_bytes().to_vec();
        }
    }
    for (s, v, m, ns) in [
        (Section::AllocLpmV4, 4u8, b"SLP2", 21usize),
        (Section::AllocLpmV6, 6u8, b"SLP2", 45usize),
        (Section::GeoLpmV4, 4u8, b"SLP2", 21usize),
        (Section::GeoLpmV6, 6u8, b"SLP2", 45usize),
        (Section::GeoFallbackLpmV4, 4u8, b"SLP2", 21usize),
        (Section::GeoFallbackLpmV6, 6u8, b"SLP2", 45usize),
        (Section::RouteLpmV4, 4u8, b"MLP2", 33usize),
        (Section::RouteLpmV6, 6u8, b"MLP2", 33usize),
    ] {
        if let Some(slot) = table.get_mut(s as usize) {
            *slot = empty_lpm(m, v, ns);
        }
    }
    let mut out = vec![0u8; HEADER_SIZE];
    let mut specs = [(0u64, 0u64); SECTION_COUNT];
    for (i, d) in table.iter().enumerate() {
        while !out.len().is_multiple_of(8) {
            out.push(0);
        }
        if let Some(s) = specs.get_mut(i) {
            *s = (out.len() as u64, d.len() as u64);
        }
        out.extend_from_slice(d);
    }
    write_header(&mut out, &specs);
    Ok(Arc::new(Db::from_bytes(out)?))
}

fn empty_lpm(magic: &[u8; 4], version: u8, node_size: usize) -> Vec<u8> {
    let mut buf = vec![0u8; 16 + node_size];
    if let Some(d) = buf.get_mut(0..4) {
        d.copy_from_slice(magic);
    }
    if let Some(d) = buf.get_mut(4) {
        *d = version;
    }
    if let Some(d) = buf.get_mut(8..12) {
        d.copy_from_slice(&1u32.to_be_bytes());
    }
    buf
}

fn write_header(out: &mut [u8], specs: &[(u64, u64); SECTION_COUNT]) {
    if let Some(d) = out.get_mut(0..4) {
        d.copy_from_slice(&MAGIC);
    }
    if let Some(d) = out.get_mut(4..8) {
        d.copy_from_slice(&FORMAT_VERSION.to_be_bytes());
    }
    if let Some(d) = out.get_mut(8..12) {
        d.copy_from_slice(&(SECTION_COUNT as u32).to_be_bytes());
    }
    for (i, (off, sz)) in specs.iter().enumerate() {
        let p = HEADER_FIXED_SIZE + i * SECTION_ENTRY_SIZE;
        if let Some(d) = out.get_mut(p..p + 8) {
            d.copy_from_slice(&off.to_be_bytes());
        }
        if let Some(d) = out.get_mut(p + 8..p + 16) {
            d.copy_from_slice(&sz.to_be_bytes());
        }
    }
}

fn req<'a>(method: &'a str, path: &'a str, query: &'a str) -> ApiRequest<'a> {
    ApiRequest {
        method,
        path,
        query,
        origin: None,
        origin_token: None,
    }
}

#[test]
fn ipkey_parsing_and_summary_eligibility() {
    let ip4 = parse_runtime_ip("1.1.1.1").ok_or("v4");
    assert!(ip4.is_ok());
    assert!(parse_runtime_ip("fe80::1%eth0").is_none());

    let pfx = parse_prefix("2001:db8::/32");
    assert!(pfx.is_some());

    let broad = parse_range("1.0.0.0", "1.255.255.255");
    assert!(broad.as_ref().and_then(summary_prefix_key).is_some());

    let narrow = parse_range("1.1.1.0", "1.1.1.255");
    assert!(narrow.as_ref().and_then(summary_prefix_key).is_none());
}

async fn build_test_state() -> Result<ApiState, Box<dyn std::error::Error>> {
    let db = minimal_db()?;
    let mut allowed = HashSet::new();
    allowed.insert("https://bgp.mehrnet.com".to_owned());
    Ok(ApiState::new(
        db,
        ApiConfig {
            allowed_origins: allowed,
            origin_auth_token: Some("secret".into()),
            build: BuildInfo {
                version: None,
                commit: None,
                built_at: None,
            },
            runtime_cache_control: true,
            compact_cache_bytes: 1 << 20,
            resource_cache_bytes: 1 << 20,
        },
    ))
}

#[tokio::test]
async fn handlers_enforce_contract_auth_cors_and_errors() -> Result<(), Box<dyn std::error::Error>>
{
    let state = build_test_state().await?;

    let unauth = state.handle(&req("GET", "/v1/health", "")).await;
    assert_eq!(unauth.status, 401);

    let health = state
        .handle(&ApiRequest {
            method: "GET",
            path: "/v1/health",
            query: "",
            origin: Some("https://bgp.mehrnet.com"),
            origin_token: Some("secret"),
        })
        .await;
    assert_eq!(health.status, 200);
    assert_eq!(
        health.cors_origin.as_deref(),
        Some("https://bgp.mehrnet.com")
    );
    let body = String::from_utf8(health.body.to_vec())?;
    assert!(body.contains("\"database\":\"mmap\""));

    let bad_ip = state
        .handle(&ApiRequest {
            origin_token: Some("secret"),
            ..req("GET", "/v1/ip", "query=nope")
        })
        .await;
    assert_eq!(bad_ip.status, 400);

    let missing_asn = state
        .handle(&ApiRequest {
            origin_token: Some("secret"),
            ..req("GET", "/v1/asn", "query=AS13335")
        })
        .await;
    assert_eq!(missing_asn.status, 404);
    Ok(())
}
