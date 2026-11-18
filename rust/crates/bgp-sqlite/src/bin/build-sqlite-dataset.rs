use std::path::Path;
use std::process;

use bgp_compact::cli::ParsedArgs;
use bgp_sqlite::build::build_sqlite_dataset;

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
    let input = args.get("input").unwrap_or("final_data");
    let output = args.get("output").unwrap_or("");
    let release_tag = args.get("release-tag").unwrap_or("");
    let built_at = args.get("built-at").unwrap_or("");
    let commit = args.get("source-commit").unwrap_or("");
    if output.is_empty() || release_tag.is_empty() || built_at.is_empty() {
        return Err(
            "usage: build-sqlite-dataset -input final_data -output mehrnet_bgp.db -release-tag db-... -built-at ... [-source-commit ...]".into(),
        );
    }
    build_sqlite_dataset(
        Path::new(input),
        Path::new(output),
        release_tag,
        built_at,
        commit,
    )
}
