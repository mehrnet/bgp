use std::fs::{self, File};
use std::io::{BufWriter, Read};
use std::net::{IpAddr, Ipv6Addr};
use std::path::Path;

use bgp_compact::lite_db::LiteDbBuilder;
use bgp_compact::sortkey::pad_sort_key_ip;
use flate2::read::GzDecoder;
use ipnetwork::{IpNetwork, Ipv6Network};
use maxminddb::geoip2::City;

#[derive(Debug, Clone, Copy)]
pub struct DbipCityRecord<'a> {
    pub start: IpAddr,
    pub end: IpAddr,
    pub version: u8,
    pub country: &'a str,
    pub region: &'a str,
    pub city: &'a str,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
}

/// Streams decoded DB-IP City Lite `.mmdb` or `.mmdb.gz` ranges into a callback.
pub fn for_each_dbip_city(
    path: &Path,
    mut cb: impl FnMut(&DbipCityRecord<'_>) -> Result<(), String>,
) -> Result<(), String> {
    let bytes = read_mmdb_bytes(path)?;
    let reader =
        maxminddb::Reader::from_source(bytes).map_err(|e| format!("open dbip mmdb: {e}"))?;
    for root in ["0.0.0.0/0", "::/0"] {
        let net: IpNetwork = root.parse().map_err(|e| format!("parse {root}: {e}"))?;
        let iter = reader
            .within::<City>(net)
            .map_err(|e| format!("iterate dbip {root}: {e}"))?;
        for res in iter {
            let item = res.map_err(|e| format!("decode dbip record: {e}"))?;
            if let Some(rec) = extract_city_record(item.ip_net, &item.info) {
                cb(&rec)?;
            }
        }
    }
    Ok(())
}

/// Loads DB-IP City Lite `.mmdb` or `.mmdb.gz` entries as lower-priority fallback geo ranges.
pub fn load_dbip_fallback(path: &Path, builder: &mut LiteDbBuilder) -> Result<usize, String> {
    let mut added = 0usize;
    for_each_dbip_city(path, |rec| {
        if builder.add_geofeed_fallback(rec.start, rec.end, rec.country, rec.region, rec.city) {
            added += 1;
        }
        Ok(())
    })?;
    Ok(added)
}

/// Exports DB-IP City Lite `.mmdb` or `.mmdb.gz` entries to an 8-column fallback geofeed CSV.
pub fn export_dbip_fallback_csv(dbip_path: &Path, out_path: &Path) -> Result<usize, String> {
    let tmp = out_path.with_extension("csv.tmp");
    let file = File::create(&tmp).map_err(|e| format!("create {}: {e}", tmp.display()))?;
    let mut writer = csv::Writer::from_writer(BufWriter::with_capacity(1 << 20, file));
    let mut count = 0usize;
    let mut lat_s = String::new();
    let mut lon_s = String::new();
    for_each_dbip_city(dbip_path, |rec| {
        lat_s.clear();
        lon_s.clear();
        if let (Some(lat), Some(lon)) = (rec.latitude, rec.longitude) {
            use std::fmt::Write;
            let _ = write!(lat_s, "{lat}");
            let _ = write!(lon_s, "{lon}");
        }
        writer
            .write_record([
                pad_sort_key_ip(rec.start).as_str(),
                pad_sort_key_ip(rec.end).as_str(),
                if rec.version == 4 { "4" } else { "6" },
                rec.country,
                rec.region,
                rec.city,
                lat_s.as_str(),
                lon_s.as_str(),
            ])
            .map_err(|e| format!("write {}: {e}", tmp.display()))?;
        count += 1;
        Ok(())
    })?;
    writer
        .flush()
        .map_err(|e| format!("flush {}: {e}", tmp.display()))?;
    drop(writer);
    fs::rename(&tmp, out_path).map_err(|e| format!("rename {}: {e}", out_path.display()))?;
    Ok(count)
}

fn read_mmdb_bytes(path: &Path) -> Result<Vec<u8>, String> {
    let mut file = File::open(path).map_err(|e| format!("open {}: {e}", path.display()))?;
    let mut raw = Vec::new();
    file.read_to_end(&mut raw)
        .map_err(|e| format!("read {}: {e}", path.display()))?;
    if raw.get(..2) != Some(&[0x1f, 0x8b][..]) {
        return Ok(raw);
    }
    let mut out = Vec::with_capacity(raw.len() * 2);
    GzDecoder::new(raw.as_slice())
        .read_to_end(&mut out)
        .map_err(|e| format!("gunzip {}: {e}", path.display()))?;
    Ok(out)
}

fn extract_city_record<'a>(net: IpNetwork, info: &'a City<'a>) -> Option<DbipCityRecord<'a>> {
    let (start, end) = net_bounds(net);
    let version = if net.is_ipv4() { 4 } else { 6 };
    let cc = info
        .country
        .as_ref()
        .and_then(|c| c.iso_code)
        .unwrap_or("")
        .trim();
    let reg = info
        .subdivisions
        .as_ref()
        .and_then(|v| v.first())
        .and_then(|s| s.names.as_ref())
        .and_then(|n| n.get("en").copied())
        .unwrap_or("")
        .trim();
    let city = info
        .city
        .as_ref()
        .and_then(|c| c.names.as_ref())
        .and_then(|n| n.get("en").copied())
        .unwrap_or("")
        .trim();
    if cc.is_empty() && reg.is_empty() && city.is_empty() {
        return None;
    }
    let loc = info.location.as_ref();
    let latitude = loc.and_then(|l| l.latitude).filter(|v| v.is_finite());
    let longitude = loc.and_then(|l| l.longitude).filter(|v| v.is_finite());
    Some(DbipCityRecord {
        start,
        end,
        version,
        country: cc,
        region: reg,
        city,
        latitude,
        longitude,
    })
}

fn net_bounds(net: IpNetwork) -> (IpAddr, IpAddr) {
    match net {
        IpNetwork::V4(v4) => (IpAddr::V4(v4.network()), IpAddr::V4(v4.broadcast())),
        IpNetwork::V6(v6) => v6_bounds(v6),
    }
}

fn v6_bounds(v6: Ipv6Network) -> (IpAddr, IpAddr) {
    let start = u128::from(v6.network());
    let end = if v6.prefix() == 0 {
        u128::MAX
    } else {
        start | ((1u128 << (128 - v6.prefix())) - 1)
    };
    (
        IpAddr::V6(Ipv6Addr::from(start)),
        IpAddr::V6(Ipv6Addr::from(end)),
    )
}
