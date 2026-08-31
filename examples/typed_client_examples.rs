//! Typed client examples for the four canonical LedgerLens integration flows.
//!
//! These examples are designed to be copy-pasted by downstream integrators.
//! Each module covers one flow end-to-end using the generated
//! `LedgerLensScoreContractClient` — no contract internals required.
//!
//! ## Flows covered
//!
//! | Module               | Flow                                          |
//! |----------------------|-----------------------------------------------|
//! | [`score_flow`]       | Submit and retrieve a risk score              |
//! | [`gate_flow`]        | Gate a transaction on risk score / confidence |
//! | [`history_flow`]     | Read score history and manage ring depth      |
//! | [`governance_flow`]  | Propose, inspect, and veto an upgrade         |
//!
//! Build with:
//! ```text
//! cargo build --example typed_client_examples -p ledgerlens-score
//! ```

#![allow(unused)]
#![no_std]

extern crate std;

// ── Score flow ────────────────────────────────────────────────────────────────

/// Submit a score and read it back.
///
/// This is the most basic integration: the off-chain detection pipeline writes
/// a risk score and any caller reads it.
pub mod score_flow {
    use ledgerlens_score::{LedgerLensScoreContract, LedgerLensScoreContractClient};
    use soroban_sdk::{
        symbol_short,
        testutils::{Address as _, Ledger as _},
        Address, Env, Vec,
    };

    /// Helper: deploy + initialize LedgerLens, return (client, admin, service).
    fn setup(env: &Env) -> (LedgerLensScoreContractClient<'_>, Address, Address) {
        env.mock_all_auths();
        env.ledger().with_mut(|l| l.timestamp = 1_700_000_000);
        let id = env.register_contract(None, LedgerLensScoreContract);
        let client = LedgerLensScoreContractClient::new(env, &id);
        let admin = Address::generate(env);
        let service = Address::generate(env);
        client.initialize(&admin, &service);
        (client, admin, service)
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use ledgerlens_score::Error;

        // ── Success path ──────────────────────────────────────────────────────

        /// Submit a score and get it back.  Verifies the stored fields match
        /// exactly what was submitted.
        #[test]
        fn submit_and_get_score() {
            let env = Env::default();
            let (client, _admin, _service) = setup(&env);

            let wallet = Address::generate(&env);
            let pair = symbol_short!("XLM_USDC");

            // The service account submits a risk score.
            client.submit_score(
                &Vec::new(&env), // signers (empty → single-service mode)
                &wallet,
                &pair,
                &72,            // score: 0-100, higher = more suspicious
                &true,          // benford_flag: Benford's Law anomaly detected
                &false,         // ml_flag: ML classifier did not flag
                &1_700_000_000, // timestamp: ledger seconds
                &88,            // confidence: 0-100
                &1,             // model_version
                &None,          // attestation_input (optional)
            );

            let score = client.get_score(&wallet, &pair);
            assert_eq!(score.score, 72);
            assert_eq!(score.confidence, 88);
            assert!(score.benford_flag);
            assert!(!score.ml_flag);
            assert_eq!(score.model_version, 1);
        }

        /// Batch-submit two scores in one call.  `BatchResult` tells you which
        /// entries were accepted and which were rejected (with rejection codes).
        #[test]
        fn batch_submit_scores() {
            use ledgerlens_score::ScoreSubmission;

            let env = Env::default();
            let (client, _admin, _service) = setup(&env);

            let wallet_a = Address::generate(&env);
            let wallet_b = Address::generate(&env);
            let pair = symbol_short!("XLM_USDC");

            let mut submissions = Vec::new(&env);
            submissions.push_back(ScoreSubmission {
                wallet: wallet_a.clone(),
                asset_pair: pair.clone(),
                score: 30,
                benford_flag: false,
                ml_flag: false,
                timestamp: 1_700_000_000,
                confidence: 95,
                model_version: 1,
            });
            submissions.push_back(ScoreSubmission {
                wallet: wallet_b.clone(),
                asset_pair: pair.clone(),
                score: 80,
                benford_flag: true,
                ml_flag: true,
                timestamp: 1_700_000_000,
                confidence: 90,
                model_version: 1,
            });

            let result = client.submit_scores_batch(&submissions);
            assert_eq!(result.accepted_count, 2);
            assert_eq!(result.rejected_count, 0);
        }

        // ── Failure path ──────────────────────────────────────────────────────

        /// Score > 100 is invalid; `get_score` returns `ScoreNotFound` for an
        /// unscored wallet rather than panicking.
        #[test]
        fn get_score_returns_not_found_for_unknown_wallet() {
            let env = Env::default();
            let (client, _admin, _service) = setup(&env);

            let unknown = Address::generate(&env);
            let pair = symbol_short!("XLM_USDC");

            let result = client.try_get_score(&unknown, &pair);
            assert_eq!(result, Err(Ok(Error::ScoreNotFound)));
        }

        /// Submitting score 101 is rejected with `InvalidScore`.
        #[test]
        fn submit_score_out_of_range_is_rejected() {
            let env = Env::default();
            let (client, _admin, _service) = setup(&env);

            let wallet = Address::generate(&env);
            let pair = symbol_short!("XLM_USDC");

            let result = client.try_submit_score(
                &Vec::new(&env),
                &wallet,
                &pair,
                &101, // > 100 → InvalidScore
                &false,
                &false,
                &1_700_000_000,
                &90,
                &1,
                &None,
            );
            assert_eq!(result, Err(Ok(Error::InvalidScore)));
        }
    }
}

// ── Gate flow ─────────────────────────────────────────────────────────────────

/// Gate a swap (or any on-chain action) on a LedgerLens risk score.
///
/// `query_risk_gate` is **infallible** and **side-effect free** — use it
/// directly inside a guard clause without a `try_*` wrapper.
pub mod gate_flow {
    use ledgerlens_score::{LedgerLensScoreContract, LedgerLensScoreContractClient};
    use soroban_sdk::{
        symbol_short,
        testutils::{Address as _, Ledger as _},
        Address, Env, Vec,
    };

    fn setup(env: &Env) -> LedgerLensScoreContractClient<'_> {
        env.mock_all_auths();
        env.ledger().with_mut(|l| l.timestamp = 1_700_000_000);
        let id = env.register_contract(None, LedgerLensScoreContract);
        let client = LedgerLensScoreContractClient::new(env, &id);
        let admin = Address::generate(env);
        let service = Address::generate(env);
        client.initialize(&admin, &service);
        client
    }

    fn submit(
        env: &Env,
        client: &LedgerLensScoreContractClient,
        wallet: &Address,
        score: u32,
        confidence: u32,
    ) {
        client.submit_score(
            &Vec::new(env),
            wallet,
            &symbol_short!("XLM_USDC"),
            &score,
            &false,
            &false,
            &env.ledger().timestamp(),
            &confidence,
            &1,
            &None,
        );
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        const THRESHOLD: u32 = 75;
        const MIN_CONFIDENCE: u32 = 50;

        // ── query_risk_gate ───────────────────────────────────────────────────

        /// Low-risk wallet: score 30 < threshold 75 → gate passes (`true`).
        #[test]
        fn gate_passes_for_low_risk_wallet() {
            let env = Env::default();
            let client = setup(&env);
            let wallet = Address::generate(&env);
            submit(&env, &client, &wallet, 30, 90);

            // The canonical guard clause — no try_, no ?, no error handling.
            let is_safe = client.query_risk_gate(&wallet, &symbol_short!("XLM_USDC"), &THRESHOLD);
            assert!(is_safe, "score 30 < threshold 75 should pass");
        }

        /// High-risk wallet: score 80 >= threshold 75 → gate fails (`false`).
        #[test]
        fn gate_fails_for_high_risk_wallet() {
            let env = Env::default();
            let client = setup(&env);
            let wallet = Address::generate(&env);
            submit(&env, &client, &wallet, 80, 90);

            let is_safe = client.query_risk_gate(&wallet, &symbol_short!("XLM_USDC"), &THRESHOLD);
            assert!(!is_safe, "score 80 >= threshold 75 should fail");
        }

        /// Unknown wallet (no score): gate fails closed — treating "no data" as
        /// risky is the safe default.
        #[test]
        fn gate_fails_closed_for_unknown_wallet() {
            let env = Env::default();
            let client = setup(&env);
            let unknown = Address::generate(&env);

            let is_safe = client.query_risk_gate(&unknown, &symbol_short!("XLM_USDC"), &THRESHOLD);
            assert!(!is_safe, "unknown wallet should fail closed");
        }

        // ── query_risk_gate_with_confidence ───────────────────────────────────

        /// Low score AND high confidence → passes both gates.
        #[test]
        fn confidence_gate_passes_low_score_high_confidence() {
            let env = Env::default();
            let client = setup(&env);
            let wallet = Address::generate(&env);
            submit(&env, &client, &wallet, 30, 90); // confidence 90 >= 50

            let is_safe = client.query_risk_gate_with_confidence(
                &wallet,
                &symbol_short!("XLM_USDC"),
                &THRESHOLD,
                &MIN_CONFIDENCE,
            );
            assert!(is_safe);
        }

        /// Low score but low confidence → confidence gate blocks despite safe score.
        /// Treats "uncertain safe" the same as "no data".
        #[test]
        fn confidence_gate_blocks_low_confidence_even_with_safe_score() {
            let env = Env::default();
            let client = setup(&env);
            let wallet = Address::generate(&env);
            submit(&env, &client, &wallet, 10, 20); // confidence 20 < min_confidence 50

            let is_safe = client.query_risk_gate_with_confidence(
                &wallet,
                &symbol_short!("XLM_USDC"),
                &THRESHOLD,
                &MIN_CONFIDENCE,
            );
            assert!(!is_safe, "low confidence should block even a safe score");
        }
    }
}

// ── History flow ──────────────────────────────────────────────────────────────

/// Read score history and manage the ring-buffer depth.
///
/// LedgerLens keeps a rolling window of past scores per (wallet, asset_pair).
/// The default depth is 10; the admin can change it (with a time-lock) to any
/// value in [1, 50].
pub mod history_flow {
    use ledgerlens_score::{LedgerLensScoreContract, LedgerLensScoreContractClient};
    use soroban_sdk::{
        symbol_short,
        testutils::{Address as _, Ledger as _},
        Address, Env, Vec,
    };

    fn setup(env: &Env) -> (LedgerLensScoreContractClient<'_>, Address) {
        env.mock_all_auths();
        env.ledger().with_mut(|l| l.timestamp = 1_700_000_000);
        let id = env.register_contract(None, LedgerLensScoreContract);
        let client = LedgerLensScoreContractClient::new(env, &id);
        let admin = Address::generate(env);
        let service = Address::generate(env);
        client.initialize(&admin, &service);
        (client, admin)
    }

    /// Advance ledger time past the 1-hour submission cooldown.
    fn advance(env: &Env) {
        env.ledger().with_mut(|l| l.timestamp += 3_601);
    }

    fn submit(env: &Env, client: &LedgerLensScoreContractClient, wallet: &Address, score: u32) {
        advance(env);
        client.submit_score(
            &Vec::new(env),
            wallet,
            &symbol_short!("XLM_USDC"),
            &score,
            &false,
            &false,
            &env.ledger().timestamp(),
            &90,
            &1,
            &None,
        );
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// Read the full score history for a wallet.  The most recent score is
        /// last in the returned `Vec`.
        #[test]
        fn get_score_history_returns_submissions_in_order() {
            let env = Env::default();
            let (client, _admin) = setup(&env);
            let wallet = Address::generate(&env);
            let pair = symbol_short!("XLM_USDC");

            submit(&env, &client, &wallet, 40);
            submit(&env, &client, &wallet, 55);
            submit(&env, &client, &wallet, 70);

            let history = client.get_score_history(&wallet, &pair);
            assert_eq!(history.len(), 3);
            assert_eq!(history.get(0).unwrap().score, 40);
            assert_eq!(history.get(2).unwrap().score, 70);
        }

        /// `get_score_count` returns the cumulative count of submissions — it is
        /// never truncated by the ring-buffer depth.
        #[test]
        fn get_score_count_tracks_all_submissions() {
            let env = Env::default();
            let (client, _admin) = setup(&env);
            let wallet = Address::generate(&env);
            let pair = symbol_short!("XLM_USDC");

            submit(&env, &client, &wallet, 10);
            submit(&env, &client, &wallet, 20);
            submit(&env, &client, &wallet, 30);

            assert_eq!(client.get_score_count(&wallet, &pair), 3);
        }

        /// History for an unscored wallet is an empty Vec — it never panics.
        #[test]
        fn history_for_unscored_wallet_is_empty() {
            let env = Env::default();
            let (client, _admin) = setup(&env);
            let unknown = Address::generate(&env);
            let pair = symbol_short!("XLM_USDC");

            let history = client.get_score_history(&unknown, &pair);
            assert_eq!(history.len(), 0);
        }

        /// Default ring depth is 10.
        #[test]
        fn default_history_depth_is_ten() {
            let env = Env::default();
            let (client, _admin) = setup(&env);
            assert_eq!(client.get_history_max_depth(), 10);
        }

        /// InvalidHistoryDepth is returned for depth=0 or depth>50.
        #[test]
        fn set_history_max_depth_rejects_zero() {
            use ledgerlens_score::Error;

            let env = Env::default();
            let (client, _admin) = setup(&env);

            let result = client.try_set_history_max_depth(&Vec::new(&env), &0);
            assert_eq!(result, Err(Ok(Error::InvalidHistoryDepth)));
        }
    }
}

// ── Governance flow ───────────────────────────────────────────────────────────

/// Propose, inspect, and veto a time-locked contract upgrade.
///
/// Every upgrade is gated behind a mandatory delay (≥ 48 hours) so the
/// community has time to inspect the new WASM hash and veto if needed.
pub mod governance_flow {
    use ledgerlens_score::{LedgerLensScoreContract, LedgerLensScoreContractClient};
    use soroban_sdk::{
        testutils::{Address as _, Ledger as _},
        Address, BytesN, Env, Vec,
    };

    fn setup(env: &Env) -> (LedgerLensScoreContractClient<'_>, Address) {
        env.mock_all_auths();
        env.ledger().with_mut(|l| l.timestamp = 1_700_000_000);
        let id = env.register_contract(None, LedgerLensScoreContract);
        let client = LedgerLensScoreContractClient::new(env, &id);
        let admin = Address::generate(env);
        let service = Address::generate(env);
        client.initialize(&admin, &service);
        (client, admin)
    }

    fn dummy_hash(env: &Env) -> BytesN<32> {
        BytesN::from_array(env, &[0xABu8; 32])
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use ledgerlens_score::Error;

        /// Propose an upgrade, inspect the pending proposal, then veto it.
        /// Demonstrates the full governance flow without actually executing.
        #[test]
        fn propose_inspect_and_veto_upgrade() {
            let env = Env::default();
            let (client, _admin) = setup(&env);
            let hash = dummy_hash(&env);

            // 1. Propose — starts the time-lock.
            client.propose_upgrade(&Vec::new(&env), &hash);

            // 2. Inspect — anyone can read the pending proposal.
            let proposal = client.get_pending_upgrade();
            assert_eq!(proposal.new_wasm_hash, hash);
            // The proposal is not yet executable (time-lock not elapsed).
            assert!(proposal.executable_after > env.ledger().timestamp());

            // 3. Veto — cancels the proposal before the delay elapses.
            client.veto_upgrade(&Vec::new(&env));

            // After veto there is no pending upgrade.
            let result = client.try_get_pending_upgrade();
            assert_eq!(result, Err(Ok(Error::NoPendingUpgrade)));
        }

        /// Attempting to execute before the time-lock elapses returns
        /// `UpgradeNotReady`.
        #[test]
        fn execute_upgrade_before_delay_returns_not_ready() {
            let env = Env::default();
            let (client, _admin) = setup(&env);
            let hash = dummy_hash(&env);

            client.propose_upgrade(&Vec::new(&env), &hash);

            // Try to execute immediately — should be rejected.
            let result = client.try_execute_upgrade(&Vec::new(&env));
            assert_eq!(result, Err(Ok(Error::UpgradeNotReady)));
        }

        /// Proposing a second upgrade while one is pending returns
        /// `UpgradeAlreadyPending`.
        #[test]
        fn double_propose_returns_already_pending() {
            let env = Env::default();
            let (client, _admin) = setup(&env);
            let hash = dummy_hash(&env);

            client.propose_upgrade(&Vec::new(&env), &hash);

            let result = client.try_propose_upgrade(&Vec::new(&env), &hash);
            assert_eq!(result, Err(Ok(Error::UpgradeAlreadyPending)));
        }

        /// Rotate the authorised scoring service address.
        #[test]
        fn rotate_service_address() {
            let env = Env::default();
            let (client, _admin) = setup(&env);

            let new_service = Address::generate(&env);
            client.set_service(&new_service);
            assert_eq!(client.get_service(), new_service);
        }

        /// Set and read back the submission cooldown.
        #[test]
        fn configure_submission_cooldown() {
            let env = Env::default();
            let (client, _admin) = setup(&env);

            let two_hours: u64 = 7_200;
            client.set_cooldown(&Vec::new(&env), &two_hours);
            assert_eq!(client.get_cooldown(), two_hours);
        }
    }
}
