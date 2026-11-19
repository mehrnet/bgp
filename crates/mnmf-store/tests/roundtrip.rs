mod common;

use std::net::{IpAddr, Ipv4Addr};

use common::{append_inline_str, append_uvarint, assemble_mnmf};
use mnmf_store::Db;
use mnmf_store::types::{Section, TableKind};

fn build_fixture() -> Result<Db, Box<dyn std::error::Error>> {
    let mut pool_data = Vec::new();
    let mut pool_offs = vec![0u64];
    for s in ["apnic", "AU", "AS13335"] {
        pool_data.extend_from_slice(s.as_bytes());
        pool_offs.push(pool_data.len() as u64);
    }
    let pool_offs_bytes: Vec<u8> = pool_offs.iter().flat_map(|v| v.to_be_bytes()).collect();

    let mut alloc_rec = vec![4u8];
    alloc_rec.extend_from_slice(&[1, 1, 1, 0]);
    alloc_rec.extend_from_slice(&[1, 1, 1, 255]);
    append_uvarint(&mut alloc_rec, 2); // apnic
    append_uvarint(&mut alloc_rec, 3); // AU
    append_inline_str(&mut alloc_rec, "APNIC-LABS");
    for _ in 0..6 {
        append_uvarint(&mut alloc_rec, 0);
    }
    for _ in 0..3 {
        append_inline_str(&mut alloc_rec, "");
    }
    let alloc_offs = [0u64, alloc_rec.len() as u64]
        .iter()
        .flat_map(|v| v.to_be_bytes())
        .collect();

    let mut alloc_pfx = vec![1, 1, 1, 0, 24];
    alloc_pfx.extend_from_slice(&1u32.to_be_bytes());

    let mut summary_payload = vec![16u8];
    append_uvarint(&mut summary_payload, 5);
    append_uvarint(&mut summary_payload, 2);
    append_uvarint(&mut summary_payload, 0);
    append_uvarint(&mut summary_payload, 0);

    let mut summary_idx = vec![1, 1, 0, 0, 16];
    summary_idx.extend_from_slice(&0u64.to_be_bytes());
    summary_idx.extend_from_slice(&(summary_payload.len() as u32).to_be_bytes());

    let bytes = assemble_mnmf(&[
        (Section::StringPoolOffsets, pool_offs_bytes),
        (Section::StringPoolData, pool_data),
        (Section::AllocationOffsets, alloc_offs),
        (Section::AllocationData, alloc_rec),
        (Section::AllocPrefixV4, alloc_pfx),
        (Section::SummaryIndex, summary_idx),
        (Section::SummaryData, summary_payload),
    ]);
    Ok(Db::from_bytes(bytes)?)
}

#[test]
fn reads_records_prefixes_summaries_and_rejects_truncation()
-> Result<(), Box<dyn std::error::Error>> {
    let db = build_fixture()?;
    assert_eq!(db.metadata().release_tag, "db-test");

    let alloc = db.allocation(1, false)?;
    assert_eq!(alloc.registry, "apnic");
    assert_eq!(alloc.country, "AU");
    assert_eq!(alloc.name, "APNIC-LABS");

    let lookup_alloc = db.allocation(1, true)?;
    assert_eq!(lookup_alloc.name, "APNIC-LABS");

    let ids = db.matching_prefix_ids(
        TableKind::Allocations,
        IpAddr::V4(Ipv4Addr::new(1, 1, 1, 9)),
        0,
    )?;
    assert_eq!(ids, vec![1]);

    let summary = db
        .summary(Ipv4Addr::new(1, 1, 0, 0), 16)?
        .ok_or("missing summary")?;
    assert_eq!(summary.allocation_records, 5);
    assert_eq!(summary.routes, 2);

    assert!(Db::from_bytes(vec![0u8; 32]).is_err());
    Ok(())
}

#[test]
fn decodes_legacy_non_utf8_whois_bytes_lossily() -> Result<(), Box<dyn std::error::Error>> {
    let mut pool_data = Vec::new();
    let mut pool_offs = vec![0u64];
    pool_data.extend_from_slice(b"Org\xdf");
    pool_offs.push(pool_data.len() as u64);
    let pool_offs_bytes: Vec<u8> = pool_offs.iter().flat_map(|v| v.to_be_bytes()).collect();

    let mut alloc_rec = vec![4u8, 1, 1, 1, 0, 1, 1, 1, 255];
    for _ in 0..2 {
        append_uvarint(&mut alloc_rec, 0);
    }
    append_inline_str(&mut alloc_rec, "");
    for _ in 0..5 {
        append_uvarint(&mut alloc_rec, 0);
    }
    append_uvarint(&mut alloc_rec, 2); // pooled non-UTF8 Organization
    for _ in 0..2 {
        append_inline_str(&mut alloc_rec, "");
    }
    let desc = b"Stra\xdfe";
    append_uvarint(&mut alloc_rec, desc.len() as u64);
    alloc_rec.extend_from_slice(desc);
    let alloc_offs = [0u64, alloc_rec.len() as u64]
        .iter()
        .flat_map(|v| v.to_be_bytes())
        .collect();

    let db = Db::from_bytes(assemble_mnmf(&[
        (Section::StringPoolOffsets, pool_offs_bytes),
        (Section::StringPoolData, pool_data),
        (Section::AllocationOffsets, alloc_offs),
        (Section::AllocationData, alloc_rec),
    ]))?;
    let rec = db.allocation(1, false)?;
    assert_eq!(rec.organization, "Org\u{FFFD}");
    assert_eq!(rec.description, "Stra\u{FFFD}e");
    assert!(matches!(
        db.allocation_ref(1, false)?.description,
        std::borrow::Cow::Owned(_)
    ));
    Ok(())
}

#[test]
fn bounded_prefix_search_finds_all_nested_v4_and_v6_matches()
-> Result<(), Box<dyn std::error::Error>> {
    use mnmf_store::types::TableKind;
    let v4 = [
        ([10, 0, 0, 0], 8u8, 1u32),
        ([10, 20, 0, 0], 16, 2),
        ([10, 20, 30, 0], 24, 3),
        ([10, 20, 30, 40], 32, 4),
        ([10, 20, 31, 0], 24, 99),
    ];
    let mut p4 = Vec::with_capacity(v4.len() * 9);
    for (ip, len, id) in v4 {
        p4.extend_from_slice(&ip);
        p4.push(len);
        p4.extend_from_slice(&id.to_be_bytes());
    }
    let db = Db::from_bytes(assemble_mnmf(&[(Section::AllocPrefixV4, p4)]))?;
    let ids = db.matching_prefix_ids(TableKind::Allocations, "10.20.30.40".parse()?, 0)?;
    assert_eq!(ids, vec![4, 3, 2, 1]);
    let limited = db.matching_prefix_ids(TableKind::Allocations, "10.20.30.40".parse()?, 2)?;
    assert_eq!(limited, vec![4, 3]);
    Ok(())
}
