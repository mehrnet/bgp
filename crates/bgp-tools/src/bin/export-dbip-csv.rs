use std::path::Path;
use std::process;

use bgp_compact::cli::ParsedArgs;
use bgp_tools::dbip::export_dbip_fallback_csv;

fn main() {
    if let Err(err) = run() {
        eprintln!("fatal: {err}");
        process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let args = ParsedArgs::parse(std::env::args(), &["dbip", "output"], &[])?;
    let dbip = args
        .get("dbip")
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "missing -dbip path".to_owned())?;
    let output = args
        .get("output")
        .unwrap_or("final_data/geolocations_fallback.csv");
    let count = export_dbip_fallback_csv(Path::new(dbip), Path::new(output))?;
    println!("exported {count} dbip fallback records to {output}");
    Ok(())
}
