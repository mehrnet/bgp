use std::path::Path;
use std::process;

use bgp_compact::cli::ParsedArgs;
use bgp_sqlite::range_summaries::build_range_summaries;

fn main() {
    if let Err(err) = run() {
        eprintln!("fatal: {err}");
        process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let args = ParsedArgs::parse(std::env::args(), &[], &[])?;
    let Some(db_path) = args.positionals.first() else {
        return Err("usage: build-range-summaries /path/to/mehrnet_bgp.db".into());
    };
    if args.positionals.len() != 1 {
        return Err("usage: build-range-summaries /path/to/mehrnet_bgp.db".into());
    }
    let rows = build_range_summaries(Path::new(db_path))?;
    println!("range summaries: {rows} rows");
    Ok(())
}
