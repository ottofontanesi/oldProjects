// End-to-end integration tests — cross-module flows with TestWorld harness
//
// All tests use mock transport, mock nodes, and in-memory persistence.
// No external dependencies — runs with `cargo test integration_tests::`.

mod harness;
mod mock_transport;
mod mock_node;
mod persistence;
mod test_pairing;
mod test_agent;
mod test_transport;
mod test_optimizer;
mod test_recovery;
mod test_concurrent;
mod test_errors;
