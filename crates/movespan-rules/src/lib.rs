//! Turns ranked hotspots into concrete, Aptos-specific refactor advice and
//! quantifies each suggestion by re-running the model with that location
//! neutralized — the "simulate the fix" step that yields a before/after speedup.

use serde::Serialize;

use movespan_model::{analyze, Analysis, Hotspot};
use movespan_types::{KeyClass, LocationId, Workload};

/// A refactor recommendation for one hotspot, with its projected impact.
#[derive(Debug, Clone, Serialize)]
pub struct Suggestion {
    pub location: LocationId,
    pub target: String,
    pub problem: String,
    pub fix: String,
    pub projected_speedup: f64,
    pub projected_parallelizability: f64,
    pub speedup_gain: f64,
}

/// Minimum relative speedup a simulated fix must deliver to be worth
/// recommending. Without it the report fills with no-op advice about locations
/// that are ranked but not actually limiting throughput.
const MIN_RELATIVE_GAIN: f64 = 1.01;

/// Produce ranked suggestions for the worst hotspots, keeping only those whose
/// simulated fix measurably improves throughput. Each suggestion's projected
/// metrics come from re-analyzing the workload with the hotspot's location
/// removed from every access set, modelling a fix that makes it non-conflicting
/// (for example, converting a counter to an aggregator).
pub fn suggest(workload: &Workload, analysis: &Analysis, top_n: usize) -> Vec<Suggestion> {
    analysis
        .hotspots
        .iter()
        .take(top_n * 4)
        .filter_map(|hotspot| {
            let after = analyze(&neutralize(workload, hotspot.location), analysis.threads);
            if after.estimated_speedup < analysis.estimated_speedup * MIN_RELATIVE_GAIN {
                return None;
            }
            let (problem, fix) = advise(hotspot);
            Some(Suggestion {
                location: hotspot.location,
                target: hotspot.label.clone(),
                problem,
                fix,
                projected_speedup: after.estimated_speedup,
                projected_parallelizability: after.parallelizability,
                speedup_gain: after.estimated_speedup - analysis.estimated_speedup,
            })
        })
        .take(top_n)
        .collect()
}

fn advise(hotspot: &Hotspot) -> (String, String) {
    match hotspot.class {
        KeyClass::Resource if is_counter(&hotspot.label) => (
            format!(
                "{} is a single value updated by nearly every transaction, forcing them to serialize.",
                hotspot.label
            ),
            "Replace the counter field with aptos_framework::aggregator_v2::Aggregator so its \
             updates commute and Block-STM stops treating them as conflicts. This is how the \
             Aptos framework keeps total coin supply parallel."
                .into(),
        ),
        KeyClass::Resource => (
            format!("{} is a shared resource many transactions mutate together.", hotspot.label),
            "Split independent fields into separate resources, or shard the resource per user \
             address so unrelated writers stop colliding on one object."
                .into(),
        ),
        KeyClass::TableItem if is_counter(&hotspot.label) => (
            format!("{} is a hot table entry acting as a shared counter.", hotspot.label),
            "Move the counter into an Aggregator (see AIP-43 for collection supply) so concurrent \
             updates commute instead of serializing."
                .into(),
        ),
        KeyClass::TableItem => (
            format!("{} is a hot table entry many transactions update.", hotspot.label),
            "Partition the key space so concurrent writers land on distinct entries rather than \
             one shared item."
                .into(),
        ),
        _ => (
            format!("{} introduces ordering dependencies between transactions.", hotspot.label),
            "Reduce shared writes to this location or defer them through an aggregator.".into(),
        ),
    }
}

fn is_counter(label: &str) -> bool {
    let label = label.to_lowercase();
    [
        "counter", "count", "supply", "total", "nonce", "index", "seq", "size",
    ]
    .iter()
    .any(|needle| label.contains(needle))
}

fn neutralize(workload: &Workload, target: LocationId) -> Workload {
    let txns = workload
        .txns
        .iter()
        .map(|txn| {
            let mut txn = txn.clone();
            txn.reads.remove(&target);
            txn.writes.remove(&target);
            txn
        })
        .collect();
    Workload {
        txns,
        locations: workload.locations.clone(),
    }
}
