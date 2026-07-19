//! Capture layer: replays Move transactions through the real Aptos VM and
//! records the storage read/write set of each one.
//!
//! Two sources are supported:
//! - [`Sandbox`] (Mode B) publishes a compiled package into an in-memory chain
//!   and runs a synthetic workload, so contention can be measured with no real
//!   users.
//! - [`replay`] (Mode A) forks live network state and re-executes historical
//!   transactions to produce numbers from real traffic.
//!
//! Both funnel through [`RecordingStateView`], which is the single point where
//! every VM read is observed.

pub mod recorder;
pub mod replay;
pub mod sandbox;
pub mod state_key;
pub mod workload;

pub use recorder::RecordingStateView;
pub use replay::{replay, Network, ReplayConfig};
pub use sandbox::Sandbox;
pub use state_key::{classify, LocationInterner};
pub use workload::{plan_calls, plan_init, ArgSpec, CallSpec, PlannedTxn, WorkloadSpec};
