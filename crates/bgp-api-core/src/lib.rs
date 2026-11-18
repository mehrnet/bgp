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
use tokio::sync::Semaphore;

pub use cache::{RuntimeCacheController, RuntimeCacheRelease};
pub use model::BuildInfo;

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
    pub config: ApiConfig,
    pub caches: RuntimeCacheController,
    pub overlap_slots: Semaphore,
}

impl ApiState {
    #[must_use]
    pub fn new(db: Arc<Db>, config: ApiConfig) -> Self {
        let caches =
            RuntimeCacheController::new(config.compact_cache_bytes, config.resource_cache_bytes);
        Self {
            db,
            config,
            caches,
            overlap_slots: Semaphore::new(2),
        }
    }

    pub async fn handle(&self, req: &ApiRequest<'_>) -> ApiResponse {
        handlers::dispatch(self, req).await
    }
}
