use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Stable identifier for a single on-chain storage location (a resource, table
/// item, or module) observed during capture. Interning storage keys to a small
/// integer keeps the conflict model free of VM types and cheap to operate on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct LocationId(pub u32);

/// How a location behaves under Block-STM conflict detection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyClass {
    /// A resource stored under an account address.
    Resource,
    /// Published module bytecode. Written only on publish, never a hot conflict.
    Code,
    /// An entry in a Move table (a dynamic collection).
    TableItem,
    /// An aggregator / delayed field. Concurrent updates commute, so Block-STM
    /// does not treat them as conflicts.
    Aggregator,
    /// State whose type is defined by the Aptos framework: gas payment,
    /// sequence numbers, fungible stores. Every transaction touches it, the VM
    /// already special-cases it, and a contract author cannot refactor it, so
    /// it is excluded from contract contention.
    Framework,
    /// A location that could not be classified; treated conservatively as
    /// conflicting.
    Unknown,
}

impl KeyClass {
    /// Whether a write to this location can force a concurrent reader to
    /// re-execute. Aggregators commute and code is publish-only, so neither
    /// contributes to runtime contention.
    pub fn is_conflicting(self) -> bool {
        !matches!(
            self,
            KeyClass::Aggregator | KeyClass::Code | KeyClass::Framework
        )
    }
}

/// A location paired with the human-readable label used in reports.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Location {
    pub id: LocationId,
    pub label: String,
    pub class: KeyClass,
}

/// Registry mapping every observed [`LocationId`] to its metadata.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LocationTable {
    locations: HashMap<LocationId, Location>,
}

impl LocationTable {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, location: Location) {
        self.locations.insert(location.id, location);
    }

    pub fn get(&self, id: LocationId) -> Option<&Location> {
        self.locations.get(&id)
    }

    pub fn label(&self, id: LocationId) -> &str {
        self.locations
            .get(&id)
            .map(|l| l.label.as_str())
            .unwrap_or("<unknown>")
    }

    pub fn class(&self, id: LocationId) -> KeyClass {
        self.locations
            .get(&id)
            .map(|l| l.class)
            .unwrap_or(KeyClass::Unknown)
    }

    pub fn is_conflicting(&self, id: LocationId) -> bool {
        self.class(id).is_conflicting()
    }

    pub fn len(&self) -> usize {
        self.locations.len()
    }

    pub fn is_empty(&self) -> bool {
        self.locations.is_empty()
    }
}
