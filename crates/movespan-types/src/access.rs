use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::{LocationId, LocationTable};

/// The captured storage footprint of a single transaction.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TxnAccess {
    pub reads: BTreeSet<LocationId>,
    pub writes: BTreeSet<LocationId>,
    /// Gas consumed, used as a proxy for execution cost when scheduling.
    pub gas_used: u64,
    pub success: bool,
}

impl TxnAccess {
    /// Execution cost, floored at 1 so every transaction advances the schedule.
    pub fn cost(&self) -> u64 {
        self.gas_used.max(1)
    }
}

/// A captured workload: transactions in block order plus the location registry
/// needed to interpret their access sets.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Workload {
    pub txns: Vec<TxnAccess>,
    pub locations: LocationTable,
}
