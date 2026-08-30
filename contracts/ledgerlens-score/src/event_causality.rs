/// Event Causality Identifiers
///
/// This module provides correlation IDs to link events across multi-step workflows,
/// enabling off-chain auditors to reconstruct complete workflow timelines from event logs alone.
///
/// # Workflow Patterns
///
/// The contract executes several multi-step workflows that generate sequences of related events:
///
/// 1. **Score Submission Workflow**
///    - Score submitted → Score pending (finality buffer) → Score committed/vetoed
///    - Correlation ID: Derived from (wallet, asset_pair, timestamp)
///
/// 2. **Governance Proposal Workflow**
///    - Proposal initiated → Approvals accumulated → Executed or vetoed
///    - Correlation ID: Explicitly set for governance_action_appended
///
/// 3. **Admin Transfer Workflow**
///    - Transfer initiated → Transfer accepted or cancelled
///    - Correlation ID: Derived from (from_admin, to_admin, initiation_timestamp)
///
/// 4. **Upgrade Workflow**
///    - Upgrade proposed → Approvals accumulated → Executed or vetoed
///    - Correlation ID: Derived from (new_wasm_hash, proposal_timestamp)
///
/// 5. **Dispute Workflow**
///    - Dispute opened → Dispute resolved or timeout triggered
///    - Correlation ID: Derived from (challenger, asset_pair, opening_timestamp)
///
/// # Implementation Notes
///
/// Correlation IDs are deterministic and reproducible off-chain, allowing auditors to:
/// - Verify causality without requiring additional storage
/// - Group related events even if the contract is upgraded
/// - Reconstruct workflows from historical event logs
///
/// The correlation_id is NOT stored on-chain (to avoid storage overhead), but is:
/// - Logged in event data for recovery
/// - Computed deterministically from stable inputs
/// - Documented in event sequences for human auditors
extern crate alloc;

use alloc::{vec, vec::Vec};
use soroban_sdk::{xdr::ToXdr, Address, Bytes, Env, Symbol};

/// Correlation ID uniquely identifying a causal workflow
/// This is a 32-byte hash computed from workflow parameters
pub type CorrelationId = [u8; 32];

/// Event causality tracking for multi-step workflows
pub struct EventCausality;

impl EventCausality {
    /// Generate correlation ID for score submission workflow
    /// Workflow: score_submitted → score_pending → score_committed/score_vetoed
    ///
    /// # Parameters
    /// - `wallet`: The wallet whose score is being submitted
    /// - `asset_pair`: The asset pair being scored
    /// - `timestamp`: The submission timestamp (from chain)
    pub fn score_submission_correlation_id(
        wallet: &Address,
        asset_pair: &Symbol,
        timestamp: u64,
    ) -> CorrelationId {
        let env = wallet.env().clone();
        Self::hash_bytes(
            &env,
            &[
                Bytes::from_slice(&env, b"score_submit"),
                wallet.to_xdr(&env),
                asset_pair.to_xdr(&env),
                Bytes::from_array(&env, &timestamp.to_le_bytes()),
            ],
        )
    }

    /// Generate correlation ID for admin transfer workflow
    /// Workflow: admin_transfer_initiated → admin_transfer_accepted/admin_transfer_cancelled
    ///
    /// # Parameters
    /// - `from`: Current admin address
    /// - `to`: Target admin address
    /// - `timestamp`: Initiation timestamp
    pub fn admin_transfer_correlation_id(
        from: &Address,
        to: &Address,
        timestamp: u64,
    ) -> CorrelationId {
        let env = from.env().clone();
        Self::hash_bytes(
            &env,
            &[
                Bytes::from_slice(&env, b"admin_xfer"),
                from.to_xdr(&env),
                to.to_xdr(&env),
                Bytes::from_array(&env, &timestamp.to_le_bytes()),
            ],
        )
    }

    /// Generate correlation ID for upgrade workflow
    /// Workflow: upgrade_proposed → upgrade_approval_added* → upgrade_executed/upgrade_vetoed
    ///
    /// # Parameters
    /// - `new_wasm_hash`: The new contract WASM hash being proposed
    /// - `timestamp`: The proposal timestamp
    pub fn upgrade_correlation_id(new_wasm_hash: &[u8; 32], timestamp: u64) -> CorrelationId {
        let env = Env::default();
        Self::hash_bytes(
            &env,
            &[
                Bytes::from_slice(&env, b"upgrade_prop"),
                Bytes::from_array(&env, new_wasm_hash),
                Bytes::from_array(&env, &timestamp.to_le_bytes()),
            ],
        )
    }

    /// Generate correlation ID for parameter change workflow
    /// Workflow: param_change_proposed → param_change_executed/param_change_vetoed
    ///
    /// # Parameters
    /// - `env`: The Soroban environment that owns `param_key`
    /// - `proposal_id`: Unique proposal identifier
    /// - `param_key`: The parameter key being changed
    /// - `timestamp`: The proposal timestamp
    pub fn parameter_change_correlation_id(
        env: &Env,
        proposal_id: u64,
        param_key: &Symbol,
        timestamp: u64,
    ) -> CorrelationId {
        Self::hash_bytes(
            env,
            &[
                Bytes::from_slice(env, b"param_change"),
                Bytes::from_array(env, &proposal_id.to_le_bytes()),
                param_key.to_xdr(env),
                Bytes::from_array(env, &timestamp.to_le_bytes()),
            ],
        )
    }

    /// Generate correlation ID for dispute workflow
    /// Workflow: dispute_opened → dispute_resolved/dispute_timed_out
    ///
    /// # Parameters
    /// - `challenger`: The address challenging the score
    /// - `asset_pair`: The asset pair being disputed
    /// - `timestamp`: The opening timestamp
    pub fn dispute_correlation_id(
        challenger: &Address,
        asset_pair: &Symbol,
        timestamp: u64,
    ) -> CorrelationId {
        let env = challenger.env().clone();
        Self::hash_bytes(
            &env,
            &[
                Bytes::from_slice(&env, b"dispute_open"),
                challenger.to_xdr(&env),
                asset_pair.to_xdr(&env),
                Bytes::from_array(&env, &timestamp.to_le_bytes()),
            ],
        )
    }

    /// Generate correlation ID for governance action chain
    /// Workflow: Multiple governance_action_appended events forming a chain
    ///
    /// # Parameters
    /// - `action_index`: Sequential index of this action in the governance chain
    /// - `timestamp`: The action timestamp
    pub fn governance_chain_correlation_id(action_index: u64, timestamp: u64) -> CorrelationId {
        let env = Env::default();
        Self::hash_bytes(
            &env,
            &[
                Bytes::from_slice(&env, b"gov_chain"),
                Bytes::from_array(&env, &action_index.to_le_bytes()),
                Bytes::from_array(&env, &timestamp.to_le_bytes()),
            ],
        )
    }

    /// Generate correlation ID for consensus score workflow
    /// Workflow: Multiple model submissions → consensus_score_submitted
    ///
    /// # Parameters
    /// - `wallet`: The wallet being scored
    /// - `asset_pair`: The asset pair
    /// - `round_id`: Consensus round identifier
    pub fn consensus_round_correlation_id(
        wallet: &Address,
        asset_pair: &Symbol,
        round_id: u64,
    ) -> CorrelationId {
        let env = wallet.env().clone();
        Self::hash_bytes(
            &env,
            &[
                Bytes::from_slice(&env, b"consensus_round"),
                wallet.to_xdr(&env),
                asset_pair.to_xdr(&env),
                Bytes::from_array(&env, &round_id.to_le_bytes()),
            ],
        )
    }

    /// Generate correlation ID for escalation workflow
    /// Workflow: escalation_triggered → escalation_resolved
    ///
    /// # Parameters
    /// - `wallet`: The wallet entering escalation
    /// - `asset_pair`: The asset pair
    /// - `timestamp`: The trigger timestamp
    pub fn escalation_correlation_id(
        wallet: &Address,
        asset_pair: &Symbol,
        timestamp: u64,
    ) -> CorrelationId {
        let env = wallet.env().clone();
        Self::hash_bytes(
            &env,
            &[
                Bytes::from_slice(&env, b"escalation"),
                wallet.to_xdr(&env),
                asset_pair.to_xdr(&env),
                Bytes::from_array(&env, &timestamp.to_le_bytes()),
            ],
        )
    }

    /// Internal: Hash multiple byte sequences to produce a correlation ID
    fn hash_bytes(env: &Env, parts: &[Bytes]) -> CorrelationId {
        let mut combined = Bytes::new(env);
        for part in parts {
            combined.append(part);
        }
        env.crypto().sha256(&combined).to_array()
    }
}

/// Workflow causality tracking for audit replay
pub struct WorkflowTracker;

impl WorkflowTracker {
    /// Describes a multi-step workflow and its expected event sequence
    pub fn score_submission_workflow() -> WorkflowDescription {
        WorkflowDescription {
            name: "Score Submission",
            description: "Wallet submits a risk score that undergoes finality buffering",
            steps: vec![
                WorkflowStep {
                    event: "score_submitted",
                    description: "Initial score submission recorded",
                    optional: false,
                    depends_on: None,
                },
                WorkflowStep {
                    event: "score_pending",
                    description: "Score enters finality buffer window",
                    optional: true,
                    depends_on: Some(0),
                },
                WorkflowStep {
                    event: "score_committed",
                    description: "Score finalized after buffer expires",
                    optional: true,
                    depends_on: Some(1),
                },
                WorkflowStep {
                    event: "score_vetoed",
                    description: "Admin rejected score before finalization",
                    optional: true,
                    depends_on: Some(1),
                },
                WorkflowStep {
                    event: "score_delta",
                    description: "Score changed (recorded for history)",
                    optional: true,
                    depends_on: None,
                },
            ],
        }
    }

    pub fn upgrade_workflow() -> WorkflowDescription {
        WorkflowDescription {
            name: "Contract Upgrade",
            description: "Multi-sig upgrade proposal with approval accumulation",
            steps: vec![
                WorkflowStep {
                    event: "upgrade_proposed",
                    description: "Upgrade WASM hash and timeline proposed",
                    optional: false,
                    depends_on: None,
                },
                WorkflowStep {
                    event: "upgrade_approval_added",
                    description: "Signer approves upgrade (may repeat)",
                    optional: false,
                    depends_on: Some(0),
                },
                WorkflowStep {
                    event: "upgrade_executed",
                    description: "Quorum reached, upgrade applied",
                    optional: true,
                    depends_on: Some(0),
                },
                WorkflowStep {
                    event: "upgrade_vetoed",
                    description: "Admin veto before execution",
                    optional: true,
                    depends_on: Some(0),
                },
            ],
        }
    }

    pub fn admin_transfer_workflow() -> WorkflowDescription {
        WorkflowDescription {
            name: "Admin Transfer",
            description: "Two-phase admin key rotation",
            steps: vec![
                WorkflowStep {
                    event: "admin_transfer_initiated",
                    description: "Current admin initiates transfer to new address",
                    optional: false,
                    depends_on: None,
                },
                WorkflowStep {
                    event: "admin_transfer_accepted",
                    description: "New admin accepts the transfer",
                    optional: true,
                    depends_on: Some(0),
                },
                WorkflowStep {
                    event: "admin_transfer_cancelled",
                    description: "Current admin cancels the pending transfer",
                    optional: true,
                    depends_on: Some(0),
                },
            ],
        }
    }

    pub fn dispute_workflow() -> WorkflowDescription {
        WorkflowDescription {
            name: "Score Dispute",
            description: "Challenger disputes a score, potentially receiving refund",
            steps: vec![
                WorkflowStep {
                    event: "dispute_opened",
                    description: "Challenger posts bond and opens dispute",
                    optional: false,
                    depends_on: None,
                },
                WorkflowStep {
                    event: "dispute_resolved",
                    description: "Dispute resolved with corrected score and bond return",
                    optional: true,
                    depends_on: Some(0),
                },
                WorkflowStep {
                    event: "dispute_timed_out",
                    description: "Dispute deadline passed without resolution",
                    optional: true,
                    depends_on: Some(0),
                },
            ],
        }
    }

    pub fn escalation_workflow() -> WorkflowDescription {
        WorkflowDescription {
            name: "Breach Escalation",
            description: "Repeated threshold breaches trigger escalation and recovery",
            steps: vec![
                WorkflowStep {
                    event: "breach",
                    description: "Threshold breached (consecutive count incremented)",
                    optional: false,
                    depends_on: None,
                },
                WorkflowStep {
                    event: "escalation_triggered",
                    description: "Consecutive breach count reaches threshold",
                    optional: true,
                    depends_on: Some(0),
                },
                WorkflowStep {
                    event: "escalation_resolved",
                    description: "Clean score submission or admin reset clears escalation",
                    optional: true,
                    depends_on: Some(1),
                },
            ],
        }
    }
}

/// Description of a multi-step workflow for audit documentation
pub struct WorkflowDescription {
    pub name: &'static str,
    pub description: &'static str,
    pub steps: Vec<WorkflowStep>,
}

/// A single step in a multi-step workflow
pub struct WorkflowStep {
    pub event: &'static str,
    pub description: &'static str,
    pub optional: bool,
    /// Index of the step this depends on, if any
    pub depends_on: Option<usize>,
}

#[cfg(test)]
mod test_event_causality {
    use super::*;
    use soroban_sdk::testutils::Address as _;

    #[test]
    fn test_score_submission_correlation_id_is_deterministic() {
        let env = Env::default();
        let wallet = Address::generate(&env);
        let asset_pair = Symbol::new(&env, "stellar_usdc");
        let timestamp = 1000;

        let id1 = EventCausality::score_submission_correlation_id(&wallet, &asset_pair, timestamp);
        let id2 = EventCausality::score_submission_correlation_id(&wallet, &asset_pair, timestamp);

        assert_eq!(id1, id2, "Correlation IDs should be deterministic");
    }

    #[test]
    fn test_score_submission_correlation_id_differs_by_wallet() {
        let env = Env::default();
        let wallet1 = Address::generate(&env);
        let wallet2 = Address::generate(&env);
        let asset_pair = Symbol::new(&env, "stellar_usdc");
        let timestamp = 1000;

        let id1 = EventCausality::score_submission_correlation_id(&wallet1, &asset_pair, timestamp);
        let id2 = EventCausality::score_submission_correlation_id(&wallet2, &asset_pair, timestamp);

        assert_ne!(id1, id2, "Different wallets should produce different IDs");
    }

    #[test]
    fn test_score_submission_correlation_id_differs_by_timestamp() {
        let env = Env::default();
        let wallet = Address::generate(&env);
        let asset_pair = Symbol::new(&env, "stellar_usdc");

        let id1 = EventCausality::score_submission_correlation_id(&wallet, &asset_pair, 1000);
        let id2 = EventCausality::score_submission_correlation_id(&wallet, &asset_pair, 2000);

        assert_ne!(id1, id2, "Different timestamps should produce different IDs");
    }

    #[test]
    fn test_admin_transfer_correlation_id_is_deterministic() {
        let env = Env::default();
        let from = Address::generate(&env);
        let to = Address::generate(&env);
        let timestamp = 5000;

        let id1 = EventCausality::admin_transfer_correlation_id(&from, &to, timestamp);
        let id2 = EventCausality::admin_transfer_correlation_id(&from, &to, timestamp);

        assert_eq!(id1, id2);
    }

    #[test]
    fn test_upgrade_correlation_id_is_deterministic() {
        let wasm_hash = [1u8; 32];
        let timestamp = 3000;

        let id1 = EventCausality::upgrade_correlation_id(&wasm_hash, timestamp);
        let id2 = EventCausality::upgrade_correlation_id(&wasm_hash, timestamp);

        assert_eq!(id1, id2);
    }

    #[test]
    fn test_dispute_correlation_id_is_deterministic() {
        let env = Env::default();
        let challenger = Address::generate(&env);
        let asset_pair = Symbol::new(&env, "xlm_usdc");
        let timestamp = 7000;

        let id1 = EventCausality::dispute_correlation_id(&challenger, &asset_pair, timestamp);
        let id2 = EventCausality::dispute_correlation_id(&challenger, &asset_pair, timestamp);

        assert_eq!(id1, id2);
    }

    #[test]
    fn test_workflow_descriptions_are_complete() {
        assert!(!WorkflowTracker::score_submission_workflow().steps.is_empty());
        assert!(!WorkflowTracker::upgrade_workflow().steps.is_empty());
        assert!(!WorkflowTracker::admin_transfer_workflow().steps.is_empty());
        assert!(!WorkflowTracker::dispute_workflow().steps.is_empty());
        assert!(!WorkflowTracker::escalation_workflow().steps.is_empty());
    }

    #[test]
    fn test_score_submission_workflow_has_required_event() {
        let workflow = WorkflowTracker::score_submission_workflow();
        let has_submitted =
            workflow.steps.iter().any(|s| s.event == "score_submitted" && !s.optional);
        assert!(
            has_submitted,
            "Score submission workflow must have required score_submitted event"
        );
    }

    #[test]
    fn test_upgrade_workflow_has_required_events() {
        let workflow = WorkflowTracker::upgrade_workflow();
        let has_proposed =
            workflow.steps.iter().any(|s| s.event == "upgrade_proposed" && !s.optional);
        let has_approval =
            workflow.steps.iter().any(|s| s.event == "upgrade_approval_added" && !s.optional);
        assert!(has_proposed, "Upgrade workflow must have required upgrade_proposed event");
        assert!(has_approval, "Upgrade workflow must have required upgrade_approval_added event");
    }
}
