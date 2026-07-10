//! Keeps the README browser-proof matrix aligned with executable test sources.

const README: &str = include_str!("../../README.md");

const SUITES: &[(&str, &str, usize)] = &[
    ("smoke", include_str!("smoke.rs"), 1),
    ("bootstrap", include_str!("bootstrap.rs"), 2),
    ("vfs_basic", include_str!("vfs_basic.rs"), 2),
    ("conformance", include_str!("conformance.rs"), 18),
    ("engine", include_str!("engine.rs"), 8),
    ("manifest", include_str!("manifest.rs"), 13),
    ("registry", include_str!("registry.rs"), 8),
    ("oracle", include_str!("oracle.rs"), 10),
    ("idb_spike", include_str!("idb_spike.rs"), 2),
    ("idb_store", include_str!("idb_store.rs"), 1),
    ("idb_vfs", include_str!("idb_vfs.rs"), 15),
    ("idb_crash", include_str!("idb_crash.rs"), 4),
    ("idb_receipt", include_str!("idb_receipt.rs"), 1),
    ("idb_cross_worker", include_str!("idb_cross_worker.rs"), 1),
    ("idb_cross_tab", include_str!("idb_cross_tab.rs"), 1),
];

fn documented_count(suite: &str) -> Option<usize> {
    let prefix = format!("| `{suite}` | ");
    README.lines().find_map(|line| {
        line.strip_prefix(&prefix)
            .and_then(|rest| rest.split('|').next())
            .and_then(|count| count.trim().parse().ok())
    })
}

fn attribute_count(source: &str, attribute: &str) -> usize {
    source
        .lines()
        .filter(|line| line.trim().starts_with(attribute))
        .count()
}

#[test]
fn readme_suite_counts_match_test_sources() {
    for &(suite, source, expected) in SUITES {
        assert_eq!(
            attribute_count(source, "#[wasm_bindgen_test]"),
            expected,
            "update the expected source count for {suite}"
        );
        assert_eq!(
            documented_count(suite),
            Some(expected),
            "update README's {suite} matrix entry"
        );
    }

    let receipt_cases = attribute_count(include_str!("receipt_browser.rs"), "#[wasm_bindgen_test]")
        + attribute_count(include_str!("receipt_native.rs"), "#[tokio::test");
    assert_eq!(receipt_cases, 2, "update the receipt source count");
    assert_eq!(
        documented_count("receipt"),
        Some(receipt_cases),
        "update README's receipt matrix entry"
    );
}
