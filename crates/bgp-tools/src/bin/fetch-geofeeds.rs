use std::path::Path;
use std::process;

use bgp_tools::geofeeds::fetch_geofeeds;

fn main() {
    if let Err(err) = fetch_geofeeds(
        Path::new("final_data/geofeeds.txt"),
        Path::new("final_data/geolocations.csv"),
    ) {
        eprintln!("fatal: {err}");
        process::exit(1);
    }
}
