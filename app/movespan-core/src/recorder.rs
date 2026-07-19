//! A state view wrapper that records every key the VM reads.

use std::collections::HashSet;
use std::sync::Mutex;

use aptos_types::state_store::state_key::StateKey;
use aptos_types::state_store::state_slot::StateSlot;
use aptos_types::state_store::state_storage_usage::StateStorageUsage;
use aptos_types::state_store::state_value::StateValue;
use aptos_types::state_store::{StateViewResult, TStateView};

/// Wraps a base state view and records the keys read through it.
///
/// The VM reads through either [`TStateView::get_state_value`] or the
/// lower-level [`TStateView::get_state_slot`], so both are intercepted. Reads
/// are buffered behind a `Mutex` because the executor requires the view to be
/// `Sync` and reads flow through a shared reference.
pub struct RecordingStateView<'a, S> {
    base: &'a S,
    reads: Mutex<HashSet<StateKey>>,
}

impl<'a, S> RecordingStateView<'a, S>
where
    S: TStateView<Key = StateKey> + Sync,
{
    pub fn new(base: &'a S) -> Self {
        Self {
            base,
            reads: Mutex::new(HashSet::new()),
        }
    }

    /// Drain and return the keys read since construction or the last drain.
    pub fn take_reads(&self) -> HashSet<StateKey> {
        std::mem::take(&mut self.reads.lock().expect("reads mutex poisoned"))
    }

    fn record(&self, key: &StateKey) {
        self.reads
            .lock()
            .expect("reads mutex poisoned")
            .insert(key.clone());
    }
}

impl<S> TStateView for RecordingStateView<'_, S>
where
    S: TStateView<Key = StateKey> + Sync,
{
    type Key = StateKey;

    fn get_state_value(&self, state_key: &StateKey) -> StateViewResult<Option<StateValue>> {
        self.record(state_key);
        self.base.get_state_value(state_key)
    }

    fn get_state_slot(&self, state_key: &StateKey) -> StateViewResult<StateSlot> {
        self.record(state_key);
        self.base.get_state_slot(state_key)
    }

    fn get_usage(&self) -> StateViewResult<StateStorageUsage> {
        self.base.get_usage()
    }
}
