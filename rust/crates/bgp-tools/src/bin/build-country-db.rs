use std::fs::{self, File};
use std::io::BufReader;
use std::path::Path;
use std::process;

use bgp_compact::cli::ParsedArgs;
use bgp_compact::country_db::{
    AddResult, CountryDbBuilder, DEFAULT_V4_BITS, DEFAULT_V6_BITS, LAYER_ALLOCATION, LAYER_GEOFEED,
};
use bgp_compact::sortkey::addr_from_sort_key;

#[derive(Default)]
struct Counts {
    added: usize,
    country: usize,
    granularity: usize,
    address: usize,
    malformed: usize,
}

fn main() {
    if let Err(err) = run() {
        eprintln!("fatal: {err}");
        process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let args = ParsedArgs::parse(
        std::env::args(),
        &["input", "output", "v4-prefix", "v6-prefix"],
        &["geofeeds"],
    )?;
    let input = args.get("input").unwrap_or("final_data");
    let output = args.get("output").unwrap_or("country.mncc");
    let v4 = args.get("v4-prefix").map_or(Ok(DEFAULT_V4_BITS), |s| {
        s.parse().map_err(|_| "invalid v4-prefix".to_owned())
    })?;
    let v6 = args.get("v6-prefix").map_or(Ok(DEFAULT_V6_BITS), |s| {
        s.parse().map_err(|_| "invalid v6-prefix".to_owned())
    })?;
    let use_geofeeds = args.get_bool("geofeeds", true);

    let mut builder = CountryDbBuilder::new(v4, v6)?;
    let sources = [
        ("allocations.csv", 4usize, LAYER_ALLOCATION, false),
        ("geolocations.csv", 3, LAYER_GEOFEED, !use_geofeeds),
    ];
    for (name, col, layer, skip) in sources {
        if skip {
            continue;
        }
        let c = load(&Path::new(input).join(name), col, layer, &mut builder)?;
        println!(
            "{name}: used={} no_iso_country={} finer_than_granularity={} bad_address={} malformed={}",
            c.added, c.country, c.granularity, c.address, c.malformed
        );
    }
    let tmp = format!("{output}.tmp");
    let mut file = File::create(&tmp).map_err(|e| format!("create {tmp}: {e}"))?;
    let stats = builder
        .encode(&mut file)
        .map_err(|e| format!("encode {tmp}: {e}"))?;
    drop(file);
    fs::rename(&tmp, output).map_err(|e| format!("rename {output}: {e}"))?;
    println!(
        "wrote {output}: {} bytes, ipv4_ranges={} ipv6_ranges={} countries={}",
        stats.bytes, stats.v4_entries, stats.v6_entries, stats.countries
    );
    Ok(())
}

fn load(
    path: &Path,
    country_col: usize,
    layer: i64,
    b: &mut CountryDbBuilder,
) -> Result<Counts, String> {
    let file = File::open(path).map_err(|e| format!("open {}: {e}", path.display()))?;
    let mut rdr = csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .from_reader(BufReader::with_capacity(1 << 20, file));
    let mut rec = csv::ByteRecord::new();
    let mut c = Counts::default();
    while rdr
        .read_byte_record(&mut rec)
        .map_err(|e| format!("read {}: {e}", path.display()))?
    {
        if rec.len() <= country_col {
            c.malformed += 1;
            continue;
        }
        let ver = String::from_utf8_lossy(rec.get(2).unwrap_or(&[])).into_owned();
        let (Some(s), Some(e)) = (
            addr_from_sort_key(&String::from_utf8_lossy(rec.get(0).unwrap_or(&[])), &ver),
            addr_from_sort_key(&String::from_utf8_lossy(rec.get(1).unwrap_or(&[])), &ver),
        ) else {
            c.malformed += 1;
            continue;
        };
        let cc = String::from_utf8_lossy(rec.get(country_col).unwrap_or(&[]))
            .trim()
            .to_ascii_uppercase();
        match b.add(s, e, &cc, layer) {
            AddResult::Added => c.added += 1,
            AddResult::SkippedCountry => c.country += 1,
            AddResult::SkippedGranularity => c.granularity += 1,
            AddResult::SkippedAddress => c.address += 1,
        }
    }
    Ok(c)
}
