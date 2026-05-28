// Intent citation: .kiro/specs/local-network-optimizer/design.md
// Network module — contains all Phase 9A+ network optimizer infrastructure

pub mod simulator;
pub mod discovery;
pub mod registry;
pub mod phone;
pub mod catalog;
pub mod demand;
pub mod solver_agents;
pub mod solver_contention;
pub mod solver;
pub mod executor;
pub mod download;
pub mod kv_cache;
pub mod preferences;
pub mod incentive;
pub mod observability;
pub mod lifecycle;
pub mod resilience;

#[cfg(test)]
mod integration_tests;
pub mod satisfaction;
pub mod catalog_store;
