use std::fs::{self, File};
use std::io::{BufWriter, Read};
use std::net::{IpAddr, Ipv6Addr};
use std::path::Path;

use bgp_compact::lite_db::LiteDbBuilder;
use bgp_compact::sortkey::{pad_sort_key_ip, u128_to_ip};
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
        let coords = rec.latitude.zip(rec.longitude);
        if builder.add_geofeed_with_coords(
            (rec.start, rec.end),
            (rec.country, rec.region, rec.city),
            coords,
            true,
        ) {
            added += 1;
        }
        Ok(())
    })?;
    Ok(added)
}

/// Exports coalesced DB-IP City Lite `.mmdb` or `.mmdb.gz` entries to an 8-column fallback CSV.
pub fn export_dbip_fallback_csv(dbip_path: &Path, out_path: &Path) -> Result<usize, String> {
    let tmp = out_path.with_extension("csv.tmp");
    let file = File::create(&tmp).map_err(|e| format!("create {}: {e}", tmp.display()))?;
    let mut writer = csv::Writer::from_writer(BufWriter::with_capacity(1 << 20, file));
    let mut count = 0usize;
    let mut pending: Option<PendingCsvRow> = None;
    for_each_dbip_city(dbip_path, |rec| {
        let (s, e) = (ip_to_u128(rec.start), ip_to_u128(rec.end));
        if let Some(p) = pending.as_mut()
            && p.can_merge(rec, s)
        {
            p.end = e;
            return Ok(());
        }
        if let Some(prev) = pending.take() {
            write_pending_row(&mut writer, &prev, &tmp)?;
            count += 1;
        }
        pending = Some(PendingCsvRow::from_rec(rec, s, e));
        Ok(())
    })?;
    if let Some(last) = pending {
        write_pending_row(&mut writer, &last, &tmp)?;
        count += 1;
    }
    writer
        .flush()
        .map_err(|e| format!("flush {}: {e}", tmp.display()))?;
    drop(writer);
    fs::rename(&tmp, out_path).map_err(|e| format!("rename {}: {e}", out_path.display()))?;
    Ok(count)
}

struct PendingCsvRow {
    ver: u8,
    start: u128,
    end: u128,
    country: String,
    region: String,
    city: String,
    latitude: Option<f64>,
    longitude: Option<f64>,
}

impl PendingCsvRow {
    fn from_rec(rec: &DbipCityRecord<'_>, start: u128, end: u128) -> Self {
        Self {
            ver: rec.version,
            start,
            end,
            country: rec.country.to_owned(),
            region: rec.region.to_owned(),
            city: rec.city.to_owned(),
            latitude: rec.latitude,
            longitude: rec.longitude,
        }
    }

    fn can_merge(&self, rec: &DbipCityRecord<'_>, next_start: u128) -> bool {
        self.ver == rec.version
            && self.end.checked_add(1) == Some(next_start)
            && self.country == rec.country
            && self.region == rec.region
            && self.city == rec.city
            && self.latitude.map(f64::to_bits) == rec.latitude.map(f64::to_bits)
            && self.longitude.map(f64::to_bits) == rec.longitude.map(f64::to_bits)
    }
}

fn ip_to_u128(ip: IpAddr) -> u128 {
    match ip {
        IpAddr::V4(v4) => u128::from(u32::from(v4)),
        IpAddr::V6(v6) => u128::from(v6),
    }
}

fn write_pending_row(
    writer: &mut csv::Writer<BufWriter<File>>,
    p: &PendingCsvRow,
    tmp: &Path,
) -> Result<(), String> {
    let mut lat_s = String::new();
    let mut lon_s = String::new();
    if let (Some(lat), Some(lon)) = (p.latitude, p.longitude) {
        use std::fmt::Write;
        let _ = write!(lat_s, "{lat}");
        let _ = write!(lon_s, "{lon}");
    }
    let is_v4 = p.ver == 4;
    writer
        .write_record([
            pad_sort_key_ip(u128_to_ip(p.start, is_v4)).as_str(),
            pad_sort_key_ip(u128_to_ip(p.end, is_v4)).as_str(),
            if is_v4 { "4" } else { "6" },
            p.country.as_str(),
            p.region.as_str(),
            p.city.as_str(),
            lat_s.as_str(),
            lon_s.as_str(),
        ])
        .map_err(|e| format!("write {}: {e}", tmp.display()))
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
