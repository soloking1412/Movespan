//! Classification of VM [`StateKey`]s and interning them to compact
//! [`LocationId`]s that the pure model operates on.

use std::collections::HashMap;

use aptos_types::access_path::Path;
use aptos_types::state_store::state_key::inner::StateKeyInner;
use aptos_types::state_store::state_key::StateKey;
use move_core_types::language_storage::StructTag;

use movespan_types::{KeyClass, Location, LocationId, LocationTable};

/// Assigns stable [`LocationId`]s to storage keys as they are observed and
/// records their human-readable label and class in a [`LocationTable`].
#[derive(Default)]
pub struct LocationInterner {
    ids: HashMap<StateKey, LocationId>,
    table: LocationTable,
    next: u32,
}

impl LocationInterner {
    pub fn new() -> Self {
        Self::default()
    }

    /// Return the id for `key`, assigning and classifying a fresh one on first
    /// sighting.
    pub fn intern(&mut self, key: &StateKey) -> LocationId {
        if let Some(&id) = self.ids.get(key) {
            return id;
        }
        let id = LocationId(self.next);
        self.next += 1;
        let (label, class) = classify(key);
        self.ids.insert(key.clone(), id);
        self.table.insert(Location { id, label, class });
        id
    }

    pub fn table(&self) -> &LocationTable {
        &self.table
    }

    pub fn into_table(self) -> LocationTable {
        self.table
    }
}

/// Derive a report label and contention class from a state key.
pub fn classify(key: &StateKey) -> (String, KeyClass) {
    match key.inner() {
        StateKeyInner::AccessPath(access_path) => {
            let address = access_path.address.to_hex_literal();
            match access_path.get_path() {
                Path::Code(module_id) => (
                    format!("{address}::{} (code)", module_id.name()),
                    KeyClass::Code,
                ),
                Path::Resource(tag) | Path::ResourceGroup(tag) => {
                    let label = format!("{address}::{}::{}", tag.module, tag.name);
                    let class = if is_aggregator(&tag) {
                        KeyClass::Aggregator
                    } else if is_framework(&tag) {
                        KeyClass::Framework
                    } else {
                        KeyClass::Resource
                    };
                    (label, class)
                }
            }
        }
        StateKeyInner::TableItem { handle, .. } => (
            format!("table[{}]", handle.0.to_hex_literal()),
            KeyClass::TableItem,
        ),
        StateKeyInner::Raw(_) => ("raw".to_string(), KeyClass::Unknown),
        StateKeyInner::TradingNative(_) => ("trading-native".to_string(), KeyClass::Unknown),
    }
}

/// Types defined at a reserved framework address (0x1, 0x3, 0x4, 0xa, ...) are
/// transaction infrastructure: gas payment, sequence numbers, fungible stores.
/// Every transaction writes them, so they would otherwise dominate the hotspot
/// ranking and mask the contract's own contention — which is the only thing the
/// author can actually refactor.
fn is_framework(tag: &StructTag) -> bool {
    let bytes = tag.address.into_bytes();
    let (leading, last) = bytes.split_at(bytes.len() - 1);
    leading.iter().all(|byte| *byte == 0) && last[0] <= 0x0f
}

/// Recognize aggregator / delayed-field types by name. Their updates commute,
/// so Block-STM does not serialize on them.
fn is_aggregator(tag: &StructTag) -> bool {
    let module = tag.module.as_str();
    let name = tag.name.as_str();
    module.contains("aggregator") || name.contains("Aggregator") || name.contains("DelayedField")
}
