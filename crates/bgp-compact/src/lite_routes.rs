use std::collections::HashMap;

use crate::lite_db::LiteDbBuilder;
use crate::lite_types::{DEFAULT_ROUTE_V4_BITS, DEFAULT_ROUTE_V6_BITS, GeoLocKey, RouteTables};
use crate::sweep::{Interval, flatten_intervals, same_cidr_block};

pub(crate) struct CompactDicts {
    pub(crate) asns: Vec<u32>,
    pub(crate) asn_to_idx: HashMap<u32, u32>,
    pub(crate) countries: Vec<String>,
    pub(crate) country_map: Vec<u32>,
    pub(crate) registries: Vec<String>,
    pub(crate) registry_map: Vec<u32>,
    pub(crate) statuses: Vec<String>,
    pub(crate) status_map: Vec<u32>,
    pub(crate) sources: Vec<String>,
    pub(crate) source_map: Vec<u32>,
    pub(crate) regions: Vec<String>,
    pub(crate) cities: Vec<String>,
    pub(crate) geoloc_remap: Vec<u32>,
    pub(crate) compact_geolocs: Vec<GeoLocKey>,
}

impl CompactDicts {
    pub(crate) fn from_builder(b: &LiteDbBuilder) -> Self {
        let asns: Vec<u32> = b.all_asns.iter().copied().collect();
        let mut asn_to_idx = HashMap::with_capacity(asns.len());
        for (i, &asn) in asns.iter().enumerate() {
            asn_to_idx.insert(asn, (i + 1) as u32);
        }
        let (countries, country_map) = b.countries.compact_order();
        let (registries, registry_map) = b.registries.compact_order();
        let (statuses, status_map) = b.statuses.compact_order();
        let (sources, source_map) = b.sources.compact_order();
        let (regions, region_map) = b.regions.compact_order();
        let (cities, city_map) = b.cities.compact_order();
        let (geoloc_remap, compact_geolocs) =
            compact_geolocs(b, &country_map, &region_map, &city_map);
        Self {
            asns,
            asn_to_idx,
            countries,
            country_map,
            registries,
            registry_map,
            statuses,
            status_map,
            sources,
            source_map,
            regions,
            cities,
            geoloc_remap,
            compact_geolocs,
        }
    }
}

fn compact_geolocs(
    b: &LiteDbBuilder,
    country_map: &[u32],
    region_map: &[u32],
    city_map: &[u32],
) -> (Vec<u32>, Vec<GeoLocKey>) {
    let n = b.geo_loc_list.len().saturating_sub(1);
    let mut order: Vec<u32> = (1..=n as u32).collect();
    order.sort_by(|&a, &c| {
        let fa = b.geo_loc_freq.get(a as usize).copied().unwrap_or(0);
        let fc = b.geo_loc_freq.get(c as usize).copied().unwrap_or(0);
        fc.cmp(&fa).then_with(|| a.cmp(&c))
    });
    let mut remap = vec![0u32; b.geo_loc_list.len()];
    let mut out = Vec::with_capacity(n);
    for (i, &old_id) in order.iter().enumerate() {
        if let Some(slot) = remap.get_mut(old_id as usize) {
            *slot = (i + 1) as u32;
        }
        if let Some(orig) = b.geo_loc_list.get(old_id as usize) {
            out.push(GeoLocKey {
                cc_id: country_map.get(orig.cc_id as usize).copied().unwrap_or(0),
                reg_id: region_map.get(orig.reg_id as usize).copied().unwrap_or(0),
                city_id: city_map.get(orig.city_id as usize).copied().unwrap_or(0),
                coords: orig.coords,
            });
        }
    }
    (remap, out)
}

pub(crate) fn remap_allocs(ivs: &mut [Interval], dicts: &CompactDicts) {
    for iv in ivs {
        let reg = dicts
            .registry_map
            .get((iv.val >> 16) as usize)
            .copied()
            .unwrap_or(0);
        let cc = dicts
            .country_map
            .get((iv.val & 0xffff) as usize)
            .copied()
            .unwrap_or(0);
        iv.val = (reg << 16) | cc;
    }
}

pub(crate) fn remap_geos(ivs: &mut [Interval], remap: &[u32]) {
    for iv in ivs {
        iv.val = remap.get(iv.val as usize).copied().unwrap_or(0);
    }
}

pub(crate) fn build_route_segments(
    b: &mut LiteDbBuilder,
    asn_to_idx: &HashMap<u32, u32>,
) -> RouteTables {
    let mut multi_sets: Vec<Vec<u32>> = Vec::new();
    let mut multi_map: HashMap<Vec<u32>, u32> = HashMap::new();
    let mut seq = b.seq;
    let iv4 = build_route_intervals(
        b.route_origins_v4
            .iter()
            .map(|(&(addr, bits), origs)| (u64::from(addr), bits, origs)),
        32,
        asn_to_idx,
        &mut multi_sets,
        &mut multi_map,
        &mut seq,
    );
    let iv6 = build_route_intervals(
        b.route_origins_v6
            .iter()
            .map(|(&(hi_net, bits), origs)| (hi_net, bits, origs)),
        64,
        asn_to_idx,
        &mut multi_sets,
        &mut multi_map,
        &mut seq,
    );
    b.seq = seq;
    let mut segs4 = flatten_intervals(
        iv4,
        DEFAULT_ROUTE_V4_BITS,
        Some(&same_cidr_block(DEFAULT_ROUTE_V4_BITS)),
    );
    let mut segs6 = flatten_intervals(
        iv6,
        DEFAULT_ROUTE_V6_BITS,
        Some(&same_cidr_block(DEFAULT_ROUTE_V6_BITS)),
    );
    let route_val_table = rank_route_values(&mut segs4, &mut segs6);
    RouteTables {
        segs4,
        segs6,
        multi_sets,
        route_val_table,
    }
}

fn rank_route_values(
    segs4: &mut [crate::sweep::Segment],
    segs6: &mut [crate::sweep::Segment],
) -> Vec<u32> {
    let mut freq: HashMap<u32, usize> = HashMap::new();
    for s in segs4.iter().chain(segs6.iter()).filter(|s| s.val != 0) {
        *freq.entry(s.val).or_insert(0) += 1;
    }
    let mut table: Vec<u32> = freq.keys().copied().collect();
    table.sort_by(|a, b| freq.get(b).cmp(&freq.get(a)).then_with(|| a.cmp(b)));
    let mut remap: HashMap<u32, u32> = HashMap::with_capacity(table.len());
    for (i, &orig) in table.iter().enumerate() {
        remap.insert(orig, (i + 1) as u32);
    }
    for s in segs4.iter_mut().chain(segs6.iter_mut()) {
        if let Some(&mapped) = (s.val != 0).then(|| remap.get(&s.val)).flatten() {
            s.val = mapped;
        }
    }
    table
}

fn build_route_intervals<'a, I>(
    entries: I,
    max_bits: u8,
    asn_to_idx: &HashMap<u32, u32>,
    multi_sets: &mut Vec<Vec<u32>>,
    multi_map: &mut HashMap<Vec<u32>, u32>,
    seq: &mut usize,
) -> Vec<Interval>
where
    I: ExactSizeIterator<Item = (u64, u8, &'a Vec<u32>)>,
{
    let asn_count = asn_to_idx.len() as u32;
    let max_mask = if max_bits == 32 {
        0xffff_ffff
    } else {
        u64::MAX
    };
    let mut out = Vec::with_capacity(entries.len());
    for (lo, bits, origs) in entries {
        let val = resolve_asn_set(origs, asn_to_idx, asn_count, multi_sets, multi_map);
        let shift = u32::from(max_bits - bits);
        let hi = if shift >= u32::from(max_bits) {
            max_mask
        } else {
            lo | ((1u64 << shift) - 1)
        };
        *seq += 1;
        out.push(Interval {
            start: lo,
            end: hi,
            val,
            aux: bits,
            priority: i64::from(bits),
            seq: *seq,
        });
    }
    out
}

fn resolve_asn_set(
    origs: &[u32],
    asn_to_idx: &HashMap<u32, u32>,
    asn_count: u32,
    multi_sets: &mut Vec<Vec<u32>>,
    multi_map: &mut HashMap<Vec<u32>, u32>,
) -> u32 {
    let mut sorted = origs.to_vec();
    sorted.sort_unstable();
    if sorted.len() == 1
        && let Some(&first) = sorted.first()
    {
        return asn_to_idx.get(&first).copied().unwrap_or(0);
    }
    let indices: Vec<u32> = sorted
        .iter()
        .map(|a| asn_to_idx.get(a).copied().unwrap_or(0))
        .collect();
    if let Some(&id) = multi_map.get(&indices) {
        return id;
    }
    let id = asn_count + (multi_sets.len() as u32) + 1;
    multi_map.insert(indices.clone(), id);
    multi_sets.push(indices);
    id
}
