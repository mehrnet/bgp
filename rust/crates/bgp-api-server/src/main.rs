use std::convert::Infallible;
use std::env;
use std::process;
use std::sync::Arc;
use std::time::Duration;

use bgp_api_core::{ApiConfig, ApiRequest, ApiState, BuildInfo};
use bgp_api_server::config::{
    CachePlan, GIBIBYTE, ServerEnv, full_requirement, host_memory_total, load_server_env,
    resolve_cache_plan,
};
use bgp_api_server::signals::{RuntimeWarmer, spawn_signal_loop};
use bytes::Bytes;
use http_body_util::Full;
use hyper::body::Incoming;
use hyper::header::{
    ACCESS_CONTROL_ALLOW_HEADERS, ACCESS_CONTROL_ALLOW_METHODS, ACCESS_CONTROL_ALLOW_ORIGIN,
    ACCESS_CONTROL_MAX_AGE, CONTENT_TYPE, HeaderValue, ORIGIN, VARY,
};
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::{TokioExecutor, TokioIo, TokioTimer};
use hyper_util::server::conn::auto::Builder as ConnBuilder;
use mnmf_store::Db;
use tokio::net::TcpListener;
use tokio::sync::watch;

fn main() {
    let mut rt_builder = tokio::runtime::Builder::new_multi_thread();
    rt_builder.enable_all();
    if let Some(threads) = worker_threads_from_env() {
        rt_builder.worker_threads(threads);
    }
    let runtime = match rt_builder.build() {
        Ok(rt) => rt,
        Err(err) => {
            eprintln!("fatal: build runtime: {err}");
            process::exit(1);
        }
    };
    if let Err(err) = runtime.block_on(run()) {
        eprintln!("fatal: {err}");
        process::exit(1);
    }
}

fn worker_threads_from_env() -> Option<usize> {
    env::var("GOMAXPROCS")
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .filter(|&n| (1..=256).contains(&n))
}

async fn run() -> Result<(), String> {
    let env = load_server_env()?;
    let db = Arc::new(Db::open(&env.database_path).map_err(|e| format!("open mmap dataset: {e}"))?);
    if env.validate_only {
        let tag = db.metadata().release_tag.as_str();
        let display = if tag.is_empty() { "unknown" } else { tag };
        eprintln!("validated mmap dataset {display}");
        return Ok(());
    }

    let mem_total = host_memory_total().unwrap_or_else(|e| {
        eprintln!("could not inspect host memory; using minimal cache strategy: {e}");
        0
    });
    let mut plan = resolve_cache_plan(
        &env.cache_strategy,
        mem_total,
        db.size_bytes(),
        db.compact_selector_bytes(),
    )?;
    apply_cache_overrides(&mut plan, &env, mem_total)?;
    log_cache_plan(&plan, mem_total, &env);

    let state = build_api_state(db, &plan, &env);
    let warmer = RuntimeWarmer::new(Arc::clone(&state), plan);
    if env.block_until_warmup {
        eprintln!("warming runtime caches before accepting traffic");
        warmer.warm_blocking().await?;
    }
    serve(state, warmer, env).await
}

fn apply_cache_overrides(
    plan: &mut CachePlan,
    env: &ServerEnv,
    mem_total: u64,
) -> Result<(), String> {
    if let Some(mib) = env.compact_cache_mib {
        plan.compact_cache_mib = mib;
    }
    if let Some(mib) = env.resource_cache_mib {
        plan.resource_cache_mib = mib;
    }
    if plan.effective == "full" && mem_total < full_requirement(plan) {
        return Err("full cache strategy exceeds host memory after cache overrides".into());
    }
    Ok(())
}

fn non_empty_opt(val: Option<&str>) -> Option<String> {
    val.map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

fn build_api_state(db: Arc<Db>, plan: &CachePlan, env: &ServerEnv) -> Arc<ApiState> {
    Arc::new(ApiState::new(
        db,
        ApiConfig {
            allowed_origins: env.allowed_origins.clone(),
            origin_auth_token: env.origin_auth_token.clone(),
            build: BuildInfo {
                version: Some(
                    non_empty_opt(option_env!("BGP_API_BUILD_VERSION"))
                        .unwrap_or_else(|| "dev".to_owned()),
                ),
                commit: non_empty_opt(option_env!("BGP_API_BUILD_COMMIT")),
                built_at: non_empty_opt(option_env!("BGP_API_BUILD_AT")),
            },
            runtime_cache_control: env.runtime_cache_control,
            compact_cache_bytes: plan.compact_cache_mib << 20,
            resource_cache_bytes: plan.resource_cache_mib << 20,
        },
    ))
}

async fn serve(
    state: Arc<ApiState>,
    warmer: Arc<RuntimeWarmer>,
    env: ServerEnv,
) -> Result<(), String> {
    let listener = TcpListener::bind(&env.listen_addr)
        .await
        .map_err(|e| format!("bind {}: {e}", env.listen_addr))?;
    eprintln!("bgp-api (rust) listening on {}", env.listen_addr);

    if env.defer_warmup {
        eprintln!("cache warmup deferred until SIGUSR1");
    } else if !env.block_until_warmup {
        warmer.spawn_start();
    }

    let (shutdown_tx, mut shutdown_rx) = watch::channel(false);
    spawn_signal_loop(
        Arc::clone(&state),
        warmer,
        shutdown_tx,
        env.runtime_cache_control,
    );

    loop {
        tokio::select! {
            _ = shutdown_rx.changed() => {
                tokio::time::sleep(Duration::from_millis(50)).await;
                return Ok(());
            }
            accepted = listener.accept() => {
                let Ok((stream, _)) = accepted else { continue };
                let _ = stream.set_nodelay(true);
                let st = Arc::clone(&state);
                tokio::spawn(async move {
                    let svc = service_fn(move |req| serve_hyper(Arc::clone(&st), req));
                    let mut builder = ConnBuilder::new(TokioExecutor::new());
                    builder
                        .http1()
                        .timer(TokioTimer::new())
                        .header_read_timeout(Duration::from_secs(5));
                    let _ = builder.serve_connection(TokioIo::new(stream), svc).await;
                });
            }
        }
    }
}

async fn serve_hyper(
    state: Arc<ApiState>,
    req: Request<Incoming>,
) -> Result<Response<Full<Bytes>>, Infallible> {
    let method = req.method().as_str().to_owned();
    let path = req.uri().path().to_owned();
    let query = req.uri().query().unwrap_or("").to_owned();
    let origin = req
        .headers()
        .get(ORIGIN)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    let token = req
        .headers()
        .get("X-BGP-API-Origin-Token")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    let resp = state
        .handle(&ApiRequest {
            method: &method,
            path: &path,
            query: &query,
            origin: origin.as_deref(),
            origin_token: token.as_deref(),
        })
        .await;
    Ok(to_hyper_response(resp))
}

fn to_hyper_response(resp: bgp_api_core::ApiResponse) -> Response<Full<Bytes>> {
    let status = StatusCode::from_u16(resp.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let mut builder = Response::builder().status(status);
    if let Some(orig) = resp.cors_origin
        && let Ok(val) = HeaderValue::from_str(&orig)
    {
        builder = builder.header(ACCESS_CONTROL_ALLOW_ORIGIN, val);
    }
    builder = builder
        .header(ACCESS_CONTROL_ALLOW_METHODS, "GET, OPTIONS")
        .header(ACCESS_CONTROL_ALLOW_HEADERS, "Content-Type")
        .header(VARY, "Origin");
    if resp.is_options {
        builder = builder.header(ACCESS_CONTROL_MAX_AGE, "86400");
    } else {
        builder = builder.header(CONTENT_TYPE, "application/json; charset=utf-8");
    }
    builder
        .body(Full::new(resp.body))
        .unwrap_or_else(|_| Response::new(Full::new(Bytes::new())))
}

fn log_cache_plan(plan: &CachePlan, mem_total: u64, env: &ServerEnv) {
    eprintln!(
        "cache strategy requested={} effective={} memory={:.1} GiB database={:.1} GiB selectors={:.1} MiB compact_cache={} MiB resource_cache={} MiB deferred={} blocking={} runtime_cache_control={}",
        plan.requested,
        plan.effective,
        mem_total as f64 / GIBIBYTE as f64,
        plan.database_bytes as f64 / GIBIBYTE as f64,
        plan.selector_bytes as f64 / (1u64 << 20) as f64,
        plan.compact_cache_mib,
        plan.resource_cache_mib,
        env.defer_warmup,
        env.block_until_warmup,
        env.runtime_cache_control,
    );
}
