use bgp_api_server::config::{GIBIBYTE, resolve_cache_plan};

const MIB: u64 = 1 << 20;

#[test]
fn resolves_auto_and_explicit_cache_plans() -> Result<(), String> {
    let database = 2 * GIBIBYTE + 300 * MIB;
    let selectors = 571 * MIB;

    let minimal = resolve_cache_plan("auto", GIBIBYTE, database, selectors)?;
    assert_eq!(minimal.effective, "minimal");
    assert!(!minimal.preload_selectors && !minimal.warm_dataset_page_cache);

    let balanced = resolve_cache_plan("auto", 2 * GIBIBYTE, database, selectors)?;
    assert_eq!(balanced.effective, "balanced");
    assert!(balanced.preload_selectors && !balanced.warm_dataset_page_cache);

    let full_4g = resolve_cache_plan("auto", 4 * GIBIBYTE, database, selectors)?;
    assert_eq!(full_4g.effective, "full");
    assert!(!full_4g.preload_selectors && full_4g.warm_dataset_page_cache);

    let forced_min = resolve_cache_plan("minimal", 8 * GIBIBYTE, database, selectors)?;
    assert_eq!(forced_min.effective, "minimal");

    assert!(resolve_cache_plan("full", 2 * GIBIBYTE, database, selectors).is_err());
    assert!(resolve_cache_plan("bogus", 8 * GIBIBYTE, database, selectors).is_err());
    Ok(())
}
