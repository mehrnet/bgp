pub mod asn;
pub mod cache;
pub mod details;
pub mod dto;
pub mod handlers;
pub mod ipkey;
pub mod model;
pub mod params;
pub mod range_cursor;
pub mod resource_model;
pub mod resources;
pub mod service;

use std::collections::HashSet;
use std::sync::Arc;

use bytes::Bytes;
use mnmf_store::Db;
use serde::Serialize;
use tokio::sync::Semaphore;

use model::{ErrorBody, ErrorResponse};
use service::ApiError;

pub use cache::{RuntimeCacheController, RuntimeCacheRelease};
pub use model::{BuildInfo, DatasetMetadata};

#[derive(Debug, Clone)]
pub struct ApiConfig {
    pub allowed_origins: HashSet<String>,
    pub origin_auth_token: Option<String>,
    pub build: BuildInfo,
    pub runtime_cache_control: bool,
    pub compact_cache_bytes: usize,
    pub resource_cache_bytes: usize,
}

#[derive(Debug)]
pub struct ApiRequest<'a> {
    pub method: &'a str,
    pub path: &'a str,
    pub query: &'a str,
    pub origin: Option<&'a str>,
    pub origin_token: Option<&'a str>,
}

#[derive(Debug)]
pub struct ApiResponse {
    pub status: u16,
    pub cors_origin: Option<String>,
    pub is_options: bool,
    pub body: Bytes,
}

#[derive(Debug)]
pub struct ApiState {
    pub db: Arc<Db>,
    pub dataset_meta: DatasetMetadata,
    pub config: ApiConfig,
    pub caches: RuntimeCacheController,
    pub overlap_slots: Semaphore,
}

impl ApiState {
    #[must_use]
    pub fn new(db: Arc<Db>, config: ApiConfig) -> Self {
        let dataset_meta = service::dataset_meta_from_db(&db);
        let caches =
            RuntimeCacheController::new(config.compact_cache_bytes, config.resource_cache_bytes);
        Self {
            db,
            dataset_meta,
            config,
            caches,
            overlap_slots: Semaphore::new(2),
        }
    }

    pub async fn handle(&self, req: &ApiRequest<'_>) -> ApiResponse {
        handlers::dispatch(self, req).await
    }
}

pub(crate) fn encode_json<T: Serialize>(value: &T) -> Result<Bytes, ApiError> {
    let mut vec = serde_json::to_vec(value).map_err(|e| ApiError::Internal(e.to_string()))?;
    vec.push(b'\n');
    Ok(Bytes::from(vec))
}

pub(crate) fn json_error(err: ApiError, cors_origin: Option<String>) -> ApiResponse {
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
