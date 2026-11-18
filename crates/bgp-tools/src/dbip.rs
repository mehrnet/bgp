use std::fs::File;
use std::io::Read;
use std::net::{IpAddr, Ipv6Addr};
use std::path::Path;

use bgp_compact::lite_db::LiteDbBuilder;
use flate2::read::GzDecoder;
use ipnetwork::{IpNetwork, Ipv6Network};
use maxminddb::geoip2::City;

/// Loads DB-IP City Lite `.mmdb` or `.mmdb.gz` entries as lower-priority fallback geo ranges.
pub fn load_dbip_fallback(path: &Path, builder: &mut LiteDbBuilder) -> Result<usize, String> {
    let bytes = read_mmdb_bytes(path)?;
    let reader =
        maxminddb::Reader::from_source(bytes).map_err(|e| format!("open dbip mmdb: {e}"))?;
    let mut added = 0usize;
    for root in ["0.0.0.0/0", "::/0"] {
        let net: IpNetwork = root.parse().map_err(|e| format!("parse {root}: {e}"))?;
        let iter = reader
            .within::<City>(net)
            .map_err(|e| format!("iterate dbip {root}: {e}"))?;
        for res in iter {
            let item = res.map_err(|e| format!("decode dbip record: {e}"))?;
            if add_city_record(builder, item.ip_net, &item.info) {
                added += 1;
            }
        }
    }
    Ok(added)
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

fn add_city_record(builder: &mut LiteDbBuilder, net: IpNetwork, info: &City<'_>) -> bool {
    let (start, end) = net_bounds(net);
    let cc = info.country.as_ref().and_then(|c| c.iso_code).unwrap_or("");
    let reg = info
        .subdivisions
        .as_ref()
        .and_then(|v| v.first())
        .and_then(|s| s.names.as_ref())
        .and_then(|n| n.get("en").copied())
        .unwrap_or("");
    let city = info
        .city
        .as_ref()
        .and_then(|c| c.names.as_ref())
        .and_then(|n| n.get("en").copied())
        .unwrap_or("");
    builder.add_geofeed_fallback(start, end, cc, reg, city)
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
