use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use crate::builder_meta::{build_autnums, build_geofeeds};
use crate::builder_tables::{build_allocations, build_routes};
use crate::encode::StringPoolWriter;
use crate::summary_build::SummaryCollector;
use crate::types::{
    FORMAT_VERSION, HEADER_FIXED_SIZE, HEADER_SIZE, MAGIC, Metadata, SCHEMA_VERSION, SECTION_COUNT,
    SECTION_ENTRY_SIZE, Section,
};

#[derive(Debug, Clone)]
pub struct BuildOptions {
    pub input_dir: PathBuf,
    pub output_path: PathBuf,
    pub release_tag: String,
    pub built_at: String,
    pub source_commit: String,
}

#[derive(Debug, Clone, Default)]
pub struct BuildStats {
    pub allocations: u64,
    pub routes: u64,
    pub geofeeds: u64,
    pub autnums: u64,
    pub allocation_index: u64,
    pub route_index: u64,
    pub geofeed_index: u64,
    pub allocation_range_index: u64,
    pub route_range_index: u64,
    pub materialized_summary: u64,
    pub route_lpm_nodes: u64,
    pub route_lpm_ids: u64,
    pub route_lpm_bytes: u64,
    pub allocation_lpm_nodes: u64,
    pub allocation_lpm_bytes: u64,
    pub geofeed_lpm_nodes: u64,
    pub geofeed_lpm_bytes: u64,
    pub string_pool_entries: u64,
    pub string_pool_bytes: u64,
    pub file_bytes: u64,
}

pub(crate) struct MmapWriter {
    buf: BufWriter<File>,
    pos: u64,
    sections: [(u64, u64); SECTION_COUNT],
}

impl MmapWriter {
    pub(crate) fn write_all(&mut self, bytes: &[u8]) -> Result<(), String> {
        self.buf
            .write_all(bytes)
            .map_err(|e| format!("write: {e}"))?;
        self.pos += bytes.len() as u64;
        Ok(())
    }

    pub(crate) fn begin_section(&mut self) -> Result<u64, String> {
        let rem = self.pos & 7;
        if rem != 0 {
            let pad = (8 - rem) as usize;
            let zeros = [0u8; 8];
            self.write_all(zeros.get(..pad).unwrap_or(&[]))?;
        }
        Ok(self.pos)
    }

    pub(crate) fn end_section(&mut self, sec: Section, start: u64) {
        if let Some(slot) = self.sections.get_mut(sec as usize) {
            *slot = (start, self.pos - start);
        }
    }

    pub(crate) fn write_section(&mut self, sec: Section, data: &[u8]) -> Result<(), String> {
        let start = self.begin_section()?;
        self.write_all(data)?;
        self.end_section(sec, start);
        Ok(())
    }

    pub(crate) fn write_u64_section(&mut self, sec: Section, vals: &[u64]) -> Result<(), String> {
        let start = self.begin_section()?;
        for &v in vals {
            self.write_all(&v.to_be_bytes())?;
        }
        self.end_section(sec, start);
        Ok(())
    }

    fn finish(mut self, tmp: &Path, out: &Path) -> Result<u64, String> {
        self.buf.flush().map_err(|e| format!("flush: {e}"))?;
        let file_bytes = self.pos;
        let mut file = self
            .buf
            .into_inner()
            .map_err(|e| format!("inner file: {e}"))?;
        let mut hdr = [0u8; HEADER_SIZE];
        hdr[..4].copy_from_slice(&MAGIC);
        hdr[4..8].copy_from_slice(&FORMAT_VERSION.to_be_bytes());
        hdr[8..12].copy_from_slice(&(SECTION_COUNT as u32).to_be_bytes());
        for (i, &(off, sz)) in self.sections.iter().enumerate() {
            let p = HEADER_FIXED_SIZE + i * SECTION_ENTRY_SIZE;
            if let Some(d) = hdr.get_mut(p..p + 8) {
                d.copy_from_slice(&off.to_be_bytes());
            }
            if let Some(d) = hdr.get_mut(p + 8..p + 16) {
                d.copy_from_slice(&sz.to_be_bytes());
            }
        }
        file.seek(SeekFrom::Start(0))
            .map_err(|e| format!("seek: {e}"))?;
        file.write_all(&hdr)
            .map_err(|e| format!("write header: {e}"))?;
        file.sync_all().map_err(|e| format!("sync: {e}"))?;
        drop(file);
        fs::rename(tmp, out).map_err(|e| format!("rename {}: {e}", out.display()))?;
        Ok(file_bytes)
    }
}

pub fn build_mmap_dataset(opts: &BuildOptions) -> Result<BuildStats, String> {
    if opts.input_dir.as_os_str().is_empty() || opts.output_path.as_os_str().is_empty() {
        return Err("input directory and output path are required".into());
    }
    for name in [
        "allocations.csv",
        "routes.csv",
        "autnums.csv",
        "geolocations.csv",
    ] {
        let p = opts.input_dir.join(name);
        let meta = fs::metadata(&p).map_err(|e| format!("open {name}: {e}"))?;
        if meta.is_dir() {
            return Err(format!("open {name}: path is a directory"));
        }
    }
    if let Some(parent) = opts
        .output_path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
    {
        fs::create_dir_all(parent).map_err(|e| format!("mkdir {}: {e}", parent.display()))?;
    }
    let tmp = PathBuf::from(format!("{}.building", opts.output_path.display()));
    let _ = fs::remove_file(&tmp);
    let file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&tmp)
        .map_err(|e| format!("create {}: {e}", tmp.display()))?;
    let res = run_build(file, &tmp, opts);
    if res.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    res
}

fn run_build(file: File, tmp: &Path, opts: &BuildOptions) -> Result<BuildStats, String> {
    let mut w = MmapWriter {
        buf: BufWriter::with_capacity(1 << 20, file),
        pos: 0,
        sections: [(0, 0); SECTION_COUNT],
    };
    w.write_all(&[0u8; HEADER_SIZE])?;
    let mut pool = StringPoolWriter::new();
    let mut summaries = SummaryCollector::new();
    let mut stats = BuildStats::default();

    build_allocations(
        &mut w,
        &mut pool,
        &mut summaries,
        &mut stats,
        &opts.input_dir.join("allocations.csv"),
    )?;
    build_routes(
        &mut w,
        &mut pool,
        &mut summaries,
        &mut stats,
        &opts.input_dir.join("routes.csv"),
    )?;
    build_geofeeds(
        &mut w,
        &mut pool,
        &mut stats,
        &opts.input_dir.join("geolocations.csv"),
    )?;
    build_autnums(
        &mut w,
        &mut pool,
        &mut stats,
        &opts.input_dir.join("autnums.csv"),
    )?;

    pool.clear_index();
    write_tail_sections(&mut w, &mut pool, summaries, &mut stats, opts)?;
    stats.file_bytes = w.finish(tmp, &opts.output_path)?;
    Ok(stats)
}

fn write_tail_sections(
    w: &mut MmapWriter,
    pool: &mut StringPoolWriter,
    summaries: SummaryCollector,
    stats: &mut BuildStats,
    opts: &BuildOptions,
) -> Result<(), String> {
    stats.string_pool_entries = pool.offsets.len().saturating_sub(1) as u64;
    stats.string_pool_bytes = pool.data.len() as u64;
    w.write_u64_section(Section::StringPoolOffsets, &pool.offsets)?;
    w.write_section(Section::StringPoolData, &pool.data)?;

    let (sum_data, sum_idx, sum_cnt) = summaries.build_sections();
    stats.materialized_summary = sum_cnt;
    w.write_section(Section::SummaryData, &sum_data)?;
    w.write_section(Section::SummaryIndex, &sum_idx)?;

    let meta = Metadata {
        schema_version: SCHEMA_VERSION,
        release_tag: opts.release_tag.clone(),
        built_at: opts.built_at.clone(),
        source_commit: opts.source_commit.clone(),
    };
    let meta_bytes = serde_json::to_vec(&meta).map_err(|e| format!("meta json: {e}"))?;
    w.write_section(Section::Metadata, &meta_bytes)
}
