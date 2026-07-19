use std::collections::BTreeSet;

use movespan_model::analyze;
use movespan_rules::suggest;
use movespan_types::{KeyClass, Location, LocationId, LocationTable, TxnAccess, Workload};

/// 8 users, each touching their own balance plus one shared supply counter.
/// The counter is the sole contention source; sharding away the balances leaves
/// it as the single hotspot.
fn shared_counter_workload() -> Workload {
    let mut locations = LocationTable::new();
    locations.insert(Location {
        id: LocationId(0),
        label: "0x1::coin::Supply".into(),
        class: KeyClass::Resource,
    });
    for i in 1..=8 {
        locations.insert(Location {
            id: LocationId(i),
            label: format!("0x1::coin::Balance{i}"),
            class: KeyClass::Resource,
        });
    }
    let txns = (1..=8u32)
        .map(|i| TxnAccess {
            reads: BTreeSet::from([LocationId(0), LocationId(i)]),
            writes: BTreeSet::from([LocationId(0), LocationId(i)]),
            gas_used: 100,
            success: true,
        })
        .collect();
    Workload { txns, locations }
}

#[test]
fn recommends_aggregator_for_counter() {
    let workload = shared_counter_workload();
    let analysis = analyze(&workload, 8);
    let suggestions = suggest(&workload, &analysis, 5);

    assert!(!suggestions.is_empty());
    let top = &suggestions[0];
    assert_eq!(top.target, "0x1::coin::Supply");
    assert!(top.fix.contains("aggregator_v2"));
    assert!(top.speedup_gain > 0.0, "gain {}", top.speedup_gain);
    assert!(top.projected_speedup > analysis.estimated_speedup);
}
