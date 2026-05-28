// IPC Module — Tauri command bindings for the React frontend
//
// Provides 18 async command handlers organized by domain:
// - agents: workflow start/stop/status/list
// - network: placement plan, history, optimizer
// - health: node health, topology
// - transport: adapter status, paths, failover
// - companion: phone status, assignments, pairing
//
// Event emitter infrastructure for dashboard data polling:
// - emitter: EventEmitterService with periodic tasks
// - payloads: event payload structs
// - delta: node status delta computation
// - trend: utility trend computation

pub mod state;
pub mod types;
pub mod agents;
pub mod network;
pub mod health;
pub mod transport;
pub mod companion;
pub mod emitter;
pub mod payloads;
pub mod delta;
pub mod trend;
pub mod rl;
