use std::borrow::Cow;

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct DatasetMetadata {
    pub release_tag: Option<String>,
    pub built_at: Option<String>,
    pub activated_at: Option<String>,
    pub source_commit: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ResponseMeta<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dataset: Option<Cow<'a, DatasetMetadata>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BuildInfo {
    pub version: Option<String>,
    pub commit: Option<String>,
    pub built_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeInfo {
    pub cache_control: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct HealthResponse<'a> {
    pub ok: bool,
    pub service: &'static str,
    pub version: u8,
    pub database: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub build: Option<Cow<'a, BuildInfo>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dataset: Option<Cow<'a, DatasetMetadata>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime: Option<RuntimeInfo>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ErrorBody {
    pub code: &'static str,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ErrorResponse {
    pub error: ErrorBody,
}

#[derive(Debug, Clone, Serialize)]
pub struct AsnIdentity<'a> {
    pub asn: String,
    pub as_number: u32,
    pub name: Option<Cow<'a, str>>,
    pub organization: Option<Cow<'a, str>>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct LookupNetwork<'a> {
    pub cidr: Option<String>,
    pub start_ip: Option<String>,
    pub end_ip: Option<String>,
    pub asn: Option<String>,
    pub asns: Vec<String>,
    pub origins: Vec<AsnIdentity<'a>>,
    pub as_number: Option<u32>,
    pub status: Option<Cow<'a, str>>,
    pub abuse_contact: Option<Cow<'a, str>>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct LookupAllocation<'a> {
    pub start_ip: Option<String>,
    pub end_ip: Option<String>,
    pub registry: Option<Cow<'a, str>>,
    pub country_code: Option<Cow<'a, str>>,
    pub country_raw: Option<Cow<'a, str>>,
    pub name: Option<Cow<'a, str>>,
    pub allocation_date: Option<Cow<'a, str>>,
    pub status: Option<Cow<'a, str>>,
    pub abuse_contact: Option<Cow<'a, str>>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct LookupLocation<'a> {
    pub country_code: Option<Cow<'a, str>>,
    pub region: Option<Cow<'a, str>>,
    pub city: Option<Cow<'a, str>>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct LookupSources {
    pub allocation: bool,
    pub route: bool,
    pub geofeed: bool,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct LookupDetails<'a> {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub allocations: Vec<LookupDetailRecord<'a>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub routes: Vec<LookupDetailRecord<'a>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub geofeeds: Vec<LookupDetailRecord<'a>>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct LookupDetailRecord<'a> {
    pub start_ip: String,
    pub end_ip: String,
    pub version: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub registry: Option<Cow<'a, str>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub country_code: Option<Cow<'a, str>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub country_raw: Option<Cow<'a, str>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<Cow<'a, str>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cidr: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub asn: Option<Cow<'a, str>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub as_number: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub region: Option<Cow<'a, str>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub city: Option<Cow<'a, str>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latitude: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub longitude: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<Cow<'a, str>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allocation_date: Option<Cow<'a, str>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created: Option<Cow<'a, str>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_modified: Option<Cow<'a, str>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<Cow<'a, str>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub maintainers: Option<Cow<'a, str>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub organization: Option<Cow<'a, str>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub abuse_contact: Option<Cow<'a, str>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<Cow<'a, str>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LookupResponse<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta: Option<ResponseMeta<'a>>,
    pub ip: String,
    pub version: u8,
    pub registry: Option<Cow<'a, str>>,
    pub allocation_date: Option<Cow<'a, str>>,
    pub allocation_status: Option<Cow<'a, str>>,
    pub network: LookupNetwork<'a>,
    pub allocation: LookupAllocation<'a>,
    pub location: LookupLocation<'a>,
    pub sources: LookupSources,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<LookupDetails<'a>>,
}
