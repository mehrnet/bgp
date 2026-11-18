use std::path::Path;
use std::process;

use bgp_compact::cli::ParsedArgs;
use bgp_sqlite::prefix_index::build_prefix_index;

fn main() {
    if let Err(err) = run() {
        eprintln!("fatal: {err}");
        process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let args = ParsedArgs::parse(std::env::args(), &[], &[])?;
    let Some(db_path) = args.positionals.first() else {
        return Err("usage: build-prefix-index /path/to/mehrnet_bgp.db".into());
    };
    if args.positionals.len() != 1 {
        return Err("usage: build-prefix-index /path/to/mehrnet_bgp.db".into());
    }
    build_prefix_index(Path::new(db_path))
}
