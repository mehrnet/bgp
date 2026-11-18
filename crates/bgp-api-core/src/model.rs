use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct DatasetMetadata {
    pub release_tag: Option<String>,
    pub built_at: Option<String>,
    pub activated_at: Option<String>,
    pub source_commit: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ResponseMeta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dataset: Option<DatasetMetadata>,
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
pub struct HealthResponse {
    pub ok: bool,
    pub service: &'static str,
    pub version: u8,
    pub database: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub build: Option<BuildInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dataset: Option<DatasetMetadata>,
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
pub struct AsnIdentity {
    pub asn: String,
    pub as_number: u32,
    pub name: Option<String>,
    pub organization: Option<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct LookupNetwork {
    pub cidr: Option<String>,
    pub start_ip: Option<String>,
    pub end_ip: Option<String>,
    pub asn: Option<String>,
    pub asns: Vec<String>,
    pub origins: Vec<AsnIdentity>,
    pub as_number: Option<u32>,
    pub status: Option<String>,
    pub abuse_contact: Option<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct LookupAllocation {
    pub start_ip: Option<String>,
    pub end_ip: Option<String>,
    pub registry: Option<String>,
    pub country_code: Option<String>,
    pub country_raw: Option<String>,
    pub name: Option<String>,
    pub allocation_date: Option<String>,
    pub status: Option<String>,
    pub abuse_contact: Option<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct LookupLocation {
    pub country_code: Option<String>,
    pub region: Option<String>,
    pub city: Option<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct LookupSources {
    pub allocation: bool,
    pub route: bool,
    pub geofeed: bool,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct LookupDetails {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub allocations: Vec<LookupDetailRecord>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub routes: Vec<LookupDetailRecord>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub geofeeds: Vec<LookupDetailRecord>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct LookupDetailRecord {
    pub start_ip: String,
    pub end_ip: String,
    pub version: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub registry: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub country_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub country_raw: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cidr: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub asn: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub as_number: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub city: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allocation_date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_modified: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub maintainers: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub organization: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub abuse_contact: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LookupResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta: Option<ResponseMeta>,
    pub ip: String,
    pub version: u8,
    pub registry: Option<String>,
    pub allocation_date: Option<String>,
    pub allocation_status: Option<String>,
    pub network: LookupNetwork,
    pub allocation: LookupAllocation,
    pub location: LookupLocation,
    pub sources: LookupSources,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<LookupDetails>,
}
