use std::collections::BTreeSet;

use movespan_model::analyze;
use movespan_types::{KeyClass, Location, LocationId, LocationTable, TxnAccess, Workload};

fn location(id: u32, class: KeyClass) -> Location {
    Location {
        id: LocationId(id),
        label: format!("0xtest::m::L{id}"),
        class,
    }
}

fn access(reads: &[u32], writes: &[u32], gas: u64) -> TxnAccess {
    TxnAccess {
        reads: reads
            .iter()
            .map(|&i| LocationId(i))
            .collect::<BTreeSet<_>>(),
        writes: writes
            .iter()
            .map(|&i| LocationId(i))
            .collect::<BTreeSet<_>>(),
        gas_used: gas,
        success: true,
    }
}

#[test]
fn shared_key_fully_serializes() {
    let mut locations = LocationTable::new();
    locations.insert(location(0, KeyClass::Resource));
    let txns = (0..8).map(|_| access(&[0], &[0], 100)).collect();
    let analysis = analyze(&Workload { txns, locations }, 8);

    assert_eq!(analysis.conflict_edges, 7);
    assert!(
        analysis.estimated_speedup < 1.2,
        "{}",
        analysis.estimated_speedup
    );
    assert!(analysis.parallelizability < 0.2);
    assert_eq!(analysis.hotspots.len(), 1);
    assert_eq!(analysis.hotspots[0].location, LocationId(0));
    assert_eq!(analysis.hotspots[0].edges_caused, 7);
}

#[test]
fn disjoint_keys_fully_parallelize() {
    let mut locations = LocationTable::new();
    for i in 0..8 {
        locations.insert(location(i, KeyClass::Resource));
    }
    let txns = (0..8).map(|i| access(&[i], &[i], 100)).collect();
    let analysis = analyze(&Workload { txns, locations }, 8);

    assert_eq!(analysis.conflict_edges, 0);
    assert!(
        analysis.estimated_speedup > 6.0,
        "{}",
        analysis.estimated_speedup
    );
    assert!(analysis.parallelizability > 0.75);
    assert!(analysis.hotspots.is_empty());
}

#[test]
fn aggregator_writes_never_conflict() {
    let mut locations = LocationTable::new();
    locations.insert(location(0, KeyClass::Aggregator));
    let txns = (0..8).map(|_| access(&[0], &[0], 100)).collect();
    let analysis = analyze(&Workload { txns, locations }, 8);

    assert_eq!(analysis.conflict_edges, 0);
    assert!(analysis.parallelizability > 0.75);
    assert!(analysis.hotspots.is_empty());
}

#[test]
fn empty_workload_is_well_defined() {
    let analysis = analyze(&Workload::default(), 8);
    assert_eq!(analysis.txn_count, 0);
    assert_eq!(analysis.total_work, 0);
    assert_eq!(analysis.conflict_edges, 0);
    assert!(analysis.hotspots.is_empty());
}
