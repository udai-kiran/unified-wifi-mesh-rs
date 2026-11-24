// SPDX-License-Identifier: Apache-2.0

//! The models (note 0007 D6): node, network and agent models, role-agnostic access, typed keys,
//! the typed dirty set and `ModelDelta`.
//!
//! The controller-side `MeshModel` (the node and network maps, the list layer, side maps and the
//! network lifecycle) is in [`mesh`], built on these types.

pub mod access;
pub mod agent;
mod ctx;
pub mod delta;
pub mod dirty;
pub mod e4;
pub mod error;
pub mod keys;
pub mod mesh;
pub mod network;
pub mod node;

#[cfg(test)]
mod tests;
