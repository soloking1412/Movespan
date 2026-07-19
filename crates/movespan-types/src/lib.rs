//! Shared vocabulary for Movespan: interned storage locations and the captured
//! access footprint of a workload. These types carry no dependency on the Aptos
//! VM, so the conflict model, rules, and reporting layers build and test in
//! isolation from the heavy VM crates.

mod access;
mod location;

pub use access::{TxnAccess, Workload};
pub use location::{KeyClass, Location, LocationId, LocationTable};
