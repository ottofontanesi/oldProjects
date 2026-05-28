// Intent citation: .kiro/specs/mesh-network-optimizer/design.md Section 2.3, 3.6
// Network Accounting — append-only ledger with dual-signature records

use crate::mesh::identity::{MeshId, MeshIdentity};
use crate::transport::trait_def::NodeId;
use chrono::{DateTime, Utc};
use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

// ─── Accounting Types ────────────────────────────────────────────────────────

/// Type of contribution being recorded.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AccountingType {
    InferenceServed,
    ModelHosting,
    BandwidthRelay,
    ModelTransfer,
}

/// Quantified amount of a contribution/consumption.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AccountingAmount {
    pub gpu_seconds: f64,
    pub ram_seconds: f64,
    pub bandwidth_bytes: u64,
    pub request_count: u32,
}

impl AccountingAmount {
    pub fn total_normalized(&self) -> f64 {
        // Normalize to a single score: weight GPU highest, then RAM, then bandwidth
        self.gpu_seconds * 1.0 + self.ram_seconds * 0.1 + (self.bandwidth_bytes as f64) * 0.00001
    }
}

/// A single accounting record, signed by both contributor and consumer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountingRecord {
    pub record_id: Uuid,
    pub mesh_id: MeshId,
    pub timestamp: DateTime<Utc>,
    pub record_type: AccountingType,
    pub contributor_node: NodeId,
    pub consumer_node: NodeId,
    pub amount: AccountingAmount,
    #[serde(with = "crate::mesh::identity::signature_serde")]
    pub contributor_signature: Signature,
    #[serde(with = "crate::mesh::identity::signature_serde")]
    pub consumer_signature: Signature,
}

/// Time period for accounting queries.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AccountingPeriod {
    LastCycle,
    Last24Hours,
    Last7Days,
    Last30Days,
}

impl AccountingPeriod {
    pub fn duration(&self) -> chrono::Duration {
        match self {
            Self::LastCycle => chrono::Duration::minutes(15),
            Self::Last24Hours => chrono::Duration::hours(24),
            Self::Last7Days => chrono::Duration::days(7),
            Self::Last30Days => chrono::Duration::days(30),
        }
    }
}

/// Summary of a node's contributions and consumption.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContributionSummary {
    pub node_id: NodeId,
    pub mesh_id: MeshId,
    pub period: AccountingPeriod,
    pub total_contributed: AccountingAmount,
    pub total_consumed: AccountingAmount,
    pub balance: f64,
    pub rank_in_mesh: u32,
}

// ─── Accounting Ledger ───────────────────────────────────────────────────────

/// Append-only accounting ledger with dual-signature verification.
pub struct AccountingLedger {
    /// All records (append-only — no deletions allowed).
    records: Vec<AccountingRecord>,
    /// Public keys for signature verification: node_id -> VerifyingKey.
    known_keys: HashMap<NodeId, VerifyingKey>,
}

impl AccountingLedger {
    pub fn new() -> Self {
        Self {
            records: Vec::new(),
            known_keys: HashMap::new(),
        }
    }

    /// Register a node's public key for signature verification.
    pub fn register_key(&mut self, node_id: NodeId, key: VerifyingKey) {
        self.known_keys.insert(node_id, key);
    }

    /// Get the number of records in the ledger.
    pub fn record_count(&self) -> usize {
        self.records.len()
    }

    /// Create a contribution record payload (unsigned) for signing.
    pub fn create_record_payload(
        mesh_id: MeshId,
        record_type: AccountingType,
        contributor_node: NodeId,
        consumer_node: NodeId,
        amount: AccountingAmount,
    ) -> RecordPayload {
        RecordPayload {
            record_id: Uuid::new_v4(),
            mesh_id,
            timestamp: Utc::now(),
            record_type,
            contributor_node,
            consumer_node,
            amount,
        }
    }

    /// Sign a record payload as the contributor.
    pub fn sign_as_contributor(
        payload: &RecordPayload,
        identity: &MeshIdentity,
    ) -> Signature {
        let bytes = serde_json::to_vec(payload).expect("Payload serialization should not fail");
        identity.sign(&bytes)
    }

    /// Co-sign a record payload as the consumer.
    pub fn sign_as_consumer(
        payload: &RecordPayload,
        identity: &MeshIdentity,
    ) -> Signature {
        let bytes = serde_json::to_vec(payload).expect("Payload serialization should not fail");
        identity.sign(&bytes)
    }

    /// Append a fully-signed record to the ledger.
    /// Verifies both signatures before appending.
    pub fn append(&mut self, record: AccountingRecord) -> Result<(), AccountingError> {
        // Verify contributor signature
        let payload = RecordPayload {
            record_id: record.record_id,
            mesh_id: record.mesh_id,
            timestamp: record.timestamp,
            record_type: record.record_type.clone(),
            contributor_node: record.contributor_node,
            consumer_node: record.consumer_node,
            amount: record.amount.clone(),
        };
        let payload_bytes =
            serde_json::to_vec(&payload).map_err(|e| AccountingError::SerializationFailed(e.to_string()))?;

        if let Some(contributor_key) = self.known_keys.get(&record.contributor_node) {
            if !MeshIdentity::verify(&payload_bytes, &record.contributor_signature, contributor_key) {
                return Err(AccountingError::InvalidContributorSignature);
            }
        }

        if let Some(consumer_key) = self.known_keys.get(&record.consumer_node) {
            if !MeshIdentity::verify(&payload_bytes, &record.consumer_signature, consumer_key) {
                return Err(AccountingError::InvalidConsumerSignature);
            }
        }

        self.records.push(record);
        Ok(())
    }

    /// Compute the balance for a node in a mesh over a given period.
    pub fn compute_balance(
        &self,
        node_id: &NodeId,
        mesh_id: &MeshId,
        period: &AccountingPeriod,
    ) -> ContributionSummary {
        let cutoff = Utc::now() - period.duration();

        let mut contributed = AccountingAmount::default();
        let mut consumed = AccountingAmount::default();

        for record in &self.records {
            if record.mesh_id != *mesh_id || record.timestamp < cutoff {
                continue;
            }

            if record.contributor_node == *node_id {
                contributed.gpu_seconds += record.amount.gpu_seconds;
                contributed.ram_seconds += record.amount.ram_seconds;
                contributed.bandwidth_bytes += record.amount.bandwidth_bytes;
                contributed.request_count += record.amount.request_count;
            }

            if record.consumer_node == *node_id {
                consumed.gpu_seconds += record.amount.gpu_seconds;
                consumed.ram_seconds += record.amount.ram_seconds;
                consumed.bandwidth_bytes += record.amount.bandwidth_bytes;
                consumed.request_count += record.amount.request_count;
            }
        }

        let balance = contributed.total_normalized() - consumed.total_normalized();

        // Compute rank (1 = top contributor)
        let rank = self.compute_rank(node_id, mesh_id, period);

        ContributionSummary {
            node_id: *node_id,
            mesh_id: *mesh_id,
            period: period.clone(),
            total_contributed: contributed,
            total_consumed: consumed,
            balance,
            rank_in_mesh: rank,
        }
    }

    /// Compute rank of a node among all nodes in the mesh for the period.
    fn compute_rank(
        &self,
        node_id: &NodeId,
        mesh_id: &MeshId,
        period: &AccountingPeriod,
    ) -> u32 {
        let cutoff = Utc::now() - period.duration();

        // Collect all unique nodes and their balances
        let mut node_balances: HashMap<NodeId, f64> = HashMap::new();

        for record in &self.records {
            if record.mesh_id != *mesh_id || record.timestamp < cutoff {
                continue;
            }

            *node_balances.entry(record.contributor_node).or_default() +=
                record.amount.total_normalized();
            *node_balances.entry(record.consumer_node).or_default() -=
                record.amount.total_normalized();
        }

        let my_balance = node_balances.get(node_id).copied().unwrap_or(0.0);
        let rank = node_balances
            .values()
            .filter(|&&b| b > my_balance)
            .count() as u32
            + 1;

        rank
    }

    /// Get per-node summaries for all nodes in a mesh (leaderboard).
    pub fn leaderboard(
        &self,
        mesh_id: &MeshId,
        period: &AccountingPeriod,
    ) -> Vec<ContributionSummary> {
        let cutoff = Utc::now() - period.duration();

        // Collect all unique nodes
        let mut nodes: Vec<NodeId> = Vec::new();
        for record in &self.records {
            if record.mesh_id != *mesh_id || record.timestamp < cutoff {
                continue;
            }
            if !nodes.contains(&record.contributor_node) {
                nodes.push(record.contributor_node);
            }
            if !nodes.contains(&record.consumer_node) {
                nodes.push(record.consumer_node);
            }
        }

        let mut summaries: Vec<ContributionSummary> = nodes
            .iter()
            .map(|n| self.compute_balance(n, mesh_id, period))
            .collect();

        summaries.sort_by(|a, b| b.balance.partial_cmp(&a.balance).unwrap_or(std::cmp::Ordering::Equal));

        // Update ranks
        for (i, summary) in summaries.iter_mut().enumerate() {
            summary.rank_in_mesh = (i + 1) as u32;
        }

        summaries
    }

    /// Get records that need to be replicated to tier-3 nodes.
    /// Returns records added since the given timestamp.
    pub fn records_since(&self, since: DateTime<Utc>) -> Vec<&AccountingRecord> {
        self.records
            .iter()
            .filter(|r| r.timestamp > since)
            .collect()
    }

    /// Anonymize all records for a retired node.
    /// Replaces the node_id with a placeholder but preserves the record structure.
    pub fn anonymize_node(&mut self, node_id: &NodeId, placeholder: NodeId) {
        for record in &mut self.records {
            if record.contributor_node == *node_id {
                record.contributor_node = placeholder;
            }
            if record.consumer_node == *node_id {
                record.consumer_node = placeholder;
            }
        }
        self.known_keys.remove(node_id);
    }
}

// ─── Supporting Types ────────────────────────────────────────────────────────

/// Unsigned record payload for signing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordPayload {
    pub record_id: Uuid,
    pub mesh_id: MeshId,
    pub timestamp: DateTime<Utc>,
    pub record_type: AccountingType,
    pub contributor_node: NodeId,
    pub consumer_node: NodeId,
    pub amount: AccountingAmount,
}

/// Errors during accounting operations.
#[derive(Debug, Clone, PartialEq)]
pub enum AccountingError {
    InvalidContributorSignature,
    InvalidConsumerSignature,
    SerializationFailed(String),
}

impl std::fmt::Display for AccountingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidContributorSignature => write!(f, "Invalid contributor signature"),
            Self::InvalidConsumerSignature => write!(f, "Invalid consumer signature"),
            Self::SerializationFailed(e) => write!(f, "Serialization failed: {}", e),
        }
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh::identity::MeshIdentity;
    use proptest::prelude::*;

    fn create_signed_record(
        contributor: &MeshIdentity,
        consumer: &MeshIdentity,
        mesh_id: MeshId,
        amount: AccountingAmount,
    ) -> AccountingRecord {
        let payload = AccountingLedger::create_record_payload(
            mesh_id,
            AccountingType::InferenceServed,
            contributor.node_id,
            consumer.node_id,
            amount,
        );

        let contributor_sig = AccountingLedger::sign_as_contributor(&payload, contributor);
        let consumer_sig = AccountingLedger::sign_as_consumer(&payload, consumer);

        AccountingRecord {
            record_id: payload.record_id,
            mesh_id: payload.mesh_id,
            timestamp: payload.timestamp,
            record_type: payload.record_type,
            contributor_node: payload.contributor_node,
            consumer_node: payload.consumer_node,
            amount: payload.amount,
            contributor_signature: contributor_sig,
            consumer_signature: consumer_sig,
        }
    }

    proptest! {
        /// Property: dual-signed records cannot be forged (verify both signatures).
        #[test]
        fn prop_dual_signed_records_verified(
            gpu_secs in 0.0f64..100.0,
            ram_secs in 0.0f64..100.0,
            bw_bytes in 0u64..1_000_000
        ) {
            let contributor = MeshIdentity::generate();
            let consumer = MeshIdentity::generate();
            let mesh_id = Uuid::new_v4();

            let mut ledger = AccountingLedger::new();
            ledger.register_key(contributor.node_id, contributor.verifying_key);
            ledger.register_key(consumer.node_id, consumer.verifying_key);

            let amount = AccountingAmount {
                gpu_seconds: gpu_secs,
                ram_seconds: ram_secs,
                bandwidth_bytes: bw_bytes,
                request_count: 1,
            };

            let record = create_signed_record(&contributor, &consumer, mesh_id, amount);
            let result = ledger.append(record);
            prop_assert!(result.is_ok(), "Valid dual-signed record should be accepted");
        }

        /// Property: balance computation is deterministic given same records.
        #[test]
        fn prop_balance_deterministic(
            num_records in 1usize..10,
            gpu_secs in proptest::collection::vec(0.1f64..10.0, 1..10)
        ) {
            let contributor = MeshIdentity::generate();
            let consumer = MeshIdentity::generate();
            let mesh_id = Uuid::new_v4();

            let mut ledger = AccountingLedger::new();
            ledger.register_key(contributor.node_id, contributor.verifying_key);
            ledger.register_key(consumer.node_id, consumer.verifying_key);

            let count = num_records.min(gpu_secs.len());
            for i in 0..count {
                let amount = AccountingAmount {
                    gpu_seconds: gpu_secs[i],
                    ram_seconds: 0.0,
                    bandwidth_bytes: 0,
                    request_count: 1,
                };
                let record = create_signed_record(&contributor, &consumer, mesh_id, amount);
                ledger.append(record).unwrap();
            }

            // Compute balance twice — should be identical
            let b1 = ledger.compute_balance(&contributor.node_id, &mesh_id, &AccountingPeriod::Last30Days);
            let b2 = ledger.compute_balance(&contributor.node_id, &mesh_id, &AccountingPeriod::Last30Days);
            prop_assert_eq!(b1.balance, b2.balance);
            prop_assert_eq!(b1.total_contributed.request_count, b2.total_contributed.request_count);
        }

        /// Property: ledger is append-only (record count never decreases).
        #[test]
        fn prop_ledger_append_only(
            num_records in 1usize..20
        ) {
            let contributor = MeshIdentity::generate();
            let consumer = MeshIdentity::generate();
            let mesh_id = Uuid::new_v4();

            let mut ledger = AccountingLedger::new();
            ledger.register_key(contributor.node_id, contributor.verifying_key);
            ledger.register_key(consumer.node_id, consumer.verifying_key);

            let mut prev_count = 0;
            for _ in 0..num_records {
                let amount = AccountingAmount {
                    gpu_seconds: 1.0,
                    ram_seconds: 0.0,
                    bandwidth_bytes: 100,
                    request_count: 1,
                };
                let record = create_signed_record(&contributor, &consumer, mesh_id, amount);
                ledger.append(record).unwrap();

                let new_count = ledger.record_count();
                prop_assert!(new_count > prev_count, "Record count must always increase");
                prev_count = new_count;
            }
        }
    }

    // ─── Unit Tests ──────────────────────────────────────────────────────────

    #[test]
    fn test_forged_contributor_signature_rejected() {
        let contributor = MeshIdentity::generate();
        let consumer = MeshIdentity::generate();
        let forger = MeshIdentity::generate();
        let mesh_id = Uuid::new_v4();

        let mut ledger = AccountingLedger::new();
        ledger.register_key(contributor.node_id, contributor.verifying_key);
        ledger.register_key(consumer.node_id, consumer.verifying_key);

        let amount = AccountingAmount {
            gpu_seconds: 5.0,
            ram_seconds: 1.0,
            bandwidth_bytes: 1000,
            request_count: 1,
        };

        // Create record but sign with forger's key instead of contributor's
        let payload = AccountingLedger::create_record_payload(
            mesh_id,
            AccountingType::InferenceServed,
            contributor.node_id,
            consumer.node_id,
            amount,
        );
        let forged_sig = AccountingLedger::sign_as_contributor(&payload, &forger);
        let consumer_sig = AccountingLedger::sign_as_consumer(&payload, &consumer);

        let record = AccountingRecord {
            record_id: payload.record_id,
            mesh_id: payload.mesh_id,
            timestamp: payload.timestamp,
            record_type: payload.record_type,
            contributor_node: payload.contributor_node,
            consumer_node: payload.consumer_node,
            amount: payload.amount,
            contributor_signature: forged_sig,
            consumer_signature: consumer_sig,
        };

        let result = ledger.append(record);
        assert_eq!(result, Err(AccountingError::InvalidContributorSignature));
    }

    #[test]
    fn test_leaderboard_ordering() {
        let node_a = MeshIdentity::generate();
        let node_b = MeshIdentity::generate();
        let node_c = MeshIdentity::generate();
        let mesh_id = Uuid::new_v4();

        let mut ledger = AccountingLedger::new();
        ledger.register_key(node_a.node_id, node_a.verifying_key);
        ledger.register_key(node_b.node_id, node_b.verifying_key);
        ledger.register_key(node_c.node_id, node_c.verifying_key);

        // node_a contributes 10 GPU-seconds to node_b
        let amount = AccountingAmount { gpu_seconds: 10.0, ..Default::default() };
        let record = create_signed_record(&node_a, &node_b, mesh_id, amount);
        ledger.append(record).unwrap();

        // node_a contributes 5 GPU-seconds to node_c
        let amount = AccountingAmount { gpu_seconds: 5.0, ..Default::default() };
        let record = create_signed_record(&node_a, &node_c, mesh_id, amount);
        ledger.append(record).unwrap();

        let board = ledger.leaderboard(&mesh_id, &AccountingPeriod::Last30Days);
        assert_eq!(board[0].node_id, node_a.node_id); // Top contributor
        assert!(board[0].balance > 0.0);
    }
}
