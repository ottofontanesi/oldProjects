// Intent citation: .kiro/specs/mesh-network-optimizer/design.md
// Mesh module — contains all Phase 9B mesh network optimizer infrastructure

pub mod identity;
pub mod membership;
pub mod trust;
pub mod classifier;
pub mod accounting;
pub mod reputation;
pub mod incentive;
pub mod solver;
pub mod consensus;
pub mod rate_limiter;
pub mod transfer;
pub mod leader;
pub mod observability;

#[cfg(test)]
mod integration_tests;
