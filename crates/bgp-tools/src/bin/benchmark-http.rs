use std::net::IpAddr;
use std::process;

use bgp_compact::cli::ParsedArgs;
use bgp_tools::bench::{BenchConfig, run_benchmark};

#[tokio::main]
async fn main() {
    if let Err(err) = run().await {
        eprintln!("{err}");
        process::exit(1);
    }
}

async fn run() -> Result<(), String> {
    let args = ParsedArgs::parse(
        std::env::args(),
        &[
            "url",
            "connect-ip",
            "requests",
            "concurrency",
            "warmup",
            "header",
        ],
        &["disable-compression"],
    )?;
    let url = args.get("url").unwrap_or("").to_owned();
    let requests: usize = args.get("requests").map_or(Ok(5000), |s| {
        s.parse().map_err(|_| "invalid requests".to_owned())
    })?;
    let concurrency: usize = args.get("concurrency").map_or(Ok(32), |s| {
        s.parse().map_err(|_| "invalid concurrency".to_owned())
    })?;
    let warmup: usize = args.get("warmup").map_or(Ok(200), |s| {
        s.parse().map_err(|_| "invalid warmup".to_owned())
    })?;
    if url.is_empty() || requests < 1 || concurrency < 1 {
        return Err("url is required; requests and concurrency must be positive".into());
    }
    let connect_ip = match args.get("connect-ip").filter(|s| !s.is_empty()) {
        Some(raw) => Some(
            raw.parse::<IpAddr>()
                .map_err(|_| "connect-ip must be a valid IPv4 or IPv6 address".to_owned())?,
        ),
        None => None,
    };
    let headers = parse_headers(args.get_all("header"))?;
    run_benchmark(BenchConfig {
        url,
        connect_ip,
        disable_compression: args.get_bool("disable-compression", false),
        requests,
        concurrency,
        warmup,
        headers,
    })
    .await
}

fn parse_headers(raw: &[String]) -> Result<Vec<(String, String)>, String> {
    let mut out = Vec::with_capacity(raw.len());
    for entry in raw {
        let Some((name, val)) = entry.split_once(':') else {
            return Err(format!("invalid header {entry:?}; use Name: Value"));
        };
        let k = name.trim();
        if k.is_empty() {
            return Err(format!("invalid header {entry:?}; use Name: Value"));
        }
        out.push((k.to_owned(), val.trim().to_owned()));
    }
    Ok(out)
}
