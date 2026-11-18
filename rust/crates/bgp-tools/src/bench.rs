use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use bytes::Bytes;
use http_body_util::{BodyExt, Empty};
use hyper::client::conn::http1;
use hyper::{Request, StatusCode};
use hyper_util::rt::TokioIo;
use rustls::pki_types::ServerName;
use rustls::{ClientConfig, RootCertStore};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio_rustls::TlsConnector;

pub use crate::bench_report::BenchConfig;
use crate::bench_report::{TargetEndpoint, parse_target, print_report};

type Sender = http1::SendRequest<Empty<Bytes>>;

pub async fn run_benchmark(cfg: BenchConfig) -> Result<(), String> {
    let target = Arc::new(parse_target(&cfg.url, cfg.connect_ip)?);
    let tls = build_tls_connector();
    let cfg = Arc::new(cfg);

    let mut warm_sender = None;
    for i in 0..cfg.warmup {
        if let Err(err) = send_one(&target, &tls, &cfg, &mut warm_sender).await {
            return Err(format!("warmup request {}: {err}", i + 1));
        }
    }
    drop(warm_sender);

    let (next, failures) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
    let latencies = Arc::new(Mutex::new(Vec::with_capacity(cfg.requests)));
    let started = Instant::now();
    let handles: Vec<_> = (0..cfg.concurrency)
        .map(|_| {
            spawn_worker(
                Arc::clone(&cfg),
                Arc::clone(&target),
                tls.clone(),
                Arc::clone(&next),
                Arc::clone(&failures),
                Arc::clone(&latencies),
            )
        })
        .collect();
    for h in handles {
        let _ = h.await;
    }
    let elapsed = started.elapsed();
    let failed = failures.load(Ordering::Relaxed);
    if failed > 0 {
        return Err(format!(
            "benchmark failed: {failed} of {} requests did not return HTTP 200",
            cfg.requests
        ));
    }
    let mut durs = latencies.lock().await.clone();
    print_report(&cfg, &target, &mut durs, elapsed);
    Ok(())
}

fn spawn_worker(
    c: Arc<BenchConfig>,
    t: Arc<TargetEndpoint>,
    tl: TlsConnector,
    n: Arc<AtomicUsize>,
    f: Arc<AtomicUsize>,
    l: Arc<Mutex<Vec<Duration>>>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut local = Vec::new();
        let mut sender = None;
        loop {
            if n.fetch_add(1, Ordering::Relaxed) >= c.requests {
                break;
            }
            let began = Instant::now();
            if send_one(&t, &tl, &c, &mut sender).await.is_err() {
                f.fetch_add(1, Ordering::Relaxed);
                sender = None;
                continue;
            }
            local.push(began.elapsed());
        }
        l.lock().await.extend(local);
    })
}

fn build_tls_connector() -> TlsConnector {
    let roots = RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
    };
    let cfg = ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    TlsConnector::from(Arc::new(cfg))
}

async fn connect_sender(t: &TargetEndpoint, tls: &TlsConnector) -> Result<Sender, String> {
    let addr = format_connect_addr(&t.connect_host, t.port);
    let stream = tokio::time::timeout(Duration::from_secs(15), TcpStream::connect(&addr))
        .await
        .map_err(|_| "connect timeout".to_owned())?
        .map_err(|e| format!("tcp connect: {e}"))?;
    let _ = stream.set_nodelay(true);
    if t.is_https {
        let sni = ServerName::try_from(t.host.clone()).map_err(|e| format!("invalid SNI: {e}"))?;
        let tls_stream = tls
            .connect(sni, stream)
            .await
            .map_err(|e| format!("tls: {e}"))?;
        let (sender, conn) = http1::handshake(TokioIo::new(tls_stream))
            .await
            .map_err(|e| format!("h1: {e}"))?;
        tokio::spawn(async move {
            let _ = conn.await;
        });
        Ok(sender)
    } else {
        let (sender, conn) = http1::handshake(TokioIo::new(stream))
            .await
            .map_err(|e| format!("h1: {e}"))?;
        tokio::spawn(async move {
            let _ = conn.await;
        });
        Ok(sender)
    }
}

async fn send_one(
    t: &TargetEndpoint,
    tls: &TlsConnector,
    cfg: &BenchConfig,
    slot: &mut Option<Sender>,
) -> Result<(), String> {
    let ready = if let Some(s) = slot.as_mut() {
        s.ready().await.is_ok()
    } else {
        false
    };
    if !ready {
        *slot = Some(connect_sender(t, tls).await?);
    }
    let sender = slot.as_mut().ok_or("missing HTTP sender")?;
    let mut req = Request::builder()
        .method("GET")
        .uri(&t.path_and_query)
        .header("Host", &t.host_header);
    if !cfg.disable_compression {
        req = req.header("Accept-Encoding", "gzip");
    }
    for (k, v) in &cfg.headers {
        req = req.header(k.as_str(), v.as_str());
    }
    let built = req
        .body(Empty::new())
        .map_err(|e| format!("build req: {e}"))?;
    let resp = tokio::time::timeout(Duration::from_secs(15), sender.send_request(built))
        .await
        .map_err(|_| "request timeout".to_owned())?
        .map_err(|e| format!("send req: {e}"))?;
    let status = resp.status();
    let _ = resp.into_body().collect().await;
    if status != StatusCode::OK {
        return Err(format!("unexpected HTTP status {}", status.as_u16()));
    }
    Ok(())
}

fn format_connect_addr(host: &str, port: u16) -> String {
    if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    }
}
