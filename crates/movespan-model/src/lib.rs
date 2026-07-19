//! The Block-STM contention model.
//!
//! Aptos executes a block optimistically in parallel and re-executes a
//! transaction whenever it read a location an earlier transaction wrote. That
//! read-after-write (RAW) relationship is the dominant serialization cost, so
//! Movespan models a workload as the RAW dependency DAG over captured access
//! sets and estimates the parallel makespan with a greedy list scheduler.
//!
//! Write-after-write and write-after-read pairs are intentionally ignored:
//! Block-STM's multi-versioning resolves those at commit without serializing
//! execution. Aggregator and code locations are excluded because their updates
//! commute or occur only at publish time.
//!
//! The numbers are estimates of this model, not validator-exact throughput.
//! They are designed to be accurate for *ranking* hotspots and for the
//! *direction and magnitude* of a fix, which is what drives the advice.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};

use serde::Serialize;

use movespan_types::{KeyClass, LocationId, Workload};

/// A single storage location ranked by the contention it causes.
#[derive(Debug, Clone, Serialize)]
pub struct Hotspot {
    pub location: LocationId,
    pub label: String,
    pub class: KeyClass,
    /// Number of read-after-write dependencies this location induced.
    pub edges_caused: usize,
    /// Total cost of the transactions that had to wait on this location.
    pub blocked_cost: u64,
}

/// The result of analyzing a workload at a given thread count.
#[derive(Debug, Clone, Serialize)]
pub struct Analysis {
    pub threads: usize,
    pub txn_count: usize,
    /// Sum of all transaction costs: the time a single thread would take.
    pub total_work: u64,
    /// Critical-path length: the unavoidable serial span at infinite threads.
    pub span: u64,
    /// Makespan estimate from greedy list scheduling at `threads`.
    pub estimated_time: u64,
    pub estimated_speedup: f64,
    /// Speedup as a fraction of the ideal `threads`x (headline metric, 0..1).
    pub parallelizability: f64,
    pub conflict_edges: usize,
    pub hotspots: Vec<Hotspot>,
}

/// Build the RAW dependency DAG for `workload` and estimate how well it
/// parallelizes across `threads` Block-STM workers.
pub fn analyze(workload: &Workload, threads: usize) -> Analysis {
    let threads = threads.max(1);
    let txns = &workload.txns;
    let locs = &workload.locations;

    let mut last_writer: HashMap<LocationId, usize> = HashMap::new();
    let mut edges: HashMap<LocationId, (usize, u64)> = HashMap::new();
    let mut conflict_edges = 0usize;

    // Earliest finish of each txn at infinite threads (for the critical path).
    let mut span_finish = vec![0u64; txns.len()];
    // Finish of each txn under greedy P-thread list scheduling.
    let mut sched_finish = vec![0u64; txns.len()];
    // Free-at times of the P workers; the earliest-free worker takes the next txn.
    let mut workers: BinaryHeap<Reverse<u64>> = BinaryHeap::from(vec![Reverse(0u64); threads]);

    let mut total_work = 0u64;
    let mut span = 0u64;
    let mut makespan = 0u64;

    for (j, txn) in txns.iter().enumerate() {
        let cost = txn.cost();
        total_work += cost;

        let mut dep_span = 0u64;
        let mut dep_sched = 0u64;
        for &r in &txn.reads {
            if !locs.is_conflicting(r) {
                continue;
            }
            if let Some(&i) = last_writer.get(&r) {
                if i != j {
                    conflict_edges += 1;
                    let entry = edges.entry(r).or_insert((0, 0));
                    entry.0 += 1;
                    entry.1 += cost;
                    dep_span = dep_span.max(span_finish[i]);
                    dep_sched = dep_sched.max(sched_finish[i]);
                }
            }
        }

        span_finish[j] = dep_span + cost;
        span = span.max(span_finish[j]);

        let Reverse(free) = workers.pop().expect("threads >= 1");
        let finish = dep_sched.max(free) + cost;
        sched_finish[j] = finish;
        makespan = makespan.max(finish);
        workers.push(Reverse(finish));

        for &w in &txn.writes {
            if locs.is_conflicting(w) {
                last_writer.insert(w, j);
            }
        }
    }

    let estimated_time = makespan.max(1);
    let estimated_speedup = total_work as f64 / estimated_time as f64;
    let parallelizability = (estimated_speedup / threads as f64).clamp(0.0, 1.0);

    let mut hotspots: Vec<Hotspot> = edges
        .into_iter()
        .map(|(id, (edges_caused, blocked_cost))| Hotspot {
            location: id,
            label: locs.label(id).to_string(),
            class: locs.class(id),
            edges_caused,
            blocked_cost,
        })
        .collect();
    hotspots.sort_by(|a, b| {
        b.edges_caused
            .cmp(&a.edges_caused)
            .then(b.blocked_cost.cmp(&a.blocked_cost))
            .then(a.label.cmp(&b.label))
    });

    Analysis {
        threads,
        txn_count: txns.len(),
        total_work,
        span,
        estimated_time,
        estimated_speedup,
        parallelizability,
        conflict_edges,
        hotspots,
    }
}
