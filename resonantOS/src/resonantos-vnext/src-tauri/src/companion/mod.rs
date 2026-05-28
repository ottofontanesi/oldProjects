//! Phone Companion App module.
//!
//! Turns iOS/Android phones into active compute nodes in the ResonantOS mesh.
//! Built with Tauri Mobile v2, reusing existing transport, split inference,
//! and pairing infrastructure.

pub mod types;
pub mod identity;
pub mod health;
pub mod inference_runtime;
pub mod layer_worker;
pub mod assignment;
pub mod lifecycle;
pub mod npu;
pub mod pairing;
pub mod transport_bridge;
pub mod commands;
pub mod service;

#[cfg(test)]
mod property_tests;
