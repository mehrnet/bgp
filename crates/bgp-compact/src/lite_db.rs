use std::collections::{BTreeSet, HashMap};
use std::io::Write;
use std::net::IpAddr;

use crate::freq_dict::FreqDict;
use crate::lite_encode::encode_mnl1;
use crate::lite_routes::{CompactDicts, build_route_segments, remap_allocs, remap_geos};
use crate::lite_tables::{
    FALLBACK_PRIORITY_BIAS, insert_geofeed, push_asn_route, push_interval, push_unique,
};
use crate::lite_types::{
    AsnRoute, AutnumInput, DEFAULT_ALLOC_V4_BITS, DEFAULT_ALLOC_V6_BITS, DEFAULT_GEO_V4_BITS,
    DEFAULT_GEO_V6_BITS, DEFAULT_ROUTE_V6_BITS, EncodeInput, GeoLocKey, LiteBuildOptions,
    LiteStats,
};
use crate::sortkey::{normalize_country, slot_range, unmap_ip};
use crate::sweep::{Interval, flatten_intervals};

#[derive(Debug)]
pub struct LiteDbBuilder {
    pub(crate) opts: LiteBuildOptions,
    pub(crate) autnums: HashMap<u32, AutnumInput>,
    pub(crate) all_asns: BTreeSet<u32>,
    pub(crate) asn_routes: HashMap<u32, Vec<AsnRoute>>,
    pub(crate) countries: FreqDict,
    pub(crate) registries: FreqDict,
    pub(crate) statuses: FreqDict,
    pub(crate) sources: FreqDict,
    pub(crate) regions: FreqDict,
    pub(crate) cities: FreqDict,
    pub(crate) geo_loc_map: HashMap<GeoLocKey, u32>,
    pub(crate) geo_loc_list: Vec<GeoLocKey>,
    pub(crate) geo_loc_freq: Vec<usize>,
    pub(crate) alloc_v4: Vec<Interval>,
    pub(crate) alloc_v6: Vec<Interval>,
    pub(crate) geo_v4: Vec<Interval>,
    pub(crate) geo_v6: Vec<Interval>,
    pub(crate) route_origins_v4: HashMap<(u32, u8), Vec<u32>>,
    pub(crate) route_origins_v6: HashMap<(u64, u8), Vec<u32>>,
    pub(crate) seq: usize,
}

impl LiteDbBuilder {
    #[must_use]
    pub fn new(mut opts: LiteBuildOptions) -> Self {
        if opts.alloc_v4_bits == 0 {
            opts.alloc_v4_bits = DEFAULT_ALLOC_V4_BITS;
        }
        if opts.alloc_v6_bits == 0 {
            opts.alloc_v6_bits = DEFAULT_ALLOC_V6_BITS;
        }
        if opts.geo_v4_bits == 0 {
            opts.geo_v4_bits = DEFAULT_GEO_V4_BITS;
        }
        if opts.geo_v6_bits == 0 {
            opts.geo_v6_bits = DEFAULT_GEO_V6_BITS;
        }
        Self {
            opts,
            autnums: HashMap::new(),
            all_asns: BTreeSet::new(),
            asn_routes: HashMap::new(),
            countries: FreqDict::new(),
            registries: FreqDict::new(),
            statuses: FreqDict::new(),
            sources: FreqDict::new(),
            regions: FreqDict::new(),
            cities: FreqDict::new(),
            geo_loc_map: HashMap::new(),
            geo_loc_list: vec![GeoLocKey {
                cc_id: 0,
                reg_id: 0,
                city_id: 0,
            }],
            geo_loc_freq: vec![0],
            alloc_v4: Vec::new(),
            alloc_v6: Vec::new(),
            geo_v4: Vec::new(),
            geo_v6: Vec::new(),
            route_origins_v4: HashMap::new(),
            route_origins_v6: HashMap::new(),
            seq: 0,
        }
    }

    pub fn add_autnum(&mut self, mut rec: AutnumInput) -> bool {
        if rec.as_number == 0 {
            return false;
        }
        self.all_asns.insert(rec.as_number);
        if self.autnums.contains_key(&rec.as_number) {
            return false;
        }
        rec.registry = rec.registry.trim().to_ascii_lowercase();
        rec.country = normalize_country(&rec.country).unwrap_or_default();
        rec.name = rec.name.trim().to_owned();
        rec.organization = rec.organization.trim().to_owned();
        rec.status = rec.status.trim().to_owned();
        rec.source = rec.source.trim().to_owned();
        rec.maintainers = rec.maintainers.trim().to_owned();
        rec.abuse_contact = rec.abuse_contact.trim().to_owned();
        rec.description = rec.description.trim().to_owned();
        self.registries.bump(&rec.registry);
        self.countries.bump(&rec.country);
        self.statuses.bump(&rec.status);
        self.sources.bump(&rec.source);
        self.autnums.insert(rec.as_number, rec);
        true
    }

    pub fn add_allocation(
        &mut self,
        start: IpAddr,
        end: IpAddr,
        registry: &str,
        country: &str,
    ) -> bool {
        let reg = registry.trim().to_ascii_lowercase();
        let cc = normalize_country(country).unwrap_or_default();
        if reg.is_empty() && cc.is_empty() {
            return false;
        }
        let (fam, lo, hi, ok) =
            slot_range(start, end, self.opts.alloc_v4_bits, self.opts.alloc_v6_bits);
        if !ok {
            return false;
        }
        let val = (self.registries.id(&reg) << 16) | self.countries.id(&cc);
        self.seq += 1;
        push_interval(self, fam, (lo, hi), val, true, 0);
        true
    }

    pub fn add_geofeed(
        &mut self,
        start: IpAddr,
        end: IpAddr,
        country: &str,
        region: &str,
        city: &str,
    ) -> bool {
        insert_geofeed(self, (start, end), (country, region, city), 0)
    }

    pub fn add_geofeed_fallback(
        &mut self,
        start: IpAddr,
        end: IpAddr,
        country: &str,
        region: &str,
        city: &str,
    ) -> bool {
        insert_geofeed(
            self,
            (start, end),
            (country, region, city),
            FALLBACK_PRIORITY_BIAS,
        )
    }

    pub fn add_route(&mut self, net_ip: IpAddr, bits: u8, as_number: u32) -> bool {
        if as_number == 0 {
            return false;
        }
        self.all_asns.insert(as_number);
        match unmap_ip(net_ip) {
            IpAddr::V4(v4) if bits <= 32 => {
                let v = u32::from(v4);
                push_unique(
                    self.route_origins_v4.entry((v, bits)).or_default(),
                    as_number,
                );
                push_asn_route(self, as_number, IpAddr::V4(v4), bits);
                true
            }
            IpAddr::V6(v6) if bits <= 128 => {
                let hi = (u128::from(v6) >> 64) as u64;
                push_asn_route(self, as_number, IpAddr::V6(v6), bits);
                if bits <= DEFAULT_ROUTE_V6_BITS {
                    push_unique(
                        self.route_origins_v6.entry((hi, bits)).or_default(),
                        as_number,
                    );
                }
                true
            }
            _ => false,
        }
    }

    pub fn encode(mut self, w: &mut impl Write) -> Result<LiteStats, String> {
        let dicts = CompactDicts::from_builder(&self);
        remap_allocs(&mut self.alloc_v4, &dicts);
        remap_allocs(&mut self.alloc_v6, &dicts);
        remap_geos(&mut self.geo_v4, &dicts.geoloc_remap);
        remap_geos(&mut self.geo_v6, &dicts.geoloc_remap);
        let routes = build_route_segments(&mut self, &dicts.asn_to_idx);
        let (a4, a6, g4, g6) = (
            flatten_intervals(
                std::mem::take(&mut self.alloc_v4),
                self.opts.alloc_v4_bits,
                None,
            ),
            flatten_intervals(
                std::mem::take(&mut self.alloc_v6),
                self.opts.alloc_v6_bits,
                None,
            ),
            flatten_intervals(
                std::mem::take(&mut self.geo_v4),
                self.opts.geo_v4_bits,
                None,
            ),
            flatten_intervals(
                std::mem::take(&mut self.geo_v6),
                self.opts.geo_v6_bits,
                None,
            ),
        );
        let input = EncodeInput {
            builder: &self,
            dicts: &dicts,
            routes: &routes,
            alloc_s4: &a4,
            alloc_s6: &a6,
            geo_s4: &g4,
            geo_s6: &g6,
        };
        encode_mnl1(&input, w)
    }
}
