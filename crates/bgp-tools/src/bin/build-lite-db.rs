use std::fs::{self, File};
use std::io::BufReader;
use std::path::Path;
use std::process;

use bgp_compact::cli::ParsedArgs;
use bgp_compact::lite_db::LiteDbBuilder;
use bgp_compact::lite_types::{
    AutnumInput, DEFAULT_ALLOC_V4_BITS, DEFAULT_ALLOC_V6_BITS, LiteBuildOptions, LiteMetadata,
};
use bgp_compact::sortkey::{addr_from_sort_key, cidr_to_bounds, parse_asn};

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
            "alloc-v4-prefix",
            "alloc-v6-prefix",
        ],
        &["include-asn-routes"],
    )?;
    let opts = parse_options(&args)?;
    let input = Path::new(args.get("input").unwrap_or("final_data"));
    let output = args.get("output").unwrap_or("bgp-lite.mnl");
    let mut b = LiteDbBuilder::new(opts);
    let (ac, alc, gc, rc) = load_csvs(input, &mut b)?;
    let tmp = format!("{output}.tmp");
    let mut file = File::create(&tmp).map_err(|e| format!("create {tmp}: {e}"))?;
    let stats = b.encode(&mut file)?;
    drop(file);
    fs::rename(&tmp, output).map_err(|e| format!("rename {output}: {e}"))?;
    println!("loaded autnums={ac} allocations={alc} geofeeds={gc} routes={rc}");
    println!(
        "wrote {output}: bytes={} ({:.2} MiB) countries={} registries={} regions={} cities={} geolocs={} alloc=({},{}) geo=({},{}) route=({},{}) multi_sets={} asn_routes={}",
        stats.bytes,
        stats.bytes as f64 / (1024.0 * 1024.0),
        stats.countries,
        stats.registries,
        stats.regions,
        stats.cities,
        stats.geo_locations,
        stats.alloc_v4_segs,
        stats.alloc_v6_segs,
        stats.geo_v4_segs,
        stats.geo_v6_segs,
        stats.route_v4_segs,
        stats.route_v6_segs,
        stats.multi_asn_sets,
        stats.asn_route_items
    );
    Ok(())
}

fn parse_options(args: &ParsedArgs) -> Result<LiteBuildOptions, String> {
    let v4: u8 = args
        .get("alloc-v4-prefix")
        .map_or(Ok(DEFAULT_ALLOC_V4_BITS), |s| {
            s.parse().map_err(|_| "invalid alloc-v4-prefix".to_owned())
        })?;
    let v6: u8 = args
        .get("alloc-v6-prefix")
        .map_or(Ok(DEFAULT_ALLOC_V6_BITS), |s| {
            s.parse().map_err(|_| "invalid alloc-v6-prefix".to_owned())
        })?;
    if !(1..=32).contains(&v4) {
        return Err(format!("invalid -alloc-v4-prefix {v4} (must be 1..32)"));
    }
    if !(1..=64).contains(&v6) {
        return Err(format!("invalid -alloc-v6-prefix {v6} (must be 1..64)"));
    }
    Ok(LiteBuildOptions {
        metadata: LiteMetadata {
            release_tag: args.get("release-tag").unwrap_or("").to_owned(),
            built_at: args.get("built-at").unwrap_or("").to_owned(),
            source_commit: args.get("source-commit").unwrap_or("").to_owned(),
        },
        include_asn_routes: args.get_bool("include-asn-routes", true),
        alloc_v4_bits: v4,
        alloc_v6_bits: v6,
    })
}

fn load_csvs(dir: &Path, b: &mut LiteDbBuilder) -> Result<(usize, usize, usize, usize), String> {
    Ok((
        load_autnums(&dir.join("autnums.csv"), b)?,
        load_allocations(&dir.join("allocations.csv"), b)?,
        load_geofeeds(&dir.join("geolocations.csv"), b)?,
        load_routes(&dir.join("routes.csv"), b)?,
    ))
}

fn load_autnums(path: &Path, b: &mut LiteDbBuilder) -> Result<usize, String> {
    let mut ac = 0usize;
    each_row(path, |r| {
        let asn = if r.len() >= 12 {
            parse_asn(&col(r, 0))
        } else {
            0
        };
        if asn != 0
            && b.add_autnum(AutnumInput {
                as_number: asn,
                registry: col(r, 1),
                country: col(r, 2),
                name: col(r, 3),
                organization: col(r, 4),
                status: col(r, 5),
                source: col(r, 8),
                maintainers: col(r, 9),
                abuse_contact: col(r, 10),
                description: col(r, 11),
            })
        {
            ac += 1;
        }
    })?;
    Ok(ac)
}

fn load_allocations(path: &Path, b: &mut LiteDbBuilder) -> Result<usize, String> {
    let mut alc = 0usize;
    each_row(path, |r| {
        if let (true, Some(s), Some(e)) = (
            r.len() >= 6,
            addr_from_sort_key(&col(r, 0), &col(r, 2)),
            addr_from_sort_key(&col(r, 1), &col(r, 2)),
        ) && b.add_allocation(s, e, &col(r, 3), &col(r, 4))
        {
            alc += 1;
        }
    })?;
    Ok(alc)
}

fn load_geofeeds(path: &Path, b: &mut LiteDbBuilder) -> Result<usize, String> {
    let mut gc = 0usize;
    each_row(path, |r| {
        if let (true, Some(s), Some(e)) = (
            r.len() >= 6,
            addr_from_sort_key(&col(r, 0), &col(r, 2)),
            addr_from_sort_key(&col(r, 1), &col(r, 2)),
        ) && b.add_geofeed(s, e, &col(r, 3), &col(r, 4), &col(r, 5))
        {
            gc += 1;
        }
    })?;
    Ok(gc)
}

fn load_routes(path: &Path, b: &mut LiteDbBuilder) -> Result<usize, String> {
    let mut rc = 0usize;
    each_row(path, |r| {
        if let (true, Some((_, _, _, net, bits))) = (r.len() >= 5, cidr_to_bounds(&col(r, 3)))
            && b.add_route(net, bits, parse_asn(&col(r, 4)))
        {
            rc += 1;
        }
    })?;
    Ok(rc)
}

fn each_row(path: &Path, mut cb: impl FnMut(&csv::ByteRecord)) -> Result<(), String> {
    let file = File::open(path).map_err(|e| format!("open {}: {e}", path.display()))?;
    let mut rdr = csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .from_reader(BufReader::with_capacity(1 << 20, file));
    let mut rec = csv::ByteRecord::new();
    while rdr
        .read_byte_record(&mut rec)
        .map_err(|e| format!("read {}: {e}", path.display()))?
    {
        cb(&rec);
    }
    Ok(())
}

fn col(r: &csv::ByteRecord, i: usize) -> String {
    r.get(i)
        .map(|b| String::from_utf8_lossy(b).into_owned())
        .unwrap_or_default()
}
