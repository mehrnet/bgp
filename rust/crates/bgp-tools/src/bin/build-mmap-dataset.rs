use std::path::PathBuf;
use std::process;

use bgp_compact::cli::ParsedArgs;
use mnmf_store::builder::{BuildOptions, build_mmap_dataset};

fn main() {
    if let Err(err) = run() {
        eprintln!("fatal: {err}");
        process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let args = ParsedArgs::parse(
        std::env::args(),
        &[
            "input",
            "output",
            "release-tag",
            "built-at",
            "source-commit",
        ],
        &[],
    )?;
    let opts = BuildOptions {
        input_dir: PathBuf::from(args.get("input").unwrap_or("")),
        output_path: PathBuf::from(args.get("output").unwrap_or("")),
        release_tag: args.get("release-tag").unwrap_or("").to_owned(),
        built_at: args.get("built-at").unwrap_or("").to_owned(),
        source_commit: args.get("source-commit").unwrap_or("").to_owned(),
    };
    let s = build_mmap_dataset(&opts)?;
    println!(
        "built {}: file_bytes={} allocations={} routes={} geofeeds={} autnums={} allocation_index={} route_index={} geofeed_index={} allocation_range_index={} route_range_index={} route_lpm_nodes={} route_lpm_ids={} route_lpm_bytes={} allocation_lpm_nodes={} allocation_lpm_bytes={} geofeed_lpm_nodes={} geofeed_lpm_bytes={} string_pool_entries={} string_pool_bytes={} materialized_summaries={}",
        opts.output_path.display(),
        s.file_bytes,
        s.allocations,
        s.routes,
        s.geofeeds,
        s.autnums,
        s.allocation_index,
        s.route_index,
        s.geofeed_index,
        s.allocation_range_index,
        s.route_range_index,
        s.route_lpm_nodes,
        s.route_lpm_ids,
        s.route_lpm_bytes,
        s.allocation_lpm_nodes,
        s.allocation_lpm_bytes,
        s.geofeed_lpm_nodes,
        s.geofeed_lpm_bytes,
        s.string_pool_entries,
        s.string_pool_bytes,
        s.materialized_summary,
    );
    Ok(())
}
