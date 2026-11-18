use std::path::Path;
use std::process;

use bgp_compact::cli::ParsedArgs;
use bgp_sqlite::validate::validate_sqlite_db;

fn main() {
    if let Err(err) = run() {
        eprintln!("fatal: {err}");
        process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let args = ParsedArgs::parse(std::env::args(), &[], &["indexed"])?;
    let Some(db_path) = args.positionals.first() else {
        return Err("usage: validate-database [-indexed] /path/to/mehrnet_bgp.db".into());
    };
    if args.positionals.len() != 1 {
        return Err("usage: validate-database [-indexed] /path/to/mehrnet_bgp.db".into());
    }
    validate_sqlite_db(Path::new(db_path), args.get_bool("indexed", false))
}
