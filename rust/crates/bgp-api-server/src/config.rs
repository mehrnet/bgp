use std::collections::HashSet;
use std::env;
use std::fs;

const MEBIBYTE: u64 = 1 << 20;
pub const GIBIBYTE: u64 = 1 << 30;
const BALANCED_RESERVE: u64 = 512 * MEBIBYTE;
const FULL_RESERVE: u64 = 768 * MEBIBYTE;

#[derive(Debug, Clone)]
pub struct CachePlan {
    pub requested: String,
    pub effective: &'static str,
    pub compact_cache_mib: usize,
    pub resource_cache_mib: usize,
    pub preload_selectors: bool,
    pub warm_dataset_page_cache: bool,
    pub database_bytes: u64,
    pub selector_bytes: u64,
}

#[derive(Debug, Clone)]
pub struct ServerEnv {
    pub database_path: String,
    pub validate_only: bool,
    pub listen_addr: String,
    pub cache_strategy: String,
    pub compact_cache_mib: Option<usize>,
    pub resource_cache_mib: Option<usize>,
    pub defer_warmup: bool,
    pub block_until_warmup: bool,
    pub runtime_cache_control: bool,
    pub origin_auth_token: Option<String>,
    pub allowed_origins: HashSet<String>,
}

pub fn load_server_env() -> Result<ServerEnv, String> {
    let database_path = env_non_empty("BGP_API_DATABASE_PATH")
        .unwrap_or_else(|| "/var/lib/bgp-api/primary.mmap".to_owned());
    let validate_only = env::var("BGP_API_VALIDATE_ONLY").ok().as_deref() == Some("1");
    let listen_addr = match env_non_empty("LISTEN_ADDR") {
        Some(addr) => addr,
        None => format!("127.0.0.1:{}", parse_positive_int("PORT")?.unwrap_or(3102)),
    };
    let defer_warmup = parse_bool("BGP_API_DEFER_CACHE_WARMUP", false)?;
    let block_until_warmup = parse_bool("BGP_API_BLOCK_UNTIL_CACHE_WARMUP", false)?;
    if defer_warmup && block_until_warmup {
        return Err("BGP_API_DEFER_CACHE_WARMUP and BGP_API_BLOCK_UNTIL_CACHE_WARMUP cannot both be enabled".into());
    }
    Ok(ServerEnv {
        database_path,
        validate_only,
        listen_addr,
        cache_strategy: env::var("BGP_API_CACHE_STRATEGY").unwrap_or_default(),
        compact_cache_mib: parse_positive_int("BGP_API_COMPACT_CACHE_MIB")?,
        resource_cache_mib: parse_positive_int("BGP_API_RESOURCE_CACHE_MIB")?,
        defer_warmup,
        block_until_warmup,
        runtime_cache_control: parse_bool("BGP_API_RUNTIME_CACHE_CONTROL", false)?,
        origin_auth_token: env_non_empty("ORIGIN_AUTH_TOKEN"),
        allowed_origins: parse_allowed_origins()?,
    })
}

pub fn host_memory_total() -> Result<u64, String> {
    let contents =
        fs::read_to_string("/proc/meminfo").map_err(|e| format!("read /proc/meminfo: {e}"))?;
    for line in contents.lines() {
        let mut parts = line.split_whitespace();
        if parts.next() == Some("MemTotal:")
            && let Some(kb_str) = parts.next()
            && let Ok(kb) = kb_str.parse::<u64>()
            && kb > 0
        {
            return Ok(kb << 10);
        }
    }
    Err("MemTotal missing from /proc/meminfo".into())
}

fn preset_plans(
    requested: String,
    database_bytes: u64,
    selector_bytes: u64,
) -> (CachePlan, CachePlan, CachePlan) {
    let full = CachePlan {
        requested,
        effective: "full",
        compact_cache_mib: 128,
        resource_cache_mib: 32,
        preload_selectors: false,
        warm_dataset_page_cache: true,
        database_bytes,
        selector_bytes,
    };
    let balanced = CachePlan {
        effective: "balanced",
        compact_cache_mib: 256,
        resource_cache_mib: 64,
        preload_selectors: true,
        warm_dataset_page_cache: false,
        ..full.clone()
    };
    let minimal = CachePlan {
        effective: "minimal",
        compact_cache_mib: 64,
        resource_cache_mib: 16,
        preload_selectors: false,
        warm_dataset_page_cache: false,
        ..full.clone()
    };
    (full, balanced, minimal)
}

pub fn resolve_cache_plan(
    requested: &str,
    memory_total: u64,
    database_bytes: u64,
    selector_bytes: u64,
) -> Result<CachePlan, String> {
    let norm = requested.trim().to_ascii_lowercase();
    let req_str = if norm.is_empty() {
        "auto".to_owned()
    } else {
        norm
    };
    let (full, balanced, minimal) = preset_plans(req_str.clone(), database_bytes, selector_bytes);
    match req_str.as_str() {
        "minimal" => Ok(minimal),
        "balanced" => Ok(balanced),
        "full" if memory_total < full_requirement(&full) => Err(format!(
            "full cache strategy needs {:.1} GiB for a {:.1} GiB dataset; host has {:.1} GiB",
            full_requirement(&full) as f64 / GIBIBYTE as f64,
            database_bytes as f64 / GIBIBYTE as f64,
            memory_total as f64 / GIBIBYTE as f64
        )),
        "full" => Ok(full),
        "auto" if memory_total >= full_requirement(&full) => Ok(full),
        "auto" if memory_total >= balanced_requirement(&balanced) => Ok(balanced),
        "auto" => Ok(minimal),
        _ => Err("BGP_API_CACHE_STRATEGY must be auto, minimal, balanced, or full".into()),
    }
}

pub fn full_requirement(plan: &CachePlan) -> u64 {
    plan.database_bytes
        + ((plan.compact_cache_mib + plan.resource_cache_mib) as u64) * MEBIBYTE
        + FULL_RESERVE
}

fn balanced_requirement(plan: &CachePlan) -> u64 {
    plan.selector_bytes
        + ((plan.compact_cache_mib + plan.resource_cache_mib) as u64) * MEBIBYTE
        + BALANCED_RESERVE
}

fn env_non_empty(name: &str) -> Option<String> {
    env::var(name).ok().filter(|v| !v.is_empty())
}

fn parse_positive_int(name: &str) -> Result<Option<usize>, String> {
    let Some(raw) = env_non_empty(name) else {
        return Ok(None);
    };
    raw.parse::<usize>()
        .ok()
        .filter(|&n| n >= 1)
        .map(Some)
        .ok_or_else(|| format!("{name} must be a positive integer"))
}

fn parse_bool(name: &str, fallback: bool) -> Result<bool, String> {
    let Ok(raw) = env::var(name) else {
        return Ok(fallback);
    };
    match raw.trim().to_ascii_lowercase().as_str() {
        "" => Ok(fallback),
        "1" | "true" | "yes" | "on" => Ok(true),
        "0" | "false" | "no" | "off" => Ok(false),
        _ => Err(format!("{name} must be a boolean")),
    }
}

fn parse_allowed_origins() -> Result<HashSet<String>, String> {
    let Some(raw) = env_non_empty("CORS_ALLOWED_ORIGINS_JSON") else {
        return Ok(HashSet::new());
    };
    let list: Vec<String> = serde_json::from_str(&raw)
        .map_err(|e| format!("CORS_ALLOWED_ORIGINS_JSON must be a JSON array: {e}"))?;
    Ok(list
        .into_iter()
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .collect())
}
