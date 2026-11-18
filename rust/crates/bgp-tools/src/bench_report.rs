use std::net::IpAddr;
use std::time::Duration;

use hyper::Uri;

#[derive(Debug)]
pub struct BenchConfig {
    pub url: String,
    pub connect_ip: Option<IpAddr>,
    pub disable_compression: bool,
    pub requests: usize,
    pub concurrency: usize,
    pub warmup: usize,
    pub headers: Vec<(String, String)>,
}

pub(crate) struct TargetEndpoint {
    pub(crate) is_https: bool,
    pub(crate) host: String,
    pub(crate) host_header: String,
    pub(crate) port: u16,
    pub(crate) path_and_query: String,
    pub(crate) connect_host: String,
}

pub(crate) fn parse_target(
    raw: &str,
    connect_ip: Option<IpAddr>,
) -> Result<TargetEndpoint, String> {
    let uri: Uri = raw
        .parse()
        .map_err(|_| "url must be an absolute HTTP or HTTPS endpoint".to_owned())?;
    let is_https = match uri.scheme_str().unwrap_or("") {
        "http" => false,
        "https" => true,
        _ => return Err("url must be an absolute HTTP or HTTPS endpoint".into()),
    };
    let auth = uri
        .authority()
        .ok_or("url must be an absolute HTTP or HTTPS endpoint")?;
    let host = auth.host().to_owned();
    let port = auth.port_u16().unwrap_or(if is_https { 443 } else { 80 });
    let connect_host = connect_ip.map_or_else(|| host.clone(), |ip| ip.to_string());
    let pq = uri.path_and_query().map_or("/", |p| p.as_str()).to_owned();
    Ok(TargetEndpoint {
        is_https,
        host,
        host_header: auth.as_str().to_owned(),
        port,
        path_and_query: pq,
        connect_host,
    })
}

pub(crate) fn print_report(
    cfg: &BenchConfig,
    t: &TargetEndpoint,
    durs: &mut [Duration],
    elapsed: Duration,
) {
    durs.sort_unstable();
    let total: Duration = durs.iter().copied().sum();
    let rps = cfg.requests as f64 / elapsed.as_secs_f64();
    println!("target: {}", cfg.url);
    if let Some(ip) = cfg.connect_ip {
        println!("connect_ip: {ip} (TLS SNI and Host remain {})", t.host);
    }
    println!(
        "requests: {}  concurrency: {}  keep_alive: true  warmup: {}",
        cfg.requests, cfg.concurrency, cfg.warmup
    );
    println!("compression: {}", !cfg.disable_compression);
    println!("throughput: {rps:.1} req/s");
    println!(
        "latency_ms: min={:.3} avg={:.3} p50={:.3} p95={:.3} p99={:.3} max={:.3}",
        ms(durs.first().copied().unwrap_or_default()),
        ms(total) / (cfg.requests.max(1) as f64),
        ms(pct(durs, 50)),
        ms(pct(durs, 95)),
        ms(pct(durs, 99)),
        ms(durs.last().copied().unwrap_or_default()),
    );
}

fn pct(durs: &[Duration], p: usize) -> Duration {
    if durs.is_empty() {
        return Duration::ZERO;
    }
    let idx = ((p * durs.len()).div_ceil(100)).saturating_sub(1);
    durs.get(idx).copied().unwrap_or_default()
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}
