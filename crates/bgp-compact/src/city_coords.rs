use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use crate::freq_dict::FreqDict;
use crate::lite_types::GeoLocKey;
use crate::sortkey::normalize_country;

/// Canonical lookup key `(ISO-3166-1 alpha-2 uppercase, trimmed lowercase city)`.
#[must_use]
pub fn city_coord_key(country: &str, city: &str) -> Option<(String, String)> {
    let cc = normalize_country(country)?;
    let trimmed = city.trim().to_ascii_lowercase();
    (!trimmed.is_empty()).then_some((cc, trimmed))
}

/// Parses a finite floating-point latitude or longitude string.
#[must_use]
pub fn parse_coord_str(raw: &str) -> Option<f64> {
    let v = raw.trim().parse::<f64>().ok()?;
    v.is_finite().then_some(v)
}

#[derive(Clone, Copy, Debug)]
struct CoordVote {
    lat: f64,
    lon: f64,
    quantized: (i32, i32),
    weight: usize,
}

/// Maps normalized `(country, city)` pairs to fallback coordinates loaded from CSV.
#[derive(Debug, Default)]
pub struct CityCoordIndex {
    votes: HashMap<(String, String), CoordVote>,
}

impl CityCoordIndex {
    /// Loads city centroids from `path` if it exists, or returns an empty index otherwise.
    pub fn from_fallback_csv_if_exists(path: &Path) -> Result<Self, String> {
        if !path.is_file() {
            return Ok(Self::default());
        }
        Self::from_fallback_csv(path)
    }

    /// Loads majority-vote `(country, city)` coordinates from a fallback geofeed CSV.
    pub fn from_fallback_csv(path: &Path) -> Result<Self, String> {
        let file = File::open(path).map_err(|e| format!("open {}: {e}", path.display()))?;
        let mut rdr = csv::ReaderBuilder::new()
            .has_headers(false)
            .flexible(true)
            .from_reader(BufReader::with_capacity(1 << 20, file));
        let mut rec = csv::ByteRecord::new();
        let mut idx = Self::default();
        while rdr
            .read_byte_record(&mut rec)
            .map_err(|e| format!("read {}: {e}", path.display()))?
        {
            idx.record_csv_row(&rec);
        }
        Ok(idx)
    }

    fn record_csv_row(&mut self, rec: &csv::ByteRecord) {
        let Some(lat) = field_coord(rec, 6) else {
            return;
        };
        let Some(lon) = field_coord(rec, 7) else {
            return;
        };
        let cc = field_str(rec, 3);
        let city = field_str(rec, 5);
        let Some(key) = city_coord_key(&cc, &city) else {
            return;
        };
        let quantized = (
            (lat * 100_000.0).round() as i32,
            (lon * 100_000.0).round() as i32,
        );
        match self.votes.get_mut(&key) {
            Some(vote) if vote.quantized == quantized => vote.weight += 1,
            Some(vote) if vote.weight <= 1 => {
                *vote = CoordVote {
                    lat,
                    lon,
                    quantized,
                    weight: 1,
                };
            }
            Some(vote) => vote.weight -= 1,
            None => {
                self.votes.insert(
                    key,
                    CoordVote {
                        lat,
                        lon,
                        quantized,
                        weight: 1,
                    },
                );
            }
        }
    }

    /// Returns fallback `(latitude, longitude)` when `(country, city)` matches an indexed city.
    #[must_use]
    pub fn lookup(&self, country: &str, city: &str) -> Option<(f64, f64)> {
        let key = city_coord_key(country, city)?;
        let vote = self.votes.get(&key)?;
        Some((vote.lat, vote.lon))
    }

    /// Returns explicit `(lat, lon)` when both are present, else falls back by `(country, city)`.
    #[must_use]
    pub fn resolve(
        &self,
        raw_lat: Option<f64>,
        raw_lon: Option<f64>,
        country: &str,
        city: &str,
    ) -> (Option<f64>, Option<f64>) {
        if let (Some(lat), Some(lon)) = (raw_lat, raw_lon) {
            return (Some(lat), Some(lon));
        }
        match self.lookup(country, city) {
            Some((lat, lon)) => (Some(lat), Some(lon)),
            None => (raw_lat, raw_lon),
        }
    }
}

fn field_str(rec: &csv::ByteRecord, idx: usize) -> String {
    rec.get(idx)
        .map(|b| String::from_utf8_lossy(b).trim().to_owned())
        .unwrap_or_default()
}

fn field_coord(rec: &csv::ByteRecord, idx: usize) -> Option<f64> {
    let raw = rec.get(idx)?;
    parse_coord_str(&String::from_utf8_lossy(raw))
}

/// Fills missing coordinates on compact geofeed entries when `(country, city)` matches a known entry.
pub(crate) fn backfill_geo_coords(
    geo_loc_list: &mut [GeoLocKey],
    geo_loc_freq: &[usize],
    countries: &FreqDict,
    cities: &FreqDict,
) {
    let mut best: HashMap<(String, String), ((i32, i32), usize)> = HashMap::new();
    for (id, loc) in geo_loc_list.iter().enumerate() {
        let Some(coords) = loc.coords else {
            continue;
        };
        let Some(key) = city_coord_key(countries.get(loc.cc_id), cities.get(loc.city_id)) else {
            continue;
        };
        let freq = geo_loc_freq.get(id).copied().unwrap_or(0);
        match best.get_mut(&key) {
            Some((prev_coords, prev_freq)) if freq > *prev_freq => {
                *prev_coords = coords;
                *prev_freq = freq;
            }
            Some(_) => {}
            None => {
                best.insert(key, (coords, freq));
            }
        }
    }
    for loc in geo_loc_list.iter_mut() {
        if loc.coords.is_some() {
            continue;
        }
        let Some(key) = city_coord_key(countries.get(loc.cc_id), cities.get(loc.city_id)) else {
            continue;
        };
        if let Some(&(coords, _)) = best.get(&key) {
            loc.coords = Some(coords);
        }
    }
}
