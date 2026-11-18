use std::net::IpAddr;

use bgp_compact::country_db::{
    AddResult, CountryDbBuilder, DEFAULT_V4_BITS, DEFAULT_V6_BITS, LAYER_ALLOCATION, LAYER_GEOFEED,
};
use bgp_compact::country_reader::CountryDb;
use bgp_compact::lite_db::LiteDbBuilder;
use bgp_compact::lite_types::{
    AutnumInput, DEFAULT_ALLOC_V4_BITS, DEFAULT_ALLOC_V6_BITS, DEFAULT_GEO_V4_BITS,
    DEFAULT_GEO_V6_BITS, LiteBuildOptions, LiteMetadata,
};

fn ip(s: &str) -> Result<IpAddr, std::net::AddrParseError> {
    s.parse()
}

#[test]
fn country_db_roundtrips_alloc_geofeed_and_granularity() -> Result<(), Box<dyn std::error::Error>> {
    let mut b = CountryDbBuilder::new(DEFAULT_V4_BITS, DEFAULT_V6_BITS)?;
    assert_eq!(
        b.add(ip("1.0.0.0")?, ip("1.255.255.255")?, "AU", LAYER_ALLOCATION),
        AddResult::Added
    );
    assert_eq!(
        b.add(ip("1.1.1.0")?, ip("1.1.1.255")?, "US", LAYER_GEOFEED),
        AddResult::Added
    );
    assert_eq!(
        b.add(ip("1.1.1.0")?, ip("1.1.1.127")?, "DE", LAYER_GEOFEED),
        AddResult::SkippedGranularity
    );
    assert_eq!(
        b.add(ip("2.2.2.0")?, ip("2.2.2.255")?, "ZZZ", LAYER_ALLOCATION),
        AddResult::SkippedCountry
    );
    assert_eq!(
        b.add(
            ip("2001:db8::")?,
            ip("2001:db8:ffff:ffff:ffff:ffff:ffff:ffff")?,
            "JP",
            LAYER_ALLOCATION
        ),
        AddResult::Added
    );
    let mut bytes = Vec::new();
    let stats = b.encode(&mut bytes)?;
    assert_eq!(stats.countries, 3);
    let db = CountryDb::open(&bytes)?;
    assert_eq!(db.lookup(ip("1.0.5.5")?), Some("AU"));
    assert_eq!(db.lookup(ip("1.1.1.42")?), Some("US"));
    assert_eq!(db.lookup(ip("::ffff:1.1.1.42")?), Some("US"));
    assert_eq!(db.lookup(ip("2001:db8::1")?), Some("JP"));
    assert_eq!(db.lookup(ip("8.8.8.8")?), None);
    Ok(())
}

#[test]
fn lite_db_encodes_valid_mnl1_header_and_tables() -> Result<(), Box<dyn std::error::Error>> {
    let mut b = LiteDbBuilder::new(LiteBuildOptions {
        metadata: LiteMetadata {
            release_tag: "db-rs-lite".into(),
            built_at: "2026-11-18T00:00:00Z".into(),
            source_commit: "deadbeef".into(),
        },
        include_asn_routes: true,
        alloc_v4_bits: DEFAULT_ALLOC_V4_BITS,
        alloc_v6_bits: DEFAULT_ALLOC_V6_BITS,
        geo_v4_bits: DEFAULT_GEO_V4_BITS,
        geo_v6_bits: DEFAULT_GEO_V6_BITS,
    });
    assert!(b.add_autnum(AutnumInput {
        as_number: 13335,
        registry: "APNIC".into(),
        country: "US".into(),
        name: "CLOUDFLARENET".into(),
        ..Default::default()
    }));
    assert!(b.add_allocation(ip("1.1.1.0")?, ip("1.1.1.255")?, "APNIC", "AU"));
    assert!(b.add_geofeed(ip("1.1.1.0")?, ip("1.1.1.255")?, "AU", "QLD", "Brisbane"));
    assert!(b.add_route(ip("1.1.1.0")?, 24, 13335));
    assert!(b.add_route(ip("1.1.1.0")?, 24, 64500));

    let mut buf = Vec::new();
    let stats = b.encode(&mut buf)?;
    assert_eq!(
        buf.get(..13),
        Some(
            &[
                b'M',
                b'N',
                b'L',
                b'1',
                6,
                0b0000_1111,
                64,
                32,
                64,
                32,
                64,
                32,
                64
            ][..]
        )
    );
    assert_eq!(stats.autnums, 1);
    assert_eq!(stats.multi_asn_sets, 1);
    assert_eq!(stats.asn_route_items, 2);
    Ok(())
}

#[test]
fn lite_db_splits_alloc_and_geo_quantization() -> Result<(), Box<dyn std::error::Error>> {
    let mut b = LiteDbBuilder::new(LiteBuildOptions {
        metadata: LiteMetadata::default(),
        include_asn_routes: false,
        alloc_v4_bits: DEFAULT_ALLOC_V4_BITS,
        alloc_v6_bits: DEFAULT_ALLOC_V6_BITS,
        geo_v4_bits: 24,
        geo_v6_bits: 48,
    });
    assert!(b.add_allocation(ip("1.1.2.0")?, ip("1.1.2.15")?, "APNIC", "JP"));
    assert!(!b.add_geofeed(ip("1.1.2.0")?, ip("1.1.2.15")?, "JP", "Tokyo", "Tokyo"));
    let mut buf = Vec::new();
    let stats = b.encode(&mut buf)?;
    assert_eq!(stats.alloc_v4_segs, 3);
    assert_eq!(stats.geo_v4_segs, 1);
    Ok(())
}

#[test]
fn lite_db_rfc8805_overrides_narrower_fallback() -> Result<(), Box<dyn std::error::Error>> {
    let mut b = LiteDbBuilder::new(LiteBuildOptions {
        metadata: LiteMetadata::default(),
        include_asn_routes: false,
        alloc_v4_bits: DEFAULT_ALLOC_V4_BITS,
        alloc_v6_bits: DEFAULT_ALLOC_V6_BITS,
        geo_v4_bits: DEFAULT_GEO_V4_BITS,
        geo_v6_bits: DEFAULT_GEO_V6_BITS,
    });
    assert!(b.add_geofeed_fallback(ip("1.1.1.0")?, ip("1.1.1.127")?, "DE", "Berlin", "Berlin"));
    assert!(b.add_geofeed(ip("1.1.1.0")?, ip("1.1.1.255")?, "AU", "QLD", "Brisbane"));
    let mut buf = Vec::new();
    let stats = b.encode(&mut buf)?;
    assert_eq!(stats.geo_v4_segs, 3);
    assert_eq!(stats.geo_locations, 2);
    Ok(())
}
