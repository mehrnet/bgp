use std::collections::{HashMap, HashSet};
use std::fmt;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::Path;

use bgp_compact::sortkey::{cidr_to_sort_keys, pad_sort_key_str};

use crate::rir_emit::{RpslMeta, parse_delegated, write_alloc, write_route};

pub struct RirWriters {
    pub allocations: csv::Writer<BufWriter<File>>,
    pub routes: csv::Writer<BufWriter<File>>,
    pub autnums: csv::Writer<BufWriter<File>>,
    pub geofeeds: BufWriter<File>,
}

impl fmt::Debug for RirWriters {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RirWriters").finish_non_exhaustive()
    }
}

impl RirWriters {
    pub fn create(out_dir: &Path, rir_lower: &str) -> Result<Self, String> {
        let mk_csv = |prefix: &str| -> Result<csv::Writer<BufWriter<File>>, String> {
            let p = out_dir.join(format!("{prefix}_{rir_lower}.csv"));
            let f = File::create(&p).map_err(|e| format!("create {}: {e}", p.display()))?;
            Ok(csv::Writer::from_writer(BufWriter::with_capacity(
                256 * 1024,
                f,
            )))
        };
        let gp = out_dir.join(format!("geofeeds_{rir_lower}.txt"));
        let gf = File::create(&gp).map_err(|e| format!("create {}: {e}", gp.display()))?;
        Ok(Self {
            allocations: mk_csv("allocations")?,
            routes: mk_csv("routes")?,
            autnums: mk_csv("autnums")?,
            geofeeds: BufWriter::with_capacity(64 * 1024, gf),
        })
    }

    pub fn flush(mut self) -> Result<(), String> {
        self.allocations
            .flush()
            .map_err(|e| format!("flush alloc: {e}"))?;
        self.routes
            .flush()
            .map_err(|e| format!("flush routes: {e}"))?;
        self.autnums
            .flush()
            .map_err(|e| format!("flush autnums: {e}"))?;
        self.geofeeds
            .flush()
            .map_err(|e| format!("flush geofeeds: {e}"))
    }
}

pub fn parse_rir_dir(raw_dir: &Path, out_dir: &Path, rir: &str) -> Result<(), String> {
    let rir_lower = rir.to_ascii_lowercase();
    let rir_upper = rir.to_ascii_uppercase();
    let mut w = RirWriters::create(out_dir, &rir_lower)?;
    let entries =
        fs::read_dir(raw_dir).map_err(|e| format!("readdir {}: {e}", raw_dir.display()))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("direntry: {e}"))?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.contains("delegated-arin") {
            parse_delegated(&path, &rir_upper, &mut w);
        } else {
            parse_rpsl(&path, &rir_upper, &mut w);
        }
    }
    w.flush()
}

pub fn parse_rpsl(path: &Path, registry: &str, w: &mut RirWriters) {
    let Ok(file) = File::open(path) else {
        eprintln!("open {}: skipped", path.display());
        return;
    };
    let reader: Box<dyn BufRead> = if path.extension().and_then(|e| e.to_str()) == Some("gz") {
        Box::new(BufReader::with_capacity(
            256 * 1024,
            flate2::read::GzDecoder::new(file),
        ))
    } else {
        Box::new(BufReader::with_capacity(256 * 1024, file))
    };
    read_rpsl_blocks(reader, path, registry, w);
}

fn read_rpsl_blocks(mut reader: Box<dyn BufRead>, path: &Path, registry: &str, w: &mut RirWriters) {
    let (mut buf, mut block) = (Vec::new(), Vec::new());
    loop {
        buf.clear();
        match reader.read_until(b'\n', &mut buf) {
            Ok(0) => break,
            Ok(_) => {
                while matches!(buf.last(), Some(b'\n' | b'\r')) {
                    buf.pop();
                }
                if buf.is_empty() {
                    if !block.is_empty() {
                        process_rpsl_block(&block, registry, w);
                        block.clear();
                    }
                } else {
                    block.push(String::from_utf8_lossy(&buf).into_owned());
                }
            }
            Err(err) => {
                eprintln!("scan {}: {err}", path.display());
                break;
            }
        }
    }
    if !block.is_empty() {
        process_rpsl_block(&block, registry, w);
    }
}

fn process_rpsl_block(lines: &[String], registry: &str, w: &mut RirWriters) {
    let mut attrs: HashMap<String, Vec<String>> = HashMap::new();
    for line in lines {
        let Some((k, v)) = rpsl_attribute(line) else {
            continue;
        };
        if (k == "geofeed" || (k == "remarks" && v.to_ascii_lowercase().contains("geofeed")))
            && let Some(idx) = v.find("https://")
        {
            let _ = writeln!(w.geofeeds, "{}", v.get(idx..).unwrap_or("").trim());
        }
        attrs.entry(k).or_default().push(v);
    }
    let first = |k: &str| {
        attrs
            .get(k)
            .and_then(|v| v.first())
            .map_or("", String::as_str)
    };
    let join = |k: &str| join_attributes(attrs.get(k).map_or(&[], Vec::as_slice));
    let abuse = [join("abuse-c"), join("abuse-mailbox")]
        .into_iter()
        .find(|s| !s.is_empty())
        .unwrap_or_default();
    let meta = RpslMeta {
        country: first("country").to_ascii_uppercase(),
        status: first("status").to_owned(),
        created: first("created").to_owned(),
        last_modified: first("last-modified").to_owned(),
        source: first("source")
            .split_whitespace()
            .next()
            .unwrap_or("")
            .to_owned(),
        mnt_by: join("mnt-by"),
        org: first("org").to_owned(),
        abuse_contact: abuse,
        description: join("descr"),
    };
    emit_rpsl_object(&first, registry, &meta, w);
}

fn emit_rpsl_object<'a>(
    first: &impl Fn(&str) -> &'a str,
    reg: &str,
    m: &RpslMeta,
    w: &mut RirWriters,
) {
    if let Some((a, b)) = (!first("inetnum").is_empty())
        .then(|| first("inetnum").split_once('-'))
        .flatten()
    {
        write_alloc(
            &mut w.allocations,
            (&pad_sort_key_str(a), &pad_sort_key_str(b), 4),
            reg,
            m,
            first("netname"),
            "",
        );
    } else if !first("inet6num").is_empty() {
        if let Some((s, e, _)) = cidr_to_sort_keys(first("inet6num")) {
            write_alloc(
                &mut w.allocations,
                (&s, &e, 6),
                reg,
                m,
                first("netname"),
                "",
            );
        }
    } else if !first("route").is_empty() {
        write_route(&mut w.routes, first("route"), 4, first("origin"), reg, m);
    } else if !first("route6").is_empty() {
        write_route(&mut w.routes, first("route6"), 6, first("origin"), reg, m);
    } else if !first("aut-num").is_empty() {
        let _ = w.autnums.write_record([
            first("aut-num").to_ascii_uppercase(),
            reg.to_owned(),
            m.country.clone(),
            first("as-name").to_owned(),
            m.org.clone(),
            m.status.clone(),
            m.created.clone(),
            m.last_modified.clone(),
            m.source.clone(),
            m.mnt_by.clone(),
            m.abuse_contact.clone(),
            m.description.clone(),
        ]);
    }
}

fn rpsl_attribute(line: &str) -> Option<(String, String)> {
    let first = line.as_bytes().first().copied()?;
    if matches!(first, b'#' | b'%' | b' ' | b'\t') {
        return None;
    }
    let (k, v) = line.split_once(':')?;
    let (kt, vt) = (k.trim().to_ascii_lowercase(), v.trim().to_owned());
    (!kt.is_empty() && !vt.is_empty()).then_some((kt, vt))
}

fn join_attributes(vals: &[String]) -> String {
    let mut seen = HashSet::with_capacity(vals.len());
    let mut out = Vec::with_capacity(vals.len());
    for v in vals {
        let t = v.trim();
        if !t.is_empty() && seen.insert(t) {
            out.push(t);
        }
    }
    out.join(" | ")
}
