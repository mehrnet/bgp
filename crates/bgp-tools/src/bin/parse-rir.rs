use std::path::Path;
use std::process;

use bgp_tools::rir::parse_rir_dir;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let Some(rir) = args.get(1).filter(|_| args.len() == 2) else {
        eprintln!("usage: parse-rir <rir>");
        process::exit(1);
    };
    if let Err(err) = parse_rir_dir(Path::new("raw_data"), Path::new("."), rir) {
        eprintln!("fatal: {err}");
        process::exit(1);
    }
}
