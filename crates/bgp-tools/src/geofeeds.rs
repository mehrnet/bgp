use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter};
use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use bgp_compact::sortkey::cidr_to_sort_keys;

const CONCURRENCY: usize = 50;

pub fn fetch_geofeeds(urls_path: &Path, out_path: &Path) -> Result<(), String> {
    let input = File::open(urls_path).map_err(|e| format!("open {}: {e}", urls_path.display()))?;
    let urls: Vec<String> = BufReader::new(input)
        .lines()
        .map_while(Result::ok)
        .map(|s| s.trim().to_owned())
        .filter(|s| s.starts_with("https://"))
        .collect();

    let out_file =
        File::create(out_path).map_err(|e| format!("create {}: {e}", out_path.display()))?;
    let writer = Mutex::new(csv::Writer::from_writer(BufWriter::with_capacity(
        256 * 1024,
        out_file,
    )));
    let cursor = AtomicUsize::new(0);
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(10)))
        .build()
        .into();

    std::thread::scope(|scope| {
        for _ in 0..CONCURRENCY.min(urls.len().max(1)) {
            scope.spawn(|| worker(&urls, &cursor, &agent, &writer));
        }
    });

    let mut w = writer.into_inner().map_err(|e| format!("csv lock: {e}"))?;
    w.flush().map_err(|e| format!("flush geofeeds csv: {e}"))
}

fn worker(
    urls: &[String],
    cursor: &AtomicUsize,
    agent: &ureq::Agent,
    writer: &Mutex<csv::Writer<BufWriter<File>>>,
) {
    loop {
        let idx = cursor.fetch_add(1, Ordering::Relaxed);
        let Some(url) = urls.get(idx) else { break };
        fetch_one(url, agent, writer);
    }
}

fn fetch_one(url: &str, agent: &ureq::Agent, writer: &Mutex<csv::Writer<BufWriter<File>>>) {
    let Ok(resp) = agent.get(url).call() else {
        return;
    };
    if !(200..300).contains(&resp.status().as_u16()) {
        return;
    }
    let body = resp.into_body().into_reader();
    let mut rows = Vec::new();
    for line in BufReader::with_capacity(64 * 1024, body)
        .lines()
        .map_while(Result::ok)
    {
        if line.starts_with('#') {
            continue;
        }
        let parts: Vec<&str> = line.split(',').collect();
        if parts.len() < 4 {
            continue;
        }
        let cidr_s = parts.first().copied().unwrap_or("");
        let Some((start, end, ver)) = cidr_to_sort_keys(cidr_s) else {
            continue;
        };
        rows.push([
            start,
            end,
            ver.to_string(),
            parts.get(1).copied().unwrap_or("").trim().to_owned(),
            parts.get(2).copied().unwrap_or("").trim().to_owned(),
            parts.get(3).copied().unwrap_or("").trim().to_owned(),
        ]);
    }
    if !rows.is_empty()
        && let Ok(mut guard) = writer.lock()
    {
        for r in rows {
            let _ = guard.write_record(&r);
        }
    }
}
