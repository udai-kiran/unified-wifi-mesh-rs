// SPDX-License-Identifier: Apache-2.0

//! Native records of the stored data model (0007 D1, D3), grouped by topic.
//!
//! Every record is declared with `record!`, which generates the struct and, under `cfg(test)`,
//! the field table that the golden test compares with the inventory.

pub mod bss;
pub mod device;
pub mod mld;
pub mod network;
pub mod policy;
pub mod radio;
pub mod radio_cap;
pub mod scan;
pub mod sta;
pub mod unassoc;

pub use bss::*;
pub use device::*;
pub use mld::*;
pub use network::*;
pub use policy::*;
pub use radio::*;
pub use radio_cap::*;
pub use scan::*;
pub use sta::*;
pub use unassoc::*;
