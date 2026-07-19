use std::collections::BTreeSet;

use movespan_model::analyze;
use movespan_report::{to_html, to_json, to_text};
use movespan_rules::suggest;
use movespan_types::{KeyClass, Location, LocationId, LocationTable, TxnAccess, Workload};

fn workload() -> Workload {
    let mut locations = LocationTable::new();
    locations.insert(Location {
        id: LocationId(0),
        label: "0x1::coin::Supply".into(),
        class: KeyClass::Resource,
    });
    let txns = (0..8).map(|_| TxnAccess {
        reads: BTreeSet::from([LocationId(0)]),
        writes: BTreeSet::from([LocationId(0)]),
        gas_used: 100,
        success: true,
    });
    Workload {
        txns: txns.collect(),
        locations,
    }
}

#[test]
fn renders_all_formats() {
    let workload = workload();
    let analysis = analyze(&workload, 8);
    let suggestions = suggest(&workload, &analysis, 5);

    let text = to_text(&analysis, &suggestions);
    assert!(text.contains("MOVESPAN CONTENTION REPORT"));
    assert!(text.contains("0x1::coin::Supply"));

    let json = to_json(&analysis, &suggestions);
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed["analysis"]["txn_count"], 8);
    assert!(parsed["suggestions"].is_array());

    let html = to_html(&analysis, &suggestions);
    assert!(html.starts_with("<!doctype html>"));
    assert!(html.contains("Movespan Contention Report"));
}
