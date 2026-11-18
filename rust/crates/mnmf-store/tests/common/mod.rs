use mnmf_store::types::{
    FORMAT_VERSION, HEADER_FIXED_SIZE, HEADER_SIZE, MAGIC, SCHEMA_VERSION, SECTION_COUNT,
    SECTION_ENTRY_SIZE, Section,
};

pub fn append_uvarint(buf: &mut Vec<u8>, mut val: u64) {
    while val >= 0x80 {
        buf.push((val as u8) | 0x80);
        val >>= 7;
    }
    buf.push(val as u8);
}

pub fn append_inline_str(buf: &mut Vec<u8>, s: &str) {
    append_uvarint(buf, s.len() as u64);
    buf.extend_from_slice(s.as_bytes());
}

pub fn empty_selection_lpm(version: u8) -> Vec<u8> {
    let node_size = if version == 4 { 21 } else { 45 };
    let mut out = vec![0u8; 16 + node_size];
    if let Some(m) = out.get_mut(0..4) {
        m.copy_from_slice(b"SLP2");
    }
    if let Some(v) = out.get_mut(4) {
        *v = version;
    }
    if let Some(n) = out.get_mut(8..12) {
        n.copy_from_slice(&1u32.to_be_bytes());
    }
    out
}

pub fn empty_route_lpm(version: u8) -> Vec<u8> {
    let mut out = vec![0u8; 16 + 33];
    if let Some(m) = out.get_mut(0..4) {
        m.copy_from_slice(b"MLP2");
    }
    if let Some(v) = out.get_mut(4) {
        *v = version;
    }
    if let Some(n) = out.get_mut(8..12) {
        n.copy_from_slice(&1u32.to_be_bytes());
    }
    out
}

pub fn assemble_mnmf(sections: &[(Section, Vec<u8>)]) -> Vec<u8> {
    let mut table: [Vec<u8>; SECTION_COUNT] = std::array::from_fn(|_| Vec::new());
    for (sec, data) in sections {
        if let Some(slot) = table.get_mut(*sec as usize) {
            *slot = data.clone();
        }
    }
    fill_default_sections(&mut table);

    let mut out = vec![0u8; HEADER_SIZE];
    let mut specs = [(0u64, 0u64); SECTION_COUNT];
    for (idx, data) in table.iter().enumerate() {
        while !out.len().is_multiple_of(8) {
            out.push(0);
        }
        let off = out.len() as u64;
        out.extend_from_slice(data);
        if let Some(s) = specs.get_mut(idx) {
            *s = (off, data.len() as u64);
        }
    }
    write_header(&mut out, &specs);
    out
}

fn fill_default_sections(table: &mut [Vec<u8>; SECTION_COUNT]) {
    let meta = format!(r#"{{"schema_version":{SCHEMA_VERSION},"release_tag":"db-test"}}"#);
    set_if_empty(table, Section::Metadata, meta.into_bytes());
    for s in [
        Section::StringPoolOffsets,
        Section::AllocationOffsets,
        Section::RouteOffsets,
        Section::GeofeedOffsets,
    ] {
        set_if_empty(table, s, 0u64.to_be_bytes().to_vec());
    }
    set_if_empty(table, Section::AllocLpmV4, empty_selection_lpm(4));
    set_if_empty(table, Section::AllocLpmV6, empty_selection_lpm(6));
    set_if_empty(table, Section::GeoLpmV4, empty_selection_lpm(4));
    set_if_empty(table, Section::GeoLpmV6, empty_selection_lpm(6));
    set_if_empty(table, Section::RouteLpmV4, empty_route_lpm(4));
    set_if_empty(table, Section::RouteLpmV6, empty_route_lpm(6));
}

fn set_if_empty(table: &mut [Vec<u8>; SECTION_COUNT], sec: Section, default: Vec<u8>) {
    if let Some(slot) = table.get_mut(sec as usize)
        && slot.is_empty()
    {
        *slot = default;
    }
}

fn write_header(out: &mut [u8], specs: &[(u64, u64); SECTION_COUNT]) {
    if let Some(m) = out.get_mut(0..4) {
        m.copy_from_slice(&MAGIC);
    }
    if let Some(v) = out.get_mut(4..8) {
        v.copy_from_slice(&FORMAT_VERSION.to_be_bytes());
    }
    if let Some(c) = out.get_mut(8..12) {
        c.copy_from_slice(&(SECTION_COUNT as u32).to_be_bytes());
    }
    for (idx, (off, sz)) in specs.iter().enumerate() {
        let pos = HEADER_FIXED_SIZE + idx * SECTION_ENTRY_SIZE;
        if let Some(dst) = out.get_mut(pos..pos + 8) {
            dst.copy_from_slice(&off.to_be_bytes());
        }
        if let Some(dst) = out.get_mut(pos + 8..pos + 16) {
            dst.copy_from_slice(&sz.to_be_bytes());
        }
    }
}
