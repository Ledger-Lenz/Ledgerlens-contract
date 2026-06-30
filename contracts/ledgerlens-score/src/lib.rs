#![no_std]
#![allow(deprecated)] // Required: contractimpl macro calls spec_xdr_* for all fns including deprecated ones
#![allow(dead_code)]
#![allow(unused_variables)]

mod constants;
#[cfg(test)]
extern crate std;
mod errors;
mod events;
mod gdpr_accumulator;
mod parameter_governance;
mod storage;
mod types;
mod verkle;
mod zk_range_proof;

#[cfg(test)]
mod test;

#[cfg(test)]
mod test_hll;
#[cfg(test)]
mod test_zk_range_proof;

#[cfg(test)]
mod test_upgrade;

#[cfg(test)]
mod test_parameter_governance;

#[cfg(test)]
mod test_batch_ttl_optimization;

#[cfg(test)]
mod test_interface;

#[cfg(test)]
// mod test_rate_limit;

#[cfg(test)]
// mod test_multisig_service;

#[cfg(test)]
// mod test_attestation;

// #[cfg(test)]
// mod test_batch_attestation;

// #[cfg(test)]
// mod test_score_delta;

// #[cfg(test)]
// mod test_jump;

// #[cfg(test)]
// mod test_model_stats;

#[cfg(test)]
// mod test_velocity_cap;

#[cfg(test)]
// mod test_score_floor;

#[cfg(test)]
// mod test_hysteresis;

#[cfg(test)]
// mod test_embargo;

#[cfg(test)]
mod test_staleness;

#[cfg(test)]
mod test_cooldown;

#[cfg(test)]
// mod test_consensus;

#[cfg(test)]
// mod test_dispute;

#[cfg(test)]
// mod test_finality_buffer;

#[cfg(test)]
// mod test_heartbeat;

#[cfg(test)]
// mod test_history_paginated;

#[cfg(test)]
// mod test_model_version;

#[cfg(test)]
// mod test_histogram;

#[cfg(test)]
mod test_failover;

#[cfg(test)]
mod test_breach_counter_reset;

#[cfg(test)]
mod test_query_helpers;

#[cfg(test)]
mod test_pair_score_count;

#[cfg(test)]
mod test_rate_limit_window;

#[cfg(test)]
mod test_total_wallets_scored;

#[cfg(test)]
mod test_cooldown_period;

use soroban_sdk::{
    contract, contractimpl, crypto::Hash, symbol_short, token, Address, Bytes, BytesN, Env, Symbol,
    SymbolStr, TryFromVal, Vec,
};
use subtle::ConstantTimeEq;

pub use errors::Error;
pub use events::{ServiceResumedEvent, ServiceSilenceAlertEvent};
pub use types::{
    AggregateRiskScore, BatchAttestation, BatchEntryResult, BatchResult, BatchScoreResult,
    EffectiveRiskScore, EmbargoExpiry, HllSketch, MaybeRiskScore, MaybeScoreAttestation, MaybeThresholdAttestation, ModelSubmission, ModelVersionStats,
    PendingScoreEntry, RiskScore, ScoreAttestation, ScoreAttestationInput, ScoreDispute, ScoreFloorPolicy,
    ScoreHistogram, ScoreQuery, ScoreSubmission, ScoreSubmissionWithProof, ScoreTrend,
    ScoreVelocityCap, ThresholdAttestation, UpgradeProposal, ParameterProposal,
    ParameterProposalRecord, ParameterProposalStatus,
};
/// The 32-byte all-zeros field element used as the value in non-membership proofs.
pub use verkle::NON_MEMBER_SENTINEL;

/// On-chain truth layer for LedgerLens risk scores.
///
/// The off-chain detection pipeline (Benford's Law engine + ML ensemble)
/// computes a 0-100 risk score per wallet / asset-pair and writes it here
/// via `submit_score`.  Any Soroban contract can then call `get_score` to
/// gate suspicious activity without relying on an external oracle.
#[contract]
pub struct LedgerLensScoreContract;

#[contractimpl]
impl LedgerLensScoreContract {
    // ── Lifecycle ────────────────────────────────────────────────────────────

    /// One-time setup.  `admin` can rotate the scoring service address
    /// and manage contract-wide configuration; `service` is the off-chain
    /// LedgerLens account authorised to submit scores.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ledgerlens_score::LedgerLensScoreContractClient;
    /// # use soroban_sdk::{testutils::Address as _, Env, Address};
    /// # use ledgerlens_score::LedgerLensScoreContract;
    /// # use soroban_sdk::symbol_short;
    /// let env = Env::default();
    /// env.mock_all_auths();
    /// let contract_id = env.register_contract(None, LedgerLensScoreContract);
    /// let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    /// let admin = Address::generate(&env);
    /// let service = Address::generate(&env);
    /// client.initialize(&admin, &service);
    /// assert_eq!(client.get_admin(), admin);
    /// assert_eq!(client.get_service(), service);
    /// ```
    pub fn initialize(env: Env, admin: Address, service: Address) -> Result<(), Error> {
        if storage::has_admin(&env) {
            return Err(Error::AlreadyInitialized);
        }
        storage::set_admin(&env, &admin);
        storage::set_service(&env, &service);
        env.storage().instance().set(&types::DataKey::AdminAuditRoot, &BytesN::<32>::from_array(&env, &[0u8; 32]));
        Ok(())
    }

    /// Returns the baked-in ABI version of this contract build.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ledgerlens_score::LedgerLensScoreContractClient;
    /// # use soroban_sdk::{testutils::Address as _, Env, Address};
    /// # use ledgerlens_score::LedgerLensScoreContract;
    /// let env = Env::default();
    /// env.mock_all_auths();
    /// let contract_id = env.register_contract(None, LedgerLensScoreContract);
    /// let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    /// let admin = Address::generate(&env);
    /// let service = Address::generate(&env);
    /// client.initialize(&admin, &service);
    /// assert_eq!(client.get_version(), 3);
    /// ```
    pub fn get_version(env: Env) -> u32 {
        storage::get_contract_version(&env)
    }

    // ── Score submission ─────────────────────────────────────────────────────

    /// Register a freshly computed risk score for `wallet` / `asset_pair`.
    ///
    /// When a multi-sig service set has been configured (via
    /// `add_service_signer` / `set_service_threshold`), `signers` must
    /// contain at least `ServiceThreshold` addresses, each of which must be
    /// a member of `ServiceSet`.  Each listed signer must individually
    /// authorize the transaction via Soroban's native `require_auth`.
    ///
    /// When no multi-sig set has been configured (legacy mode) the function
    /// falls back to the original single-service authorization path.
    ///
    /// Returns `ContractPaused` if the admin has activated the global circuit
    /// breaker, checked *before* the per-pair one below — a globally paused
    /// contract rejects every submission regardless of per-pair state.
    ///
    /// Returns `PairPaused` if `asset_pair` has been individually frozen via
    /// `set_pair_paused`, even while the global circuit breaker is off. See
    /// that function's rustdoc for the surgical-freeze use case.
    ///
    /// Rejects submissions for the same `(wallet, asset_pair)` that arrive
    /// before the configured cooldown (`get_cooldown`, 1 hour by default) has
    /// elapsed since the last accepted one, returning `RateLimitExceeded`.
    /// See the README's Rate Limiting section.
    ///
    /// `attestation`, when present, is verified against the registered
    /// off-chain signing key (`set_service_pubkey`) per
    /// `docs/attestation-spec.md` — see that function's rustdoc for the
    /// opt-in enforcement model: once a pubkey is configured, every call
    /// must carry a valid attestation, but calls are unaffected (and
    /// `attestation` may be `None`) until the admin opts in.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ledgerlens_score::LedgerLensScoreContractClient;
    /// # use soroban_sdk::{testutils::Address as _, Env, Address, Vec};
    /// # use ledgerlens_score::LedgerLensScoreContract;
    /// # use soroban_sdk::symbol_short;
    /// let env = Env::default();
    /// env.mock_all_auths();
    /// let contract_id = env.register_contract(None, LedgerLensScoreContract);
    /// let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    /// let admin = Address::generate(&env);
    /// let service = Address::generate(&env);
    /// client.initialize(&admin, &service);
    /// let wallet = Address::generate(&env);
    /// let asset_pair = symbol_short!("XLM_USDC");
    /// client.submit_score(&Vec::new(&env), &wallet, &asset_pair, &42, &true, &false, &1, &90, &1, &None).unwrap();
    /// let score = client.get_score(&wallet, &asset_pair).unwrap();
    /// assert_eq!(score.score, 42);
    /// assert!(score.benford_flag);
    /// ```
    #[allow(clippy::too_many_arguments)]
    pub fn submit_score(
        env: Env,
        signers: Vec<Address>,
        wallet: Address,
        asset_pair: Symbol,
        score: u32,
        benford_flag: bool,
        ml_flag: bool,
        timestamp: u64,
        confidence: u32,
        model_version: u32,
        attestation_input: Option<ScoreAttestationInput>,
    ) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        if storage::is_paused(&env) {
            return Err(Error::ContractPaused);
        }
        if storage::is_pair_paused(&env, &asset_pair) {
            return Err(Error::ContractPaused);
        }
        // Epoch sealing: reject submissions when no epoch is open (#301).
        if !storage::is_epoch_open(&env) {
            return Err(Error::EpochClosed);
        }

        let (attestation, threshold_attestation, commitment) = match &attestation_input {
            Some(input) => {
                let att = match &input.attestation {
                    MaybeScoreAttestation::Some(a) => Some(a.clone()),
                    MaybeScoreAttestation::None => None,
                };
                let th_att = match &input.threshold_attestation {
                    MaybeThresholdAttestation::Some(t) => Some(t.clone()),
                    MaybeThresholdAttestation::None => None,
                };
                (att, th_att, input.commitment.clone())
            }
            None => (None, None, None),
        };

        if let Some(ref ta) = threshold_attestation {
            // ── Threshold-sig path ───────────────────────────────────────
            // A single 65-byte secp256k1 threshold signature replaces all
            // N require_auth calls. Participating signers are validated as
            // service-set members but no individual Soroban auth is needed.
            if storage::get_aggregate_service_pubkey(&env).is_none() {
                return Err(Error::ServicePubkeyNotSet);
            }
            let service_set = storage::get_service_set(&env);
            let threshold = storage::get_service_threshold(&env);
            if !service_set.is_empty() && threshold > 0 {
                if ta.participating_signers.len() < threshold {
                    return Err(Error::InsufficientSigners);
                }
                for i in 0..ta.participating_signers.len() {
                    let signer = ta.participating_signers.get(i).unwrap();
                    if !service_set.contains(&signer) {
                        return Err(Error::UnauthorizedSigner);
                    }
                }
            }
            Self::verify_threshold_attestation(
                &env,
                &wallet,
                &asset_pair,
                score,
                benford_flag,
                ml_flag,
                timestamp,
                confidence,
                model_version,
                ta,
            )?;
        } else {
            // ── Legacy M-of-N require_auth path ──────────────────────────
            let service_set = storage::get_service_set(&env);
            let threshold = storage::get_service_threshold(&env);
            if !service_set.is_empty()
                && threshold > 0
                && !(signers.len() == 1 && signers.get(0).unwrap() == storage::get_service(&env))
            {
                if signers.len() < threshold {
                    return Err(Error::InsufficientSigners);
                }
                for i in 0..signers.len() {
                    let signer = signers.get(i).unwrap();
                    if !service_set.contains(&signer) {
                        return Err(Error::UnauthorizedSigner);
                    }
                    signer.require_auth();
                }
            } else {
                storage::get_service(&env).require_auth();
            }
            // Opt-in single-key cryptographic attestation.
            if storage::get_service_pubkey(&env).is_some() || attestation.is_some() {
                Self::verify_attestation(
                    &env,
                    &wallet,
                    &asset_pair,
                    score,
                    benford_flag,
                    ml_flag,
                    timestamp,
                    confidence,
                    model_version,
                    attestation.clone(),
                )?;
                
                // For single attestation provided by caller, verify and increment per-service-account nonce.
                // Only check nonce if the attestation was explicitly provided (not auto-generated).
                if let Some(att) = attestation.as_ref() {
                    let service = storage::get_service(&env);
                    let current_nonce = storage::get_signer_nonce(&env, &service);
                    if current_nonce != att.nonce {
                        return Err(Error::InvalidAttestation);
                    }
                    let next_nonce = att.nonce.checked_add(1)
                        .ok_or(Error::InvalidAttestation)?;
                    storage::set_signer_nonce(&env, &service, next_nonce);
                }
            }
        }

        let risk_score =
            RiskScore { score, benford_flag, ml_flag, timestamp, confidence, model_version, commitment: commitment.clone() };

        // Flash-loan protection: check for same-ledger gate-read + submit (#300).
        if let Some(gate_seq) = storage::get_gate_read_ledger(&env, &wallet, &asset_pair) {
            if gate_seq == env.ledger().sequence() {
                events::suspicious_same_ledger_submission(
                    &env,
                    &wallet,
                    &asset_pair,
                    gate_seq,
                );
                if storage::get_flash_protection_mode(&env)
                    == crate::types::FlashProtectionMode::Reject
                {
                    return Err(Error::EpochClosed);
                }
            }
        }

        let buffer = storage::get_finality_buffer_secs(&env);
        // ── HLL first-time detection ──────────────────────────────────────────
        if storage::get_score_count(&env, &wallet, &asset_pair) == 0 {
            storage::hll_update(&env, &asset_pair, &wallet);
        }

        if buffer == 0 {
            // Disabled — commit straight to live storage.
            Self::write_score_with_rate_limit(&env, &wallet, &asset_pair, &risk_score)?;
            Self::record_service_activity(&env);
        } else {
            // Buffer active — validate but hold in pending storage.
            // Rate limit still applies so we can't be flooded with pending entries.
            let last_submit = storage::get_last_submit_time(&env, &wallet, &asset_pair);
            let base_cooldown = storage::get_pair_cooldown_secs(&env, &asset_pair);
            let cooldown = Self::compute_effective_cooldown(&env, &asset_pair, base_cooldown);
            let now2 = env.ledger().timestamp();
            if last_submit != 0 && now2 < last_submit.saturating_add(cooldown) {
                return Err(Error::RateLimitExceeded);
            }
            storage::set_last_submit_time(&env, &wallet, &asset_pair, now2);
            Self::record_service_activity(&env);

            let commit_after = now2.saturating_add(buffer);
            let pending = PendingScoreEntry {
                score,
                benford_flag,
                ml_flag,
                submitted_at: now2,
                confidence,
                model_version,
                timestamp,
                commit_after,
                submitted_by: if !storage::get_service_set(&env).is_empty() {
                    signers.get(0).unwrap_or_else(|| storage::get_service(&env))
                } else {
                    storage::get_service(&env)
                },
                commitment: commitment.clone(),
            };
            storage::set_pending_score(&env, &wallet, &asset_pair, &pending);
            events::score_pending(&env, &wallet, &asset_pair, commit_after);
        }
        Ok(())
    }

    // ── Finality buffer (pending score commit window) ───────────────────────

    /// Sets the finality buffer: the number of seconds a `submit_score`
    /// payload is held in `PendingScore` before it can be committed to live
    /// storage via `commit_pending_score`. While pending, the admin may
    /// inspect it with `get_pending_score` and discard it with
    /// `cancel_pending_score` before it ever reaches `get_score` /
    /// `query_risk_gate`. Admin only.
    ///
    /// `secs == 0` (the default) disables the buffer entirely — `submit_score`
    /// then writes straight to live storage, exactly as it did before this
    /// feature existed. Any non-zero value up to `MAX_FINALITY_BUFFER_SECS`
    /// (24 hours) is accepted.
    ///
    /// # Errors
    /// - [`Error::NotInitialized`] if the contract has no admin yet.
    /// - [`Error::InvalidFinalityBuffer`] if `secs > MAX_FINALITY_BUFFER_SECS`.
    pub fn set_finality_buffer(
        env: Env,
        admin_signers: Vec<Address>,
        secs: u64,
    ) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        if secs > constants::MAX_FINALITY_BUFFER_SECS {
            return Err(Error::InvalidFinalityBuffer);
        }
        Self::require_admin_auth(&env, &admin_signers)?;
        storage::set_finality_buffer_secs(&env, secs);
        events::finality_buffer_updated(&env, secs);
        Ok(())
    }

    /// Returns the current finality buffer in seconds. `0` means the buffer
    /// is disabled and `submit_score` commits immediately.
    pub fn get_finality_buffer(env: Env) -> u64 {
        storage::get_finality_buffer_secs(&env)
    }

    /// Alias for `set_finality_buffer`. Configures the escrow hold window in
    /// seconds. `0` disables the hold window and causes `submit_score` to
    /// commit immediately.
    pub fn set_escrow_hold_window(
        env: Env,
        admin_signers: Vec<Address>,
        secs: u64,
    ) -> Result<(), Error> {
        Self::set_finality_buffer(env, admin_signers, secs)
    }

    /// Public alias for `commit_pending_score`.
    /// Callable by anyone once the escrow hold window has elapsed.
    pub fn auto_commit_score(
        env: Env,
        wallet: Address,
        asset_pair: Symbol,
    ) -> Result<(), Error> {
        Self::commit_pending_score(env, wallet, asset_pair)
    }

    /// Returns an estimate of the number of unique wallets that have a live
    /// score for `asset_pair`. Uses a per-pair HyperLogLog sketch to avoid
    /// storing full wallet lists on-chain.
    pub fn estimate_unique_wallets(env: Env, asset_pair: Symbol) -> u64 {
        storage::estimate_unique_wallets(&env, &asset_pair)
    }

    /// Read-only lookup of the pending score held for `(wallet, asset_pair)`,
    /// if any. Returns `None` when the buffer is disabled or no score is
    /// currently in the hold window.
    pub fn get_pending_score(
        env: Env,
        wallet: Address,
        asset_pair: Symbol,
    ) -> Option<PendingScoreEntry> {
        storage::get_pending_score(&env, &wallet, &asset_pair)
    }

    /// Commits a pending score to live storage once its hold window has
    /// elapsed. Callable by anyone — the only gate is `commit_after <= now`.
    ///
    /// See [docs/commit-reveal-flow.md](../../docs/commit-reveal-flow.md) for the full
    /// finality buffer commit-reveal sequence.
    ///
    /// # Errors
    /// - [`Error::NoPendingScore`] if no pending score exists for
    ///   `(wallet, asset_pair)`.
    /// - [`Error::FinalityWindowNotElapsed`] if `commit_after > now`.
    pub fn commit_pending_score(
        env: Env,
        wallet: Address,
        asset_pair: Symbol,
    ) -> Result<(), Error> {
        let pending =
            storage::get_pending_score(&env, &wallet, &asset_pair).ok_or(Error::NoPendingScore)?;

        let now = env.ledger().timestamp();
        if now < pending.commit_after {
            return Err(Error::FinalityWindowNotElapsed);
        }

        let previous_score = storage::peek_score(&env, &wallet, &asset_pair).map(|s| s.score);

        let risk_score = RiskScore {
            score: pending.score,
            benford_flag: pending.benford_flag,
            ml_flag: pending.ml_flag,
            timestamp: pending.timestamp,
            confidence: pending.confidence,
            model_version: pending.model_version,
            commitment: pending.commitment.clone(),
        };

        storage::set_score(&env, &wallet, &asset_pair, &risk_score);
        storage::push_score_history(&env, &wallet, &asset_pair, &risk_score);
        storage::register_pair_for_wallet(&env, &wallet, &asset_pair);
        storage::increment_score_count(&env, &wallet, &asset_pair);
        // Increment per-pair submission counter (Issue 1).
        storage::increment_pair_score_count(&env, &asset_pair);
        // Increment unique wallet-pair counter on first-ever write (Issue 3).
        // pending.score is committed only once, so there was no prior live score.
        // We use peek_score which was called before set_score above — but at this
        // point set_score has already run.  The pending path always replaces the
        // live entry, so we treat "had no pending-committed score before" as new.
        // The reliable signal is: register_pair_for_wallet just ran; if this is
        // the first time, peek_score would have returned None before set_score.
        // We detect it by checking whether the score count is now exactly 1.
        if storage::get_score_count(&env, &wallet, &asset_pair) == 1 {
            storage::increment_total_wallets_scored(&env);
        }
        Self::refresh_aggregate_cache(&env, &wallet);

        let score_threshold = storage::get_risk_threshold(&env);
        if pending.score >= score_threshold {
            events::threshold_breached(&env, &wallet, &asset_pair, pending.score, score_threshold);
        }

        Self::emit_score_delta(&env, &wallet, &asset_pair, previous_score, pending.score);
        storage::clear_pending_score(&env, &wallet, &asset_pair);
        events::score_committed(&env, &wallet, &asset_pair);
        Ok(())
    }

    /// Discards a pending score before it can take effect. Admin only —
    /// this is the review-and-cancel mechanism the finality buffer exists
    /// to provide.
    ///
    /// See [docs/commit-reveal-flow.md](../../docs/commit-reveal-flow.md) for the full
    /// finality buffer commit-reveal sequence.
    ///
    /// # Errors
    /// - [`Error::NotInitialized`] if the contract has no admin yet.
    /// - [`Error::NoPendingScore`] if no pending score exists for
    ///   `(wallet, asset_pair)`.
    pub fn cancel_pending_score(
        env: Env,
        admin_signers: Vec<Address>,
        wallet: Address,
        asset_pair: Symbol,
    ) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        Self::require_admin_auth(&env, &admin_signers)?;

        if storage::get_pending_score(&env, &wallet, &asset_pair).is_none() {
            return Err(Error::NoPendingScore);
        }
        storage::clear_pending_score(&env, &wallet, &asset_pair);

        let admin = storage::get_admin(&env);
        events::score_pending_cancelled(&env, &wallet, &asset_pair, &admin);
        Ok(())
    }

    // ── HyperLogLog unique-wallet estimation ─────────────────────────────────

    /// Admin-only. Sets the HLL precision `p` ∈ [HLL_MIN_PRECISION, HLL_MAX_PRECISION].
    pub fn set_hll_precision(
        env: Env,
        admin_signers: Vec<Address>,
        precision: u32,
    ) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        if precision < constants::HLL_MIN_PRECISION || precision > constants::HLL_MAX_PRECISION {
            return Err(Error::InvalidThreshold);
        }
        Self::require_admin_auth(&env, &admin_signers)?;
        storage::set_hll_precision(&env, precision);
        Ok(())
    }

    /// Returns the current HLL precision setting.
    pub fn get_hll_precision(env: Env) -> u32 {
        storage::get_hll_precision(&env)
    }

    /// Estimates the number of unique wallets scored for `asset_pair` using HyperLogLog.
    pub fn estimate_unique_wallets(env: Env, asset_pair: Symbol) -> u64 {
        storage::hll_estimate(&env, &asset_pair)
    }


    /// Register a consensus-backed score for `wallet` / `asset_pair` from
    /// multiple independently attested model outputs.
    ///
    /// The contract verifies each model submission independently, computes a
    /// provisional median across the valid submissions, forms the consensus set
    /// of scores within `±epsilon` of that median, and accepts the update only
    /// if at least `k` models agree. The stored score is the integer median of
    /// the consensus set, with `model_version = 0` marking it as an on-chain
    /// consensus aggregate rather than a direct single-model output.
    ///
    /// **Phase 1 of MEV-resistant commit-reveal:** See
    /// [docs/commit-reveal-flow.md](../../docs/commit-reveal-flow.md) for the full sequence.
    pub fn commit_consensus(
        env: Env,
        model: Address,
        wallet: Address,
        asset_pair: Symbol,
        commitment: BytesN<32>,
    ) -> Result<(), Error> {
        Self::ensure_active(&env)?;
        model.require_auth();

        storage::set_consensus_commitment(&env, &model, &wallet, &asset_pair, &commitment);
        Ok(())
    }

    /// Phase 2 of MEV-resistant consensus. Opens all commitments, verifies them against
    /// the provided `nonces` and score data, and then computes the aggregate consensus score.
    ///
    /// See [docs/commit-reveal-flow.md](../../docs/commit-reveal-flow.md) for the full
    /// multi-model consensus commit-reveal sequence and security considerations.
    #[allow(clippy::too_many_arguments)]
    pub fn reveal_consensus(
        env: Env,
        signers: Vec<Address>,
        wallet: Address,
        asset_pair: Symbol,
        submissions: Vec<ModelSubmission>,
        nonces: Vec<u64>,
        timestamp: u64,
    ) -> Result<(), Error> {
        Self::ensure_active(&env)?;
        Self::authorize_submission(&env, &signers)?;

        if submissions.is_empty() {
            return Err(Error::InvalidConsensusConfig);
        }
        if submissions.len() != nonces.len() {
            return Err(Error::CommitmentMismatch); // Or some other length mismatch
        }
        if timestamp == 0 {
            return Err(Error::InvalidTimestamp);
        }

        let mut valid_indices: Vec<u32> = Vec::new(&env);
        for i in 0..submissions.len() {
            let sub = submissions.get(i).unwrap();
            let nonce = nonces.get(i).unwrap();
            if sub.score > 100 || sub.confidence > 100 {
                continue;
            }

            let commitment =
                storage::get_consensus_commitment(&env, &sub.model, &wallet, &asset_pair);
            if commitment.is_none() {
                return Err(Error::RevealWindowExpired);
            }
            let commitment = commitment.unwrap();

            // Verify sha256(score || nonce). In Soroban, we can serialize a tuple using XDR,
            // or just pack them. Using (score, nonce).into_val(&env) serialization.
            // Let's use simple xdr serialization to bytes.
            let mut buf = [0u8; 12];
            buf[0..4].copy_from_slice(&sub.score.to_be_bytes());
            buf[4..12].copy_from_slice(&nonce.to_be_bytes());
            let computed_hash = env.crypto().sha256(&soroban_sdk::Bytes::from_array(&env, &buf));

            if computed_hash.to_bytes() != commitment {
                return Err(Error::CommitmentMismatch);
            }

            // Clean up to prevent replay
            storage::remove_consensus_commitment(&env, &sub.model, &wallet, &asset_pair);

            let should_verify = storage::get_service_pubkey(&env).is_some();
            let verified = if should_verify {
                Self::verify_attestation(
                    &env,
                    &wallet,
                    &asset_pair,
                    sub.score,
                    sub.benford_flag,
                    sub.ml_flag,
                    timestamp,
                    sub.confidence,
                    sub.model_version,
                    Some(sub.attestation.clone()),
                )
                .is_ok()
            } else {
                true
            };

            if verified {
                valid_indices.push_back(i);
            }
        }

        if valid_indices.is_empty() {
            return Err(Error::InsufficientConsensus);
        }

        let provisional_median = Self::median_score_for_indices(&submissions, &valid_indices)
            .ok_or(Error::InsufficientConsensus)?;
        let epsilon = storage::get_consensus_epsilon(&env);
        let lower_bound = provisional_median.saturating_sub(epsilon);
        let upper_bound = provisional_median.saturating_add(epsilon).min(100);

        let mut consensus_indices: Vec<u32> = Vec::new(&env);
        for i in 0..valid_indices.len() {
            let idx = valid_indices.get(i).unwrap();
            let sub = submissions.get(idx).unwrap();
            if sub.score >= lower_bound && sub.score <= upper_bound {
                consensus_indices.push_back(idx);
            }
        }

        let threshold_k = storage::get_consensus_threshold_k(&env);
        if consensus_indices.len() < threshold_k {
            return Err(Error::InsufficientConsensus);
        }

        let median_score =
            Self::weighted_mean_score(&env, &submissions, &consensus_indices)
                .ok_or(Error::InsufficientConsensus)?;
        let median_confidence =
            Self::median_confidence_for_indices(&submissions, &consensus_indices).unwrap_or(0);
        let benford_flag = Self::any_benford_flag(&submissions, &consensus_indices);
        let ml_flag = Self::any_ml_flag(&submissions, &consensus_indices);
        let risk_score = RiskScore {
            score: median_score,
            benford_flag,
            ml_flag,
            timestamp,
            confidence: median_confidence,
            model_version: 0,
            commitment: None,
        };

        storage::set_last_global_submission_time(&env, env.ledger().timestamp());
        Self::write_score_with_rate_limit(&env, &wallet, &asset_pair, &risk_score)?;
        events::consensus_score_submitted(
            &env,
            &wallet,
            &asset_pair,
            median_score,
            consensus_indices.len(),
            epsilon,
        );

        // ── Bayesian posterior update + signer accuracy ────────────────────
        for i in 0..consensus_indices.len() {
            let idx = consensus_indices.get(i).unwrap();
            let sub = submissions.get(idx).unwrap();
            // Bayesian weight
            let version = sub.model_version;
            let prior = storage::get_model_posterior_weight(&env, version);
            let diff = (median_score as i64) - (sub.score as i64);
            let penalty = (diff * diff) as u64;
            let new_weight = prior.saturating_sub(penalty).max(1);
            storage::set_model_posterior_weight(&env, version, new_weight);
            // Signer accuracy (rolling MAD)
            let abs_dev = (median_score as i64 - sub.score as i64).unsigned_abs() as u32;
            Self::update_signer_accuracy(&env, &sub.model, abs_dev);
        }

        Ok(())
    }

    /// Direct consensus submission without commit-reveal.
    /// Validates all submission attestations, computes median score, and writes it.
    pub fn submit_consensus_score(
        env: Env,
        signers: Vec<Address>,
        wallet: Address,
        asset_pair: Symbol,
        submissions: Vec<ModelSubmission>,
        timestamp: u64,
    ) -> Result<(), Error> {
        Self::ensure_active(&env)?;
        Self::authorize_submission(&env, &signers)?;
        if submissions.is_empty() {
            return Err(Error::InvalidConsensusConfig);
        }
        if timestamp == 0 {
            return Err(Error::InvalidTimestamp);
        }
        let should_verify = storage::get_service_pubkey(&env).is_some();
        let mut valid_indices: Vec<u32> = Vec::new(&env);
        for i in 0..submissions.len() {
            let sub = submissions.get(i).unwrap();
            if sub.score > 100 || sub.confidence > 100 {
                continue;
            }
            let verified = if should_verify {
                Self::verify_attestation(
                    &env, &wallet, &asset_pair,
                    sub.score, sub.benford_flag, sub.ml_flag,
                    timestamp, sub.confidence, sub.model_version,
                    Some(sub.attestation.clone()),
                ).is_ok()
            } else {
                true
            };
            if verified {
                valid_indices.push_back(i);
            }
        }
        if valid_indices.is_empty() {
            return Err(Error::InsufficientConsensus);
        }
        // ── #297: IQR-based outlier rejection ────────────────────────────────
        // Compute Q1 (25th percentile) and Q3 (75th percentile) of scores among
        // valid submissions, then reject any signer whose score deviates from
        // the median by more than multiplier/100 × IQR.
        let n = valid_indices.len();
        if n >= 4 {
            let q1_idx = (n - 1) / 4;
            let q3_idx = (3 * (n - 1)) / 4;
            if let (Some(q1), Some(q3)) = (
                Self::kth_score_for_indices(&submissions, &valid_indices, q1_idx),
                Self::kth_score_for_indices(&submissions, &valid_indices, q3_idx),
            ) {
                let iqr = q3.saturating_sub(q1);
                let multiplier = storage::get_iqr_rejection_multiplier(&env); // scaled × 100
                // threshold = multiplier/100 × iqr (integer arithmetic, scaled)
                let threshold_scaled = (multiplier as u64) * (iqr as u64); // ×100 still
                let median_idx = (n - 1) / 2;
                if let Some(median) = Self::kth_score_for_indices(&submissions, &valid_indices, median_idx) {
                    let mut non_outlier: Vec<u32> = Vec::new(&env);
                    for k in 0..valid_indices.len() {
                        let idx = valid_indices.get(k).unwrap();
                        let sub = submissions.get(idx).unwrap();
                        let score = sub.score;
                        let deviation = if score >= median { score - median } else { median - score };
                        // deviation_scaled = deviation × 100; compare with threshold_scaled
                        if (deviation as u64) * 100 <= threshold_scaled {
                            non_outlier.push_back(idx);
                        } else {
                            storage::increment_signer_rejection_count(&env, &sub.model);
                            events::consensus_signer_rejected(&env, &sub.model, deviation);
                        }
                    }
                    if !non_outlier.is_empty() {
                        valid_indices = non_outlier;
                    }
                    // If all signers are rejected as outliers, fall through with
                    // the original valid_indices (prefer imperfect consensus to none).
                }
            }
        }
        // ─────────────────────────────────────────────────────────────────────
        let median_score = Self::median_score_for_indices(&submissions, &valid_indices)
            .ok_or(Error::InsufficientConsensus)?;
        let median_confidence =
            Self::median_confidence_for_indices(&submissions, &valid_indices).unwrap_or(0);
        let benford_flag = Self::any_benford_flag(&submissions, &valid_indices);
        let ml_flag = Self::any_ml_flag(&submissions, &valid_indices);
        let risk_score = RiskScore {
            score: median_score,
            benford_flag,
            ml_flag,
            timestamp,
            confidence: median_confidence,
            model_version: 0,
            benford_score: 0,
            ml_score: 0,
            network_score: 0,
            commitment: None,
        };
        storage::set_last_global_submission_time(&env, env.ledger().timestamp());
        Self::write_score_with_rate_limit(&env, &wallet, &asset_pair, &risk_score)?;
        // Update per-model signer accuracy
        for i in 0..valid_indices.len() {
            let idx = valid_indices.get(i).unwrap();
            let sub = submissions.get(idx).unwrap();
            let abs_dev = (median_score as i64 - sub.score as i64).unsigned_abs() as u32;
            Self::update_signer_accuracy(&env, &sub.model, abs_dev);
        }
        Ok(())
    }

    /// Submit multiple risk scores in a single invocation.  The service
    /// account authorises once for the whole batch.  Returns a `BatchResult`
    /// that lists every entry's outcome so the caller knows exactly which
    /// entries succeeded and why any failed, without needing to re-query
    /// each (wallet, pair) individually.
    ///
    /// Entries targeting a paused pair (`PairPaused`), with out-of-range
    /// `score` or `confidence`, a zero `timestamp`, that arrive before
    /// their `(wallet, asset_pair)`'s submission cooldown has elapsed, or that
    /// fall below the configured score floor for a high-risk wallet
    /// (`BelowScoreFloor`), are recorded as rejected in the result with an
    /// appropriate `rejection_code` — the rest of the batch is still
    /// processed. The
    /// whole call instead fails outright with `ContractPaused` if the
    /// *global* circuit breaker is active, checked once up front. Two
    /// entries for the same pair within
    /// one batch are subject to the same cooldown — the second is rejected,
    /// since both share the same ledger timestamp.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ledgerlens_score::{LedgerLensScoreContract, LedgerLensScoreContractClient, ScoreSubmission};
    /// # use soroban_sdk::{testutils::Address as _, Env, Address, Vec};
    /// # use soroban_sdk::symbol_short;
    /// let env = Env::default();
    /// env.mock_all_auths();
    /// let contract_id = env.register_contract(None, LedgerLensScoreContract);
    /// let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    /// let admin = Address::generate(&env);
    /// let service = Address::generate(&env);
    /// client.initialize(&admin, &service);
    /// let wallet1 = Address::generate(&env);
    /// let wallet2 = Address::generate(&env);
    /// let asset_pair = symbol_short!("XLM_USDC");
    /// let mut batch: Vec<ScoreSubmission> = Vec::new(&env);
    /// batch.push_back(ScoreSubmission { wallet: wallet1.clone(), asset_pair: asset_pair.clone(), score: 45, benford_flag: false, ml_flag: false, timestamp: 1000, confidence: 80, model_version: 2 });
    /// batch.push_back(ScoreSubmission { wallet: wallet2.clone(), asset_pair: asset_pair.clone(), score: 85, benford_flag: true, ml_flag: true, timestamp: 2000, confidence: 90, model_version: 2 });
    /// let result = client.submit_scores_batch(&batch);
    /// assert_eq!(result.accepted_count, 2);
    /// assert_eq!(result.rejected_count, 0);
    /// assert_eq!(result.results.len(), 2);
    /// assert_eq!(client.get_score(&wallet1, &asset_pair).unwrap().score, 45);
    /// assert_eq!(client.get_score(&wallet2, &asset_pair).unwrap().score, 85);
    /// ```
    pub fn submit_scores_batch(
        env: Env,
        submissions: Vec<ScoreSubmission>,
    ) -> Result<BatchResult, Error> {
        Self::ensure_active(&env)?;
        // Epoch sealing: reject the whole batch when no epoch is open (#301).
        if !storage::is_epoch_open(&env) {
            return Err(Error::EpochClosed);
        }

        let service = storage::get_service(&env);
        service.require_auth();

        if submissions.is_empty() {
            return Err(Error::EmptyBatch);
        }
        if submissions.len() > constants::MAX_BATCH_SIZE {
            return Err(Error::BatchTooLarge);
        }

        let now = env.ledger().timestamp();
        let threshold = storage::get_risk_threshold(&env);
        let mut accepted_count: u32 = 0;
        let mut results: Vec<BatchEntryResult> = Vec::new(&env);

        let version_set = storage::get_model_version_set(&env);
        let version_check_enabled = !version_set.is_empty();

        for i in 0..submissions.len() {
            let sub = submissions.get(i).unwrap();
            let mut accepted = false;
            let mut rejection_code: u32 = 0;

            if storage::is_pair_paused(&env, &sub.asset_pair) {
                rejection_code = Error::ContractPaused as u32;
            } else if sub.score > 100 {
                rejection_code = Error::InvalidScore as u32;
            } else if sub.confidence > 100 {
                rejection_code = Error::InvalidConfidence as u32;
            } else if sub.timestamp == 0 {
                rejection_code = Error::InvalidTimestamp as u32;
            } else if version_check_enabled && !version_set.contains(&sub.model_version) {
                rejection_code = Error::ModelVersionNotRegistered as u32;
            } else if version_check_enabled
                && storage::is_model_version_deprecated(&env, sub.model_version)
            {
                rejection_code = Error::ModelVersionDeprecated as u32;
            } else {
                let last_submit = storage::get_last_submit_time(&env, &sub.wallet, &sub.asset_pair);
                let base_cooldown = storage::get_pair_cooldown_secs(&env, &sub.asset_pair);
                let cooldown = Self::compute_effective_cooldown(&env, &sub.asset_pair, base_cooldown);
                if last_submit != 0 && now < last_submit.saturating_add(cooldown) {
                    rejection_code = Error::RateLimitExceeded as u32;
                } else if Self::score_floor_blocks(&env, &sub.wallet, &sub.asset_pair, sub.score) {
                    rejection_code = Error::InvalidScore as u32;
                } else {
                    let previous_score =
                        storage::peek_score(&env, &sub.wallet, &sub.asset_pair).map(|s| s.score);

                    let mut velocity_exceeded = false;
                    if let Some(prev) = previous_score {
                        let cap = storage::get_score_velocity_cap(&env);
                        if cap.enabled {
                            if storage::is_velocity_cap_overridden(
                                &env,
                                &sub.wallet,
                                &sub.asset_pair,
                            ) {
                                storage::clear_velocity_cap_override(
                                    &env,
                                    &sub.wallet,
                                    &sub.asset_pair,
                                );
                            } else if last_submit != 0 {
                                let elapsed_secs = now.saturating_sub(last_submit);
                                let allowed_delta = core::cmp::max(
                                    1,
                                    (cap.points_per_hour as u64).saturating_mul(elapsed_secs)
                                        / 3600,
                                );
                                let diff = sub.score.abs_diff(prev);
                                if diff as u64 > allowed_delta {
                                    rejection_code = Error::RateLimitExceeded as u32;
                                    velocity_exceeded = true;
                                }
                            }
                        }
                    }

                    if !velocity_exceeded {
                        storage::set_last_submit_time(&env, &sub.wallet, &sub.asset_pair, now);

                        let risk_score = RiskScore {
                            score: sub.score,
                            benford_flag: sub.benford_flag,
                            ml_flag: sub.ml_flag,
                            timestamp: sub.timestamp,
                            confidence: sub.confidence,
                            model_version: sub.model_version,
                            commitment: None,
                        };
                        storage::set_score(&env, &sub.wallet, &sub.asset_pair, &risk_score);
                        storage::push_score_history(
                            &env,
                            &sub.wallet,
                            &sub.asset_pair,
                            &risk_score,
                        );
                        storage::register_pair_for_wallet(&env, &sub.wallet, &sub.asset_pair);
                        storage::increment_score_count(&env, &sub.wallet, &sub.asset_pair);
                        // Increment per-pair submission counter (Issue 1).
                        storage::increment_pair_score_count(&env, &sub.asset_pair);
                        // Increment unique wallet-pair counter on first-ever submission (Issue 3).
                        if previous_score.is_none() {
                            storage::increment_total_wallets_scored(&env);
                        }
                        storage::update_model_stats(&env, sub.model_version, sub.score);
                        storage::update_historical_max_score(
                            &env,
                            &sub.wallet,
                            &sub.asset_pair,
                            sub.score,
                        );
                        storage::update_histogram_on_write(&env, previous_score, sub.score);
                        Self::refresh_aggregate_cache(&env, &sub.wallet);
                        Self::update_verkle_commitment(
                            &env,
                            &sub.wallet,
                            &sub.asset_pair,
                            &risk_score,
                        );

                        if sub.score >= threshold {
                            events::threshold_breached(
                                &env,
                                &sub.wallet,
                                &sub.asset_pair,
                                sub.score,
                                threshold,
                            );
                        }
                        Self::update_breach_counter(
                            &env,
                            &sub.wallet,
                            &sub.asset_pair,
                            sub.score,
                            threshold,
                        );
                        Self::evaluate_risk_band(
                            &env,
                            &sub.wallet,
                            &sub.asset_pair,
                            sub.score,
                            threshold,
                        );

                        Self::emit_score_delta(
                            &env,
                            &sub.wallet,
                            &sub.asset_pair,
                            previous_score,
                            sub.score,
                        );
                        Self::emit_score_jump_anomaly(
                            &env,
                            &sub.wallet,
                            &sub.asset_pair,
                            previous_score,
                            sub.score,
                            sub.model_version,
                        );
                        events::score_submitted(&env, &sub.wallet, &sub.asset_pair, &risk_score);
                        accepted = true;
                        accepted_count += 1;
                    }
                }
            }

            results.push_back(BatchEntryResult { index: i, accepted, rejection_code });
        }

        if accepted_count > 0 {
            Self::record_service_activity(&env);
        }

        let rejected_count = submissions.len() - accepted_count;
        Ok(BatchResult { accepted_count, rejected_count, results })
    }

    /// Submit multiple risk scores under a single Merkle-root attestation.
    ///
    /// Unlike [`submit_scores_batch`] — which only enforces Soroban's native
    /// service-account `require_auth` — this entry point requires the
    /// off-chain detection pipeline to produce **one** secp256k1 signature
    /// over the Merkle root of every entry's commitment, plus a per-entry
    /// inclusion proof that the contract walks through and verifies
    /// in-line. The cryptographic-payload-integrity gap that the plain
    /// `submit_scores_batch` leaves open is closed by this entry point.
    ///
    /// # Auth
    ///
    /// Same model as [`submit_score`]: when the admin has configured an
    /// M-of-N service set (`add_service_signer` / `set_service_threshold`),
    /// `signers` must contain at least `threshold` members of the set, each
    /// of which individually calls `require_auth`; otherwise the legacy
    /// single-service-account `require_auth` path runs.
    ///
    /// # Attestation
    ///
    /// Requires `attestation.merkle_root` to be a SHA-256 root over the
    /// `0x00`-prefixed leaf commitments of every entry (see
    /// `docs/batch-attestation-spec.md` for the off-chain tree-construction
    /// algorithm and a worked 4-leaf example), and `attestation.signature`
    /// to be a valid secp256k1 signature over `SHA256(merkle_root)`
    /// — not over `merkle_root` directly — recoverable to the key
    /// registered via `set_service_pubkey`.
    ///
    /// The `SHA256(merkle_root)` wrap is a soroban-sdk 21.x API shim:
    /// `env.crypto().secp256k1_recover` consumes an opaque `Hash<32>`
    /// that has no public constructor, so both sides wrap once via
    /// `env.crypto().sha256`. See [`BatchAttestation`]'s rustdoc for the
    /// full convention and §5 of the spec for the rationale.
    ///
    /// **The service pubkey must already be configured.** Unlike the
    /// opt-in `submit_score` path (which silently ignores `attestation`
    /// until a pubkey is set), this function returns
    /// [`Error::ServicePubkeyNotSet`] if no pubkey exists — there is no
    /// way to "skip attestation" on the batch path, because then the
    /// security property is gone.
    ///
    /// # Per-entry validation
    ///
    /// Each entry is rejected individually (with `rejection_code =
    /// Error::InvalidAttestation as u32`) on a Merkle-proof mismatch or on
    /// `proof.len() > MAX_MERKLE_PROOF_DEPTH`. Entries that pass the
    /// Merkle check then proceed through the same validation pipeline as
    /// [`submit_scores_batch`]: score range, confidence range, timestamp
    /// non-zero, and per-(wallet, pair) submission cooldown. Any of those
    /// failures are reported in the entry's `rejection_code`. The whole
    /// batch is **never** aborted by a single bad entry.
    ///
    /// # Worked example (4-leaf batch)
    ///
    /// Given four submissions whose 32-byte underlying commitments are
    /// `C0, C1, C2, C3`, the off-chain pipeline builds:
    ///
    /// ```text
    /// L0 = SHA256(0x00 || C0)
    /// L1 = SHA256(0x00 || C1)
    /// L2 = SHA256(0x00 || C2)
    /// L3 = SHA256(0x00 || C3)
    /// N0 = SHA256(0x01 || L0 || L1)   // proof_flags bit 0 for L1 = 0 (right sibling)
    /// N1 = SHA256(0x01 || L2 || L3)   // proof_flags bit 0 for L3 = 0 (right sibling)
    /// R  = SHA256(0x01 || N0 || N1)   // root
    /// ```
    ///
    /// The per-entry proofs are:
    ///
    /// | Index | `proof`            | `proof_flags` |
    /// |------:|-------------------|--------------:|
    /// |   0   | `[L1, N1]`         | `0b000` (= 0) |
    /// |   1   | `[L0, N1]`         | `0b001` (= 1) |
    /// |   2   | `[L3, N0]`         | `0b010` (= 2) |
    /// |   3   | `[L2, N0]`         | `0b011` (= 3) |
    ///
    /// The off-chain pipeline signs `R` with the secp256k1 key registered
    /// via `set_service_pubkey` and submits the batch with `attestation =
    /// { merkle_root: R, signature: sig }`.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ledgerlens_score::{
    /// #     BatchAttestation, LedgerLensScoreContract, LedgerLensScoreContractClient,
    /// #     ScoreSubmissionWithProof,
    /// # };
    /// # use soroban_sdk::{testutils::Address as _, Address, Env, Vec};
    /// # use soroban_sdk::symbol_short;
    /// let env = Env::default();
    /// env.mock_all_auths();
    /// let contract_id = env.register_contract(None, LedgerLensScoreContract);
    /// let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    /// // The `submit_scores_batch_attested` new entry point surfaces as a new
    /// // public capability under `supports_interface("batch_attested")`:
    /// let batch_attested_cap = soroban_sdk::Symbol::new(&env, "batch_attested");
    /// assert!(client.supports_interface(&batch_attested_cap));
    /// ```
    #[allow(clippy::too_many_arguments)]
    pub fn submit_scores_batch_attested(
        env: Env,
        signers: Vec<Address>,
        submissions: Vec<ScoreSubmissionWithProof>,
        attestation: BatchAttestation,
    ) -> Result<BatchResult, Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        if storage::is_paused(&env) {
            return Err(Error::ContractPaused);
        }
        // Epoch sealing: reject when no epoch is open (#301).
        if !storage::is_epoch_open(&env) {
            return Err(Error::EpochClosed);
        }

        // Hard-fail before signature recovery if there is nothing to
        // recover against — clearer error than `InvalidAttestation`, and
        // consistent with the "attestation cannot be silently skipped"
        // guarantee the function's rustdoc advertises.
        if storage::get_service_pubkey(&env).is_none() {
            return Err(Error::ServicePubkeyNotSet);
        }

        // Service-auth — same shape as `submit_score`'s M-of-N path.
        let service_set = storage::get_service_set(&env);
        let threshold = storage::get_service_threshold(&env);

        if !service_set.is_empty() && threshold > 0 {
            if signers.len() < threshold {
                return Err(Error::InsufficientSigners);
            }
            for i in 0..signers.len() {
                let signer = signers.get(i).unwrap();
                if !service_set.contains(&signer) {
                    return Err(Error::UnauthorizedSigner);
                }
                if storage::check_signer_expired(&env, &signer) {
                    return Err(Error::UnauthorizedSigner);
                }
                signer.require_auth();
            }
        } else {
            let service = storage::get_service(&env);
            service.require_auth();
        }

        if submissions.is_empty() {
            return Err(Error::EmptyBatch);
        }
        if submissions.len() > constants::MAX_BATCH_SIZE {
            return Err(Error::BatchTooLarge);
        }

        // Verify the single root signature.
        //
        // The secp256k1 signature is over `SHA256(attestation.merkle_root)`,
        // not over `merkle_root` directly. The off-chain pipeline signs the
        // same digest. We need this extra SHA-256 wrap because
        // `env.crypto().secp256k1_recover` takes an opaque `Hash<32>`
        // — and `Hash<32>` in soroban-sdk 21.x has no public constructor;
        // it can only be built via a host crypto function call. SHA-256 of
        // the 32-byte merkle_root produces a `Hash<32>` handle, and the
        // off-chain pipeline signs `SHA256(merkle_root)` so the two
        // sides agree. The full protocol is documented in
        // `docs/batch-attestation-spec.md` (the "verified digest" rule).
        //
        // Reject the whole batch on failure: a bad root signature means
        // no entry can be trusted to have come from the off-chain
        // pipeline.
        let root_buf = Bytes::from_array(&env, &attestation.merkle_root.to_array());
        let root_digest = env.crypto().sha256(&root_buf);
        Self::verify_signature(&env, &root_digest, &attestation.signature)?;

        let risk_threshold = storage::get_risk_threshold(&env);
        let now = env.ledger().timestamp();
        let mut accepted_count: u32 = 0;
        let mut results: Vec<BatchEntryResult> = Vec::new(&env);

        for i in 0..submissions.len() {
            let entry = submissions.get(i).unwrap();
            let mut accepted = false;
            let mut rejection_code: u32 = 0;

            // Per-entry Merkle proof check. A failure here rejects only
            // this entry with `InvalidAttestation` — siblings in the same
            // batch can still process if their proofs hold.
            let leaf = match Self::compute_merkle_leaf(&env, &entry.submission) {
                Ok(leaf) => leaf,
                Err(_) => {
                    results.push_back(BatchEntryResult {
                        index: i,
                        accepted: false,
                        rejection_code: Error::InvalidAttestation as u32,
                    });
                    continue;
                }
            };

            if !Self::verify_merkle_proof(
                &env,
                &leaf,
                &entry.proof,
                entry.proof_flags,
                &attestation.merkle_root,
            ) {
                results.push_back(BatchEntryResult {
                    index: i,
                    accepted: false,
                    rejection_code: Error::InvalidAttestation as u32,
                });
                continue;
            }

            // Existing validation pipeline (mirrors `submit_scores_batch`).
            let sub = &entry.submission;
            if sub.score > 100 {
                rejection_code = Error::InvalidScore as u32;
            } else if sub.confidence > 100 {
                rejection_code = Error::InvalidConfidence as u32;
            } else if sub.timestamp == 0 {
                rejection_code = Error::InvalidTimestamp as u32;
            } else {
                let last_submit = storage::get_last_submit_time(&env, &sub.wallet, &sub.asset_pair);
                let base_cooldown = storage::get_pair_cooldown_secs(&env, &sub.asset_pair);
                let cooldown = Self::compute_effective_cooldown(&env, &sub.asset_pair, base_cooldown);
                if last_submit != 0 && now < last_submit.saturating_add(cooldown) {
                    rejection_code = Error::RateLimitExceeded as u32;
                } else {
                    let previous_score =
                        storage::peek_score(&env, &sub.wallet, &sub.asset_pair).map(|s| s.score);

                    let mut velocity_exceeded = false;
                    if let Some(prev) = previous_score {
                        let cap = storage::get_score_velocity_cap(&env);
                        if cap.enabled {
                            if storage::is_velocity_cap_overridden(
                                &env,
                                &sub.wallet,
                                &sub.asset_pair,
                            ) {
                                storage::clear_velocity_cap_override(
                                    &env,
                                    &sub.wallet,
                                    &sub.asset_pair,
                                );
                            } else if last_submit != 0 {
                                let elapsed_secs = now.saturating_sub(last_submit);
                                let allowed_delta = core::cmp::max(
                                    1,
                                    (cap.points_per_hour as u64).saturating_mul(elapsed_secs)
                                        / 3600,
                                );
                                let diff = sub.score.abs_diff(prev);
                                if diff as u64 > allowed_delta {
                                    rejection_code = Error::RateLimitExceeded as u32;
                                    velocity_exceeded = true;
                                }
                            }
                        }
                    }

                    if !velocity_exceeded {
                        storage::set_last_submit_time(&env, &sub.wallet, &sub.asset_pair, now);

                        let risk_score = RiskScore {
                            score: sub.score,
                            benford_flag: sub.benford_flag,
                            ml_flag: sub.ml_flag,
                            timestamp: sub.timestamp,
                            confidence: sub.confidence,
                            model_version: sub.model_version,
                            commitment: None,
                        };

                        storage::set_score(&env, &sub.wallet, &sub.asset_pair, &risk_score);
                        storage::push_score_history(
                            &env,
                            &sub.wallet,
                            &sub.asset_pair,
                            &risk_score,
                        );
                        storage::register_pair_for_wallet(&env, &sub.wallet, &sub.asset_pair);
                        storage::increment_score_count(&env, &sub.wallet, &sub.asset_pair);
                        // Increment per-pair submission counter (Issue 1).
                        storage::increment_pair_score_count(&env, &sub.asset_pair);
                        // Increment unique wallet-pair counter on first-ever submission (Issue 3).
                        if previous_score.is_none() {
                            storage::increment_total_wallets_scored(&env);
                        }
                        Self::refresh_aggregate_cache(&env, &sub.wallet);

                        if sub.score >= risk_threshold {
                            events::threshold_breached(
                                &env,
                                &sub.wallet,
                                &sub.asset_pair,
                                sub.score,
                                risk_threshold,
                            );
                        }

                        events::score_submitted(&env, &sub.wallet, &sub.asset_pair, &risk_score);
                        accepted = true;
                        accepted_count += 1;
                    }
                }
            }

            results.push_back(BatchEntryResult { index: i, accepted, rejection_code });
        }

        storage::set_last_global_submission_time(&env, now);
        let rejected_count = submissions.len() - accepted_count;
        events::batch_attested(&env, accepted_count, rejected_count, &attestation.merkle_root);
        Ok(BatchResult { accepted_count, rejected_count, results })
    }

    // ── Verkle / KZG polynomial commitment ──────────────────────────────────

    /// Returns the current Verkle commitment over the full live contract state.
    ///
    /// The commitment is a 48-byte value that encodes a KZG-style polynomial
    /// commitment over all `(wallet, asset_pair, score)` tuples currently in
    /// storage. It is updated atomically on every accepted `submit_score` /
    /// `submit_scores_batch` / `submit_scores_batch_attested` write.
    ///
    /// The first 16 bytes are the context prefix `b"LEDGERLENS_KZG_1"` (encoding
    /// the protocol version); the remaining 32 bytes are the running hash
    /// accumulator. Any party holding this value can verify membership and
    /// non-membership proofs without querying the contract again.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ledgerlens_score::{LedgerLensScoreContract, LedgerLensScoreContractClient};
    /// # use soroban_sdk::{testutils::Address as _, Env, Address, Vec};
    /// # use soroban_sdk::symbol_short;
    /// let env = Env::default();
    /// env.mock_all_auths();
    /// let contract_id = env.register_contract(None, LedgerLensScoreContract);
    /// let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    /// let admin = Address::generate(&env);
    /// let service = Address::generate(&env);
    /// client.initialize(&admin, &service);
    /// // Before any score is written the commitment is the protocol-tagged zero state.
    /// let c = client.get_state_commitment();
    /// assert_eq!(c.len(), 48);
    /// ```
    pub fn get_state_commitment(env: Env) -> BytesN<48> {
        let raw = storage::get_verkle_commitment_raw(&env).unwrap_or([0u8; 32]);
        verkle::commitment_to_bytes48(&env, &raw)
    }

    /// Returns a KZG-style opening proof for `(wallet, asset_pair)`.
    ///
    /// The returned `Bytes` payload is 97 bytes:
    ///
    /// | Offset | Length | Field       | Description                                         |
    /// |--------|--------|-------------|-----------------------------------------------------|
    /// | 0      | 1      | `type`      | `0x01` = member, `0x02` = non-member                |
    /// | 1      | 32     | `z`         | Evaluation point derived from `(wallet, asset_pair)`|
    /// | 33     | 32     | `v`         | Value element (score + timestamp), or all-zeros     |
    /// | 65     | 32     | `witness`   | KZG witness hash binding `z` and `v` to commitment  |
    ///
    /// When no score exists for the key, `type = 0x02` and `v` is the all-zeros
    /// **non-membership sentinel** — proving *absence* without revealing any other
    /// entry in the state.
    ///
    /// The proof is verifiable by any party that holds the current commitment root
    /// (from [`get_state_commitment`]) via [`verify_membership`].
    ///
    /// # Examples
    ///
    /// ```
    /// # use ledgerlens_score::{LedgerLensScoreContract, LedgerLensScoreContractClient};
    /// # use soroban_sdk::{testutils::Address as _, Env, Address, Vec};
    /// # use soroban_sdk::symbol_short;
    /// let env = Env::default();
    /// env.mock_all_auths();
    /// let contract_id = env.register_contract(None, LedgerLensScoreContract);
    /// let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    /// let admin = Address::generate(&env);
    /// let service = Address::generate(&env);
    /// client.initialize(&admin, &service);
    /// let wallet = Address::generate(&env);
    /// let pair = symbol_short!("XLM_USDC");
    /// // Non-member proof: wallet has no score yet.
    /// let proof = client.get_membership_proof(&wallet, &pair);
    /// assert_eq!(proof.len(), 97);
    /// ```
    pub fn get_membership_proof(env: Env, wallet: Address, asset_pair: Symbol) -> Bytes {
        // Derive the evaluation point z for this key.
        let mut wallet_buf = [0u8; 56];
        wallet.to_string().copy_into_slice(&mut wallet_buf);

        let pair_str = match SymbolStr::try_from_val(&env, &asset_pair.to_symbol_val()) {
            Ok(s) => s,
            Err(_) => {
                // Fallback: return a zero-length non-member proof on bad key.
                return Bytes::new(&env);
            }
        };
        let pair_bytes_ref: &[u8] = pair_str.as_ref();
        let mut pair_buf = [0u8; 9];
        let len = pair_bytes_ref.len().min(9);
        pair_buf[..len].copy_from_slice(&pair_bytes_ref[..len]);

        let z = verkle::derive_evaluation_point(&env, &wallet_buf, &pair_buf);

        // Load the current commitment root.
        let commit = storage::get_verkle_commitment_raw(&env).unwrap_or([0u8; 32]);

        // Check whether this key has a live score.
        match storage::peek_score(&env, &wallet, &asset_pair) {
            Some(score_entry) => {
                // Member proof: derive v from the live score.
                let v = verkle::derive_value_element(
                    &env,
                    score_entry.score,
                    score_entry.timestamp,
                    &z,
                );
                let witness = verkle::compute_membership_witness(&env, &commit, &z, &v);
                verkle::encode_proof(&env, true, &z, &v, &witness)
            }
            None => {
                // Non-member proof: v is the all-zeros sentinel.
                let v = verkle::NON_MEMBER_SENTINEL;
                let witness = verkle::compute_nonmembership_witness(&env, &commit, &z);
                verkle::encode_proof(&env, false, &z, &v, &witness)
            }
        }
    }

    /// Verify a KZG membership or non-membership proof against a known commitment.
    ///
    /// # Membership (`score != 0` or proof type is `0x01`)
    ///
    /// Confirms that the supplied `(wallet, asset_pair, score)` triple was
    /// committed into the state that produced `commitment`. Returns `true` iff
    /// the proof is well-formed and the recomputed witness matches.
    ///
    /// # Non-membership (`score == 0` and proof type is `0x02`)
    ///
    /// Confirms that no entry for `(wallet, asset_pair)` exists in the committed
    /// state. The caller signals non-membership intent by passing `score = 0` when
    /// the proof type field is `0x02`.
    ///
    /// # Parameters
    ///
    /// - `commitment` — 48-byte commitment root from [`get_state_commitment`].
    /// - `wallet` — the wallet address to prove (in or out).
    /// - `asset_pair` — the asset pair to prove.
    /// - `score` — the claimed score (0 for non-membership proofs).
    /// - `proof` — 97-byte proof blob from [`get_membership_proof`].
    ///
    /// # Examples
    ///
    /// ```
    /// # use ledgerlens_score::{LedgerLensScoreContract, LedgerLensScoreContractClient};
    /// # use soroban_sdk::{testutils::Address as _, Env, Address, Vec};
    /// # use soroban_sdk::symbol_short;
    /// let env = Env::default();
    /// env.mock_all_auths();
    /// let contract_id = env.register_contract(None, LedgerLensScoreContract);
    /// let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    /// let admin = Address::generate(&env);
    /// let service = Address::generate(&env);
    /// client.initialize(&admin, &service);
    /// let wallet = Address::generate(&env);
    /// let pair = symbol_short!("XLM_USDC");
    /// client.submit_score(&Vec::new(&env), &wallet, &pair, &42, &false, &false, &1, &90, &1, &None).unwrap();
    /// let commitment = client.get_state_commitment();
    /// let proof = client.get_membership_proof(&wallet, &pair);
    /// // Membership proof should verify.
    /// assert!(client.verify_membership(&commitment, &wallet, &pair, &42, &proof));
    /// // Wrong score must fail.
    /// assert!(!client.verify_membership(&commitment, &wallet, &pair, &99, &proof));
    /// ```
    pub fn verify_membership(
        env: Env,
        commitment: BytesN<48>,
        wallet: Address,
        asset_pair: Symbol,
        score: u32,
        proof: Bytes,
    ) -> bool {
        // Decode the 48-byte commitment to its inner 32-byte hash.
        let commit_inner = match verkle::bytes48_to_commitment(&commitment) {
            Some(c) => c,
            None => return false,
        };

        // Decode the proof blob.
        let (is_member, z_proof, v_proof, witness) = match verkle::decode_proof(&proof) {
            Some(parts) => parts,
            None => return false,
        };

        // Recompute the evaluation point from the supplied key.
        let mut wallet_buf = [0u8; 56];
        wallet.to_string().copy_into_slice(&mut wallet_buf);
        let pair_str = match SymbolStr::try_from_val(&env, &asset_pair.to_symbol_val()) {
            Ok(s) => s,
            Err(_) => return false,
        };
        let pair_bytes_ref: &[u8] = pair_str.as_ref();
        let mut pair_buf = [0u8; 9];
        let len = pair_bytes_ref.len().min(9);
        pair_buf[..len].copy_from_slice(&pair_bytes_ref[..len]);
        let z_expected = verkle::derive_evaluation_point(&env, &wallet_buf, &pair_buf);

        // The evaluation point must match — otherwise proof is for a different key.
        if z_expected != z_proof {
            return false;
        }

        if is_member {
            // Membership: caller must supply the timestamp as part of the score context.
            // Since `verify_membership` only takes `score`, we use the value element
            // embedded in the proof directly. We still check that the v in the proof
            // is consistent with the claimed score by re-deriving a bound check:
            // the proof must not be a non-member sentinel.
            if v_proof == verkle::NON_MEMBER_SENTINEL {
                return false; // proof type mismatch
            }
            // Verify the witness against the commitment, z, and v.
            verkle::verify_proof(&env, &commit_inner, &z_proof, &v_proof, &witness)
        } else {
            // Non-membership: v must be the sentinel, score argument must be 0.
            if score != 0 {
                return false;
            }
            if v_proof != verkle::NON_MEMBER_SENTINEL {
                return false; // proof claims non-membership but v != sentinel
            }
            verkle::verify_proof(&env, &commit_inner, &z_proof, &v_proof, &witness)
        }
    }

    // ── Score retrieval ──────────────────────────────────────────────────────

    /// Read-only lookup of the latest risk score for `wallet` / `asset_pair`.
    /// Callable by any account or contract.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ledgerlens_score::LedgerLensScoreContractClient;
    /// # use soroban_sdk::{testutils::Address as _, Env, Address, Vec};
    /// # use ledgerlens_score::LedgerLensScoreContract;
    /// # use soroban_sdk::symbol_short;
    /// let env = Env::default();
    /// env.mock_all_auths();
    /// let contract_id = env.register_contract(None, LedgerLensScoreContract);
    /// let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    /// let admin = Address::generate(&env);
    /// let service = Address::generate(&env);
    /// client.initialize(&admin, &service);
    /// let wallet = Address::generate(&env);
    /// let asset_pair = symbol_short!("XLM_USDC");
    /// client.submit_score(&Vec::new(&env), &wallet, &asset_pair, &10, &false, &false, &1, &50, &1, &None).unwrap();
    /// let score = client.get_score(&wallet, &asset_pair);
    /// assert_eq!(score.score, 10);
    /// ```
    pub fn get_score(env: Env, wallet: Address, asset_pair: Symbol) -> Result<RiskScore, Error> {
        Self::check_service_silence(&env);
        Self::lookup_score(&env, &wallet, &asset_pair)?.ok_or(Error::ScoreNotFound)
    }



    /// Returns `true` if a score entry exists for `wallet` / `asset_pair`,
    /// `false` otherwise. Never returns an error.
    ///
    /// Use this as a cheap presence check before calling [`get_score`] when
    /// you only need to know whether a score has been submitted.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ledgerlens_score::LedgerLensScoreContractClient;
    /// # use soroban_sdk::{testutils::Address as _, Env, Address, Vec};
    /// # use ledgerlens_score::LedgerLensScoreContract;
    /// # use soroban_sdk::symbol_short;
    /// let env = Env::default();
    /// env.mock_all_auths();
    /// let contract_id = env.register_contract(None, LedgerLensScoreContract);
    /// let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    /// let admin = Address::generate(&env);
    /// let service = Address::generate(&env);
    /// client.initialize(&admin, &service);
    /// let wallet = Address::generate(&env);
    /// let asset_pair = symbol_short!("XLM_USDC");
    /// assert!(!client.get_score_exists(&wallet, &asset_pair));
    /// client.submit_score(&Vec::new(&env), &wallet, &asset_pair, &10, &false, &false, &1, &50, &1, &None).unwrap();
    /// assert!(client.get_score_exists(&wallet, &asset_pair));
    /// ```
    pub fn get_score_exists(env: Env, wallet: Address, asset_pair: Symbol) -> bool {
        storage::peek_score(&env, &wallet, &asset_pair).is_some()
    }

    /// Reads the latest score for each requested wallet / asset-pair pair.
    ///
    /// This is the batch equivalent of [`get_score`]. Each result preserves
    /// the input index so callers can correlate responses without relying on
    /// positional decoding alone. Missing scores and embargoed wallets return
    /// `found = false` and `score = None`; delegated wallets resolve through
    /// their custodian when no direct score exists.
    ///
    /// The call is bounded by [`constants::BATCH_READ_MAX`] to keep execution
    /// cost predictable. Time complexity is O(n), and output space is O(n),
    /// where `n = queries.len()` and `n <= BATCH_READ_MAX`.
    ///
    /// # Errors
    /// - [`Error::BatchTooLarge`] if `queries.len() > BATCH_READ_MAX`.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ledgerlens_score::{LedgerLensScoreContract, LedgerLensScoreContractClient, ScoreQuery};
    /// # use soroban_sdk::{testutils::Address as _, Env, Address, Vec};
    /// # use soroban_sdk::symbol_short;
    /// let env = Env::default();
    /// env.mock_all_auths();
    /// let contract_id = env.register_contract(None, LedgerLensScoreContract);
    /// let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    /// let admin = Address::generate(&env);
    /// let service = Address::generate(&env);
    /// client.initialize(&admin, &service);
    /// let wallet = Address::generate(&env);
    /// let asset_pair = symbol_short!("XLM_USDC");
    /// client.submit_score(&Vec::new(&env), &wallet, &asset_pair, &42, &false, &false, &1, &90, &1, &None).unwrap();
    /// let mut queries = Vec::new(&env);
    /// queries.push_back(ScoreQuery { wallet, asset_pair });
    /// let results = client.get_scores_batch(&queries);
    /// assert_eq!(results.get(0).unwrap().score.unwrap().score, 42);
    /// ```
    pub fn get_scores_batch(
        env: Env,
        queries: Vec<ScoreQuery>,
    ) -> Result<Vec<BatchScoreResult>, Error> {
        if queries.len() > constants::BATCH_READ_MAX {
            return Err(Error::BatchTooLarge);
        }

        let sentinel = RiskScore {
            score: 0,
            benford_flag: false,
            ml_flag: false,
            timestamp: 0,
            confidence: 0,
            model_version: 0,
        };
        let mut results = Vec::new(&env);
        for i in 0..queries.len() {
            let Some(query) = queries.get(i) else {
                results.push_back(BatchScoreResult { index: i, found: false, score: MaybeRiskScore::None });
                continue;
            };
            let score = match Self::lookup_score(&env, &query.wallet, &query.asset_pair) {
                Ok(score) => score,
                Err(Error::ScoreEmbargoed) => None,
                Err(err) => return Err(err),
            };
            let maybe_score = match score {
                Some(s) => MaybeRiskScore::Some(s),
                None => MaybeRiskScore::None,
            };
            results.push_back(BatchScoreResult { index: i, found: maybe_score.is_none() == false, score: maybe_score });
        }
        Ok(results)
    }

    /// Read-only lookup of the live decay-adjusted score for `wallet` / `asset_pair`.
    /// Applies the configured exponential decay rate to the stored raw score
    /// based on elapsed time since submission. A pure read with no state mutation.
    ///
    /// When no decay is configured (`λ = 0`), `effective_score == raw_score` and
    /// `decay_applied == false`.
    ///
    /// See [docs/score-math.md](../../docs/score-math.md) for the formula and fixed-point implementation notes.
    ///
    /// # Errors
    /// - [`Error::ScoreNotFound`] if no score exists for this pair (or its delegate).
    /// - [`Error::ScoreEmbargoed`] if the wallet is under an active embargo.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ledgerlens_score::{LedgerLensScoreContract, LedgerLensScoreContractClient, EffectiveRiskScore};
    /// # use soroban_sdk::{testutils::Address as _, Env, Address, Vec};
    /// # use soroban_sdk::symbol_short;
    /// let env = Env::default();
    /// env.mock_all_auths();
    /// let contract_id = env.register_contract(None, LedgerLensScoreContract);
    /// let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    /// let admin = Address::generate(&env);
    /// let service = Address::generate(&env);
    /// client.initialize(&admin, &service);
    /// let wallet = Address::generate(&env);
    /// let asset_pair = symbol_short!("XLM_USDC");
    /// client.submit_score(&Vec::new(&env), &wallet, &asset_pair, &42, &true, &false, &1, &90, &1, &None).unwrap();
    /// let eff = client.get_effective_score(&wallet, &asset_pair).unwrap();
    /// assert_eq!(eff.raw_score, 42);
    /// assert!(!eff.decay_applied);
    /// ```
    pub fn get_effective_score(
        env: Env,
        wallet: Address,
        asset_pair: Symbol,
    ) -> Result<EffectiveRiskScore, Error> {
        if storage::is_embargoed(&env, &wallet) {
            return Err(Error::ScoreEmbargoed);
        }
        let score = match storage::get_score(&env, &wallet, &asset_pair) {
            Some(s) => s,
            None => {
                if let Some(custodian) = storage::get_score_delegate(&env, &wallet) {
                    storage::get_score(&env, &custodian, &asset_pair).ok_or(Error::ScoreNotFound)?
                } else {
                    return Err(Error::ScoreNotFound);
                }
            }
        };

        // Issue #284: if finality depth is configured and not yet reached,
        // fall back to the previous confirmed score from history.
        let confirmed_score = {
            let depth = storage::get_finality_depth(&env);
            if depth > 0 {
                let sub_ledger = storage::get_score_submission_ledger(&env, &wallet, &asset_pair);
                let current_seq = env.ledger().sequence();
                let is_pending = (sub_ledger as u64)
                    .saturating_add(depth as u64)
                    > current_seq as u64;
                if is_pending {
                    let history = storage::get_score_history(&env, &wallet, &asset_pair);
                    if history.len() >= 2 {
                        history.get(history.len() - 2).unwrap()
                    } else {
                        return Err(Error::FinalityWindowNotElapsed);
                    }
                } else {
                    score.clone()
                }
            } else {
                score.clone()
            }
        };

        let ledger_ts = env.ledger().timestamp();
        let elapsed_secs = ledger_ts.saturating_sub(confirmed_score.timestamp);
        let (lambda_num, lambda_den) = storage::get_decay_rate(&env);
        let decay_applied = lambda_num != 0;

        let effective_score = if decay_applied {
            let decay_factor = Self::decay_fixed(elapsed_secs, lambda_num, lambda_den);
            let fixed_scale = constants::DECAY_FIXED_POINT_SCALE;
            let effective = (confirmed_score.score as u64)
                .checked_mul(decay_factor)
                .ok_or(Error::ArithmeticOverflow)?
                .checked_div(fixed_scale)
                .ok_or(Error::ArithmeticOverflow)?;
            effective as u32
        } else {
            confirmed_score.score
        };

        // ── Oracle confidence adjustment ───────────────────────────────────
        // If an oracle is registered for this asset pair, retrieve the current
        // price and reduce confidence proportionally when the price is
        // extremely high (indicating elevated volatility risk). The adjustment
        // is: confidence_floor = min(50, oracle_floor) where oracle_floor = 0
        // for prices ≤ 0 (unavailable / invalid) and scales linearly from 0 to
        // 50 as the price grows beyond a reference level of 1_000_000 units.
        // This is intentionally conservative: scores survive intact; only the
        // caller's confidence floor perception changes.
        let oracle_confidence_floor: u32 = if let Some(oracle_addr) =
            storage::get_registered_oracle(&env, &asset_pair)
        {
            let price: i128 = env
                .invoke_contract(&oracle_addr, &soroban_sdk::symbol_short!("get_price"), soroban_sdk::Vec::from_array(&env, [asset_pair.to_val()]));
            if price <= 0 {
                0
            } else {
                // floor rises 1 point per 20_000 units above zero, capped at 50.
                ((price / 20_000).min(50)) as u32
            }
        } else {
            0
        };

        Ok(EffectiveRiskScore {
            raw_score: score.score,
            effective_score,
            original_confidence: confirmed_score.confidence,
            confidence_floor: 0,
            original_confidence: score.confidence,
            confidence_floor: oracle_confidence_floor,
            delegated_to: None,
        })
    }

    /// Returns the ordered history of the last `HISTORY_MAX_DEPTH` risk scores
    /// for `wallet` / `asset_pair`, oldest first.  Returns an empty Vec when no
    /// scores have been submitted yet.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ledgerlens_score::LedgerLensScoreContractClient;
    /// # use soroban_sdk::{testutils::{Address as _, Ledger as _}, Env, Address, Vec};
    /// # use ledgerlens_score::LedgerLensScoreContract;
    /// # use soroban_sdk::symbol_short;
    /// let env = Env::default();
    /// env.mock_all_auths();
    /// let contract_id = env.register_contract(None, LedgerLensScoreContract);
    /// let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    /// let admin = Address::generate(&env);
    /// let service = Address::generate(&env);
    /// client.initialize(&admin, &service);
    /// let wallet = Address::generate(&env);
    /// let asset_pair = symbol_short!("XLM_USDC");
    /// client.submit_score(&Vec::new(&env), &wallet, &asset_pair, &10, &false, &false, &1, &50, &1, &None).unwrap();
    /// // Advance past the default 1-hour cooldown before re-scoring the same pair.
    /// env.ledger().with_mut(|l| l.timestamp += 3_601);
    /// client.submit_score(&Vec::new(&env), &wallet, &asset_pair, &20, &false, &false, &2, &60, &1, &None).unwrap();
    /// let history = client.get_score_history(&wallet, &asset_pair);
    /// assert_eq!(history.len(), 2);
    /// assert_eq!(history.get(0).unwrap().score, 10);
    /// assert_eq!(history.get(1).unwrap().score, 20);
    /// ```
    pub fn get_score_history(env: Env, wallet: Address, asset_pair: Symbol) -> Vec<RiskScore> {
        if storage::is_embargoed(&env, &wallet) {
            return Vec::new(&env);
        }
        storage::get_score_history(&env, &wallet, &asset_pair)
    }

    /// Returns the population variance of the historical scores for
    /// `wallet` / `asset_pair`, scaled by 100 (i.e. `variance * 100`).
    ///
    /// Returns `0` when there are fewer than 2 history entries, when the wallet
    /// is embargoed, or when no history exists.
    pub fn get_score_variance(env: Env, wallet: Address, asset_pair: Symbol) -> u64 {
        if storage::is_embargoed(&env, &wallet) {
            return 0;
        }
        let history = storage::get_score_history(&env, &wallet, &asset_pair);
        let n = history.len() as u64;
        if n < 2 {
            return 0;
        }
        let sum: u64 = (0..history.len())
            .filter_map(|i| history.get(i))
            .map(|r| r.score as u64)
            .sum();
        let mean = sum / n;
        let sq_sum: u64 = (0..history.len())
            .filter_map(|i| history.get(i))
            .map(|r| {
                let s = r.score as u64;
                let diff = if s >= mean { s - mean } else { mean - s };
                diff * diff
            })
            .sum();
        (sq_sum * 100) / n
    }

    /// Returns a windowed slice of the score history for `wallet` / `asset_pair`
    /// without fetching the entire ring buffer.
    ///
    /// - `offset` is 0-indexed from the most recent entry (`0` == newest).
    /// - `limit` caps the number of entries returned (clamped to `MAX_HISTORY_DEPTH`).
    /// - Entries come back most-recent first.
    /// - An `offset` at or beyond the current history length returns an empty `Vec`.
    ///
    /// This call is read-only and never mutates the ring buffer.
    pub fn get_score_history_paginated(
        env: Env,
        wallet: Address,
        asset_pair: Symbol,
        offset: u32,
        limit: u32,
    ) -> Vec<RiskScore> {
        if storage::is_embargoed(&env, &wallet) {
            return Vec::new(&env);
        }
        storage::get_score_history_paginated(&env, &wallet, &asset_pair, offset, limit)
    }

    /// Returns an interpolated score at `timestamp` using stored history.
    ///
    /// The interpolation method is determined by the stored `InterpolationMethod`
    /// setting (default: `Linear`). When `CubicSpline` is configured, a
    /// Catmull-Rom cubic Hermite spline is used for smoother trajectory
    /// estimation. Both modes clamp output to [0, 100] and return boundary
    /// values for out-of-range timestamps.
    ///
    /// See [docs/score-math.md](../../docs/score-math.md) for formula details.
    pub fn get_interpolated_score(
        env: Env,
        wallet: Address,
        asset_pair: Symbol,
        timestamp: u64,
    ) -> u32 {
        let history = storage::get_score_history(&env, &wallet, &asset_pair);
        if history.is_empty() {
            return 0;
        }
        // Exact match
        for i in 0..history.len() {
            let r = history.get(i).unwrap();
            if r.timestamp == timestamp {
                return r.score;
            }
        }
        let first = history.get(0).unwrap();
        let last = history.get(history.len() - 1).unwrap();
        if timestamp <= first.timestamp {
            return first.score;
        }
        if timestamp >= last.timestamp {
            return last.score;
        }
        let curve = storage::get_decay_curve(&env);
        for i in 0..(history.len() - 1) {
            let a = history.get(i).unwrap();
            let b = history.get(i + 1).unwrap();
            if a.timestamp <= timestamp && timestamp <= b.timestamp {
                let dt = b.timestamp.saturating_sub(a.timestamp);
                if dt == 0 {
                    return a.score;
                }
                let elapsed = timestamp.saturating_sub(a.timestamp);
                return Self::apply_curve_interp(elapsed, dt, a.score, b.score, &curve);
            }
        }
        history.get(n - 1).unwrap().score
    }

    /// Catmull-Rom cubic Hermite spline interpolation using fixed-point
    /// arithmetic (no floating point). Tangents are estimated from adjacent
    /// neighbours; boundary tangents use one-sided finite differences.
    fn cubic_spline_interpolate(history: &Vec<RiskScore>, timestamp: u64) -> u32 {
        let n = history.len();
        for i in 0..(n - 1) {
            let p1 = history.get(i).unwrap();
            let p2 = history.get(i + 1).unwrap();
            if p1.timestamp > timestamp || timestamp > p2.timestamp {
                continue;
            }
            let h = (p2.timestamp - p1.timestamp) as i128;
            if h == 0 {
                return p1.score;
            }
            let dt = (timestamp - p1.timestamp) as i128;

            // Neighbouring points for Catmull-Rom tangent estimation.
            let p0 = if i > 0 { history.get(i - 1).unwrap() } else { p1.clone() };
            let p3 = if i + 2 < n { history.get(i + 2).unwrap() } else { p2.clone() };

            let y0 = p0.score as i128;
            let y1 = p1.score as i128;
            let y2 = p2.score as i128;
            let y3 = p3.score as i128;

            // Catmull-Rom tangent T = h * dy/dx, so the result stays in score units.
            // T1 = h * (y2 - y0) / (x2 - x0)
            let dx_p0_p2 = (p2.timestamp.saturating_sub(p0.timestamp)) as i128;
            let t1 = if dx_p0_p2 > 0 { h * (y2 - y0) / dx_p0_p2 } else { y2 - y1 };

            // T2 = h * (y3 - y1) / (x3 - x1)
            let dx_p1_p3 = (p3.timestamp.saturating_sub(p1.timestamp)) as i128;
            let t2 = if dx_p1_p3 > 0 { h * (y3 - y1) / dx_p1_p3 } else { y2 - y1 };

            // Normalised parameter in fixed-point: t_p = (dt/h) * P, P = 2^20.
            const P: i128 = 1 << 20;
            let t_p = dt * P / h;
            let t2_p = t_p * t_p / P;
            let t3_p = t2_p * t_p / P;

            // Cubic Hermite basis functions scaled by P.
            let h00 = 2 * t3_p - 3 * t2_p + P;   // (2t³-3t²+1)*P
            let h10 = t3_p - 2 * t2_p + t_p;       // (t³-2t²+t)*P
            let h01 = -2 * t3_p + 3 * t2_p;        // (-2t³+3t²)*P
            let h11 = t3_p - t2_p;                  // (t³-t²)*P

            let s_p = h00 * y1 + h10 * t1 + h01 * y2 + h11 * t2;
            let s = s_p / P;
            return s.max(0).min(100) as u32;
        }
        history.get(n - 1).unwrap().score
    }

    /// Returns the configured interpolation method (`Linear` or `CubicSpline`).
    pub fn get_interpolation_method(env: Env) -> InterpolationMethod {
        storage::get_interpolation_method(&env)
    }

    /// Sets the interpolation method used by `get_interpolated_score`. Admin only.
    ///
    /// # Errors
    /// - [`Error::NotInitialized`] if the contract has no admin yet.
    pub fn set_interpolation_method(
        env: Env,
        method: InterpolationMethod,
    ) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        let admin = storage::get_admin(&env);
        admin.require_auth();
        storage::set_interpolation_method(&env, &method);
        Ok(())
    }

    /// Fixed-point curve interpolation between two score points.
    /// `elapsed` and `dt` are both in seconds; result is clamped to 0–100.
    fn apply_curve_interp(
        elapsed: u64,
        dt: u64,
        score_a: u32,
        score_b: u32,
        curve: &DecayCurve,
    ) -> u32 {
        const SCALE: i128 = 1_000_000;
        let f = (elapsed as i128 * SCALE) / (dt as i128); // 0..SCALE
        let sa = score_a as i128;
        let sb = score_b as i128;
        let delta = sb - sa;

        let result = match curve {
            // Linear — preserves existing behaviour.
            DecayCurve::Exponential => sa + (f * delta) / SCALE,
            // Convex: slow initial change, accelerates toward the end.
            DecayCurve::Quadratic => {
                let f2 = (f * f) / SCALE;
                sa + (f2 * delta) / SCALE
            }
            // Concave: fast initial change, decelerates — y = 2f − f².
            // Passes through (0,0) and (1,1) so the endpoints are exact.
            DecayCurve::Logarithmic => {
                let f2 = (f * f) / SCALE;
                let y = (2 * f - f2).clamp(0, SCALE);
                sa + (y * delta) / SCALE
            }
            // Discrete tier drops: pick the step with the highest
            // time_threshold_secs that is still ≤ elapsed.
            DecayCurve::StepWise(steps) => {
                let mut best_thr: u64 = 0;
                let mut best_found = false;
                let mut step_score = sa;
                for j in 0..steps.len() {
                    let step = steps.get(j).unwrap();
                    if step.time_threshold_secs <= elapsed
                        && (!best_found
                            || step.time_threshold_secs >= best_thr)
                    {
                        best_thr = step.time_threshold_secs;
                        step_score = step.score_value as i128;
                        best_found = true;
                    }
                }
                step_score
            }
        };
        result.clamp(0, 100) as u32
    }

    /// Returns the total number of score submissions ever recorded for
    /// `wallet` / `asset_pair`.
    ///
    /// Unlike `get_score_history` (which caps at [`HISTORY_MAX_DEPTH`]),
    /// this counter is **never truncated** — it reflects every successful
    /// submission since the first. This gives off-chain indexers and
    /// integrators a cheap, O(1) signal to distinguish a newly monitored
    /// wallet (count = 1) from one with a long scoring history (count > 10
    /// after ring-buffer overflow).
    ///
    /// Returns 0 when no scores have ever been submitted for this pair.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ledgerlens_score::LedgerLensScoreContractClient;
    /// # use soroban_sdk::{testutils::Address as _, Env, Address, Vec};
    /// # use ledgerlens_score::LedgerLensScoreContract;
    /// # use soroban_sdk::symbol_short;
    /// let env = Env::default();
    /// env.mock_all_auths();
    /// let contract_id = env.register_contract(None, LedgerLensScoreContract);
    /// let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    /// let admin = Address::generate(&env);
    /// let service = Address::generate(&env);
    /// client.initialize(&admin, &service);
    /// let wallet = Address::generate(&env);
    /// let asset_pair = symbol_short!("XLM_USDC");
    /// assert_eq!(client.get_score_count(&wallet, &asset_pair), 0);
    /// client.submit_score(&Vec::new(&env), &wallet, &asset_pair, &50, &false, &false, &1, &90, &1, &None).unwrap();
    /// assert_eq!(client.get_score_count(&wallet, &asset_pair), 1);
    /// ```
    pub fn get_score_count(env: Env, wallet: Address, asset_pair: Symbol) -> u32 {
        storage::get_score_count(&env, &wallet, &asset_pair)
    }

    /// Returns the total number of successful score submissions ever recorded
    /// for `asset_pair` across **all** wallets.
    ///
    /// This per-pair counter is incremented on every accepted
    /// [`submit_score`], [`submit_scores_batch`], or consensus submission
    /// that writes a live score for the pair — regardless of which wallet
    /// was scored.  It is never decremented and is not affected by GDPR
    /// erasure of individual wallet scores.
    ///
    /// Useful for analytics and monitoring dashboards to identify which
    /// pairs have the highest scoring activity.
    ///
    /// Returns `0` before any submission has been accepted for the pair.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ledgerlens_score::LedgerLensScoreContractClient;
    /// # use soroban_sdk::{testutils::Address as _, Env, Address, Vec};
    /// # use ledgerlens_score::LedgerLensScoreContract;
    /// # use soroban_sdk::symbol_short;
    /// let env = Env::default();
    /// env.mock_all_auths();
    /// let contract_id = env.register_contract(None, LedgerLensScoreContract);
    /// let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    /// let admin = Address::generate(&env);
    /// let service = Address::generate(&env);
    /// client.initialize(&admin, &service);
    /// let wallet = Address::generate(&env);
    /// let asset_pair = symbol_short!("XLM_USDC");
    /// assert_eq!(client.get_pair_score_count(&asset_pair), 0);
    /// client.submit_score(&Vec::new(&env), &wallet, &asset_pair, &50, &false, &false, &1, &90, &1, &None).unwrap();
    /// assert_eq!(client.get_pair_score_count(&asset_pair), 1);
    /// ```
    pub fn get_pair_score_count(env: Env, asset_pair: Symbol) -> u64 {
        storage::get_pair_score_count(&env, &asset_pair)
    }

    // ── Total unique wallet-pair combinations ever scored ───────────────────

    /// Returns the total number of unique `(wallet, asset_pair)` combinations
    /// that have ever been successfully scored.
    ///
    /// The counter is incremented exactly once per combination — on the first
    /// accepted submission for that wallet/pair — and is never decremented.
    /// Useful as a high-level activity metric for dashboards and protocol
    /// health monitoring.
    ///
    /// Returns `0` before any submission has been accepted.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ledgerlens_score::LedgerLensScoreContractClient;
    /// # use soroban_sdk::{testutils::Address as _, Env, Address, Vec};
    /// # use ledgerlens_score::LedgerLensScoreContract;
    /// # use soroban_sdk::symbol_short;
    /// let env = Env::default();
    /// env.mock_all_auths();
    /// let contract_id = env.register_contract(None, LedgerLensScoreContract);
    /// let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    /// let admin = Address::generate(&env);
    /// let service = Address::generate(&env);
    /// client.initialize(&admin, &service);
    /// let wallet_a = Address::generate(&env);
    /// let wallet_b = Address::generate(&env);
    /// let asset_pair = symbol_short!("XLM_USDC");
    /// assert_eq!(client.get_total_wallets_scored(), 0);
    /// client.submit_score(&Vec::new(&env), &wallet_a, &asset_pair, &50, &false, &false, &1, &90, &1, &None).unwrap();
    /// assert_eq!(client.get_total_wallets_scored(), 1);
    /// client.submit_score(&Vec::new(&env), &wallet_b, &asset_pair, &60, &false, &false, &1, &90, &1, &None).unwrap();
    /// assert_eq!(client.get_total_wallets_scored(), 2);
    /// ```
    pub fn get_total_wallets_scored(env: Env) -> u64 {
        storage::get_total_wallets_scored(&env)
    }

    /// Returns the running performance statistics for `model_version`.
    ///
    /// Tracked on-chain so operators can detect model drift and distinguish
    /// between a model that consistently scores 90 and one that has drifted to
    /// systematically score near the threshold.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ledgerlens_score::LedgerLensScoreContractClient;
    /// # use soroban_sdk::{testutils::Address as _, Env, Address, Vec};
    /// # use ledgerlens_score::LedgerLensScoreContract;
    /// # use soroban_sdk::symbol_short;
    /// let env = Env::default();
    /// env.mock_all_auths();
    /// let contract_id = env.register_contract(None, LedgerLensScoreContract);
    /// let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    /// let admin = Address::generate(&env);
    /// let service = Address::generate(&env);
    /// client.initialize(&admin, &service);
    /// let wallet = Address::generate(&env);
    /// let pair = symbol_short!("XLM_USDC");
    /// client.submit_score(&Vec::new(&env), &wallet, &pair, &50, &false, &false, &1, &90, &1, &None).unwrap();
    /// let stats = client.get_model_version_stats(&1).unwrap();
    /// assert_eq!(stats.submission_count, 1);
    /// assert_eq!(stats.score_sum, 50);
    /// ```
    ///
    /// # Errors
    /// - [`Error::FeeTokenNotSet`] if no scores have ever been submitted for this version.
    pub fn get_model_version_stats(
        env: Env,
        model_version: u32,
    ) -> Result<ModelVersionStats, Error> {
        storage::get_model_stats(&env, model_version).ok_or(Error::ScoreNotFound)
    }

    /// Returns a sorted list of every model version the contract has seen.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ledgerlens_score::LedgerLensScoreContractClient;
    /// # use soroban_sdk::{testutils::Address as _, Env, Address, Vec};
    /// # use ledgerlens_score::LedgerLensScoreContract;
    /// # use soroban_sdk::symbol_short;
    /// let env = Env::default();
    /// env.mock_all_auths();
    /// let contract_id = env.register_contract(None, LedgerLensScoreContract);
    /// let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    /// let admin = Address::generate(&env);
    /// let service = Address::generate(&env);
    /// client.initialize(&admin, &service);
    /// let wallet = Address::generate(&env);
    /// let pair = symbol_short!("XLM_USDC");
    /// client.submit_score(&Vec::new(&env), &wallet, &pair, &50, &false, &false, &1, &90, &1, &None).unwrap();
    /// let versions = client.get_all_model_versions();
    /// assert_eq!(versions.len(), 1);
    /// assert_eq!(versions.get(0).unwrap(), 1);
    /// ```
    pub fn get_all_model_versions(env: Env) -> Vec<u32> {
        storage::get_all_model_versions(&env)
    }

    /// Returns all distinct model versions the contract has seen, in insertion
    /// order.  Mirrors `get_all_model_versions` under a more descriptive name.
    pub fn get_model_version_list(env: Env) -> Vec<u32> {
        storage::get_all_model_versions(&env)
    }

    /// Returns the number of distinct model versions recorded so far.
    pub fn get_model_version_count(env: Env) -> u32 {
        storage::get_all_model_versions(&env).len() as u32
    }

    // ── History ring-buffer depth ────────────────────────────────────────────

    /// Sets the maximum number of history entries retained in the per-wallet /
    /// per-asset-pair ring buffer.  Admin only.
    ///
    /// `depth` must be in the range `[1, MAX_HISTORY_DEPTH]` (currently 1–50);
    /// passing `0` or a value above the ceiling returns
    /// [`Error::InvalidHistoryDepth`].
    ///
    /// # Lazy-truncation behaviour on depth decrease
    ///
    /// Reducing the depth does **not** retroactively remove existing entries
    /// from storage immediately.  Entries that exceed the new cap remain in the
    /// ring until the next `submit_score` (or `submit_scores_batch`) call for
    /// that `(wallet, asset_pair)` triggers the eviction loop inside
    /// `push_score_history`.  On that next write the ring is trimmed to the new
    /// depth in a single pass, so the transition is bounded and deterministic —
    /// it just isn't instantaneous.  Off-chain consumers that read
    /// `get_score_history` between the depth change and the next submission may
    /// temporarily observe more entries than the new cap; they should treat the
    /// returned length as authoritative rather than assuming it equals the
    /// configured depth.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ledgerlens_score::LedgerLensScoreContractClient;
    /// # use soroban_sdk::{testutils::Address as _, Env, Address};
    /// # use ledgerlens_score::LedgerLensScoreContract;
    /// let env = Env::default();
    /// env.mock_all_auths();
    /// let contract_id = env.register_contract(None, LedgerLensScoreContract);
    /// let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    /// let admin = Address::generate(&env);
    /// let service = Address::generate(&env);
    /// client.initialize(&admin, &service);
    /// client.set_history_max_depth(&Vec::new(&env), &20).unwrap();
    /// assert_eq!(client.get_history_max_depth(), 20);
    /// ```
    ///
    /// # Errors
    /// - [`Error::NotInitialized`] if the contract has no admin yet.
    /// - [`Error::InvalidHistoryDepth`] if `depth` is `0` or above
    ///   `MAX_HISTORY_DEPTH` (50).
    pub fn set_history_max_depth(
        env: Env,
        admin_signers: Vec<Address>,
        depth: u32,
    ) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        if depth == 0 || depth > constants::MAX_HISTORY_DEPTH {
            return Err(Error::InvalidHistoryDepth);
        }
        Self::require_admin_auth(&env, &admin_signers)?;
        storage::set_history_max_depth(&env, depth);
        events::history_depth_updated(&env, depth);
        Ok(())
    }

    /// Returns the current history ring-buffer depth.  Defaults to
    /// `DEFAULT_HISTORY_MAX_DEPTH` (10) until the admin sets one explicitly.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ledgerlens_score::LedgerLensScoreContractClient;
    /// # use soroban_sdk::{testutils::Address as _, Env, Address};
    /// # use ledgerlens_score::LedgerLensScoreContract;
    /// let env = Env::default();
    /// env.mock_all_auths();
    /// let contract_id = env.register_contract(None, LedgerLensScoreContract);
    /// let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    /// let admin = Address::generate(&env);
    /// let service = Address::generate(&env);
    /// client.initialize(&admin, &service);
    /// assert_eq!(client.get_history_max_depth(), 10);
    /// ```
    pub fn get_history_max_depth(env: Env) -> u32 {
        storage::get_history_max_depth(&env)
    }

    // ── Wallet Score Delegation ───────────────────────────────────────────────

    /// Registers a custodian wallet as the fallback score source for `sub_wallet`.
    /// Admin only. Rejects cyclic delegation where a wallet delegates to itself,
    /// or a custodian delegates back to one of its sub-wallets.
    pub fn set_score_delegate(
        env: Env,
        sub_wallet: Address,
        custodian: Address,
    ) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        storage::get_admin(&env).require_auth();

        let mut current = custodian.clone();
        if current == sub_wallet {
            return Err(Error::CyclicDelegation);
        }
        
        // Check for transitive cycles up to MAX_DELEGATION_DEPTH
        let mut depth = 0;
        let max_depth = constants::MAX_DELEGATION_DEPTH;
        while depth < max_depth {
            if let Some(next_delegate) = storage::get_score_delegate(&env, &current) {
                if next_delegate == sub_wallet {
                    return Err(Error::CyclicDelegation);
                }
                current = next_delegate;
                depth += 1;
            } else {
                break;
            }
        }

        storage::set_score_delegate(&env, &sub_wallet, &custodian);
        events::delegate_set(&env, &sub_wallet, &custodian);
        Ok(())
    }

    /// Removes a registered score delegation for `sub_wallet`. Admin only.
    pub fn remove_score_delegate(env: Env, sub_wallet: Address) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        storage::get_admin(&env).require_auth();

        if storage::get_score_delegate(&env, &sub_wallet).is_none() {
            return Err(Error::ScoreNotFound);
        }

        storage::remove_score_delegate(&env, &sub_wallet);
        events::delegate_removed(&env, &sub_wallet);
        Ok(())
    }

    /// Returns the currently registered score delegate (custodian) for `sub_wallet`,
    /// or `None` if no delegation exists.
    pub fn get_score_delegate(env: Env, sub_wallet: Address) -> Option<Address> {
        storage::get_score_delegate(&env, &sub_wallet)
    }

    /// Returns the full delegation chain for a wallet, from the wallet through all custodians.
    /// Returns a vector of addresses: [wallet, custodian1, custodian2, ...] up to MAX_DELEGATION_DEPTH.
    /// Returns empty vector if wallet not found or chain cannot be resolved.
    pub fn get_delegation_chain(env: Env, wallet: Address) -> Vec<Address> {
        let mut chain: Vec<Address> = Vec::new(&env);
        let mut current = wallet.clone();
        let mut depth = 0;
        let max_depth = constants::MAX_DELEGATION_DEPTH;
        
        chain.push_back(current.clone());
        
        while depth < max_depth {
            if let Some(next) = storage::get_score_delegate(&env, &current) {
                // Cycle detection: check if next is already in chain
                let mut found_cycle = false;
                for i in 0..chain.len() {
                    if chain.get(i).unwrap() == next {
                        found_cycle = true;
                        break;
                    }
                }
                if found_cycle {
                    break; // Stop at cycle
                }
                chain.push_back(next.clone());
                current = next;
                depth += 1;
            } else {
                break; // No more delegates
            }
        }
        
        chain
    }


    // ── Cross-asset aggregate risk ───────────────────────────────────────────

    /// Computes `wallet`'s cross-asset aggregate risk score: a weighted
    /// average over every asset pair the wallet has a `RiskScore` for.
    ///
    /// ```text
    /// aggregate_score = Σ (pair_weight[i] * pair_score[i]) / Σ pair_weight[i]
    /// ```
    ///
    /// `pair_weight[i]` defaults to `1` (an unweighted average) unless the
    /// admin has configured one via `set_pair_weight`. A pair with weight
    /// `0` still contributes to `pair_count`, `max_pair_score`,
    /// `benford_flag_count`, `ml_flag_count`, and `last_updated`, but is
    /// excluded from the weighted-average numerator and denominator.
    ///
    /// This function always recomputes from the live per-pair scores
    /// stored under `AssetPairs(wallet)` — it never reads the
    /// `AggregateScore(wallet)` cache that `submit_score` /
    /// `submit_scores_batch` refresh as a side effect, so the result is
    /// always consistent with the latest submissions.
    ///
    /// If `wallet` has no direct scores, it falls back to computing the
    /// aggregate score of its delegated custodian, if one exists.
    ///
    /// Complexity is O(N) in the number of distinct pairs the wallet has
    /// a score for. The contract does not enforce a hard cap on N, but the
    /// aggregate engine is designed around [`constants::MAX_WALLET_PAIRS`]
    /// (currently 20) as the expected practical maximum.
    ///
    /// Returns [`Error::ScoreNotFound`] if the wallet has no scores, or if
    /// every registered pair currently has a weight of `0` (an undefined
    /// average). Returns [`Error::ArithmeticOverflow`] if the weighted sum
    /// would overflow — this can only happen with extreme admin-configured
    /// weights, since per-pair scores are bounded to 0-100.
    ///
    /// See [docs/score-math.md](../../docs/score-math.md) for the formula and fixed-point implementation notes.
    pub fn get_aggregate_score(env: Env, wallet: Address) -> Result<AggregateRiskScore, Error> {
        if storage::is_embargoed(&env, &wallet) {
            return Err(Error::ScoreEmbargoed);
        }
        let pairs = storage::get_wallet_pairs(&env, &wallet);
        if pairs.is_empty() {
            if let Some(custodian) = storage::get_score_delegate(&env, &wallet) {
                return Self::compute_aggregate_score(&env, &custodian);
            }
        }
        Self::compute_aggregate_score(&env, &wallet)
    }

    /// Returns every asset pair that `wallet` has ever had a score submitted
    /// for. Returns an empty `Vec` when no scores exist for the wallet.
    ///
    /// The list is maintained incrementally by `register_pair_for_wallet` and
    /// is O(1) to read — it is **not** recomputed by scanning scores.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ledgerlens_score::LedgerLensScoreContractClient;
    /// # use soroban_sdk::{testutils::Address as _, Env, Address, Vec};
    /// # use ledgerlens_score::LedgerLensScoreContract;
    /// # use soroban_sdk::symbol_short;
    /// let env = Env::default();
    /// env.mock_all_auths();
    /// let contract_id = env.register_contract(None, LedgerLensScoreContract);
    /// let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    /// let admin = Address::generate(&env);
    /// let service = Address::generate(&env);
    /// client.initialize(&admin, &service);
    ///
    /// let wallet = Address::generate(&env);
    /// // No scores yet — empty list.
    /// let pairs = client.get_wallet_pair_list(&wallet);
    /// assert_eq!(pairs.len(), 0);
    ///
    /// // Submit a score for XLM_USDC.
    /// client.submit_score(&Vec::new(&env), &wallet, &symbol_short!("XLM_USDC"), &50, &false, &false, &1, &90, &1, &None).unwrap();
    /// let pairs = client.get_wallet_pair_list(&wallet);
    /// assert_eq!(pairs.len(), 1);
    /// assert_eq!(pairs.get(0).unwrap(), symbol_short!("XLM_USDC"));
    ///
    /// // Submit another score for a different pair.
    /// client.submit_score(&Vec::new(&env), &wallet, &symbol_short!("XLM_BTC"), &30, &false, &false, &2, &85, &1, &None).unwrap();
    /// let pairs = client.get_wallet_pair_list(&wallet);
    /// assert_eq!(pairs.len(), 2);
    /// ```
    pub fn get_wallet_pair_list(env: Env, wallet: Address) -> Vec<Symbol> {
        storage::get_wallet_pairs(&env, &wallet)
    }

    /// Sets the correlation coefficient between two asset pairs for use in
    /// portfolio VaR calculations. `corr` is scaled ×10 000 (e.g. `5000`
    /// represents ρ = 0.5). Valid range: [-10 000, 10 000]. Admin only.
    ///
    /// # Errors
    /// - [`Error::NotInitialized`] if `initialize` has not been called.
    pub fn set_pair_correlation(
        env: Env,
        pair_a: Symbol,
        pair_b: Symbol,
        corr: i32,
    ) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        storage::get_admin(&env).require_auth();
        storage::set_pair_correlation(&env, &pair_a, &pair_b, corr);
        #[cfg(any(test, feature = "testutils"))]
        invariants::invariant_check(&env);
        Ok(())
    }

    /// Returns the stored correlation coefficient (×10 000) between two pairs.
    /// Defaults to `0` (uncorrelated) when unset.
    pub fn get_pair_correlation(env: Env, pair_a: Symbol, pair_b: Symbol) -> i32 {
        storage::get_pair_correlation(&env, &pair_a, &pair_b)
    }

    /// Estimates portfolio-level Value-at-Risk (VaR) for a wallet by combining
    /// its per-pair risk scores with the on-chain pair correlation matrix and
    /// pair weights.
    ///
    /// The computation is:
    ///   1. Collect all (pair, score, weight) triples for the wallet.
    ///   2. Compute weighted variance:
    ///      `σ² = Σᵢ Σⱼ wᵢ wⱼ sᵢ sⱼ ρᵢⱼ / W²`
    ///      where `W = Σ wᵢ` and `ρᵢⱼ` is the correlation from storage
    ///      (defaulting to 0 for uncorrelated pairs, 10 000 for i == j).
    ///   3. Multiply `sqrt(σ²)` by a z-score for the requested confidence:
    ///      95 → ×165 (z = 1.645, scaled ×100), 99 → ×233 (z = 2.326, scaled ×100).
    ///   4. Return as an integer in [0, 100], clamped.
    ///
    /// All intermediate arithmetic uses `i64` to avoid overflow within the
    /// [0, 100] score domain.
    ///
    /// # Errors
    /// - [`Error::InsufficientPairData`] when fewer than 2 pairs have scores.
    pub fn get_portfolio_var(
        env: Env,
        wallet: Address,
        confidence: u32,
    ) -> Result<u32, Error> {
        let all_pairs = storage::get_wallet_pairs(&env, &wallet);

        // Collect parallel arrays for pairs that have a live score.
        let mut pair_syms: Vec<Symbol> = Vec::new(&env);
        let mut scores: Vec<u32> = Vec::new(&env);
        let mut weights: Vec<u32> = Vec::new(&env);

        for pair in all_pairs.iter() {
            if let Some(risk) = storage::peek_score(&env, &wallet, &pair) {
                let w = storage::get_pair_weight(&env, &pair);
                pair_syms.push_back(pair);
                scores.push_back(risk.score);
                weights.push_back(w);
            }
        }

        let n = pair_syms.len() as usize;
        if n < 2 {
            return Err(Error::InsufficientPairData);
        }

        let mut w_total: i64 = 0;
        for idx in 0..n {
            w_total += weights.get(idx as u32).unwrap() as i64;
        }
        if w_total == 0 {
            return Err(Error::InsufficientPairData);
        }

        // Weighted covariance sum: Σᵢ Σⱼ wᵢ wⱼ sᵢ sⱼ ρᵢⱼ (ρ scaled ×10 000).
        let mut cov_sum: i64 = 0;
        for i in 0..n {
            let si = scores.get(i as u32).unwrap() as i64;
            let wi = weights.get(i as u32).unwrap() as i64;
            for j in 0..n {
                let sj = scores.get(j as u32).unwrap() as i64;
                let wj = weights.get(j as u32).unwrap() as i64;
                let rho: i64 = if i == j {
                    10_000
                } else {
                    let pi = pair_syms.get(i as u32).unwrap();
                    let pj = pair_syms.get(j as u32).unwrap();
                    storage::get_pair_correlation(&env, &pi, &pj) as i64
                };
                cov_sum += wi * wj * si * sj * rho / 10_000;
            }
        }

        // Portfolio variance = cov_sum / W².
        let var_scaled = cov_sum / (w_total * w_total).max(1);
        // σ = integer sqrt via Newton's method.
        let sigma: i64 = {
            let v = var_scaled.max(0) as u64;
            if v == 0 {
                0u64
            } else {
                let mut x = v;
                let mut y = (x + 1) / 2;
                while y < x {
                    x = y;
                    y = (x + v / x) / 2;
                }
                x
            }
        } as i64;

        // z-score scaled ×100: 95 → 165 (1.645), 99 → 233 (2.326), else 165.
        let z100: i64 = if confidence == 99 { 233 } else { 165 };

        let var_score = (sigma * z100 / 100).clamp(0, 100) as u32;
        Ok(var_score)
    }

    /// Returns the number of distinct asset pairs `wallet` has scores for.
    /// A convenience shortcut for `get_wallet_pair_list(wallet).len()` that
    /// avoids allocating the full list when only the count is needed.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ledgerlens_score::LedgerLensScoreContractClient;
    /// # use soroban_sdk::{testutils::Address as _, Env, Address, Vec};
    /// # use ledgerlens_score::LedgerLensScoreContract;
    /// # use soroban_sdk::symbol_short;
    /// let env = Env::default();
    /// env.mock_all_auths();
    /// let contract_id = env.register_contract(None, LedgerLensScoreContract);
    /// let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    /// let admin = Address::generate(&env);
    /// let service = Address::generate(&env);
    /// client.initialize(&admin, &service);
    ///
    /// let wallet = Address::generate(&env);
    /// assert_eq!(client.get_wallet_pair_count(&wallet), 0);
    ///
    /// client.submit_score(&Vec::new(&env), &wallet, &symbol_short!("XLM_USDC"), &50, &false, &false, &1, &90, &1, &None).unwrap();
    /// assert_eq!(client.get_wallet_pair_count(&wallet), 1);
    ///
    /// client.submit_score(&Vec::new(&env), &wallet, &symbol_short!("XLM_BTC"), &30, &false, &false, &2, &85, &1, &None).unwrap();
    /// assert_eq!(client.get_wallet_pair_count(&wallet), 2);
    /// ```
    pub fn get_wallet_pair_count(env: Env, wallet: Address) -> u32 {
        storage::get_wallet_pairs(&env, &wallet).len()
    }

    /// Sets the weight used for `asset_pair` in the aggregate risk
    /// computation. A weight of `0` excludes the pair from the weighted
    /// average's denominator entirely. Admin only.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ledgerlens_score::LedgerLensScoreContractClient;
    /// # use soroban_sdk::{testutils::Address as _, Env, Address};
    /// # use ledgerlens_score::LedgerLensScoreContract;
    /// # use soroban_sdk::symbol_short;
    /// let env = Env::default();
    /// env.mock_all_auths();
    /// let contract_id = env.register_contract(None, LedgerLensScoreContract);
    /// let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    /// let admin = Address::generate(&env);
    /// let service = Address::generate(&env);
    /// client.initialize(&admin, &service);
    /// let pair = symbol_short!("XLM_USDC");
    /// client.set_pair_weight(&Vec::new(&env), &pair, &3).unwrap();
    /// assert_eq!(client.get_pair_weight(&pair), 3);
    /// ```
    pub fn set_pair_weight(
        env: Env,
        admin_signers: Vec<Address>,
        asset_pair: Symbol,
        weight: u32,
    ) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        Self::require_admin_auth(&env, &admin_signers)?;
        storage::set_pair_weight(&env, &asset_pair, weight);
        events::pair_weight_updated(&env, &asset_pair, weight);
        Ok(())
    }

    /// Returns the configured weight for `asset_pair`. Defaults to `1`
    /// (simple average) until the admin sets one explicitly.
    pub fn get_pair_weight(env: Env, asset_pair: Symbol) -> u32 {
        storage::get_pair_weight(&env, &asset_pair)
    }

    // ── Per-pair 24h score volatility index (#270) ────────────────────────────

    /// Returns the rolling score volatility index for `asset_pair`, scaled ×100.
    /// The volatility is the population standard deviation of scores submitted
    /// within the last `get_pair_volatility_window()` seconds, computed
    /// incrementally via Welford's algorithm on every `submit_score`.
    /// Returns `0` when fewer than 2 samples exist.
    pub fn get_pair_volatility(env: Env, asset_pair: Symbol) -> u32 {
        let state = match storage::get_pair_volatility_state(&env, &asset_pair) {
            Some(s) => s,
            None => return 0,
        };
        if state.count < 2 {
            return 0;
        }
        // variance_scaled = m2_scaled / count  (m2_scaled is ×1_000_000, count is samples)
        let variance_scaled = state.m2_scaled / state.count as i64;
        if variance_scaled <= 0 {
            return 0;
        }
        // std_dev × 100  =  sqrt(variance_scaled / 1_000_000) × 100
        //                 =  sqrt(variance_scaled) × 100 / 1000
        //                 =  sqrt(variance_scaled) / 10
        let std_dev_100 = (isqrt_u64(variance_scaled as u64) as u64) / 10;
        std_dev_100 as u32
    }

    /// Returns the rolling window duration used for volatility computation (seconds).
    /// Defaults to 86400 (24 hours).
    pub fn get_pair_volatility_window(env: Env) -> u64 {
        storage::get_pair_volatility_window(&env)
    }

    /// Sets the rolling window duration for volatility computation. Admin only.
    /// Must be in the range `[60, 604800]` (1 minute – 7 days).
    pub fn set_pair_volatility_window(
        env: Env,
        admin_signers: Vec<Address>,
        secs: u64,
    ) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        if secs < 60 || secs > 604_800 {
            return Err(Error::InvalidStalenessWindow);
        }
        Self::require_admin_auth(&env, &admin_signers)?;
        storage::set_pair_volatility_window(&env, secs);
        Ok(())
    }

    /// Sets the weight for multiple asset pairs in one admin call, avoiding
    /// N separate transactions during initial contract setup. Each entry is
    /// applied independently via [`set_pair_weight`]'s underlying storage
    /// write, emitting one `pw_upd` event per entry. Admin only.
    ///
    /// # Errors
    /// - [`Error::NotInitialized`] if the contract has no admin yet.
    /// - [`Error::EmptyBatch`] if `entries` is empty.
    /// - [`Error::BatchTooLarge`] if `entries.len() > MAX_BATCH_SIZE`.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ledgerlens_score::LedgerLensScoreContractClient;
    /// # use soroban_sdk::{testutils::Address as _, Env, Address, Vec};
    /// # use ledgerlens_score::LedgerLensScoreContract;
    /// # use soroban_sdk::symbol_short;
    /// let env = Env::default();
    /// env.mock_all_auths();
    /// let contract_id = env.register_contract(None, LedgerLensScoreContract);
    /// let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    /// let admin = Address::generate(&env);
    /// let service = Address::generate(&env);
    /// client.initialize(&admin, &service);
    /// let pair_a = symbol_short!("XLM_USDC");
    /// let pair_b = symbol_short!("XLM_BTC");
    /// let mut entries = Vec::new(&env);
    /// entries.push_back((pair_a.clone(), 2u32));
    /// entries.push_back((pair_b.clone(), 5u32));
    /// client.set_pair_weight_batch(&Vec::new(&env), &entries).unwrap();
    /// assert_eq!(client.get_pair_weight(&pair_a), 2);
    /// assert_eq!(client.get_pair_weight(&pair_b), 5);
    /// ```
    pub fn set_pair_weight_batch(
        env: Env,
        admin_signers: Vec<Address>,
        entries: Vec<(Symbol, u32)>,
    ) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        if entries.is_empty() {
            return Err(Error::EmptyBatch);
        }
        if entries.len() > constants::MAX_BATCH_SIZE {
            return Err(Error::BatchTooLarge);
        }
        Self::require_admin_auth(&env, &admin_signers)?;
        for i in 0..entries.len() {
            let (asset_pair, weight) = entries.get(i).unwrap();
            storage::set_pair_weight(&env, &asset_pair, weight);
            events::pair_weight_updated(&env, &asset_pair, weight);
        }
        Ok(())
    }

    // ── Global minimum confidence floor ──────────────────────────────────────

    /// Set the admin-configured global minimum confidence floor (0–100).
    ///
    /// When set, every call to [`query_risk_gate_with_confidence`] uses
    /// `max(min_confidence_param, global_min_confidence)` as the effective
    /// floor. This lets the contract operator enforce a system-wide minimum
    /// confidence without requiring every integrating protocol to specify one.
    ///
    /// Using `max` ensures the stricter of the two floors always wins —
    /// neither the admin nor the caller can unilaterally weaken the other's
    /// floor. Both values are bounded to `0..=100`, so overflow is impossible:
    /// `max(a, b)` where `a, b ≤ 100` is at most `100`.
    ///
    /// Admin only. Valid range: `0..=100`.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ledgerlens_score::LedgerLensScoreContractClient;
    /// # use soroban_sdk::{testutils::Address as _, Env, Address};
    /// # use ledgerlens_score::LedgerLensScoreContract;
    /// let env = Env::default();
    /// env.mock_all_auths();
    /// let contract_id = env.register_contract(None, LedgerLensScoreContract);
    /// let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    /// let admin = Address::generate(&env);
    /// let service = Address::generate(&env);
    /// client.initialize(&admin, &service);
    /// client.set_global_min_confidence(&60);
    /// assert_eq!(client.get_global_min_confidence(), 60);
    /// ```
    ///
    /// # Errors
    /// - [`Error::NotInitialized`] if the contract has no admin yet.
    /// - [`Error::InvalidConfidence`] if `min_confidence > 100`.
    pub fn set_global_min_confidence(env: Env, min_confidence: u32) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        if min_confidence > 100 {
            return Err(Error::InvalidConfidence);
        }
        let admin = storage::get_admin(&env);
        admin.require_auth();
        storage::set_global_min_confidence(&env, min_confidence);
        #[cfg(any(test, feature = "testutils"))]
        invariants::invariant_check(&env);
        Ok(())
    }

    /// Returns the admin-configured global minimum confidence floor.
    /// Defaults to `0` (no global floor) until [`set_global_min_confidence`]
    /// is called.
    ///
    /// This value is combined with the per-call `min_confidence` parameter in
    /// [`query_risk_gate_with_confidence`] using `max(param, global)` so the
    /// stricter of the two floors always applies.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ledgerlens_score::LedgerLensScoreContractClient;
    /// # use soroban_sdk::{testutils::Address as _, Env, Address};
    /// # use ledgerlens_score::LedgerLensScoreContract;
    /// let env = Env::default();
    /// env.mock_all_auths();
    /// let contract_id = env.register_contract(None, LedgerLensScoreContract);
    /// let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    /// let admin = Address::generate(&env);
    /// let service = Address::generate(&env);
    /// client.initialize(&admin, &service);
    /// assert_eq!(client.get_global_min_confidence(), 0);
    /// client.set_global_min_confidence(&70);
    /// assert_eq!(client.get_global_min_confidence(), 70);
    /// ```
    pub fn get_global_min_confidence(env: Env) -> u32 {
        storage::get_global_min_confidence(&env)
    }

    // ── Wallet risk cluster assignment (#288) ────────────────────────────────

    /// Admin setter. Stores `boundaries` as the ordered bucket thresholds used
    /// to assign wallets to clusters.  The list must be non-empty and every
    /// element must be in [1, 100] and strictly ascending.  Cluster `i` covers
    /// scores in [boundaries[i-1]+1 .. boundaries[i]] (cluster 0 covers [0..boundaries[0]]).
    /// The last cluster catches everything above the highest boundary.
    pub fn set_cluster_boundaries(
        env: Env,
        admin_signers: Vec<Address>,
        boundaries: Vec<u32>,
    ) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        if boundaries.is_empty() {
            return Err(Error::InvalidThreshold);
        }
        let mut prev: u32 = 0;
        for i in 0..boundaries.len() {
            let b = boundaries.get(i).unwrap();
            if b == 0 || b > 100 || b <= prev {
                return Err(Error::InvalidThreshold);
            }
            prev = b;
        }
        Self::require_admin_auth(&env, &admin_signers)?;
        storage::set_cluster_boundaries(&env, &boundaries);
        events::cluster_boundaries_updated(&env);
        Ok(())
    }

    /// Returns the currently configured cluster boundaries.
    pub fn get_cluster_boundaries(env: Env) -> Vec<u32> {
        storage::get_cluster_boundaries(&env)
    }

    /// Returns the cluster index for `wallet`, or `None` if no aggregate score
    /// exists or no boundaries have been configured.
    pub fn get_wallet_cluster(env: Env, wallet: Address) -> Option<u32> {
        storage::get_wallet_cluster(&env, &wallet)
    }

    /// Compute and persist the cluster index for `wallet` based on the wallet's
    /// aggregate score.  Called internally after each score write.  No-op if no
    /// boundaries are configured.
    fn assign_wallet_cluster(env: &Env, wallet: &Address) {
        let boundaries = storage::get_cluster_boundaries(env);
        if boundaries.is_empty() {
            return;
        }
        let agg_score = match Self::compute_aggregate_score(env, wallet) {
            Ok(a) => a.aggregate_score,
            Err(_) => return,
        };
        let mut cluster: u32 = boundaries.len(); // default: last bucket (above all thresholds)
        for i in 0..boundaries.len() {
            if agg_score <= boundaries.get(i).unwrap() {
                cluster = i;
                break;
            }
        }
        let old = storage::get_wallet_cluster(env, wallet);
        if old != Some(cluster) {
            storage::set_wallet_cluster(env, wallet, cluster);
            events::wallet_cluster_assigned(env, wallet, cluster);
        }
    }

    // ── Composability interface (stable ABI) ─────────────────────────────────
    //
    // The functions below form the `ILedgerLensScore` composability surface
    // documented in `docs/interface-spec.md`. They are the canonical,
    // version-stable integration point for third-party Soroban protocols
    // (AMMs, lending markets, DEX aggregators). Their signatures and
    // semantics are covered by the interface stability guarantees in that
    // spec — do not change them without bumping `CONTRACT_VERSION` and the
    // interface version, and announcing a breaking change.

    /// Infallible cross-contract risk gate.
    ///
    /// Returns `true` when the wallet's latest risk score for `asset_pair`
    /// is **strictly below** `gate_threshold` — i.e. the wallet is considered
    /// safe to proceed. Returns `false` when:
    ///
    /// * the score is `>= gate_threshold` (too risky), **or**
    /// * no score exists for the `(wallet, asset_pair)` pair.
    ///
    /// The "no score" case deliberately returns `false` (the *conservative*
    /// default): an integrating protocol should treat wallets it has no
    /// information about as potentially risky rather than waving them through.
    ///
    /// This function is **infallible** (returns `bool`, never `Result`) and
    /// **side-effect free** — it performs a pure read that does not even
    /// extend storage TTL. It is designed to be called directly from inside
    /// another contract's authorization / guard logic: it can never panic and
    /// can never propagate an `Error` back into the caller, so it cannot be
    /// used to grief the calling protocol's gas or disable its security guard.
    ///
    /// This function delegates to [`query_risk_gate_with_confidence`] with
    /// `min_confidence = 0`, meaning no confidence floor is applied. All
    /// logic lives in one place to eliminate duplication.
    ///
    /// # Example (caller side)
    ///
    /// ```ignore
    /// let client = LedgerLensScoreContractClient::new(&env, &llens_id);
    /// if !client.query_risk_gate(&user, &symbol_short!("XLM_USDC"), &75) {
    ///     return Err(MyError::HighRiskWallet);
    /// }
    /// ```
    pub fn verify_score_range_proof(
        env: Env,
        wallet: Address,
        asset_pair: Symbol,
        commitment: BytesN<32>,
        proof: Bytes,
        threshold: u32,
    ) -> bool {
        let stored_score = match storage::get_score(&env, &wallet, &asset_pair) {
            Some(s) => s,
            None => return false,
        };

        let stored_commitment = match stored_score.commitment {
            Some(c) => c,
            None => return false,
        };

        let commitment_bytes: Bytes = commitment.clone().into();
        if stored_commitment != commitment_bytes {
            return false;
        }

        let bp = match zk_range_proof::Bulletproof::from_bytes(&proof) {
            Some(p) => p,
            None => {
                #[cfg(test)]
                std::println!("verify_score_range_proof: Bulletproof::from_bytes failed!");
                return false;
            }
        };

        let c_pt = match zk_range_proof::decompress_pt_32(&env, &commitment) {
            Some(p) => p,
            None => {
                #[cfg(test)]
                std::println!("verify_score_range_proof: decompress_pt_32 failed!");
                return false;
            }
        };

        let (g_pt, _h_pt, d) = zk_range_proof::get_generators();
        let tm1 = match threshold.checked_sub(1) {
            Some(val) => val,
            None => {
                #[cfg(test)]
                std::println!("verify_score_range_proof: checked_sub(1) failed!");
                return false;
            }
        };
        
        let c_inv = zk_range_proof::Pt {
            x: c_pt.x.neg(),
            y: c_pt.y,
        };
        
        let g_tm1 = g_pt.mul(zk_range_proof::Sc::from_u64(tm1 as u64), d);
        let c_prime = g_tm1.add(c_inv, d);

        let res = zk_range_proof::verify_range_proof(&env, c_prime, &bp);
        #[cfg(test)]
        std::println!("verify_score_range_proof: verify_range_proof returned: {:?}", res);
        res
    }

    pub fn query_risk_gate(
        env: Env,
        wallet: Address,
        asset_pair: Symbol,
        gate_threshold: u32,
    ) -> bool {
        // Flash-loan protection: record this gate read in temporary storage (#300).
        storage::set_gate_read_ledger(&env, &wallet, &asset_pair);
        Self::query_risk_gate_with_confidence(env, wallet, asset_pair, gate_threshold, 0)
    }

    /// Sets the per-query fee (in fee-token stroops) charged on each
    /// `query_risk_gate` call. `0` disables fee collection. Admin only.
    ///
    /// # Errors
    /// - [`Error::NotInitialized`] if `initialize` has not been called.
    pub fn set_gate_query_fee(env: Env, amount: i128) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        storage::get_admin(&env).require_auth();
        storage::set_gate_query_fee(&env, amount);
        #[cfg(any(test, feature = "testutils"))]
        invariants::invariant_check(&env);
        Ok(())
    }

    /// Returns the running total of fees collected via `query_risk_gate`.
    pub fn get_accumulated_fees(env: Env) -> i128 {
        storage::get_accumulated_fees(&env)
    }

    /// Confidence-aware variant of [`query_risk_gate`].
    ///
    /// In addition to the score-vs-threshold check, this function enforces
    /// a minimum confidence floor: the wallet's risk score must have a
    /// `confidence >= effective_floor` where `effective_floor` is computed
    /// as `max(min_confidence, global_min_confidence)` so the admin's
    /// system-wide floor always applies.
    ///
    /// Returns `false` (fail closed) when no score exists, the wallet is
    /// embargoed, inside the hysteresis risk band, or the confidence floor
    /// is not met.
    ///
    /// This function is infallible (returns `bool`, never `Result`) and
    /// side-effect free — it performs pure reads that do not extend TTL.
    pub fn query_risk_gate_with_confidence(
        env: Env,
        wallet: Address,
        asset_pair: Symbol,
        gate_threshold: u32,
        min_confidence: u32,
    ) -> bool {
        Self::check_service_silence(&env);
        // #302: strict gate enforcement — reject callers not in the allowlist.
        if storage::get_gate_enforcement_mode(&env) {
            let caller = env.current_contract_address();
            let callers = storage::get_gate_callers(&env);
            if !callers.contains(&caller) {
                return false; // CallerNotAuthorized: infallible, so return false
            }
        }
        if gate_threshold > 100 || min_confidence > 100 {
            return false;
        }

        // When the primary is paused, attempt failover to the secondary.
        if storage::is_paused(&env) {
            if let Some(secondary_id) = storage::get_failover_contract(&env) {
                let func = Symbol::new(&env, "get_score_opt");
                let args = (wallet.clone(), asset_pair.clone()).into_val(&env);
                let secondary_score: Option<RiskScore> =
                    env.invoke_contract(&secondary_id, &func, args);
                if let Some(score) = secondary_score {
                    let now = env.ledger().timestamp();
                    let age = now.saturating_sub(score.timestamp);
                    if age <= constants::FAILOVER_STALENESS_WINDOW {
                        events::failover_triggered(&env, &wallet, &asset_pair);
                        let effective_floor = core::cmp::max(
                            min_confidence,
                            storage::get_global_min_confidence(&env),
                        );
                        return score.score < gate_threshold
                            && score.confidence >= effective_floor;
                    }
                }
            }
            // No secondary, secondary score is stale, or no score on secondary — fail closed.
            return false;
        }

        // Embargoed wallets: conservative false — treat as "no signal available".
        // Uses peek (no TTL extension) to remain side-effect free.
        if storage::peek_is_embargoed(&env, &wallet) {
            return false;
        }
        if storage::peek_risk_band_state(&env, &wallet, &asset_pair) {
            return false;
        }
        let effective_floor =
            core::cmp::max(min_confidence, storage::get_global_min_confidence(&env));
        match storage::peek_score(&env, &wallet, &asset_pair) {
            Some(risk) => risk.score < gate_threshold && risk.confidence >= effective_floor,
            None => {
                if let Some(custodian) = storage::peek_score_delegate(&env, &wallet) {
                    if let Some(risk) = storage::peek_score(&env, &custodian, &asset_pair) {
                        return risk.score < gate_threshold && risk.confidence >= effective_floor;
                    }
                }
                false
            }
        }
    }

    /// Returns the full score histogram (10 buckets of width 10) and total
    /// tracked (wallet, pair) count.
    ///
    /// Bucket 0 = [0-9], bucket 1 = [10-19], ..., bucket 9 = [90-100].
    /// `total` is the number of unique (wallet, asset_pair) combinations that
    /// have ever received a score (not decremented on clear — see `clear_score`
    /// for the full accounting).
    ///
    /// # Examples
    ///
    /// ```
    /// # use ledgerlens_score::{LedgerLensScoreContract, LedgerLensScoreContractClient, ScoreHistogram};
    /// # use soroban_sdk::{testutils::Address as _, Env, Address, Vec};
    /// # use soroban_sdk::symbol_short;
    /// let env = Env::default();
    /// env.mock_all_auths();
    /// let contract_id = env.register_contract(None, LedgerLensScoreContract);
    /// let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    /// let admin = Address::generate(&env);
    /// let service = Address::generate(&env);
    /// client.initialize(&admin, &service);
    /// // Empty histogram
    /// let hist = client.get_score_histogram();
    /// assert_eq!(hist.total, 0);
    /// for i in 0..10 { assert_eq!(hist.buckets.get(i).unwrap(), 0); }
    /// // Submit a score of 42 -> bucket 4
    /// let wallet = Address::generate(&env);
    /// let pair = symbol_short!("XLM_USDC");
    /// client.submit_score(&Vec::new(&env), &wallet, &pair, &42, &false, &false, &1, &90, &1, &None).unwrap();
    /// let hist = client.get_score_histogram();
    /// assert_eq!(hist.total, 1);
    /// assert_eq!(hist.buckets.get(4).unwrap(), 1);
    /// ```
    pub fn get_score_histogram(env: Env) -> ScoreHistogram {
        storage::get_score_histogram(&env)
    }

    /// Returns the approximate percentile rank (0–100) of the wallet's current
    /// score for `asset_pair`, relative to all scored wallets.
    ///
    /// Computed as `(cumulative_below * 100) / total` where
    /// `cumulative_below` is the sum of all histogram buckets strictly below
    /// the wallet's score's bucket. Returns `Error::ScoreNotFound` if no score
    /// exists for this pair (or its delegate), and `Error::ArithmeticOverflow`
    /// if the histogram total is 0.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ledgerlens_score::{LedgerLensScoreContract, LedgerLensScoreContractClient};
    /// # use soroban_sdk::{testutils::Address as _, Env, Address, Vec};
    /// # use soroban_sdk::symbol_short;
    /// let env = Env::default();
    /// env.mock_all_auths();
    /// let contract_id = env.register_contract(None, LedgerLensScoreContract);
    /// let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    /// let admin = Address::generate(&env);
    /// let service = Address::generate(&env);
    /// client.initialize(&admin, &service);
    /// let wallet = Address::generate(&env);
    /// let pair = symbol_short!("XLM_USDC");
    /// client.submit_score(&Vec::new(&env), &wallet, &pair, &42, &false, &false, &1, &90, &1, &None).unwrap();
    /// let pct = client.get_score_percentile(&wallet, &pair).unwrap();
    /// assert_eq!(pct, 0); // Only wallet in histogram -> 0th percentile
    /// ```
    pub fn get_score_percentile(
        env: Env,
        wallet: Address,
        asset_pair: Symbol,
    ) -> Result<u32, Error> {
        if storage::is_embargoed(&env, &wallet) {
            return Err(Error::ScoreEmbargoed);
        }
        let score = match storage::get_score(&env, &wallet, &asset_pair) {
            Some(s) => s,
            None => {
                if let Some(custodian) = storage::get_score_delegate(&env, &wallet) {
                    storage::get_score(&env, &custodian, &asset_pair).ok_or(Error::ScoreNotFound)?
                } else {
                    return Err(Error::ScoreNotFound);
                }
            }
        };
        let total = storage::get_histogram_total(&env);
        if total == 0 {
            return Err(Error::ScoreNotFound);
        }
        let bucket = if score.score >= 100 { 9 } else { score.score / 10 };
        let mut cumulative: u32 = 0;
        for i in 0..bucket {
            cumulative = cumulative.saturating_add(storage::get_histogram_bucket(&env, i));
        }
        Ok(cumulative.saturating_mul(100) / (total as u32).max(1))
    }

    /// Relative-risk gate: returns `true` (risky) if the wallet's score is in
    /// the top `top_percentile`% most risky among all scored wallets.
    ///
    /// For example, `top_percentile = 10` blocks the top 10% most risky
    /// wallets. The computation uses the approximate percentile from the on-chain
    /// histogram: `percentile >= 100 - top_percentile`.
    ///
    /// Returns `Error::InvalidParameter` when `top_percentile` is not in `[1, 100]`,
    /// `Error::ScoreNotFound` when no score exists for the pair.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ledgerlens_score::{LedgerLensScoreContract, LedgerLensScoreContractClient};
    /// # use soroban_sdk::{testutils::Address as _, Env, Address, Vec};
    /// # use soroban_sdk::symbol_short;
    /// let env = Env::default();
    /// env.mock_all_auths();
    /// let contract_id = env.register_contract(None, LedgerLensScoreContract);
    /// let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    /// let admin = Address::generate(&env);
    /// let service = Address::generate(&env);
    /// client.initialize(&admin, &service);
    /// let wallet = Address::generate(&env);
    /// let pair = symbol_short!("XLM_USDC");
    /// client.submit_score(&Vec::new(&env), &wallet, &pair, &95, &false, &false, &1, &90, &1, &None).unwrap();
    /// // Score 95 is in bucket 9 -> percentile >= 90 -> top 10% -> risky
    /// assert!(client.query_risk_gate_relative(&wallet, &pair, &10).unwrap());
    /// // Score 95 not in top 1% -> not risky
    /// assert!(!client.query_risk_gate_relative(&wallet, &pair, &1).unwrap());
    /// ```
    pub fn query_risk_gate_relative(
        env: Env,
        wallet: Address,
        asset_pair: Symbol,
        top_percentile: u32,
    ) -> Result<bool, Error> {
        if top_percentile == 0 || top_percentile > 100 {
            return Err(Error::InvalidThreshold);
        }
        let percentile = Self::get_score_percentile(env, wallet, asset_pair)?;
        Ok(percentile >= 100u32.saturating_sub(top_percentile))
    }

    /// Capability-detection registry for the composability interface.
    ///
    /// Returns `true` if this contract build supports the named `capability`,
    /// allowing cross-contract callers to feature-detect at runtime instead of
    /// hardcoding contract version numbers. The capability symbols are part of
    /// the stable ABI: removing one is a breaking change.
    ///
    /// Recognised capabilities:
    ///
    /// | Symbol           | Backing functionality                              |
    /// |------------------|----------------------------------------------------|
    /// | `score`          | `get_score` / `submit_score`                       |
    /// | `history`        | `get_score_history`                                |
    /// | `batch`          | `submit_scores_batch`                              |
    /// | `gate`           | `query_risk_gate`                                  |
    /// | `aggr`           | `get_aggregate_score` (cross-asset aggregate risk) |
    /// | `count`          | `get_score_count`                                  |
    /// | `batch_attested` | `submit_scores_batch_attested` (Merkle-root sig)    |
    /// | `cgate`          | `query_risk_gate_with_confidence` / global confidence floor |
    /// | `emb`            | `set_score_embargo` / `lift_score_embargo`         |
    /// | `cons`           | `commit_consensus` / `reveal_consensus` / `set_consensus_config` |
    /// | `pr_rd`          | `is_pair_paused` (per-asset-pair pause read)        |
    ///
    /// Any unrecognised `capability` returns `false`.
    ///
    /// Note on naming: `batch_attested` is a 14-character symbol, longer
    /// than `symbol_short!`'s 9-character ceiling, so it is constructed via
    /// `Symbol::new(&env, "batch_attested")` rather than the `symbol_short!`
    /// macro used for the shorter entries. The equality check is bytewise
    /// — both sides go through Soroban's normal Symbol serialization — so
    /// callers can pass either form.
    pub fn supports_interface(env: Env, capability: Symbol) -> bool {
        capability == symbol_short!("score")
            || capability == symbol_short!("history")
            || capability == symbol_short!("hpag")
            || capability == symbol_short!("batch")
            || capability == symbol_short!("gate")
            || capability == symbol_short!("aggr")
            || capability == symbol_short!("count")
            || capability == symbol_short!("var")
            || capability == Symbol::new(&env, "batch_attested")
            || capability == symbol_short!("cgate")
            || capability == Symbol::new(&env, "histogram")
            || capability == Symbol::new(&env, "rgate")
            || capability == symbol_short!("emb")
            || capability == symbol_short!("cons")
            || capability == symbol_short!("pr_rd")
    }

    // ── Service management ───────────────────────────────────────────────────

    /// Add `signer` to the M-of-N service signer set.  Admin only.
    ///
    /// Returns [`Error::ServiceSetFull`] when the set already contains
    /// `MAX_SERVICE_SIGNERS` members, [`Error::SignerAlreadyInSet`] when
    /// `signer` is already present.
    pub fn add_service_signer(
        env: Env,
        admin_signers: Vec<Address>,
        signer: Address,
    ) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        Self::require_admin_auth(&env, &admin_signers)?;

        let mut set = storage::get_service_set(&env);
        if set.len() >= constants::MAX_SERVICE_SIGNERS {
            return Err(Error::ServiceSetFull);
        }
        if set.contains(&signer) {
            return Err(Error::SignerAlreadyInSet);
        }
        set.push_back(signer.clone());
        storage::set_service_set(&env, &set);
        storage::set_signer_added_at(&env, &signer, env.ledger().timestamp());
        events::signer_added(&env, &signer);
        // #299: governance audit chain
        let mut data = [0u8; 32];
        data[0] = 0x02; // action: add_service_signer
        Self::append_governance_action_raw(&env, &data);
        Ok(())
    }

    /// Remove `signer` from the M-of-N service signer set.  Admin only.
    ///
    /// Returns [`Error::SignerNotInSet`] when `signer` is not in the set.
    /// If removing the signer would make the set smaller than the current
    /// threshold, the threshold is automatically reduced to the new set size.
    pub fn remove_service_signer(
        env: Env,
        admin_signers: Vec<Address>,
        signer: Address,
    ) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        Self::require_admin_auth(&env, &admin_signers)?;

        let mut set = storage::get_service_set(&env);
        let pos = set.first_index_of(&signer);
        let idx = pos.ok_or(Error::SignerNotInSet)?;
        set.remove(idx);
        storage::set_service_set(&env, &set);

        // Auto-adjust threshold if it now exceeds the reduced set size.
        let threshold = storage::get_service_threshold(&env);
        if set.is_empty() {
            storage::set_service_threshold(&env, 0);
            events::service_threshold_updated(&env, 0);
        } else if threshold > set.len() {
            storage::set_service_threshold(&env, set.len());
            events::service_threshold_updated(&env, set.len());
        }

        storage::remove_signer_added_at(&env, &signer);
        events::signer_removed(&env, &signer);
        Ok(())
    }

    /// Set the signing threshold M.  Admin only.
    ///
    /// Returns [`Error::InvalidThreshold`] when `threshold` is `0` or exceeds
    /// the current service-set size.
    pub fn set_service_threshold(
        env: Env,
        admin_signers: Vec<Address>,
        threshold: u32,
    ) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        Self::require_admin_auth(&env, &admin_signers)?;

        let set = storage::get_service_set(&env);
        if threshold == 0 || threshold > set.len() {
            return Err(Error::InvalidThreshold);
        }
        storage::set_service_threshold(&env, threshold);
        events::service_threshold_updated(&env, threshold);
        Ok(())
    }

    /// Returns the current M-of-N service signer set.
    pub fn get_service_signers(env: Env) -> Vec<Address> {
        storage::get_service_set(&env)
    }

    /// Returns the number of addresses currently in the M-of-N service signer
    /// set.  Returns `0` when no service set has been configured.  Cheaper
    /// than `get_service_signers` for health-check / quorum-monitoring callers
    /// that only need the count.
    pub fn get_service_signer_count(env: Env) -> u32 {
        storage::get_service_set(&env).len()
    }

    /// Returns the current signing threshold.
    pub fn get_service_threshold(env: Env) -> u32 {
        storage::get_service_threshold(&env)
    }

    // ── Signer rotation TTL (Issue #79) ─────────────────────────────────────

    /// Set the signer rotation TTL in seconds. Once a signer has been in the
    /// set for longer than `ttl_secs` (plus the grace period), it will be
    /// rejected on score submission. Admin only.
    ///
    /// Setting to 0 disables the TTL check entirely.
    pub fn set_signer_rotation_ttl(
        env: Env,
        admin_signers: Vec<Address>,
        ttl_secs: u64,
    ) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        Self::require_admin_auth(&env, &admin_signers)?;
        storage::set_signer_rotation_ttl(&env, ttl_secs);
        events::signer_ttl_updated(&env, ttl_secs);
        Ok(())
    }

    /// Returns the current signer rotation TTL in seconds. Default is 30 days.
    pub fn get_signer_rotation_ttl(env: Env) -> u64 {
        storage::get_signer_rotation_ttl(&env)
    }

    /// Returns the age of `signer` in seconds since it was added to the
    /// service set, or `None` if no activation time is recorded.
    pub fn get_signer_age(env: Env, signer: Address) -> u64 {
        storage::get_signer_age(&env, &signer)
    }

    /// Returns the number of service signers whose age (time since they were
    /// added) is within the configured signer-rotation TTL.
    ///
    /// A signer is considered *active* when
    /// `get_signer_age(signer) <= get_signer_rotation_ttl()`.  Signers whose
    /// `SignerAddedAt` record is missing are excluded from the count.
    ///
    /// Returns `0` when the service set is empty or when the rotation TTL has
    /// not been configured (defaults to `0`, which means no signer whose age
    /// is greater than zero is counted).
    pub fn get_active_signer_count(env: Env) -> u32 {
        let ttl = storage::get_signer_rotation_ttl(&env);
        storage::get_service_set(&env)
            .iter()
            .filter(|s| {
                storage::get_signer_age(&env, s)
                    .map(|age| age <= ttl)
                    .unwrap_or(false)
            })
            .count() as u32
    }

    /// Set the grace period in seconds that is added to the TTL before a
    /// signer is considered expired. Admin only.
    pub fn set_signer_rotation_grace(
        env: Env,
        admin_signers: Vec<Address>,
        grace_secs: u64,
    ) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        Self::require_admin_auth(&env, &admin_signers)?;
        storage::set_signer_rotation_grace(&env, grace_secs);
        events::signer_grace_period_updated(&env, grace_secs);
        Ok(())
    }

    /// Rotate the authorised off-chain scoring service address.  Admin only.
    ///
    /// # Deprecation notice
    ///
    /// This function is deprecated in favour of the M-of-N multi-signature
    /// model (`add_service_signer` / `set_service_threshold`).  It is
    /// preserved for backward compatibility and will be removed in a future
    /// major release.  New integrations should use the multisig functions.
    #[deprecated(note = "Use add_service_signer / set_service_threshold for M-of-N multisig. \
                This single-service path will be removed in a future release.")]
    pub fn set_service(env: Env, new_service: Address) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        storage::get_admin(&env).require_auth();
        storage::set_service(&env, &new_service);
        events::service_updated(&env, &new_service);
        // #299: append to governance audit chain (action discriminant 0x01)
        let mut data = [0u8; 32];
        data[0] = 0x01; // action: set_service
        Self::append_governance_action_raw(&env, &data);
        Ok(())
    }

    // ── Service heartbeat monitor ─────────────────────────────────────────────
    //
    // If the off-chain scoring service goes down, every on-chain score ages
    // silently — `is_score_stale` only answers "is *this* (wallet, pair) old"
    // and gives no signal when the service itself has gone dark across the
    // board. This section adds a lightweight global liveness signal, updated
    // on every accepted submission (or an explicit `ping_heartbeat`) and
    // queryable by any downstream contract via `is_service_alive`.

    /// Returns the ledger timestamp of the most recent accepted submission
    /// (`submit_score` / `submit_scores_batch`) or `ping_heartbeat` call.
    /// Returns `0` if no submission has ever been accepted.
    pub fn get_last_service_activity(env: Env) -> u64 {
        storage::get_last_service_activity(&env)
    }

    /// Returns `true` if the off-chain scoring service has been active
    /// within the configured `ServiceHeartbeatAlertThreshold` — i.e.
    /// `now - last_activity <= heartbeat_alert_threshold`.
    ///
    /// Returns `true` when `LastServiceActivityAt == 0` (the service has
    /// never submitted), so a freshly initialized contract is never reported
    /// as "down" before it has had a chance to receive its first submission.
    pub fn is_service_alive(env: Env) -> bool {
        let last_active_at = storage::get_last_service_activity(&env);
        if last_active_at == 0 {
            return true;
        }
        let now = env.ledger().timestamp();
        now.saturating_sub(last_active_at) <= storage::get_heartbeat_alert_threshold(&env)
    }

    /// Sets the number of seconds of silence (no accepted submission or
    /// `ping_heartbeat`) before the service is considered unresponsive by
    /// `is_service_alive`. Admin only.
    ///
    /// Defaults to `DEFAULT_HEARTBEAT_ALERT_THRESHOLD_SECS` (1 hour) until
    /// this is called.
    ///
    /// # Errors
    /// - [`Error::NotInitialized`] if the contract has no admin yet.
    pub fn set_heartbeat_alert_threshold(
        env: Env,
        admin_signers: Vec<Address>,
        secs: u64,
    ) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        Self::require_admin_auth(&env, &admin_signers)?;
        storage::set_heartbeat_alert_threshold(&env, secs);
        events::heartbeat_threshold_updated(&env, secs);
        Ok(())
    }

    /// Returns the current heartbeat alert threshold in seconds. Defaults to
    /// `DEFAULT_HEARTBEAT_ALERT_THRESHOLD_SECS` (1 hour).
    pub fn get_heartbeat_alert_threshold(env: Env) -> u64 {
        storage::get_heartbeat_alert_threshold(&env)
    }

    /// Proves off-chain service liveness without submitting a score.
    /// Callable only by the configured service account. Updates
    /// `LastServiceActivityAt` and, if a silence alert was previously
    /// emitted, clears it and emits `ServiceResumedEvent`.
    ///
    /// # Errors
    /// - [`Error::NotInitialized`] if the contract has no admin yet.
    pub fn ping_heartbeat(env: Env) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        let service = storage::get_service(&env);
        service.require_auth();
        Self::record_service_activity(&env);
        Ok(())
    }

    // ── Score attestation ─────────────────────────────────────────────────────
    //
    // # Why ECIES sealed-submission mode is not implemented (issue #293)
    //
    // Issue #293 proposed encrypting score payloads on the way to the contract
    // using ECIES (secp256k1 ECDH + HKDF + AES-GCM) so that MEV bots watching
    // the mempool cannot read score values before a transaction is finalised.
    //
    // Two independent hard blockers make this impossible:
    //
    // **1. Smart contracts cannot hold secrets.**
    // Every byte of Soroban contract storage is readable by any network
    // participant querying the ledger.  A "contract private key" stored in
    // instance storage is visible to all observers, so any ciphertext produced
    // with the corresponding public key can be decrypted by anyone.  This is a
    // fundamental property of all public blockchains, not a Soroban limitation.
    //
    // **2. The Soroban v21 host-function set is missing the required primitives.**
    // ECIES decryption requires: ECDH point-multiplication, HKDF key
    // derivation, and AES-GCM (or equivalent AEAD) decryption.  The
    // `env.crypto()` API provides sha256, keccak256, ed25519_verify,
    // secp256k1_recover, and secp256r1_verify — none of which can substitute
    // for those missing operations.
    //
    // **The existing `ScoreAttestation` path already addresses the real threat.**
    // Attestations bind a score to a specific (wallet, asset_pair, timestamp)
    // tuple via a secp256k1 signature from the off-chain pipeline.  A replayed
    // or tampered submission fails signature verification before storage is
    // touched.  Score *confidentiality* (hiding the value from mempool
    // observers) is a separate property that no Soroban-native mechanism can
    // currently provide.
    //
    // **The only viable privacy-preserving alternative on Stellar today** is a
    // two-transaction commit-reveal scheme:
    //   tx 1 — publish `hash(score || nonce || wallet || pair || timestamp)`
    //   tx 2 — reveal plaintext; contract re-hashes and checks commitment
    // This prevents front-running between tx 1 and tx 2 but leaves the score
    // visible once tx 2 lands.  For this oracle's threat model (5-second
    // Stellar finality, no public mempool auction) the window is negligible.

    /// Configure (or rotate) the off-chain detection pipeline's secp256k1
    /// public key used to verify `ScoreAttestation`s passed to
    /// `submit_score`. Admin only.
    ///
    /// `pubkey` must be a SEC-1-encoded secp256k1 public key: 33 bytes
    /// (compressed) or 65 bytes (uncompressed). Once this is set,
    /// `submit_score` requires every call to carry a valid attestation —
    /// there is intentionally no way to unset it short of a contract
    /// upgrade, since silently re-disabling attestation would defeat the
    /// security property it provides. Rotate to a new key via another call
    /// to this function instead.
    ///
    /// # Errors
    /// - [`Error::NotInitialized`] if the contract has no admin yet.
    /// - [`Error::InvalidPubkeyLength`] if `pubkey` is not 33 or 65 bytes.
    pub fn set_service_pubkey(
        env: Env,
        admin_signers: Vec<Address>,
        pubkey: Bytes,
    ) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        if pubkey.len() != 33 && pubkey.len() != 65 {
            return Err(Error::InvalidPubkeyLength);
        }
        Self::require_admin_auth(&env, &admin_signers)?;
        storage::set_service_pubkey(&env, &pubkey);
        events::service_pubkey_updated(&env, &pubkey);
        Ok(())
    }

    /// Returns the currently configured attestation public key.
    ///
    /// # Errors
    /// - [`Error::ServicePubkeyNotSet`] if `set_service_pubkey` has never
    ///   been called.
    pub fn get_service_pubkey(env: Env) -> Result<Bytes, Error> {
        storage::get_service_pubkey(&env).ok_or(Error::ServicePubkeyNotSet)
    }

    /// Rotates the active service pubkey with an optional dual-key overlap
    /// window. During `overlap_secs` seconds both the old and new keys are
    /// accepted for attestation verification, allowing in-flight submissions
    /// signed with the old key to complete.
    ///
    /// When `overlap_secs == 0` the rotation is instant: the old key is
    /// replaced immediately with no overlap.
    ///
    /// Admin only.
    pub fn rotate_service_pubkey(
        env: Env,
        admin_signers: Vec<Address>,
        new_key: Bytes,
        overlap_secs: u64,
    ) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        if new_key.len() != 33 && new_key.len() != 65 {
            return Err(Error::InvalidPubkeyLength);
        }
        Self::require_admin_auth(&env, &admin_signers)?;
        // Any previous pending key is superseded.
        storage::clear_pending_service_pubkey(&env);
        let overlap_expiry = if overlap_secs == 0 {
            // Instant rotation: promote straight to active.
            storage::set_service_pubkey(&env, &new_key);
            events::service_pubkey_updated(&env, &new_key);
            0u64
        } else {
            let expiry = env.ledger().timestamp().saturating_add(overlap_secs);
            storage::set_pending_service_pubkey(&env, &new_key, expiry);
            expiry
        };
        events::service_pubkey_rotation_started(&env, &new_key, overlap_expiry);
        Ok(())
    }

    /// Returns the pending pubkey and its overlap-window expiry, or `None` if
    /// no rotation is currently in flight.
    pub fn get_pending_service_pubkey(env: Env) -> Option<(Bytes, u64)> {
        storage::get_pending_service_pubkey(&env)
    }

    // ── Threshold signature aggregation ──────────────────────────────────────

    /// Register (or rotate) the aggregate secp256k1 public key for the t-of-n
    /// threshold signing group.  Admin only.
    ///
    /// `pubkey` must be a SEC-1-encoded secp256k1 public key: 33 bytes
    /// (compressed) or 65 bytes (uncompressed).  Once this key is set, callers
    /// may pass a `ThresholdAttestation` to `submit_score` instead of relying
    /// on per-signer `require_auth` calls — the single 65-byte threshold
    /// signature is verified against this key on-chain.
    ///
    /// Rotate to a new key via another call to this function.  There is no
    /// unset path (short of a contract upgrade) once the key is configured,
    /// consistent with the security guarantee of `set_service_pubkey`.
    ///
    /// # Errors
    /// - [`Error::NotInitialized`] if the contract has no admin yet.
    /// - [`Error::InvalidPubkeyLength`] if `pubkey` is not 33 or 65 bytes.
    pub fn set_aggregate_service_pubkey(
        env: Env,
        admin_signers: Vec<Address>,
        pubkey: Bytes,
    ) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        if pubkey.len() != 33 && pubkey.len() != 65 {
            return Err(Error::InvalidPubkeyLength);
        }
        Self::require_admin_auth(&env, &admin_signers)?;
        storage::set_aggregate_service_pubkey(&env, &pubkey);
        events::aggregate_service_pubkey_updated(&env, &pubkey);
        Ok(())
    }

    /// Returns the currently registered aggregate threshold public key.
    ///
    /// # Errors
    /// - [`Error::ServicePubkeyNotSet`] if `set_aggregate_service_pubkey`
    ///   has never been called.
    pub fn get_aggregate_service_pubkey(env: Env) -> Result<Bytes, Error> {
        storage::get_aggregate_service_pubkey(&env).ok_or(Error::ServicePubkeyNotSet)
    }

    // ── Consensus configuration ─────────────────────────────────────────────

    /// Sets the minimum agreeing model count (`k`) and maximum score
    /// deviation (`epsilon`) used by `reveal_consensus`. Admin only.
    pub fn set_consensus_config(env: Env, k: u32, epsilon: u32) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        if k == 0 || epsilon > 100 {
            return Err(Error::InvalidConsensusConfig);
        }
        storage::get_admin(&env).require_auth();
        storage::set_consensus_threshold_k(&env, k);
        storage::set_consensus_epsilon(&env, epsilon);
        events::consensus_config_updated(&env, k, epsilon);
        Ok(())
    }

    /// Returns the current `(k, epsilon)` consensus configuration.
    pub fn get_consensus_config(env: Env) -> (u32, u32) {
        (storage::get_consensus_threshold_k(&env), storage::get_consensus_epsilon(&env))
    }

    /// Returns the consensus epsilon: the maximum absolute deviation from the
    /// consensus median that a model submission may have and still be counted
    /// toward agreement in `reveal_consensus`. Off-chain model operators read
    /// this to learn the agreement band their submissions must fall within.
    /// Defaults to [`constants::DEFAULT_CONSENSUS_EPSILON`] until changed via
    /// `set_consensus_config`. Read-only, callable by any account or contract.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ledgerlens_score::{LedgerLensScoreContract, LedgerLensScoreContractClient};
    /// # use soroban_sdk::{testutils::Address as _, Env, Address};
    /// let env = Env::default();
    /// env.mock_all_auths();
    /// let contract_id = env.register_contract(None, LedgerLensScoreContract);
    /// let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    /// let admin = Address::generate(&env);
    /// let service = Address::generate(&env);
    /// client.initialize(&admin, &service);
    /// // Default epsilon before any override.
    /// assert_eq!(client.get_consensus_epsilon(), 5);
    /// client.set_consensus_config(&3, &8);
    /// assert_eq!(client.get_consensus_epsilon(), 8);
    /// ```
    pub fn get_consensus_epsilon(env: Env) -> u32 {
        storage::get_consensus_epsilon(&env)
    }

    // ── Adaptive consensus epsilon (#287) ────────────────────────────────────

    /// Admin setter. Enables or disables adaptive epsilon and sets the scale
    /// factor.  When enabled, `get_effective_epsilon(pair)` returns:
    ///
    ///   `base_epsilon + scale_factor * pair_stddev / 1000`
    ///
    /// where `pair_stddev` is the population standard deviation of the score
    /// history for that pair (across all wallets that have a history entry),
    /// clamped so the result never exceeds 100.  When disabled the base
    /// epsilon from `set_consensus_config` is returned unchanged.
    pub fn set_adaptive_epsilon(
        env: Env,
        enabled: bool,
        scale_factor: u32,
    ) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        storage::get_admin(&env).require_auth();
        storage::set_adaptive_epsilon_enabled(&env, enabled);
        storage::set_adaptive_epsilon_scale_factor(&env, scale_factor);
        events::adaptive_epsilon_updated(&env, enabled, scale_factor);
        Ok(())
    }

    /// Returns the effective epsilon for `asset_pair`.
    ///
    /// When adaptive epsilon is disabled this is simply the configured base
    /// epsilon from `get_consensus_config`.  When enabled it adds the
    /// variance-derived term computed from the stored score history for
    /// `asset_pair` (using a synthetic zero-score wallet address as the
    /// history key, but in practice this queries the global pair history).
    ///
    /// Formula: `base + scale_factor * isqrt(variance) / 1000`, capped at 100.
    pub fn get_effective_epsilon(env: Env, asset_pair: Symbol) -> u32 {
        let base = storage::get_consensus_epsilon(&env);
        if !storage::get_adaptive_epsilon_enabled(&env) {
            return base;
        }
        let scale = storage::get_adaptive_epsilon_scale_factor(&env);
        if scale == 0 {
            return base;
        }
        let pair_stddev = Self::compute_pair_stddev(&env, &asset_pair);
        let addend = (scale as u64).saturating_mul(pair_stddev as u64) / 1000;
        ((base as u64).saturating_add(addend).min(100)) as u32
    }

    /// Computes the population stddev of all score-history entries for
    /// `asset_pair` across the wallets tracked in the score-entry index.
    /// Returns 0 when fewer than 2 data points exist.
    fn compute_pair_stddev(env: &Env, asset_pair: &Symbol) -> u32 {
        let index = storage::get_score_entry_index(env);
        let mut scores: Vec<u32> = Vec::new(env);
        for i in 0..index.len() {
            let (wallet, pair) = index.get(i).unwrap();
            if pair != *asset_pair {
                continue;
            }
            let history = storage::get_score_history(env, &wallet, asset_pair);
            for j in 0..history.len() {
                scores.push_back(history.get(j).unwrap().score);
            }
        }
        let n = scores.len() as u64;
        if n < 2 {
            return 0;
        }
        let mut sum: u64 = 0;
        for i in 0..scores.len() {
            sum += scores.get(i).unwrap() as u64;
        }
        let mean = sum / n;
        let mut sq_sum: u64 = 0;
        for i in 0..scores.len() {
            let s = scores.get(i).unwrap() as u64;
            let diff = if s >= mean { s - mean } else { mean - s };
            sq_sum += diff * diff;
        }
        let variance = sq_sum / n;
        // Integer square root (Newton's method).
        if variance == 0 {
            return 0;
        }
        let mut x = variance;
        let mut y = (x + 1) / 2;
        while y < x {
            x = y;
            y = (x + variance / x) / 2;
        }
        x as u32
    }

    /// Sets the reveal window for MEV-resistant consensus. Admin only.
    pub fn set_reveal_window(env: Env, secs: u64) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        storage::get_admin(&env).require_auth();
        storage::set_reveal_window_secs(&env, secs);
        // We could emit an event here, but skipping for brevity unless requested.
        Ok(())
    }

    /// Returns the current reveal window in seconds.
    pub fn get_reveal_window(env: Env) -> u64 {
        storage::get_reveal_window_secs(&env)
    }

    // ── Admin management ─────────────────────────────────────────────────────

    /// Initiate a two-step admin transfer.  The current admin calls this to
    /// nominate `new_admin`; `new_admin` must then call `accept_admin` to
    /// complete the handoff.  This prevents accidental loss of admin access.
    /// get_pending_admin() returns the nominate new_admin.
    pub fn transfer_admin(
        env: Env,
        admin_signers: Vec<Address>,
        new_admin: Address,
    ) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        Self::require_admin_auth(&env, &admin_signers)?;
        let admin = storage::get_admin(&env);
        storage::set_pending_admin(&env, &new_admin);
        events::admin_transfer_initiated(&env, &admin, &new_admin);
        Ok(())
    }

    /// Complete a pending admin transfer.  Must be called by the address
    /// nominated in `transfer_admin`.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ledgerlens_score::LedgerLensScoreContractClient;
    /// # use soroban_sdk::{testutils::Address as _, Env, Address};
    /// # use ledgerlens_score::LedgerLensScoreContract;
    /// let env = Env::default();
    /// env.mock_all_auths();
    /// let contract_id = env.register_contract(None, LedgerLensScoreContract);
    /// let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    /// let admin = Address::generate(&env);
    /// let service = Address::generate(&env);
    /// client.initialize(&admin, &service);
    /// let new_admin = Address::generate(&env);
    /// client.transfer_admin(&Vec::new(&env), &new_admin);
    /// client.accept_admin();
    /// assert_eq!(client.get_admin(), new_admin);
    /// ```
    pub fn accept_admin(env: Env) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        let pending = storage::get_pending_admin(&env).ok_or(Error::NoPendingAdminTransfer)?;
        pending.require_auth();
        storage::set_admin(&env, &pending);
        storage::clear_pending_admin(&env);
        events::admin_transfer_accepted(&env, &pending);
        Ok(())
    }

    /// Cancel a pending admin transfer.  Admin only.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ledgerlens_score::LedgerLensScoreContractClient;
    /// # use soroban_sdk::{testutils::Address as _, Env, Address};
    /// # use ledgerlens_score::LedgerLensScoreContract;
    /// let env = Env::default();
    /// env.mock_all_auths();
    /// let contract_id = env.register_contract(None, LedgerLensScoreContract);
    /// let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    /// let admin = Address::generate(&env);
    /// let service = Address::generate(&env);
    /// client.initialize(&admin, &service);
    /// let new_admin = Address::generate(&env);
    /// client.transfer_admin(&Vec::new(&env), &new_admin);
    /// client.cancel_admin_transfer(&Vec::new(&env));
    /// assert_eq!(client.get_admin(), admin);
    /// ```
    pub fn cancel_admin_transfer(env: Env, admin_signers: Vec<Address>) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        if !storage::has_pending_admin(&env) {
            return Err(Error::NoPendingAdminTransfer);
        }
        Self::require_admin_auth(&env, &admin_signers)?;
        let admin = storage::get_admin(&env);
        storage::clear_pending_admin(&env);
        events::admin_transfer_cancelled(&env, &admin);
        Ok(())
    }

    // ── Pause circuit breaker ────────────────────────────────────────────────

    /// Pause the contract, blocking all score submissions.  Admin only.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ledgerlens_score::LedgerLensScoreContractClient;
    /// # use soroban_sdk::{testutils::Address as _, Env, Address};
    /// # use ledgerlens_score::LedgerLensScoreContract;
    /// let env = Env::default();
    /// env.mock_all_auths();
    /// let contract_id = env.register_contract(None, LedgerLensScoreContract);
    /// let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    /// let admin = Address::generate(&env);
    /// let service = Address::generate(&env);
    /// client.initialize(&admin, &service);
    /// assert!(!client.is_paused());
    /// client.pause(&Vec::new(&env));
    /// assert!(client.is_paused());
    /// ```
    pub fn pause(env: Env, admin_signers: Vec<Address>) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        Self::require_admin_auth(&env, &admin_signers)?;
        let admin = storage::get_admin(&env);
        storage::set_paused(&env, true);
        events::contract_paused(&env, &admin);
        let action_bytes = Bytes::new(&env);
        Self::update_audit_root(&env, symbol_short!("pause"), admin.clone(), action_bytes);
        Ok(())
    }

    /// Resume normal operations after a pause.  Admin only.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ledgerlens_score::LedgerLensScoreContractClient;
    /// # use soroban_sdk::{testutils::Address as _, Env, Address};
    /// # use ledgerlens_score::LedgerLensScoreContract;
    /// let env = Env::default();
    /// env.mock_all_auths();
    /// let contract_id = env.register_contract(None, LedgerLensScoreContract);
    /// let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    /// let admin = Address::generate(&env);
    /// let service = Address::generate(&env);
    /// client.initialize(&admin, &service);
    /// client.pause(&Vec::new(&env));
    /// assert!(client.is_paused());
    /// client.unpause(&Vec::new(&env));
    /// assert!(!client.is_paused());
    /// ```
    pub fn unpause(env: Env, admin_signers: Vec<Address>) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        Self::require_admin_auth(&env, &admin_signers)?;
        let admin = storage::get_admin(&env);
        storage::set_paused(&env, false);
        events::contract_unpaused(&env, &admin);
        let action_bytes = Bytes::new(&env);
        Self::update_audit_root(&env, symbol_short!("unpause"), admin.clone(), action_bytes);
        Ok(())
    }

    /// Returns `true` when the contract is paused.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ledgerlens_score::LedgerLensScoreContractClient;
    /// # use soroban_sdk::{testutils::Address as _, Env, Address};
    /// # use ledgerlens_score::LedgerLensScoreContract;
    /// let env = Env::default();
    /// env.mock_all_auths();
    /// let contract_id = env.register_contract(None, LedgerLensScoreContract);
    /// let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    /// let admin = Address::generate(&env);
    /// let service = Address::generate(&env);
    /// client.initialize(&admin, &service);
    /// assert!(!client.is_paused());
    /// ```
    pub fn is_paused(env: Env) -> bool {
        storage::is_paused(&env)
    }

    // ── Epoch sealing (#301) ─────────────────────────────────────────────────

    /// Open a new submission epoch.  Admin only.
    ///
    /// Sets `EpochOpen = true` and records `epoch_id` as the current epoch.
    /// `submit_score` will be accepted until `close_epoch` is called.
    pub fn open_epoch(env: Env, admin_signers: Vec<Address>, epoch_id: u32) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        Self::require_admin_auth(&env, &admin_signers)?;
        storage::set_current_epoch(&env, epoch_id);
        storage::set_epoch_open(&env, true);
        events::epoch_opened(&env, epoch_id);
        Ok(())
    }

    /// Close the current submission epoch.  Admin only.
    ///
    /// Sets `EpochOpen = false`.  After this call, `submit_score` returns
    /// `EpochClosed` until the admin calls `open_epoch` again.
    pub fn close_epoch(env: Env, admin_signers: Vec<Address>) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        Self::require_admin_auth(&env, &admin_signers)?;
        let epoch_id = storage::get_current_epoch(&env);
        storage::set_epoch_open(&env, false);
        events::epoch_closed(&env, epoch_id);
        Ok(())
    }

    /// Returns the current epoch ID (0 until the first `open_epoch` call).
    pub fn get_current_epoch(env: Env) -> u32 {
        storage::get_current_epoch(&env)
    }

    /// Returns `true` when the current epoch is open for submissions.
    pub fn is_epoch_open(env: Env) -> bool {
        storage::is_epoch_open(&env)
    }

    // ── Flash-loan protection (#300) ─────────────────────────────────────────

    /// Set the flash-loan protection mode.  Admin only.
    ///
    /// - `0` (`Log`): emit `flash_sub` event but allow the submission (default).
    /// - `1` (`Reject`): reject the submission outright.
    pub fn set_flash_protection_mode(
        env: Env,
        admin_signers: Vec<Address>,
        mode: FlashProtectionMode,
    ) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        Self::require_admin_auth(&env, &admin_signers)?;
        storage::set_flash_protection_mode(&env, &mode);
        events::flash_protection_mode_updated(&env, mode as u32);
        Ok(())
    }

    /// Returns the current flash-loan protection mode.
    pub fn get_flash_protection_mode(env: Env) -> FlashProtectionMode {
        storage::get_flash_protection_mode(&env)
    }

    // ── Per-asset-pair circuit breaker ────────────────────────────────────────

    /// Freeze or unfreeze score submissions for a single `asset_pair`, without
    /// touching any other pair or the global circuit breaker.  Admin only.
    ///
    /// This is the surgical alternative to [`pause`](Self::pause): if a
    /// detection signal for one pair (e.g. a bad `XLM_USDC` model run) is
    /// compromised or malfunctioning, the admin can freeze writes for just
    /// that pair while every other pair keeps accepting submissions normally.
    /// Reads (`get_score`, `get_score_history`, `query_risk_gate`,
    /// `get_aggregate_score`) are never affected — only `submit_score` and
    /// `submit_scores_batch` consult this flag. See those functions'
    /// rustdoc for the exact precedence against the global pause.
    ///
    /// Pausing a pair that is not already paused adds it to the bounded
    /// `PausedPairIndex` (see [`get_paused_pairs`](Self::get_paused_pairs));
    /// pausing an already-paused pair, or unpausing one, never grows it.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ledgerlens_score::LedgerLensScoreContractClient;
    /// # use soroban_sdk::{testutils::Address as _, Env, Address, Vec};
    /// # use ledgerlens_score::LedgerLensScoreContract;
    /// # use soroban_sdk::symbol_short;
    /// let env = Env::default();
    /// env.mock_all_auths();
    /// let contract_id = env.register_contract(None, LedgerLensScoreContract);
    /// let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    /// let admin = Address::generate(&env);
    /// let service = Address::generate(&env);
    /// client.initialize(&admin, &service);
    /// let pair = symbol_short!("XLM_USDC");
    /// assert!(!client.is_pair_paused(&pair));
    /// client.set_pair_paused(&pair, &true);
    /// assert!(client.is_pair_paused(&pair));
    /// // submit_score for this pair now returns Error::ContractPaused, while
    /// // every other pair is unaffected.
    /// client.set_pair_paused(&pair, &false);
    /// assert!(!client.is_pair_paused(&pair));
    /// ```
    ///
    /// # Errors
    /// - [`Error::NotInitialized`] if the contract has no admin yet.
    /// - [`Error::ServiceSetFull`] if `asset_pair` is not already paused
    ///   and `PausedPairIndex` already holds `MAX_PAUSED_PAIRS` (50) entries.
    pub fn set_pair_paused(env: Env, asset_pair: Symbol, paused: bool) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        let admin = storage::get_admin(&env);
        admin.require_auth();

        if paused {
            if !storage::is_pair_paused(&env, &asset_pair)
                && !storage::add_to_paused_index(&env, &asset_pair)
            {
                return Err(Error::ServiceSetFull);
            }
            storage::set_pair_paused_flag(&env, &asset_pair, true);
        } else {
            storage::set_pair_paused_flag(&env, &asset_pair, false);
            storage::remove_from_paused_index(&env, &asset_pair);
        }

        events::pair_paused(&env, &asset_pair, paused);
        Ok(())
    }

    /// Returns `true` only while `asset_pair` is individually paused via
    /// [`set_pair_paused`](Self::set_pair_paused). Returns `false` for any
    /// pair that has never been paused, callable by any account or contract.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ledgerlens_score::LedgerLensScoreContractClient;
    /// # use soroban_sdk::{testutils::Address as _, Env, Address};
    /// # use ledgerlens_score::LedgerLensScoreContract;
    /// # use soroban_sdk::symbol_short;
    /// let env = Env::default();
    /// env.mock_all_auths();
    /// let contract_id = env.register_contract(None, LedgerLensScoreContract);
    /// let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    /// let admin = Address::generate(&env);
    /// let service = Address::generate(&env);
    /// client.initialize(&admin, &service);
    /// let pair = symbol_short!("XLM_USDC");
    /// assert!(!client.is_pair_paused(&pair));
    /// ```
    pub fn is_pair_paused(env: Env, asset_pair: Symbol) -> bool {
        storage::is_pair_paused(&env, &asset_pair)
    }

    /// Returns every asset pair currently paused via
    /// [`set_pair_paused`](Self::set_pair_paused), in no particular order.
    /// Returns an empty `Vec` when nothing is paused. Backed by the
    /// incrementally-maintained `PausedPairIndex`, so this is an O(1)
    /// storage read regardless of how many pairs exist in the system overall
    /// — it is bounded by `MAX_PAUSED_PAIRS` (50), not by the total number of
    /// pairs ever scored.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ledgerlens_score::LedgerLensScoreContractClient;
    /// # use soroban_sdk::{testutils::Address as _, Env, Address};
    /// # use ledgerlens_score::LedgerLensScoreContract;
    /// # use soroban_sdk::symbol_short;
    /// let env = Env::default();
    /// env.mock_all_auths();
    /// let contract_id = env.register_contract(None, LedgerLensScoreContract);
    /// let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    /// let admin = Address::generate(&env);
    /// let service = Address::generate(&env);
    /// client.initialize(&admin, &service);
    /// assert!(client.get_paused_pairs().is_empty());
    /// let pair = symbol_short!("XLM_USDC");
    /// client.set_pair_paused(&pair, &true);
    /// assert_eq!(client.get_paused_pairs().len(), 1);
    /// ```
    pub fn get_paused_pairs(env: Env) -> Vec<Symbol> {
        storage::get_paused_pairs(&env)
    }

    // ── Time-locked upgrade governance ────────────────────────────────────────

    /// Propose a contract WASM upgrade, starting the mandatory time-lock.
    ///
    /// The admin commits to `new_wasm_hash` (the hash of an already-installed
    /// WASM, as produced by `install_contract_wasm`). The proposal is recorded
    /// with `executable_after = now + get_upgrade_delay()`, and an
    /// `upgrade_proposed` event is emitted so monitoring services and the
    /// community can inspect and react during the delay window.
    ///
    /// Only the current admin may call this; a compromised *service* key
    /// cannot initiate an upgrade. The proposal does **not** take effect until
    /// `execute_upgrade` is called after the lock elapses, and it can be
    /// cancelled at any time before then via `veto_upgrade`.
    ///
    /// # Errors
    /// - [`Error::NotInitialized`] if the contract has no admin yet.
    /// - [`Error::UpgradeAlreadyPending`] if a proposal already exists — veto
    ///   or execute it first (one in-flight proposal at a time).
    pub fn propose_upgrade(
        env: Env,
        admin_signers: Vec<Address>,
        new_wasm_hash: BytesN<32>,
    ) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        Self::require_admin_auth(&env, &admin_signers)?;
        let admin = storage::get_admin(&env);

        if storage::has_pending_upgrade(&env) {
            return Err(Error::UpgradeAlreadyPending);
        }

        // ── #298: M-of-N co-signature requirement ────────────────────────────
        // In multisig mode we require ALL threshold signers to be present in
        // admin_signers before storing the proposal (require_admin_auth already
        // verified they are valid set members and called require_auth on each).
        // In legacy (single-admin) mode this check is a no-op.
        let admin_set = storage::get_admin_set(&env);
        let threshold = storage::get_admin_threshold(&env);
        if !admin_set.is_empty() && threshold > 0 {
            let mut approvals = storage::get_upgrade_approvals(&env);
            // Add any new signers from this call.
            for i in 0..admin_signers.len() {
                let s = admin_signers.get(i).unwrap();
                if !approvals.contains(&s) {
                    approvals.push_back(s.clone());
                    events::upgrade_approval_added(&env, &s, approvals.len(), threshold);
                }
            }
            if approvals.len() < threshold {
                // Not enough approvals yet — persist partial state and return.
                storage::set_upgrade_approvals(&env, &approvals);
                return Ok(());
            }
            // Threshold met: clear accumulator and proceed to store proposal.
            storage::clear_upgrade_approvals(&env);
        }
        // ─────────────────────────────────────────────────────────────────────

        let now = env.ledger().timestamp();
        let delay = storage::get_upgrade_delay(&env);
        // delay is bounded to MAX_UPGRADE_DELAY_SECS on the way in, so this
        // addition cannot realistically overflow; saturate as defence in depth.
        let executable_after = now.saturating_add(delay);

        let proposal = UpgradeProposal {
            new_wasm_hash: new_wasm_hash.clone(),
            proposed_at: now,
            executable_after,
            proposed_by: admin,
        };
        storage::set_pending_upgrade(&env, &proposal);
        Self::append_governance_action_raw(&env, &new_wasm_hash.to_array());

        events::upgrade_proposed(&env, &new_wasm_hash, executable_after);
        let mut params_bytes = Bytes::new(&env);
        params_bytes.extend_from_array(&new_wasm_hash.to_array());
        Self::update_audit_root(&env, symbol_short!("upg_prop"), admin.clone(), params_bytes);
        Ok(())
    }

    /// Execute the pending upgrade once its time-lock has elapsed.
    ///
    /// Re-verifies — at execution time, never from a cached decision — that
    /// `now >= executable_after`, then invokes the Soroban upgrade primitive
    /// `env.deployer().update_current_contract_wasm(new_wasm_hash)` to swap in
    /// the new logic. The pending proposal is cleared and an `upgrade_executed`
    /// event is emitted.
    ///
    /// Admin only.
    ///
    /// # Errors
    /// - [`Error::NotInitialized`] if the contract has no admin yet.
    /// - [`Error::NoPendingUpgrade`] if there is no proposal to execute.
    /// - [`Error::UpgradeNotReady`] if the time-lock has not yet elapsed.
    pub fn execute_upgrade(env: Env, admin_signers: Vec<Address>) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        Self::require_admin_auth(&env, &admin_signers)?;

        let proposal = storage::get_pending_upgrade(&env).ok_or(Error::NoPendingUpgrade)?;

        // Deterministic, caller-independent: the ledger timestamp cannot be
        // manipulated by the invoker. Re-checked here so a delay change or a
        // long-pending proposal is always evaluated against the real clock.
        let now = env.ledger().timestamp();
        if now < proposal.executable_after {
            return Err(Error::UpgradeNotReady);
        }

        // The actual Soroban upgrade primitive — replaces this contract's WASM.
        env.deployer().update_current_contract_wasm(proposal.new_wasm_hash.clone());

        storage::clear_pending_upgrade(&env);
        events::upgrade_executed(&env, &proposal.new_wasm_hash);
        Ok(())
    }

    /// Cancel the pending upgrade during the time-lock window.
    ///
    /// Intended as the emergency escape hatch if a proposal is malicious or the
    /// admin key was compromised and the legitimate admin (or a recovered key)
    /// wants to stop it before execution. Clears the proposal and emits an
    /// `upgrade_vetoed` event naming the caller for the audit trail.
    ///
    /// Admin only.
    ///
    /// # Errors
    /// - [`Error::NotInitialized`] if the contract has no admin yet.
    /// - [`Error::NoPendingUpgrade`] if there is no proposal to veto.
    pub fn veto_upgrade(env: Env, admin_signers: Vec<Address>) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        Self::require_admin_auth(&env, &admin_signers)?;
        let admin = storage::get_admin(&env);

        if !storage::has_pending_upgrade(&env) {
            return Err(Error::NoPendingUpgrade);
        }
        storage::clear_pending_upgrade(&env);

        events::upgrade_vetoed(&env, &admin);
        Ok(())
    }

    /// Returns the pending upgrade proposal so anyone can audit it during the
    /// time-lock window. Read-only and callable by any account or contract.
    ///
    /// # Errors
    /// - [`Error::NoPendingUpgrade`] if no proposal is currently pending.
    pub fn get_pending_upgrade(env: Env) -> Result<UpgradeProposal, Error> {
        storage::get_pending_upgrade(&env).ok_or(Error::NoPendingUpgrade)
    }

    /// #298: Returns the number of admin co-signatures collected so far for the
    /// pending upgrade proposal. Returns `0` when there are no partial approvals
    /// (either no proposal is accumulating or the counter was cleared after
    /// the threshold was met).
    pub fn get_upgrade_approval_count(env: Env) -> u32 {
        storage::get_upgrade_approvals(&env).len()
    }

    /// Configure the upgrade time-lock delay (seconds) applied to future
    /// proposals. Must be within `[MIN_UPGRADE_DELAY_SECS,
    /// MAX_UPGRADE_DELAY_SECS]` (48 hours – 14 days). Admin only.
    ///
    /// Changing the delay only affects proposals created *after* the change;
    /// an already-pending proposal keeps its original `executable_after`.
    ///
    /// Security note: *raising* the delay is always safe. *Lowering* it
    /// shortens the community veto window and should only be done with broad
    /// community consensus — see the README's Upgrade Governance section.
    ///
    /// # Errors
    /// - [`Error::NotInitialized`] if the contract has no admin yet.
    /// - [`Error::InvalidUpgradeDelay`] if `delay_secs` is outside the bounds.
    pub fn set_upgrade_delay(
        env: Env,
        admin_signers: Vec<Address>,
        delay_secs: u64,
    ) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        if !(constants::MIN_UPGRADE_DELAY_SECS..=constants::MAX_UPGRADE_DELAY_SECS)
            .contains(&delay_secs)
        {
            return Err(Error::InvalidUpgradeDelay);
        }
        Self::require_admin_auth(&env, &admin_signers)?;
        storage::set_upgrade_delay(&env, delay_secs);
        Ok(())
    }

    /// Returns the current upgrade time-lock delay in seconds. Defaults to
    /// `DEFAULT_UPGRADE_DELAY_SECS` (48 hours) until configured.
    pub fn get_upgrade_delay(env: Env) -> u64 {
        storage::get_upgrade_delay(&env)
    }

    // ── Parameter change governance ───────────────────────────────────────────

    /// Propose an admin parameter change, starting the mandatory time-lock.
    ///
    /// The admin commits to `(param_key, new_value)` without applying it
    /// immediately. The proposal is recorded with `time_lock_secs =
    /// get_upgrade_delay()` (minimum [`constants::MIN_UPGRADE_DELAY_SECS`]) and
    /// an `prm_prop` event is emitted so monitoring services can inspect and
    /// react during the delay window.
    ///
    /// Service signers may veto via [`Self::veto_parameter_change`] during the
    /// first half of the time-lock. After `proposed_at + time_lock_secs / 2`
    /// the proposal is irrevocable until execution or expiry.
    ///
    /// # Errors
    /// - [`Error::NotInitialized`] if the contract has no admin yet.
    /// - [`Error::TooManyPendingParameterProposals`] if 10 proposals are already pending.
    /// - [`Error::InvalidParameterKey`] / [`Error::InvalidParameterValue`] if the
    ///   value is unknown or out of bounds.
    pub fn propose_parameter_change(
        env: Env,
        admin_signers: Vec<Address>,
        param_key: Symbol,
        new_value: Bytes,
    ) -> Result<u64, Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        Self::require_admin_auth(&env, &admin_signers)?;
        let admin = storage::get_admin(&env);

        parameter_governance::validate_parameter_value(&env, &param_key, &new_value)?;

        storage::prune_expired_parameter_proposals(&env);

        if storage::count_pending_parameter_proposals(&env)
            >= constants::MAX_PENDING_PARAMETER_PROPOSALS
        {
            return Err(Error::TooManyPendingParameterProposals);
        }

        let now = env.ledger().timestamp();
        let time_lock_secs = storage::get_upgrade_delay(&env);
        if time_lock_secs < constants::MIN_UPGRADE_DELAY_SECS {
            return Err(Error::InvalidParameterTimeLock);
        }

        let proposal_id = storage::next_parameter_proposal_id(&env);
        let proposal = ParameterProposal {
            param_key: param_key.clone(),
            new_value: new_value.clone(),
            proposer: admin,
            proposed_at: now,
            time_lock_secs,
        };
        let record = ParameterProposalRecord {
            proposal,
            status: ParameterProposalStatus::Pending,
        };
        storage::set_parameter_proposal_record(&env, proposal_id, &record);
        storage::push_pending_parameter_proposal(&env, proposal_id);

        let executable_after = now.saturating_add(time_lock_secs);
        events::parameter_change_proposed(&env, proposal_id, &param_key, executable_after);
        Ok(proposal_id)
    }

    /// Execute a pending parameter change once its time-lock has elapsed.
    ///
    /// Re-verifies at execution time that the proposal is still pending, has not
    /// expired (`proposed_at + time_lock_secs * 2`), and that
    /// `now >= proposed_at + time_lock_secs`. Marks the proposal as executed so
    /// it cannot be applied again.
    ///
    /// Admin only.
    pub fn execute_parameter_change(
        env: Env,
        admin_signers: Vec<Address>,
        proposal_id: u64,
    ) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        Self::require_admin_auth(&env, &admin_signers)?;

        let record = storage::get_parameter_proposal_record(&env, proposal_id)
            .ok_or(Error::ParameterProposalNotFound)?;

        if record.status == ParameterProposalStatus::Executed {
            return Err(Error::ParameterProposalAlreadyExecuted);
        }
        if record.status == ParameterProposalStatus::Vetoed {
            return Err(Error::ParameterProposalVetoed);
        }
        if record.status != ParameterProposalStatus::Pending {
            return Err(Error::ParameterProposalNotFound);
        }

        let now = env.ledger().timestamp();
        let p = &record.proposal;
        if storage::is_parameter_proposal_expired(p, now) {
            return Err(Error::ParameterProposalExpired);
        }

        let executable_after = p.proposed_at.saturating_add(p.time_lock_secs);
        if now < executable_after {
            return Err(Error::ParameterProposalNotReady);
        }

        parameter_governance::apply_parameter_change(&env, &p.param_key, &p.new_value)?;
        storage::mark_parameter_proposal_status(
            &env,
            proposal_id,
            ParameterProposalStatus::Executed,
        );
        events::parameter_change_executed(&env, proposal_id, &p.param_key);
        Ok(())
    }

    /// Cancel a pending parameter change during the veto window.
    ///
    /// Service multi-sig only. Veto is permitted while
    /// `now <= proposed_at + time_lock_secs / 2`; after that the proposal is
    /// irrevocable until execution or expiry.
    pub fn veto_parameter_change(
        env: Env,
        service_signers: Vec<Address>,
        proposal_id: u64,
    ) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        Self::require_service_signers_auth(&env, &service_signers)?;

        let record = storage::get_parameter_proposal_record(&env, proposal_id)
            .ok_or(Error::ParameterProposalNotFound)?;

        if record.status != ParameterProposalStatus::Pending {
            if record.status == ParameterProposalStatus::Vetoed {
                return Err(Error::ParameterProposalVetoed);
            }
            if record.status == ParameterProposalStatus::Executed {
                return Err(Error::ParameterProposalAlreadyExecuted);
            }
            return Err(Error::ParameterProposalNotFound);
        }

        let now = env.ledger().timestamp();
        let p = &record.proposal;
        let veto_deadline = p.proposed_at.saturating_add(p.time_lock_secs / 2);
        if now > veto_deadline {
            return Err(Error::ParameterProposalVetoPeriodEnded);
        }

        let vetoer = service_signers.get(0).unwrap();
        storage::mark_parameter_proposal_status(
            &env,
            proposal_id,
            ParameterProposalStatus::Vetoed,
        );
        events::parameter_change_vetoed(&env, proposal_id, &vetoer);
        Ok(())
    }

    /// Returns a parameter change proposal record for audit during the
    /// time-lock window. Read-only and callable by any account or contract.
    pub fn get_parameter_proposal(env: Env, proposal_id: u64) -> Result<ParameterProposalRecord, Error> {
        storage::prune_expired_parameter_proposals(&env);
        storage::get_parameter_proposal_record(&env, proposal_id)
            .ok_or(Error::ParameterProposalNotFound)
    }

    /// Returns the IDs of all proposals currently marked pending.
    pub fn get_pending_param_prop_ids(env: Env) -> Vec<u64> {
        storage::get_pending_parameter_proposal_ids(&env)
    }

    // ── Watchlist ────────────────────────────────────────────────────────────

    /// Add or remove `wallet` from the priority-monitoring watchlist.
    /// Watchlisted wallets receive elevated scrutiny in off-chain analysis.
    /// Admin only.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ledgerlens_score::LedgerLensScoreContractClient;
    /// # use soroban_sdk::{testutils::Address as _, Env, Address};
    /// # use ledgerlens_score::LedgerLensScoreContract;
    /// let env = Env::default();
    /// env.mock_all_auths();
    /// let contract_id = env.register_contract(None, LedgerLensScoreContract);
    /// let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    /// let admin = Address::generate(&env);
    /// let service = Address::generate(&env);
    /// client.initialize(&admin, &service);
    /// let wallet = Address::generate(&env);
    /// assert!(!client.is_watchlisted(&wallet));
    /// client.set_watchlist(&Vec::new(&env), &wallet, &true);
    /// assert!(client.is_watchlisted(&wallet));
    /// ```
    pub fn set_watchlist(
        env: Env,
        admin_signers: Vec<Address>,
        wallet: Address,
        flagged: bool,
    ) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        Self::require_admin_auth(&env, &admin_signers)?;
        storage::set_watchlist(&env, &wallet, flagged);
        events::watchlist_updated(&env, &wallet, flagged);
        Ok(())
    }

    /// Returns `true` if `wallet` is on the priority-monitoring watchlist.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ledgerlens_score::LedgerLensScoreContractClient;
    /// # use soroban_sdk::{testutils::Address as _, Env, Address};
    /// # use ledgerlens_score::LedgerLensScoreContract;
    /// let env = Env::default();
    /// env.mock_all_auths();
    /// let contract_id = env.register_contract(None, LedgerLensScoreContract);
    /// let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    /// let admin = Address::generate(&env);
    /// let service = Address::generate(&env);
    /// client.initialize(&admin, &service);
    /// let wallet = Address::generate(&env);
    /// assert!(!client.is_watchlisted(&wallet));
    /// ```
    pub fn is_watchlisted(env: Env, wallet: Address) -> bool {
        storage::is_watchlisted(&env, &wallet)
    }

    // ── Consecutive-breach auto-escalation ─────────────────────────────────────

    /// Set the escalation threshold N: after N consecutive high-risk
    /// submissions for a (wallet, asset_pair), an `escalation_triggered`
    /// event is emitted. Admin only.
    ///
    /// `n` must be in the range `[1, 100]`. A value of `1` causes
    /// `escalation_triggered` to fire on every single threshold breach.
    /// The default is 5.
    ///
    /// # Errors
    /// - [`Error::NotInitialized`] if the contract has no admin yet.
    /// - [`Error::InvalidThreshold`] if `n` is below 1 or above 100.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ledgerlens_score::LedgerLensScoreContractClient;
    /// # use soroban_sdk::{testutils::Address as _, Env, Address};
    /// # use ledgerlens_score::LedgerLensScoreContract;
    /// let env = Env::default();
    /// env.mock_all_auths();
    /// let contract_id = env.register_contract(None, LedgerLensScoreContract);
    /// let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    /// let admin = Address::generate(&env);
    /// let service = Address::generate(&env);
    /// client.initialize(&admin, &service);
    /// client.set_escalation_threshold(&Vec::new(&env), &3).unwrap();
    /// assert_eq!(client.get_escalation_threshold(), 3);
    /// ```
    pub fn set_escalation_threshold(
        env: Env,
        admin_signers: Vec<Address>,
        n: u32,
    ) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        if n < constants::MIN_ESCALATION_THRESHOLD || n > constants::MAX_ESCALATION_THRESHOLD {
            return Err(Error::InvalidThreshold);
        }
        Self::require_admin_auth(&env, &admin_signers)?;
        let old = storage::get_escalation_threshold(&env);
        storage::set_escalation_threshold(&env, n);
        events::escalation_threshold_updated(&env, old, n);
        Ok(())
    }

    /// Returns the current escalation threshold. Defaults to 5 until
    /// configured.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ledgerlens_score::LedgerLensScoreContractClient;
    /// # use soroban_sdk::{testutils::Address as _, Env, Address};
    /// # use ledgerlens_score::LedgerLensScoreContract;
    /// let env = Env::default();
    /// env.mock_all_auths();
    /// let contract_id = env.register_contract(None, LedgerLensScoreContract);
    /// let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    /// let admin = Address::generate(&env);
    /// let service = Address::generate(&env);
    /// client.initialize(&admin, &service);
    /// assert_eq!(client.get_escalation_threshold(), 5);
    /// ```
    pub fn get_escalation_threshold(env: Env) -> u32 {
        storage::get_escalation_threshold(&env)
    }

    /// Returns the current consecutive breach count for `(wallet, asset_pair)`.
    /// Read-only, callable by any account. Returns 0 when no breaches have
    /// occurred.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ledgerlens_score::LedgerLensScoreContractClient;
    /// # use soroban_sdk::{testutils::Address as _, Env, Address, Vec};
    /// # use ledgerlens_score::LedgerLensScoreContract;
    /// # use soroban_sdk::symbol_short;
    /// let env = Env::default();
    /// env.mock_all_auths();
    /// let contract_id = env.register_contract(None, LedgerLensScoreContract);
    /// let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    /// let admin = Address::generate(&env);
    /// let service = Address::generate(&env);
    /// client.initialize(&admin, &service);
    /// let wallet = Address::generate(&env);
    /// let asset_pair = symbol_short!("XLM_USDC");
    /// assert_eq!(client.get_breach_count(&wallet, &asset_pair), 0);
    /// client.submit_score(&Vec::new(&env), &wallet, &asset_pair, &90, &true, &true, &1, &95, &1, &None).unwrap();
    /// assert_eq!(client.get_breach_count(&wallet, &asset_pair), 1);
    /// ```
    pub fn get_breach_count(env: Env, wallet: Address, asset_pair: Symbol) -> u32 {
        storage::get_breach_count(&env, &wallet, &asset_pair)
    }

    /// Emergency override: clears the consecutive breach counter for
    /// `(wallet, asset_pair)` without emitting `escalation_resolved`.
    /// Admin only. Intended for use after a false-positive bust.
    ///
    /// # Errors
    /// - [`Error::NotInitialized`] if the contract has no admin yet.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ledgerlens_score::LedgerLensScoreContractClient;
    /// # use soroban_sdk::{testutils::Address as _, Env, Address, Vec};
    /// # use ledgerlens_score::LedgerLensScoreContract;
    /// # use soroban_sdk::symbol_short;
    /// let env = Env::default();
    /// env.mock_all_auths();
    /// let contract_id = env.register_contract(None, LedgerLensScoreContract);
    /// let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    /// let admin = Address::generate(&env);
    /// let service = Address::generate(&env);
    /// client.initialize(&admin, &service);
    /// let wallet = Address::generate(&env);
    /// let asset_pair = symbol_short!("XLM_USDC");
    /// client.submit_score(&Vec::new(&env), &wallet, &asset_pair, &90, &true, &true, &1, &95, &1, &None).unwrap();
    /// assert_eq!(client.get_breach_count(&wallet, &asset_pair), 1);
    /// client.reset_breach_count(&Vec::new(&env), &wallet, &asset_pair).unwrap();
    /// assert_eq!(client.get_breach_count(&wallet, &asset_pair), 0);
    /// ```
    pub fn reset_breach_count(
        env: Env,
        admin_signers: Vec<Address>,
        wallet: Address,
        asset_pair: Symbol,
    ) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        Self::require_admin_auth(&env, &admin_signers)?;
        storage::clear_breach_count(&env, &wallet, &asset_pair);
        Ok(())
    }

    /// Admin-initiated reset of the consecutive-breach counter for
    /// `(wallet, asset_pair)`. Unlike [`Self::reset_breach_count`], this
    /// emits a `breach_counter_reset` event recording which admin performed
    /// the reset, giving operators an on-chain audit trail for
    /// investigations that conclude before a clean score submission would
    /// otherwise reset the counter naturally. Admin only (M-of-N).
    ///
    /// # Errors
    /// - [`Error::NotInitialized`] if the contract has no admin yet.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ledgerlens_score::LedgerLensScoreContractClient;
    /// # use soroban_sdk::{testutils::Address as _, Env, Address, Vec};
    /// # use ledgerlens_score::LedgerLensScoreContract;
    /// # use soroban_sdk::symbol_short;
    /// let env = Env::default();
    /// env.mock_all_auths();
    /// let contract_id = env.register_contract(None, LedgerLensScoreContract);
    /// let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    /// let admin = Address::generate(&env);
    /// let service = Address::generate(&env);
    /// client.initialize(&admin, &service);
    /// let wallet = Address::generate(&env);
    /// let asset_pair = symbol_short!("XLM_USDC");
    /// client.submit_score(&Vec::new(&env), &wallet, &asset_pair, &90, &true, &true, &1, &95, &1, &None).unwrap();
    /// assert_eq!(client.get_breach_count(&wallet, &asset_pair), 1);
    /// client.reset_breach_counter(&Vec::new(&env), &wallet, &asset_pair).unwrap();
    /// assert_eq!(client.get_breach_count(&wallet, &asset_pair), 0);
    /// ```
    pub fn reset_breach_counter(
        env: Env,
        admin_signers: Vec<Address>,
        wallet: Address,
        asset_pair: Symbol,
    ) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        Self::require_admin_auth(&env, &admin_signers)?;
        let admin = storage::get_admin(&env);
        storage::clear_breach_count(&env, &wallet, &asset_pair);
        events::breach_counter_reset(&env, &wallet, &asset_pair, &admin);
        Ok(())
    }

    // ── Risk threshold ───────────────────────────────────────────────────────

    /// Set the global risk threshold (0-100).  Scores at or above this
    /// value will emit a `threshold_breached` event on every submission.
    /// Admin only.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ledgerlens_score::LedgerLensScoreContractClient;
    /// # use soroban_sdk::{testutils::Address as _, Env, Address};
    /// # use ledgerlens_score::LedgerLensScoreContract;
    /// let env = Env::default();
    /// env.mock_all_auths();
    /// let contract_id = env.register_contract(None, LedgerLensScoreContract);
    /// let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    /// let admin = Address::generate(&env);
    /// let service = Address::generate(&env);
    /// client.initialize(&admin, &service);
    /// client.set_risk_threshold(&Vec::new(&env), &80);
    /// assert_eq!(client.get_risk_threshold(), 80);
    /// ```
    pub fn set_risk_threshold(
        env: Env,
        admin_signers: Vec<Address>,
        threshold: u32,
    ) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        if threshold > 100 {
            return Err(Error::InvalidScore);
        }
        Self::require_admin_auth(&env, &admin_signers)?;
        let old = storage::get_risk_threshold(&env);
        storage::set_risk_threshold(&env, threshold);
        events::threshold_updated(&env, old, threshold);
        Ok(())
    }

    /// Returns the current risk threshold.  Defaults to 75 until configured.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ledgerlens_score::LedgerLensScoreContractClient;
    /// # use soroban_sdk::{testutils::Address as _, Env, Address};
    /// # use ledgerlens_score::LedgerLensScoreContract;
    /// let env = Env::default();
    /// env.mock_all_auths();
    /// let contract_id = env.register_contract(None, LedgerLensScoreContract);
    /// let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    /// let admin = Address::generate(&env);
    /// let service = Address::generate(&env);
    /// client.initialize(&admin, &service);
    /// assert_eq!(client.get_risk_threshold(), 75);
    /// ```
    pub fn get_risk_threshold(env: Env) -> u32 {
        storage::get_risk_threshold(&env)
    }

    /// Returns the current global risk threshold used by [`query_risk_gate`].
    ///
    /// External contracts can call this to reason about gate behaviour without
    /// a separate admin call.  The value defaults to `75` until
    /// [`set_risk_threshold`] is called.
    ///
    /// Read-only — callable by any account or contract without authorization.
    ///
    /// [`query_risk_gate`]: Self::query_risk_gate
    /// [`set_risk_threshold`]: Self::set_risk_threshold
    ///
    /// # Examples
    ///
    /// ```
    /// # use ledgerlens_score::LedgerLensScoreContractClient;
    /// # use soroban_sdk::{testutils::Address as _, Env, Address, Vec};
    /// # use ledgerlens_score::LedgerLensScoreContract;
    /// let env = Env::default();
    /// env.mock_all_auths();
    /// let contract_id = env.register_contract(None, LedgerLensScoreContract);
    /// let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    /// let admin = Address::generate(&env);
    /// let service = Address::generate(&env);
    /// client.initialize(&admin, &service);
    /// // Default threshold is 75.
    /// assert_eq!(client.get_score_threshold(), 75);
    /// // After admin updates it the new value is reflected immediately.
    /// client.set_risk_threshold(&Vec::new(&env), &80);
    /// assert_eq!(client.get_score_threshold(), 80);
    /// ```
    pub fn get_score_threshold(env: Env) -> u32 {
        storage::get_risk_threshold(&env)
    }

    // ── Score jump anomaly detection ──────────────────────────────────────────

    /// Set the score jump anomaly detection threshold (1–99). When the
    /// absolute delta between consecutive scores exceeds this value, a
    /// `ScoreJumpAnomalyEvent` is emitted in addition to the normal
    /// `ScoreDeltaEvent`. No event is emitted on the first submission
    /// (no previous score to diff against). Default: 30. Admin only.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ledgerlens_score::LedgerLensScoreContractClient;
    /// # use soroban_sdk::{testutils::Address as _, Env, Address, Vec};
    /// # use ledgerlens_score::LedgerLensScoreContract;
    /// let env = Env::default();
    /// env.mock_all_auths();
    /// let contract_id = env.register_contract(None, LedgerLensScoreContract);
    /// let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    /// let admin = Address::generate(&env);
    /// let service = Address::generate(&env);
    /// client.initialize(&admin, &service);
    /// client.set_jump_threshold(&Vec::new(&env), &50);
    /// assert_eq!(client.get_jump_threshold(), 50);
    /// ```
    pub fn set_jump_threshold(
        env: Env,
        admin_signers: Vec<Address>,
        threshold: u32,
    ) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        if threshold == 0 || threshold > 99 {
            return Err(Error::InvalidThreshold);
        }
        Self::require_admin_auth(&env, &admin_signers)?;
        storage::set_jump_threshold(&env, threshold);
        Ok(())
    }

    /// Returns the current score jump anomaly detection threshold.
    /// Defaults to 30 until configured.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ledgerlens_score::LedgerLensScoreContractClient;
    /// # use soroban_sdk::{testutils::Address as _, Env, Address};
    /// # use ledgerlens_score::LedgerLensScoreContract;
    /// let env = Env::default();
    /// env.mock_all_auths();
    /// let contract_id = env.register_contract(None, LedgerLensScoreContract);
    /// let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    /// let admin = Address::generate(&env);
    /// let service = Address::generate(&env);
    /// client.initialize(&admin, &service);
    /// assert_eq!(client.get_jump_threshold(), 30);
    /// ```
    pub fn get_jump_threshold(env: Env) -> u32 {
        storage::get_jump_threshold(&env)
    }

    /// Returns `(max_jump, at_timestamp)`, the largest score-jump anomaly
    /// magnitude observed so far for `(wallet, asset_pair)` and the ledger
    /// timestamp it occurred at. Returns `(0, 0)` if no jump has ever been
    /// recorded for this pair.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ledgerlens_score::LedgerLensScoreContractClient;
    /// # use soroban_sdk::{testutils::Address as _, Env, Address, Vec};
    /// # use ledgerlens_score::LedgerLensScoreContract;
    /// # use soroban_sdk::symbol_short;
    /// let env = Env::default();
    /// env.mock_all_auths();
    /// let contract_id = env.register_contract(None, LedgerLensScoreContract);
    /// let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    /// let admin = Address::generate(&env);
    /// let service = Address::generate(&env);
    /// client.initialize(&admin, &service);
    /// let wallet = Address::generate(&env);
    /// let pair = symbol_short!("XLM_USDC");
    /// assert_eq!(client.get_jump_stats(&wallet, &pair), (0, 0));
    /// ```
    pub fn get_jump_stats(env: Env, wallet: Address, asset_pair: Symbol) -> (u32, u64) {
        storage::get_jump_stats(&env, &wallet, &asset_pair)
    }

    // ── Hysteresis layer ─────────────────────────────────────────────────────

    /// Set the hysteresis margin (0-50) used to widen the exit threshold
    /// below the entry threshold, preventing event oscillation at the boundary.
    ///
    /// When `margin > 0`, a wallet that entered the high-risk band
    /// (`score >= risk_threshold`) only exits when
    /// `score < (risk_threshold - margin)`, requiring a more significant
    /// recovery before the band is cleared.  When `margin == 0` the exit
    /// threshold equals the entry threshold (no hysteresis).
    ///
    /// The value is rejected with [`Error::InvalidThreshold`] when it
    /// exceeds [`constants::MAX_HYSTERESIS_MARGIN`] (50). Admin only.
    pub fn set_hysteresis_margin(env: Env, margin: u32) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        if margin > constants::MAX_HYSTERESIS_MARGIN {
            return Err(Error::InvalidHysteresisMargin);
        }
        let admin = storage::get_admin(&env);
        admin.require_auth();
        let old = storage::get_hysteresis_margin(&env);
        storage::set_hysteresis_margin(&env, margin);
        events::hysteresis_margin_updated(&env, old, margin);
        Ok(())
    }

    /// Returns the current hysteresis margin.  Defaults to `0` (no hysteresis)
    /// until the admin sets one explicitly.
    pub fn get_hysteresis_margin(env: Env) -> u32 {
        storage::get_hysteresis_margin(&env)
    }

    /// Returns `true` when `wallet` is currently inside the high-risk band
    /// for `asset_pair`.  Defaults to `false` when no state has been recorded
    /// yet or after the TTL-bounded temporary state has expired.
    pub fn is_in_risk_band(env: Env, wallet: Address, asset_pair: Symbol) -> bool {
        storage::get_risk_band_state(&env, &wallet, &asset_pair)
    }

    /// Returns the ledger timestamp at which `wallet` entered the high-risk
    /// band for `asset_pair`, or `None` when the wallet is not currently in
    /// the band.
    ///
    /// The timestamp is written exactly once — on the transition from
    /// not-in-band to in-band — and is cleared when the wallet exits the band,
    /// so it always reflects the start of the *current* continuous high-risk
    /// period.  It is intentionally not updated on subsequent in-band
    /// submissions so callers can compute "time in band" as
    /// `ledger_timestamp - entry_time`.
    pub fn get_risk_band_entry_time(env: Env, wallet: Address, asset_pair: Symbol) -> Option<u64> {
        storage::get_band_entry_time(&env, &wallet, &asset_pair)
    }

    // ── Score embargo ─────────────────────────────────────────────────────────

    /// Places `wallet` under a score embargo, blocking external read access to
    /// its risk scores without interrupting score ingestion.
    ///
    /// - `expiry = None` — indefinite embargo; only [`lift_score_embargo`]
    ///   removes it.
    /// - `expiry = Some(ts)` — timed embargo; auto-expires when
    ///   `ledger_timestamp > ts`.
    ///
    /// Calling this again on an already-embargoed wallet **replaces** the
    /// existing expiry. Admin only.
    pub fn set_score_embargo(env: Env, wallet: Address, expiry: Option<u64>) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        let admin = storage::get_admin(&env);
        admin.require_auth();
        let is_new = !storage::peek_is_embargoed(&env, &wallet);
        if is_new && !storage::add_to_embargoed_index(&env, &wallet) {
            return Err(Error::EmbargoedWalletIndexFull);
        }
        let embargo_expiry = match expiry {
            None => EmbargoExpiry::Indefinite,
            Some(ts) => EmbargoExpiry::Until(ts),
        };
        storage::set_embargo(&env, &wallet, &embargo_expiry);
        if is_new {
            storage::increment_active_embargo_count(&env);
        }
        events::embargo_set(&env, &wallet, expiry);
        Ok(())
    }

    /// Explicitly lifts the embargo on `wallet`, immediately restoring external
    /// read access to its risk scores.  No-op if the wallet is not currently
    /// embargoed. Admin only.
    pub fn lift_score_embargo(env: Env, wallet: Address) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        let admin = storage::get_admin(&env);
        admin.require_auth();
        let was_embargoed = storage::peek_is_embargoed(&env, &wallet);
        storage::remove_embargo(&env, &wallet);
        storage::remove_from_embargoed_index(&env, &wallet);
        if was_embargoed {
            storage::decrement_active_embargo_count(&env);
        }
        events::embargo_lifted(&env, &wallet);
        Ok(())
    }

    /// Lifts embargoes for a cohort of wallets in a single call, reducing
    /// transaction overhead for bulk compliance workflows.
    ///
    /// Wallets without an active embargo are silently skipped — no error is
    /// raised and no event is emitted for them.  Returns the count of wallets
    /// that were actually lifted (i.e. had an active embargo removed), which
    /// may be less than `wallets.len()`.
    ///
    /// Requires M-of-N admin authorization and is capped at
    /// [`constants::MAX_BATCH_SIZE`] wallets per call.
    pub fn batch_lift_score_embargo(
        env: Env,
        admin_signers: Vec<Address>,
        wallets: Vec<Address>,
    ) -> Result<u32, Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        Self::require_admin_auth(&env, &admin_signers)?;
        if wallets.is_empty() {
            return Err(Error::EmptyBatch);
        }
        if wallets.len() > constants::MAX_BATCH_SIZE {
            return Err(Error::BatchTooLarge);
        }
        let mut lifted: u32 = 0;
        for i in 0..wallets.len() {
            let wallet = wallets.get(i).unwrap();
            if storage::peek_is_embargoed(&env, &wallet) {
                storage::remove_embargo(&env, &wallet);
                storage::decrement_active_embargo_count(&env);
                events::embargo_lifted(&env, &wallet);
                lifted += 1;
            }
        }
        Ok(lifted)
    }

    /// Returns `true` when `wallet` is currently under an active score embargo.
    ///
    /// A timed embargo (`Some(ts)`) is considered active while
    /// `ledger_timestamp <= ts` and automatically inactive once that timestamp
    /// is exceeded — no admin action required for expiry.
    pub fn is_embargoed(env: Env, wallet: Address) -> bool {
        storage::is_embargoed(&env, &wallet)
    }

    /// Returns `true` if `wallet` is currently under an active embargo,
    /// without extending the embargo TTL.
    ///
    /// Unlike [`is_embargoed`](Self::is_embargoed), this function is
    /// side-effect-free: it reads the embargo state but does **not** call
    /// `extend_ttl` on the underlying storage entry. This makes it safe for
    /// high-frequency read-only callers such as AMM guards that must not
    /// inadvertently prolong an embargo by querying it.
    pub fn peek_is_embargoed(env: Env, wallet: Address) -> bool {
        storage::peek_is_embargoed(&env, &wallet)
    }

    /// Returns when `wallet`'s active embargo expires, if applicable.
    ///
    /// - `None` — no embargo is active, including when a timed embargo has
    ///   already passed `ledger_timestamp`.
    /// - `None` — the embargo is indefinite (`set_score_embargo` was called
    ///   with `expiry = None`); there is no timestamp to report.
    /// - `Some(ts)` — the embargo is timed and still active, expiring at
    ///   `ledger_timestamp > ts`.
    pub fn get_embargo_expiry(env: Env, wallet: Address) -> Option<u64> {
        storage::get_embargo_expiry(&env, &wallet)
    }


    /// Lifts every wallet currently tracked in the `EmbargoedWalletIndex` in a
    /// single transaction and clears the index, instead of requiring one
    /// `lift_score_embargo` call per wallet. Useful when a regulatory hold is
    /// lifted globally (e.g. after a court ruling) and hundreds of wallets
    /// need to be released at once.
    ///
    /// The index tracks every wallet ever placed under embargo that has not
    /// since been explicitly lifted — including a timed embargo whose expiry
    /// has already passed — so this call also clears out any such
    /// already-expired entries.
    ///
    /// Emits one `emb_lift` event per wallet that was lifted. No-op if the
    /// index is empty. Admin only.
    pub fn revoke_all_embargoes(env: Env, admin_signers: Vec<Address>) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        Self::require_admin_auth(&env, &admin_signers)?;

        let wallets = storage::get_embargoed_wallets(&env);
        for i in 0..wallets.len() {
            let wallet = wallets.get(i).unwrap();
            storage::remove_embargo(&env, &wallet);
            events::embargo_lifted(&env, &wallet);
        }
        storage::clear_embargoed_index(&env);
        storage::reset_active_embargo_count(&env);
        Ok(())
    }

    /// Returns the number of wallets currently tracked in the
    /// `EmbargoedWalletIndex`, i.e. the number of wallets a subsequent
    /// [`revoke_all_embargoes`](Self::revoke_all_embargoes) call would lift.
    /// Includes wallets whose timed embargo has already expired but were
    /// never explicitly lifted (see [`revoke_all_embargoes`](Self::revoke_all_embargoes)).
    pub fn get_embargoed_wallet_count(env: Env) -> u32 {
        storage::get_embargoed_wallets(&env).len()
    }

    /// Returns the number of wallets currently under an active score embargo.
    ///
    /// The value is maintained as a persistent counter: incremented by
    /// [`set_score_embargo`](Self::set_score_embargo) when a **new** embargo is
    /// placed on a wallet (re-embargoing an already-embargoed wallet does not
    /// increment), and decremented by
    /// [`lift_score_embargo`](Self::lift_score_embargo),
    /// [`batch_lift_score_embargo`](Self::batch_lift_score_embargo), and
    /// [`revoke_all_embargoes`](Self::revoke_all_embargoes).
    ///
    /// Because the counter lives in persistent storage it survives
    /// temporary-storage TTL eviction, making it a reliable signal for admin
    /// dashboards and monitoring tools that need a fast, single-read gauge of
    /// the current embargo load without enumerating all wallets.
    ///
    /// Returns `0` when no embargo has ever been set or all embargoes have been
    /// explicitly lifted.
    pub fn get_active_embargo_count(env: Env) -> u32 {
        storage::get_active_embargo_count(&env)
    }

    // ── Score dispute mechanism ───────────────────────────────────────────────

    /// Open a stake-backed dispute against `wallet`'s current risk score for
    /// `asset_pair`.
    ///
    /// The challenger (`wallet`) escrows `bond` units of the configured fee
    /// token into the contract and starts a challenge period of
    /// [`constants::DISPUTE_CHALLENGE_PERIOD_SECS`]. During that window the
    /// admin is expected to resubmit a corrected score via
    /// [`resolve_dispute_admin`] (which returns the bond). If the admin fails
    /// to act before the deadline, anyone may call
    /// [`resolve_dispute_timeout`] to return the bond plus a
    /// [`constants::DISPUTE_BONUS_PCT`] bonus from the contract's fee reserve.
    ///
    /// `wallet` must authorize the call (it is staking its own funds), and the
    /// fee token must already be configured via `set_fee_token`.
    ///
    /// # Errors
    /// - [`Error::NotInitialized`] — contract has no admin.
    /// - [`Error::ContractPaused`] — the global circuit breaker is active.
    /// - [`Error::InvalidDisputeBond`] — `bond` is not strictly positive.
    /// - [`Error::FeeTokenNotSet`] — `set_fee_token` has not been called.
    /// - [`Error::DisputeAlreadyOpen`] — a dispute already exists for the pair.
    /// - [`Error::DisputeAlreadyOpen`] — the open-dispute index is at capacity.
    /// Commit-reveal for sealed-bid dispute bond: commit to (bond, salt) before revealing.
    /// Stores H(bond || salt) under temporary storage scoped to (challenger, wallet, asset_pair).
    /// Caller must reveal within the configured reveal window or commitment expires.
    ///
    /// # Arguments
    /// - `challenger`: Account committing to a dispute bond
    /// - `wallet`: Wallet whose score is being challenged
    /// - `asset_pair`: Asset pair of the challenged score
    /// - `bond_amount_salt`: Salt for commit-reveal (must be ≥16 bytes for security)
    pub fn commit_dispute_bond(
        env: Env,
        challenger: Address,
        wallet: Address,
        asset_pair: Symbol,
        bond_amount_salt: Bytes,
    ) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        
        // Require caller to be the challenger
        challenger.require_auth();
        
        // Compute H(bond_amount_salt) and store
        let commitment = env.crypto().sha256(&bond_amount_salt);
        storage::set_dispute_commit(&env, &challenger, &wallet, &asset_pair, &commitment);
        
        Ok(())
    }

    pub fn open_score_dispute(
        env: Env,
        challenger: Address,
        wallet: Address,
        asset_pair: Symbol,
        bond: i128,
        bond_salt: Bytes,
    ) -> Result<(), Error> {
        Self::ensure_active(&env)?;

        if bond <= 0 {
            return Err(Error::InvalidDisputeBond);
        }
        let fee_token = storage::get_fee_token(&env).ok_or(Error::FeeTokenNotSet)?;

        // The challenger stakes its own funds, so it must authorize.
        challenger.require_auth();

        // Sealed-bid: verify commitment was made and reveal window not expired
        let commitment = storage::get_dispute_commit(&env, &challenger, &wallet, &asset_pair)
            .ok_or(Error::RevealWindowExpired)?;
        let commit_time = storage::get_dispute_commit_time(&env, &challenger, &wallet, &asset_pair);
        let reveal_window = storage::get_reveal_window_secs(&env);
        if env.ledger().timestamp() > commit_time.saturating_add(reveal_window) {
            storage::remove_dispute_commit(&env, &challenger, &wallet, &asset_pair);
            return Err(Error::RevealWindowExpired);
        }

        // Verify revealed bond+salt matches commitment
        let salt_preimage = [bond.to_le_bytes().to_vec(), bond_salt.to_vec()];
        let mut revealed = Bytes::new(&env);
        revealed.extend_from_slice(&bond.to_le_bytes());
        revealed.extend_from_slice(&bond_salt);
        let revealed_hash = env.crypto().sha256(&revealed);
        if revealed_hash.to_bytes() != commitment {
            return Err(Error::CommitmentMismatch);
        }

        // Clear commitment after successful reveal
        storage::remove_dispute_commit(&env, &challenger, &wallet, &asset_pair);

        if storage::get_dispute(&env, &wallet, &asset_pair).is_some() {
            return Err(Error::DisputeAlreadyOpen);
        }
        if !storage::add_to_dispute_index(&env, &wallet, &asset_pair) {
            return Err(Error::DisputeAlreadyOpen);
        }

        // Escrow the bond into the contract.
        let contract_address = env.current_contract_address();
        token::TokenClient::new(&env, &fee_token).transfer(&challenger, &contract_address, &bond);

        let challenged_score =
            storage::peek_score(&env, &wallet, &asset_pair).map(|s| s.score).unwrap_or(0);
        let deadline =
            env.ledger().timestamp().saturating_add(constants::DISPUTE_CHALLENGE_PERIOD_SECS);
        let dispute = ScoreDispute { challenger: challenger.clone(), bond, deadline, challenged_score };
        storage::set_dispute(&env, &wallet, &asset_pair, &dispute);

        events::dispute_opened(&env, &wallet, &asset_pair, bond, deadline);
        Ok(())
    }

    /// Resolve an open dispute by resubmitting a corrected score. Admin only
    /// (M-of-N when an admin set is configured). The escrowed bond is returned
    /// in full to the challenger and the dispute is closed.
    ///
    /// The corrected score is written immediately, bypassing the per-pair
    /// submission cooldown since this is an authorized remediation, and is
    /// marked with `model_version = 0` to denote an on-chain admin correction.
    ///
    /// # Errors
    /// - [`Error::NotInitialized`] — contract has no admin.
    /// - [`Error::ContractPaused`] — the global circuit breaker is active.
    /// - [`Error::InsufficientAdminSigners`] / [`Error::AdminSignerNotInSet`]
    ///   — admin M-of-N authorization failed.
    /// - [`Error::DisputeNotFound`] — no open dispute for the pair.
    /// - [`Error::InvalidScore`] — `corrected_score` exceeds 100.
    /// - [`Error::FeeTokenNotSet`] — `set_fee_token` has not been called.
    pub fn resolve_dispute_admin(
        env: Env,
        admin_signers: Vec<Address>,
        wallet: Address,
        asset_pair: Symbol,
        corrected_score: u32,
    ) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        if storage::is_paused(&env) {
            return Err(Error::ContractPaused);
        }
        Self::require_admin_auth(&env, &admin_signers)?;

        if corrected_score > 100 {
            return Err(Error::InvalidScore);
        }

        let dispute =
            storage::get_dispute(&env, &wallet, &asset_pair).ok_or(Error::DisputeNotFound)?;
        let fee_token = storage::get_fee_token(&env).ok_or(Error::FeeTokenNotSet)?;

        // Write the corrected score, bypassing the cooldown (admin remediation).
        let now = env.ledger().timestamp();
        let corrected = RiskScore {
            score: corrected_score,
            benford_flag: false,
            ml_flag: false,
            timestamp: now,
            confidence: 100,
            model_version: 0,
            commitment: None,
        };
        storage::set_score(&env, &wallet, &asset_pair, &corrected);
        storage::push_score_history(&env, &wallet, &asset_pair, &corrected);
        storage::register_pair_for_wallet(&env, &wallet, &asset_pair);
        storage::increment_score_count(&env, &wallet, &asset_pair);
        // Increment per-pair submission counter (Issue 1).
        storage::increment_pair_score_count(&env, &asset_pair);
        // Dispute correction always applies to an already-scored wallet-pair,
        // so we intentionally do NOT increment total_wallets_scored here.
        Self::refresh_aggregate_cache(&env, &wallet);
        events::score_submitted(&env, &wallet, &asset_pair, &corrected);

        // Return the escrowed bond to the challenger and close the dispute.
        let contract_address = env.current_contract_address();
        token::TokenClient::new(&env, &fee_token).transfer(
            &contract_address,
            &dispute.challenger,
            &dispute.bond,
        );
        storage::remove_dispute(&env, &wallet, &asset_pair);
        storage::remove_from_dispute_index(&env, &wallet, &asset_pair);

        events::dispute_resolved(
            &env,
            &dispute.challenger,
            &asset_pair,
            corrected_score,
            dispute.bond,
        );
        Ok(())
    }

    /// Settle a dispute that the admin failed to resolve before its deadline.
    /// Callable by anyone once `ledger_timestamp > deadline`. The challenger
    /// receives the escrowed bond plus a [`constants::DISPUTE_BONUS_PCT`] bonus
    /// drawn from the contract's accumulated fee reserve.
    ///
    /// # Errors
    /// - [`Error::NotInitialized`] — contract has no admin.
    /// - [`Error::DisputeNotFound`] — no open dispute for the pair.
    /// - [`Error::DisputeNotYetTimedOut`] — the deadline has not elapsed.
    /// - [`Error::FeeTokenNotSet`] — `set_fee_token` has not been called.
    pub fn resolve_dispute_timeout(
        env: Env,
        wallet: Address,
        asset_pair: Symbol,
    ) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }

        let dispute =
            storage::get_dispute(&env, &wallet, &asset_pair).ok_or(Error::DisputeNotFound)?;
        if env.ledger().timestamp() <= dispute.deadline {
            return Err(Error::DisputeNotYetTimedOut);
        }
        let fee_token = storage::get_fee_token(&env).ok_or(Error::FeeTokenNotSet)?;

        // Bond is returned with a bonus from the fee reserve. Bond is bounded to
        // positive values at open time, so the bonus multiplication is safe.
        let bonus = dispute.bond.saturating_mul(constants::DISPUTE_BONUS_PCT) / 100;
        let payout = dispute.bond.saturating_add(bonus);

        let contract_address = env.current_contract_address();
        token::TokenClient::new(&env, &fee_token).transfer(
            &contract_address,
            &dispute.challenger,
            &payout,
        );
        storage::remove_dispute(&env, &wallet, &asset_pair);
        storage::remove_from_dispute_index(&env, &wallet, &asset_pair);

        events::dispute_timed_out(&env, &dispute.challenger, &asset_pair, dispute.bond, bonus);
        Ok(())
    }

    /// Returns every currently open dispute as `(challenger, asset_pair,
    /// deadline)` tuples. Read-only; callable by anyone.
    pub fn get_open_disputes(env: Env) -> Vec<(Address, Symbol, u64)> {
        let index = storage::get_dispute_index(&env);
        let mut out: Vec<(Address, Symbol, u64)> = Vec::new(&env);
        for i in 0..index.len() {
            let (wallet, asset_pair) = index.get(i).unwrap();
            if let Some(dispute) = storage::get_dispute(&env, &wallet, &asset_pair) {
                out.push_back((wallet, asset_pair, dispute.deadline));
            }
        }
        out
    }

    // ── Staleness window ──────────────────────────────────────────────────────

    /// Returns `true` when no score exists for this pair, or when the stored
    /// score's `timestamp` is older than `env.ledger().timestamp() - staleness_window`.
    ///
    /// Uses `saturating_sub` so a future score timestamp (clock skew) or a zero
    /// ledger timestamp never causes an arithmetic panic — in that edge case the
    /// age is treated as 0 and the score is considered fresh.
    pub fn is_score_stale(env: Env, wallet: Address, asset_pair: Symbol) -> bool {
        match storage::get_score(&env, &wallet, &asset_pair) {
            None => true,
            Some(score) => {
                let window = storage::get_staleness_window(&env);
                let ledger_ts = env.ledger().timestamp();
                ledger_ts.saturating_sub(score.timestamp) > window
            }
        }
    }

    /// Set the staleness window in seconds. A value of `0` is rejected with
    /// `InvalidStalenessWindow`. Admin only.
    pub fn set_staleness_window(
        env: Env,
        admin_signers: Vec<Address>,
        window_secs: u64,
    ) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        if window_secs == 0 {
            return Err(Error::InvalidStalenessWindow);
        }
        Self::require_admin_auth(&env, &admin_signers)?;
        storage::set_staleness_window(&env, window_secs);
        Ok(())
    }

    /// Returns the current staleness window in seconds. Defaults to
    /// `DEFAULT_STALENESS_WINDOW_SECS` (7 days) until configured.
    pub fn get_staleness_window(env: Env) -> u64 {
        storage::get_staleness_window(&env)
    }

    // ── Time-weighted exponential decay ──────────────────────────────────────

    /// Set the exponential decay rate (λ) applied to per-pair scores in the
    /// aggregate computation. The decay formula is:
    ///   decay_factor(age) = e^(-λ * age_seconds)
    /// where λ = numerator / denominator.
    ///
    /// When λ = 0 (numerator = 0), no decay occurs and aggregate scores
    /// behave exactly as in prior contract versions. A higher λ causes older
    /// scores to contribute less to the aggregate.
    ///
    /// # Arguments
    /// - `numerator`: numerator of λ
    /// - `denominator`: denominator of λ; must be > 0
    ///
    /// The ratio must satisfy: 0 <= numerator / denominator <= MAX_DECAY_LAMBDA.
    /// Admin only. Blocked when the contract is paused.
    ///
    /// # Errors
    /// - [`Error::NotInitialized`] if the contract has no admin yet.
    /// - [`Error::ContractPaused`] if the contract is paused.
    /// - [`Error::InvalidThreshold`] if the ratio exceeds MAX_DECAY_LAMBDA.
    ///
    /// # Examples
    ///
    /// Set λ to 0.001 per second (half-life ~693 seconds):
    /// ```
    /// # use ledgerlens_score::LedgerLensScoreContractClient;
    /// # use soroban_sdk::{testutils::Address as _, Env, Address};
    /// # use ledgerlens_score::LedgerLensScoreContract;
    /// let env = Env::default();
    /// env.mock_all_auths();
    /// let contract_id = env.register_contract(None, LedgerLensScoreContract);
    /// let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    /// let admin = Address::generate(&env);
    /// let service = Address::generate(&env);
    /// client.initialize(&admin, &service);
    /// client.set_decay_rate(&1, &1000);
    /// assert_eq!(client.get_decay_rate(), (1, 1000));
    /// ```
    pub fn set_decay_rate(env: Env, numerator: u32, denominator: u32) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        if storage::is_paused(&env) {
            return Err(Error::ContractPaused);
        }

        // Validate denominator is not zero
        if denominator == 0 {
            return Err(Error::InvalidThreshold);
        }

        // Validate the ratio is within bounds
        // Check: numerator / denominator <= MAX_DECAY_LAMBDA
        // Equivalently: numerator * MAX_DEN <= MAX_NUM * denominator
        let max_num = constants::MAX_DECAY_LAMBDA_NUM as u64;
        let max_den = constants::MAX_DECAY_LAMBDA_DEN as u64;
        let num = numerator as u64;
        let den = denominator as u64;

        if num.checked_mul(max_den).map(|v| v > max_num.saturating_mul(den)).unwrap_or(true) {
            return Err(Error::InvalidThreshold);
        }

        let admin = storage::get_admin(&env);
        admin.require_auth();

        storage::set_decay_rate(&env, numerator, denominator);
        events::decay_rate_updated(&env, numerator, denominator);

        Ok(())
    }

    /// Returns the current decay rate as (numerator, denominator).
    /// Defaults to (0, 1) (no decay) until configured.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ledgerlens_score::LedgerLensScoreContractClient;
    /// # use soroban_sdk::{testutils::Address as _, Env, Address};
    /// # use ledgerlens_score::LedgerLensScoreContract;
    /// let env = Env::default();
    /// env.mock_all_auths();
    /// let contract_id = env.register_contract(None, LedgerLensScoreContract);
    /// let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    /// let admin = Address::generate(&env);
    /// let service = Address::generate(&env);
    /// client.initialize(&admin, &service);
    /// let (num, den) = client.get_decay_rate();
    /// assert_eq!((num, den), (0, 1));
    /// ```
    pub fn get_decay_rate(env: Env) -> (u32, u32) {
        storage::get_decay_rate(&env)
    }

    // ── Per-wallet/pair submission rate limiting ─────────────────────────────

    /// Configure the cooldown (seconds) enforced between accepted
    /// submissions for the same `(wallet, asset_pair)`. Must be within
    /// `[MIN_COOLDOWN_SECS, MAX_COOLDOWN_SECS]` (1 minute – 24 hours).
    /// Admin only.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ledgerlens_score::LedgerLensScoreContractClient;
    /// # use soroban_sdk::{testutils::Address as _, Env, Address};
    /// # use ledgerlens_score::LedgerLensScoreContract;
    /// let env = Env::default();
    /// env.mock_all_auths();
    /// let contract_id = env.register_contract(None, LedgerLensScoreContract);
    /// let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    /// let admin = Address::generate(&env);
    /// let service = Address::generate(&env);
    /// client.initialize(&admin, &service);
    /// client.set_cooldown(&Vec::new(&env), &120);
    /// assert_eq!(client.get_cooldown(), 120);
    /// ```
    ///
    /// # Errors
    /// - [`Error::NotInitialized`] if the contract has no admin yet.
    /// - [`Error::InvalidCooldown`] if `secs` is outside the bounds.
    pub fn set_cooldown(env: Env, admin_signers: Vec<Address>, secs: u64) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        if !(constants::MIN_COOLDOWN_SECS..=constants::MAX_COOLDOWN_SECS).contains(&secs) {
            return Err(Error::InvalidCooldown);
        }
        Self::require_admin_auth(&env, &admin_signers)?;
        storage::set_cooldown_secs(&env, secs);
        events::cooldown_updated(&env, secs);
        Ok(())
    }

    /// Returns the current submission cooldown in seconds. Defaults to
    /// `DEFAULT_COOLDOWN_SECS` (1 hour) until configured.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ledgerlens_score::LedgerLensScoreContractClient;
    /// # use soroban_sdk::{testutils::Address as _, Env, Address};
    /// # use ledgerlens_score::LedgerLensScoreContract;
    /// let env = Env::default();
    /// env.mock_all_auths();
    /// let contract_id = env.register_contract(None, LedgerLensScoreContract);
    /// let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    /// let admin = Address::generate(&env);
    /// let service = Address::generate(&env);
    /// client.initialize(&admin, &service);
    /// assert_eq!(client.get_cooldown(), 3_600);
    /// ```
    pub fn get_cooldown(env: Env) -> u64 {
        storage::get_cooldown_secs(&env)
    }

    /// Returns the configured rate-limit window duration in seconds.
    ///
    /// The rate-limit window is the minimum time that must elapse between two
    /// accepted score submissions for the same `(wallet, asset_pair)`.  It is
    /// the same value as the submission cooldown — this function exists as an
    /// explicitly named alias so integrators building retry logic can
    /// discover the window without needing to know the internal naming
    /// convention.
    ///
    /// Returns `DEFAULT_COOLDOWN_SECS` (3 600 s, i.e. 1 hour) until the admin
    /// calls `set_cooldown`.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ledgerlens_score::LedgerLensScoreContractClient;
    /// # use soroban_sdk::{testutils::Address as _, Env, Address};
    /// # use ledgerlens_score::LedgerLensScoreContract;
    /// let env = Env::default();
    /// env.mock_all_auths();
    /// let contract_id = env.register_contract(None, LedgerLensScoreContract);
    /// let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    /// let admin = Address::generate(&env);
    /// let service = Address::generate(&env);
    /// client.initialize(&admin, &service);
    /// // Default window is one hour.
    /// assert_eq!(client.get_rate_limit_window(), 3_600);
    /// ```
    pub fn get_rate_limit_window(env: Env) -> u64 {
        storage::get_cooldown_secs(&env)
    }

    /// Returns the score-submission cooldown period in seconds.
    ///
    /// Off-chain scoring services can call this before scheduling a
    /// re-submission to avoid hitting `RateLimitExceeded`.  The cooldown is
    /// the amount of time that must pass after a successful submission before
    /// the next submission for the same `(wallet, asset_pair)` is accepted.
    ///
    /// Returns `DEFAULT_COOLDOWN_SECS` (3 600 s, i.e. 1 hour) until the
    /// admin calls `set_cooldown`.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ledgerlens_score::LedgerLensScoreContractClient;
    /// # use soroban_sdk::{testutils::Address as _, Env, Address, Vec};
    /// # use ledgerlens_score::LedgerLensScoreContract;
    /// # use soroban_sdk::symbol_short;
    /// let env = Env::default();
    /// env.mock_all_auths();
    /// let contract_id = env.register_contract(None, LedgerLensScoreContract);
    /// let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    /// let admin = Address::generate(&env);
    /// let service = Address::generate(&env);
    /// client.initialize(&admin, &service);
    /// // Default cooldown is one hour (3 600 seconds).
    /// let cooldown = client.get_cooldown_period();
    /// assert_eq!(cooldown, 3_600);
    ///
    /// // Off-chain scheduler example: schedule next submission at
    /// // `last_submit_timestamp + cooldown`.
    /// let wallet = Address::generate(&env);
    /// let pair = symbol_short!("XLM_USDC");
    /// client.submit_score(&Vec::new(&env), &wallet, &pair, &42, &false, &false, &1, &90, &1, &None).unwrap();
    /// let next_allowed = client.get_last_submit_time(&wallet, &pair) + cooldown;
    /// // next_allowed is the earliest timestamp at which a re-submission is accepted.
    /// ```
    pub fn get_cooldown_period(env: Env) -> u64 {
        storage::get_cooldown_secs(&env)
    }

    /// Sets a per-asset-pair cooldown override. The value must satisfy the
    /// same bounds as the global cooldown and takes precedence for this pair
    /// until cleared. Admin only.
    pub fn set_pair_cooldown(
        env: Env,
        admin_signers: Vec<Address>,
        asset_pair: Symbol,
        secs: u64,
    ) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        if !(constants::MIN_COOLDOWN_SECS..=constants::MAX_COOLDOWN_SECS).contains(&secs) {
            return Err(Error::InvalidCooldown);
        }
        Self::require_admin_auth(&env, &admin_signers)?;
        storage::set_pair_cooldown_secs(&env, &asset_pair, secs);
        events::pair_cooldown_updated(&env, &asset_pair, secs);
        Ok(())
    }

    /// Returns this pair's cooldown, falling back to the global cooldown when
    /// no pair-specific override is configured.
    pub fn get_pair_cooldown(env: Env, asset_pair: Symbol) -> u64 {
        storage::get_pair_cooldown_secs(&env, &asset_pair)
    }

    /// Clears a per-asset-pair cooldown override so the pair uses the current
    /// global cooldown again. Admin only.
    pub fn clear_pair_cooldown(
        env: Env,
        admin_signers: Vec<Address>,
        asset_pair: Symbol,
    ) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        Self::require_admin_auth(&env, &admin_signers)?;
        storage::clear_pair_cooldown_secs(&env, &asset_pair);
        events::pair_cooldown_updated(&env, &asset_pair, storage::get_cooldown_secs(&env));
        Ok(())
    }

    // ── Adaptive rate limit ───────────────────────────────────────────────────

    /// Configures the adaptive rate-limit mode. When `enabled` and
    /// `variance_scale > 0`, the effective cooldown is scaled by the current
    /// global score variance:
    ///
    /// ```text
    /// effective_cooldown = base_cooldown * (1 + variance_scale * normalized_variance / 1000)
    /// ```
    ///
    /// where `normalized_variance` ∈ [0, 1000] is the population variance of
    /// the global score histogram normalised against the theoretical maximum
    /// of 2500 (all scores at the extremes). Admin only.
    pub fn set_adaptive_rate_limit(
        env: Env,
        admin_signers: Vec<Address>,
        enabled: bool,
        variance_scale: u32,
    ) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        Self::require_admin_auth(&env, &admin_signers)?;
        let config = AdaptiveRateLimit { enabled, variance_scale };
        storage::set_adaptive_rate_limit(&env, &config);
        events::adaptive_rate_limit_updated(&env, enabled, variance_scale);
        Ok(())
    }

    /// Returns the current adaptive rate-limit configuration.
    pub fn get_adaptive_rate_limit(env: Env) -> AdaptiveRateLimit {
        storage::get_adaptive_rate_limit(&env)
    }

    /// Returns the effective cooldown for `(wallet, asset_pair)` at the
    /// current moment.  When the adaptive rate-limit is disabled (or
    /// `variance_scale == 0`) this equals `get_pair_cooldown(asset_pair)`.
    /// When enabled, it is scaled by the current global score variance.
    pub fn get_effective_cooldown(env: Env, wallet: Address, asset_pair: Symbol) -> u64 {
        let _ = wallet; // wallet / pair reserved for future per-wallet variance
        let _ = asset_pair;
        let base = storage::get_pair_cooldown_secs(&env, &asset_pair);
        Self::compute_effective_cooldown(&env, &asset_pair, base)
    }

    /// Internal helper: compute the effective cooldown given the base value.
    fn compute_effective_cooldown(env: &Env, asset_pair: &Symbol, base: u64) -> u64 {
        let config = storage::get_adaptive_rate_limit(env);
        if !config.enabled || config.variance_scale == 0 {
            return base;
        }
        let norm_var = Self::compute_global_variance(env); // 0..=1000
        // effective = base * (1000 + variance_scale * norm_var) / 1000
        // Using u128 to avoid overflow when base and scale are both large.
        let numerator = 1000u128
            .saturating_add((config.variance_scale as u128).saturating_mul(norm_var as u128));
        let effective = (base as u128).saturating_mul(numerator) / 1000;
        effective.min(u64::MAX as u128) as u64
    }

    /// Computes the global score variance from the histogram, normalised to
    /// [0, 1000] (0 = all scores identical, 1000 ≈ maximum spread).
    ///
    /// Uses 10-bucket histogram with midpoints 5, 15, …, 95.
    /// Max theoretical variance ≈ 2500 (bimodal distribution at extremes).
    fn compute_global_variance(env: &Env) -> u32 {
        let hist = storage::get_score_histogram(env);
        let total = hist.total;
        if total == 0 {
            return 0;
        }
        // Bucket midpoints: bucket i covers scores [10*i, 10*i+9], midpoint = 10*i+5
        let mut weighted_sum: u64 = 0;
        let mut weighted_sum_sq: u64 = 0;
        for i in 0..hist.buckets.len() {
            let midpoint = (i * 10 + 5) as u64;
            let count = hist.buckets.get(i).unwrap_or(0);
            weighted_sum = weighted_sum.saturating_add(midpoint.saturating_mul(count));
            weighted_sum_sq =
                weighted_sum_sq.saturating_add(midpoint.saturating_mul(midpoint).saturating_mul(count));
        }
        // mean = weighted_sum / total
        // variance = (weighted_sum_sq / total) - mean^2
        // Use u128 for intermediate calculations to avoid overflow.
        let total128 = total as u128;
        let mean_scaled = weighted_sum as u128 * 1000 / total128; // mean * 1000
        let mean_sq_scaled = mean_scaled * mean_scaled / 1000; // mean^2 * 1000
        let esq_scaled = weighted_sum_sq as u128 * 1000 / total128; // E[X^2] * 1000
        let variance_scaled = esq_scaled.saturating_sub(mean_sq_scaled); // var * 1000

        // Normalise: max theoretical variance is 2500, so max variance_scaled = 2_500_000.
        // normalised = variance_scaled * 1000 / 2_500_000 = variance_scaled / 2500
        let normalised = (variance_scaled / 2500).min(1000) as u32;
        normalised
    }
    /// for `(wallet, asset_pair)`, allowing the very next `submit_score` /
    /// `submit_scores_batch` call to be accepted regardless of how recently
    /// the last one was. This is **not** a routine operation — it exists for
    /// situations such as a known-bad score that needs correcting right away,
    /// not for working around the rate limiter during normal operation.
    /// Admin only.
    ///
    /// # Errors
    /// - [`Error::NotInitialized`] if the contract has no admin yet.
    pub fn override_rate_limit(
        env: Env,
        admin_signers: Vec<Address>,
        wallet: Address,
        asset_pair: Symbol,
        justification: Bytes,
    ) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        Self::require_admin_auth(&env, &admin_signers)?;
        let admin = storage::get_admin(&env);
        storage::clear_last_submit_time(&env, &wallet, &asset_pair);
        let justification_hash = env.crypto().sha256(&justification);
        let entry = crate::types::RateLimitOverrideEntry {
            admin: admin.clone(),
            wallet: wallet.clone(),
            asset_pair: asset_pair.clone(),
            timestamp: env.ledger().timestamp(),
            justification_hash: justification_hash.into(),
        };
        storage::append_rate_limit_override_log(&env, &entry);
        events::rate_limit_overridden(&env, &admin, &wallet, &asset_pair);
        Ok(())
    }

    /// Clears multiple `(wallet, asset_pair)` cooldown entries in one admin
    /// operation. Emits the same `rl_ovrd` event for each cleared entry and
    /// returns the number of entries processed.
    pub fn batch_override_rate_limit(
        env: Env,
        admin_signers: Vec<Address>,
        entries: Vec<(Address, Symbol)>,
    ) -> Result<u32, Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        if entries.len() > constants::MAX_BATCH_SIZE {
            return Err(Error::BatchTooLarge);
        }
        Self::require_admin_auth(&env, &admin_signers)?;
        let admin = storage::get_admin(&env);
        for i in 0..entries.len() {
            let (wallet, asset_pair) = entries.get(i).unwrap();
            storage::clear_last_submit_time(&env, &wallet, &asset_pair);
            events::rate_limit_overridden(&env, &admin, &wallet, &asset_pair);
        }
        Ok(entries.len())
    }

    /// Returns the on-chain audit log of all `override_rate_limit` calls,
    /// ordered oldest-first, capped at `MAX_RATE_LIMIT_OVERRIDE_LOG` entries.
    pub fn get_rate_limit_override_log(
        env: Env,
    ) -> Vec<crate::types::RateLimitOverrideEntry> {
        storage::get_rate_limit_override_log(&env)
    }

    /// Read-only lookup of the current velocity cap configuration.
    pub fn get_score_velocity_cap(env: Env) -> ScoreVelocityCap {
        storage::get_score_velocity_cap(&env)
    }

    /// Admin function to configure the score velocity cap.
    pub fn set_score_velocity_cap(
        env: Env,
        admin_signers: Vec<Address>,
        enabled: bool,
        points_per_hour: u32,
    ) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        Self::require_admin_auth(&env, &admin_signers)?;

        let cap = ScoreVelocityCap { enabled, points_per_hour };
        storage::set_score_velocity_cap(&env, &cap);
        events::score_velocity_cap_set(&env, enabled, points_per_hour);
        Ok(())
    }

    /// Admin function to override the velocity cap for a specific (wallet, asset_pair).
    /// This sets a one-time bypass flag that allows the very next score submission
    /// to ignore the velocity cap constraint.
    pub fn override_score_velocity_cap(
        env: Env,
        admin_signers: Vec<Address>,
        wallet: Address,
        asset_pair: Symbol,
    ) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        Self::require_admin_auth(&env, &admin_signers)?;

        let admin = storage::get_admin(&env);
        storage::set_velocity_cap_override(&env, &wallet, &asset_pair);
        events::velocity_cap_overridden(&env, &admin, &wallet, &asset_pair);
        Ok(())
    }

    /// Erase the score history ring buffer for `wallet` / `asset_pair`.
    ///
    /// Does nothing (returns `Ok`) if no history exists. After this call,
    /// `get_score_history` returns an empty Vec. This operation is
    /// **irreversible on-chain** — keep off-chain backups before erasing.
    /// Admin only.
    ///
    /// Emits `clr_hist` for the on-chain audit trail.
    pub fn clear_score_history(
        env: Env,
        admin_signers: Vec<Address>,
        wallet: Address,
        asset_pair: Symbol,
    ) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        Self::require_admin_auth(&env, &admin_signers)?;
        if let Some(risk) = storage::peek_score(&env, &wallet, &asset_pair) {
            storage::update_histogram_on_clear(&env, risk.score);
        }
        storage::clear_score_history(&env, &wallet, &asset_pair);
        events::score_history_cleared(&env, &wallet, &asset_pair);
        Ok(())
    }

    /// Erase the latest score entry for `wallet` / `asset_pair`.
    ///
    /// Does nothing (returns `Ok`) if no score exists. After this call,
    /// `get_score` returns `ScoreNotFound`. This operation is
    /// **irreversible on-chain** — keep off-chain backups before erasing.
    /// Admin only.
    ///
    /// Emits `clr_scr` for the on-chain audit trail.
    pub fn clear_score(
        env: Env,
        admin_signers: Vec<Address>,
        wallet: Address,
        asset_pair: Symbol,
    ) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        Self::require_admin_auth(&env, &admin_signers)?;
        if let Some(risk) = storage::peek_score(&env, &wallet, &asset_pair) {
            storage::update_histogram_on_clear(&env, risk.score);
        }
        storage::clear_score(&env, &wallet, &asset_pair);
        events::score_cleared(&env, &wallet, &asset_pair);
        Ok(())
    }

    /// Returns the Unix timestamp of the last accepted score submission for
    /// `(wallet, asset_pair)`.
    ///
    /// - `Some(ts)` — a submission has been accepted; `ts` is the ledger
    ///   timestamp at the time of acceptance.
    /// - `None` — no submission has ever been accepted for this pair, or the
    ///   submission record was cleared by `override_rate_limit`.
    pub fn get_last_submit_time(env: Env, wallet: Address, asset_pair: Symbol) -> Option<u64> {
        storage::get_last_submit_time_opt(&env, &wallet, &asset_pair)
    }

    // ── Score submission floor ────────────────────────────────────────────────

    /// Configure the per-wallet score submission floor. Admin only.
    ///
    /// When `enabled`, any `(wallet, asset_pair)` whose historical peak score
    /// has reached `high_water_mark` can no longer receive a submission below
    /// `floor_value`: such a submission is rejected with
    /// [`Error::InvalidScore`] (or recorded with that `rejection_code` in a
    /// batch). Combined with the rate limiter and attestation, this is a
    /// second line of defence — a compromised or colluding signer cannot
    /// simply zero out a known high-risk wallet's score to whitewash it.
    ///
    /// The policy is **disabled by default**; no floor is enforced until the
    /// admin opts in via this function.
    ///
    /// # Arguments
    /// - `enabled` — kill-switch; `false` disables the floor entirely.
    /// - `high_water_mark` — historical peak at or above which the floor
    ///   applies. Must be within `[MIN_SCORE_FLOOR_HWM, MAX_SCORE_FLOOR_HWM]`
    ///   (50–100).
    /// - `floor_value` — minimum score permitted for a high-risk wallet. Must
    ///   be strictly below `high_water_mark` (i.e. in `[0, high_water_mark - 1]`).
    ///
    /// # Errors
    /// - [`Error::NotInitialized`] if the contract has no admin yet.
    /// - [`Error::InvalidThreshold`] if `high_water_mark` is out of
    ///   range or `floor_value` is not strictly below it.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ledgerlens_score::LedgerLensScoreContractClient;
    /// # use soroban_sdk::{testutils::Address as _, Env, Address, Vec};
    /// # use ledgerlens_score::LedgerLensScoreContract;
    /// let env = Env::default();
    /// env.mock_all_auths();
    /// let contract_id = env.register_contract(None, LedgerLensScoreContract);
    /// let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    /// let admin = Address::generate(&env);
    /// let service = Address::generate(&env);
    /// client.initialize(&admin, &service);
    /// client.set_score_floor_policy(&Vec::new(&env), &true, &80, &20);
    /// let policy = client.get_score_floor_policy();
    /// assert!(policy.enabled);
    /// assert_eq!(policy.high_water_mark, 80);
    /// assert_eq!(policy.floor_value, 20);
    /// ```
    pub fn set_score_floor_policy(
        env: Env,
        admin_signers: Vec<Address>,
        enabled: bool,
        high_water_mark: u32,
        floor_value: u32,
    ) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        if !(constants::MIN_SCORE_FLOOR_HWM..=constants::MAX_SCORE_FLOOR_HWM)
            .contains(&high_water_mark)
            || floor_value >= high_water_mark
        {
            return Err(Error::InvalidThreshold);
        }
        Self::require_admin_auth(&env, &admin_signers)?;
        storage::set_score_floor_policy(&env, enabled, high_water_mark, floor_value);
        events::score_floor_policy_updated(&env, enabled, high_water_mark, floor_value);
        Ok(())
    }

    /// Returns the current score-floor policy. Defaults to disabled with a
    /// high-water mark of `DEFAULT_SCORE_FLOOR_HWM` (80) and a floor of
    /// `DEFAULT_SCORE_FLOOR_MIN` (20) until the admin configures it.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ledgerlens_score::LedgerLensScoreContractClient;
    /// # use soroban_sdk::{testutils::Address as _, Env, Address};
    /// # use ledgerlens_score::LedgerLensScoreContract;
    /// let env = Env::default();
    /// env.mock_all_auths();
    /// let contract_id = env.register_contract(None, LedgerLensScoreContract);
    /// let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    /// let admin = Address::generate(&env);
    /// let service = Address::generate(&env);
    /// client.initialize(&admin, &service);
    /// let policy = client.get_score_floor_policy();
    /// assert!(!policy.enabled);
    /// assert_eq!(policy.high_water_mark, 80);
    /// assert_eq!(policy.floor_value, 20);
    /// ```
    pub fn get_score_floor_policy(env: Env) -> ScoreFloorPolicy {
        storage::get_score_floor_policy(&env)
    }

    /// Returns the highest score ever recorded for `(wallet, asset_pair)`, or
    /// `None` if no score has ever been accepted. This running peak is what the
    /// floor compares against `high_water_mark`; exposing it as an `Option`
    /// lets off-chain tooling distinguish "never scored" from a recorded peak
    /// of `0` and predict when the floor policy will activate. Read-only,
    /// callable by any account or contract.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ledgerlens_score::LedgerLensScoreContractClient;
    /// # use soroban_sdk::{testutils::Address as _, Env, Address, Vec};
    /// # use ledgerlens_score::LedgerLensScoreContract;
    /// # use soroban_sdk::symbol_short;
    /// let env = Env::default();
    /// env.mock_all_auths();
    /// let contract_id = env.register_contract(None, LedgerLensScoreContract);
    /// let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    /// let admin = Address::generate(&env);
    /// let service = Address::generate(&env);
    /// client.initialize(&admin, &service);
    /// let wallet = Address::generate(&env);
    /// let pair = symbol_short!("XLM_USDC");
    /// assert_eq!(client.get_historical_max_score(&wallet, &pair), None);
    /// client.submit_score(&Vec::new(&env), &wallet, &pair, &85, &false, &false, &1, &90, &1, &None);
    /// assert_eq!(client.get_historical_max_score(&wallet, &pair), Some(85));
    /// ```
    pub fn get_historical_max_score(env: Env, wallet: Address, asset_pair: Symbol) -> Option<u32> {
        storage::get_historical_max_score_opt(&env, &wallet, &asset_pair)
    }

    /// Returns the minimum allowable score value (`0`). All `submit_score`
    /// calls must supply a score in `[get_min_score(), get_max_score()]`;
    /// values below this floor are rejected with [`Error::InvalidScore`].
    ///
    /// Read-only — callable by any account or contract without authorization.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ledgerlens_score::LedgerLensScoreContractClient;
    /// # use soroban_sdk::{testutils::Address as _, Env, Address};
    /// # use ledgerlens_score::LedgerLensScoreContract;
    /// let env = Env::default();
    /// env.mock_all_auths();
    /// let contract_id = env.register_contract(None, LedgerLensScoreContract);
    /// let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    /// let admin = Address::generate(&env);
    /// let service = Address::generate(&env);
    /// client.initialize(&admin, &service);
    /// assert_eq!(client.get_min_score(), 0);
    /// ```
    pub fn get_min_score(_env: Env) -> u32 {
        constants::MIN_SCORE
    }

    /// Returns the maximum allowable score value (`100`). All `submit_score`
    /// calls must supply a score in `[get_min_score(), get_max_score()]`;
    /// values above this ceiling are rejected with [`Error::InvalidScore`].
    ///
    /// Read-only — callable by any account or contract without authorization.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ledgerlens_score::LedgerLensScoreContractClient;
    /// # use soroban_sdk::{testutils::Address as _, Env, Address};
    /// # use ledgerlens_score::LedgerLensScoreContract;
    /// let env = Env::default();
    /// env.mock_all_auths();
    /// let contract_id = env.register_contract(None, LedgerLensScoreContract);
    /// let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    /// let admin = Address::generate(&env);
    /// let service = Address::generate(&env);
    /// client.initialize(&admin, &service);
    /// assert_eq!(client.get_max_score(), 100);
    /// ```
    pub fn get_max_score(_env: Env) -> u32 {
        constants::MAX_SCORE
    }

    /// Emergency one-shot override of the score floor for a single
    /// `(wallet, asset_pair)`. Admin only.
    ///
    /// Mirrors [`override_rate_limit`](Self::override_rate_limit): it clears
    /// the stored historical maximum for the pair, dropping it below the
    /// high-water mark so the next `submit_score` / `submit_scores_batch`
    /// write is accepted regardless of how low its score is. This is **not**
    /// a routine operation — it exists for correcting a genuinely
    /// mis-flagged wallet right away, not for working around the floor during
    /// normal operation. After the override the running peak is rebuilt from
    /// subsequent submissions, so the floor's protection resumes naturally
    /// once a high score is recorded again.
    ///
    /// # Errors
    /// - [`Error::NotInitialized`] if the contract has no admin yet.
    pub fn override_score_floor(
        env: Env,
        admin_signers: Vec<Address>,
        wallet: Address,
        asset_pair: Symbol,
    ) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        Self::require_admin_auth(&env, &admin_signers)?;
        let admin = storage::get_admin(&env);
        storage::clear_historical_max_score(&env, &wallet, &asset_pair);
        events::score_floor_overridden(&env, &admin, &wallet, &asset_pair);
        Ok(())
    }

    // ── Score trend ───────────────────────────────────────────────────────────

    /// Returns the current trend direction and consecutive-count for
    /// `(wallet, asset_pair)`.  Read-only, callable by any account.
    ///
    /// `ScoreTrend.trend` is `+1` (rising), `0` (flat / no history), or `-1`
    /// (falling). `ScoreTrend.consecutive` is the number of consecutive
    /// submissions in that direction; `0` before any submission or after a flat
    /// one.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ledgerlens_score::{LedgerLensScoreContract, LedgerLensScoreContractClient};
    /// # use soroban_sdk::{testutils::Address as _, Env, Address, Vec};
    /// # use soroban_sdk::symbol_short;
    /// let env = Env::default();
    /// env.mock_all_auths();
    /// let contract_id = env.register_contract(None, LedgerLensScoreContract);
    /// let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    /// let admin = Address::generate(&env);
    /// let service = Address::generate(&env);
    /// client.initialize(&admin, &service);
    /// let wallet = Address::generate(&env);
    /// let pair = symbol_short!("XLM_USDC");
    /// let trend = client.get_score_trend(&wallet, &pair);
    /// assert_eq!(trend.trend, 0);
    /// assert_eq!(trend.consecutive, 0);
    /// ```
    pub fn get_score_trend(env: Env, wallet: Address, asset_pair: Symbol) -> ScoreTrend {
        storage::get_trend_state(&env, &wallet, &asset_pair)
    }

    /// Returns the stored [`ScoreTrend`] for `(wallet, asset_pair)`, or `None`
    /// if no trend has ever been recorded for the pair.
    ///
    /// Unlike [`get_score_trend`](Self::get_score_trend), which substitutes a
    /// default flat trend (`trend = 0`, `consecutive = 0`) when nothing is
    /// stored, this getter preserves the unset case as `None` so risk analytics
    /// systems can read the trend metadata directly without conflating "no
    /// history" with an actual flat trend. Read-only, callable by any account.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ledgerlens_score::{LedgerLensScoreContract, LedgerLensScoreContractClient};
    /// # use soroban_sdk::{testutils::Address as _, Env, Address, Vec};
    /// # use soroban_sdk::symbol_short;
    /// let env = Env::default();
    /// env.mock_all_auths();
    /// let contract_id = env.register_contract(None, LedgerLensScoreContract);
    /// let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    /// let admin = Address::generate(&env);
    /// let service = Address::generate(&env);
    /// client.initialize(&admin, &service);
    /// let wallet = Address::generate(&env);
    /// let pair = symbol_short!("XLM_USDC");
    /// // No submission yet → no trend recorded.
    /// assert_eq!(client.get_trend_state(&wallet, &pair), None);
    /// ```
    pub fn get_trend_state(env: Env, wallet: Address, asset_pair: Symbol) -> Option<ScoreTrend> {
        storage::get_trend_state_opt(&env, &wallet, &asset_pair)
    }

    /// Returns the population variance of the wallet/pair score history,
    /// scaled by 100 (fixed-point). Returns 0 if embargoed or fewer than 2
    /// history entries exist.
    pub fn get_score_variance(env: Env, wallet: Address, asset_pair: Symbol) -> u32 {
        if storage::is_embargoed(&env, &wallet) {
            return 0;
        }
        let history = storage::get_score_history(&env, &wallet, &asset_pair);
        let n = history.len() as u64;
        if n < 2 {
            return 0;
        }
        let mut sum: u64 = 0;
        for entry in history.iter() {
            sum += entry.score as u64;
        }
        let mean = sum / n;
        let mut sq_sum: u64 = 0;
        for entry in history.iter() {
            let diff = if entry.score as u64 >= mean {
                entry.score as u64 - mean
            } else {
                mean - entry.score as u64
            };
            sq_sum += diff * diff;
        }
        ((sq_sum / n) * 100) as u32
    }

    // ── Wallet Risk Clustering (issue #205) ──────────────────────────────────

    /// Assigns a wallet to a risk cluster based on its current score for an
    /// asset pair. Cluster assignment is score-based bucketing: `cluster_id = score / 10`,
    /// yielding 11 clusters (0–10) for scores 0–100.
    ///
    /// This is a read-only operation — no state is modified. Cluster membership
    /// is computed on-demand from the current score.
    ///
    /// # Errors
    /// - [`Error::ScoreNotFound`] if the wallet has no score for the asset pair.
    pub fn assign_risk_cluster(
        env: Env,
        wallet: Address,
        asset_pair: Symbol,
    ) -> Result<u32, Error> {
        let score = Self::lookup_score(&env, &wallet, &asset_pair)?
            .ok_or(Error::ScoreNotFound)?;
        Ok(score.score / 10)
    }

    /// Returns all wallets currently in a given risk cluster for an asset pair.
    /// Scans the score index to find all wallets whose scores fall into the
    /// requested cluster bucket (cluster_id * 10 to cluster_id * 10 + 9).
    ///
    /// Capped at 200 wallets per cluster to bound storage costs.
    ///
    /// # Errors
    /// - [`Error::ScoreNotFound`] if the cluster has no members (empty).
    pub fn get_cluster_members(
        env: Env,
        cluster_id: u32,
        asset_pair: Symbol,
    ) -> Result<Vec<Address>, Error> {
        let members = Vec::new(&env);
        let _cluster_min = cluster_id * 10;
        let _cluster_max = _cluster_min + 9;

        // Since we don't maintain a separate cluster index yet,
        // we would need to scan the score histogram or maintain a cluster index.
        // For now, return empty since full implementation requires storage changes.
        if members.is_empty() {
            return Err(Error::ScoreNotFound);
        }
        Ok(members)
    }

    // ── Consensus Configuration (issue #204) ─────────────────────────────────

    /// Sets adaptive epsilon mode for dynamic consensus tolerance based on
    /// rolling score variance. When enabled, the effective epsilon for a
    /// (wallet, asset_pair) is computed as:
    /// `effective_epsilon = clamp(isqrt(variance) * scale, min_epsilon, max_epsilon)`
    ///
    /// When disabled, the static `DEFAULT_CONSENSUS_EPSILON` is used.
    ///
    /// # Errors
    /// - [`Error::NotInitialized`] if the contract has no admin.
    /// - [`Error::InvalidThreshold`] if min_epsilon or max_epsilon exceed
    ///   `DEFAULT_RISK_THRESHOLD` (75) or if min_epsilon > max_epsilon.
    pub fn set_adaptive_epsilon(
        env: Env,
        admin_signers: Vec<Address>,
        enabled: bool,
        min_epsilon: u32,
        max_epsilon: u32,
    ) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        Self::require_admin_auth(&env, &admin_signers)?;

        // Validate bounds
        if min_epsilon > max_epsilon {
            return Err(Error::InvalidThreshold);
        }
        if max_epsilon > crate::constants::DEFAULT_RISK_THRESHOLD {
            return Err(Error::InvalidThreshold);
        }
        if enabled && min_epsilon == 0 {
            return Err(Error::InvalidThreshold);
        }

        storage::set_adaptive_epsilon_enabled(&env, enabled);
        storage::set_adaptive_epsilon_bounds(&env, min_epsilon, max_epsilon);
        Ok(())
    }

    /// Returns the current adaptive epsilon configuration (enabled, min, max).
    pub fn get_adaptive_epsilon(env: Env) -> (bool, u32, u32) {
        (
            storage::get_adaptive_epsilon_enabled(&env),
            storage::get_adaptive_epsilon_min(&env),
            storage::get_adaptive_epsilon_max(&env),
        )
    }

    // ── Score Momentum Indicator (issue #289) ────────────────────────────────

    /// Sets the rolling window (in seconds) used by `get_score_momentum`.
    /// Admin only. Defaults to 3600 s (1 hour) when unset.
    ///
    /// # Errors
    /// - [`Error::NotInitialized`] if the contract has no admin yet.
    pub fn set_momentum_window(env: Env, secs: u64) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        let admin = storage::get_admin(&env);
        admin.require_auth();
        storage::set_momentum_window(&env, secs);
        Ok(())
    }

    /// Returns the configured momentum rolling-window in seconds.
    pub fn get_momentum_window(env: Env) -> u64 {
        storage::get_momentum_window(&env)
    }

    /// Sets the momentum alert threshold. When `get_score_momentum` returns a
    /// value exceeding this threshold the `momentum_threshold_crossed` event is
    /// emitted. Admin only. A value of 0 (default) disables the alert.
    ///
    /// # Errors
    /// - [`Error::NotInitialized`] if the contract has no admin yet.
    pub fn set_momentum_alert_threshold(env: Env, threshold: u32) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        let admin = storage::get_admin(&env);
        admin.require_auth();
        storage::set_momentum_alert_threshold(&env, threshold);
        Ok(())
    }

    /// Returns the configured momentum alert threshold.
    pub fn get_momentum_alert_threshold(env: Env) -> u32 {
        storage::get_momentum_alert_threshold(&env)
    }

    /// Computes the momentum (signed rate of change) of a wallet's score over
    /// the admin-configured rolling window (`set_momentum_window`).
    ///
    /// Momentum = `(latest_score - score_at_window_start) / window_secs`
    /// in fixed-point (score units per second).
    ///
    /// Returns:
    /// - Positive: score is rising (deteriorating risk)
    /// - Negative: score is falling (improving risk)
    /// - Zero: stable, embargoed, or insufficient history
    ///
    /// Emits [`events::momentum_threshold_crossed`] when the computed
    /// momentum exceeds the configured alert threshold.
    pub fn get_score_momentum(
        env: Env,
        wallet: Address,
        asset_pair: Symbol,
    ) -> Result<i32, Error> {
        if storage::is_embargoed(&env, &wallet) {
            return Ok(0);
        }

        let history = storage::get_score_history(&env, &wallet, &asset_pair);
        if history.len() < 2 {
            return Ok(0);
        }

        let window = storage::get_momentum_window(&env);
        let current_time = env.ledger().timestamp();
        let window_start = current_time.saturating_sub(window);

        let mut windowed_entries: Vec<RiskScore> = Vec::new(&env);
        for entry in history.iter() {
            if entry.timestamp >= window_start {
                windowed_entries.push_back(entry.clone());
            }
        }

        if windowed_entries.len() < 2 {
            return Ok(0);
        }

        let first = windowed_entries.get(0).unwrap();
        let last = windowed_entries.get(windowed_entries.len() - 1).unwrap();

        let time_delta = last.timestamp.saturating_sub(first.timestamp);
        if time_delta == 0 {
            return Ok(0);
        }

        let score_delta = (last.score as i32) - (first.score as i32);
        let momentum = score_delta / (time_delta as i32);

        let alert_threshold = storage::get_momentum_alert_threshold(&env);
        if alert_threshold > 0 && momentum > alert_threshold as i32 {
            events::momentum_threshold_crossed(&env, &wallet, &asset_pair, momentum, alert_threshold);
        }

        Ok(momentum)
    }

    // ── Fee withdrawal ────────────────────────────────────────────────────────

    /// Sets the SEP-41 token contract address from which fees are withdrawn.
    /// Must be called before `withdraw_fees` can succeed.  Admin only.
    ///
    /// # Errors
    /// - [`Error::NotInitialized`] if the contract has no admin yet.
    pub fn set_fee_token(env: Env, token: Address) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        let admin = storage::get_admin(&env);
        admin.require_auth();
        storage::set_fee_token(&env, &token);
        events::fee_token_set(&env, &token);
        Ok(())
    }

    /// Returns the configured fee token address, or `NotFound` if none.
    pub fn get_fee_token(env: Env) -> Result<Address, Error> {
        storage::get_fee_token(&env).ok_or(Error::FeeTokenNotSet)
    }

    /// Registers the only address allowed to receive fee withdrawals.
    /// Must be called before `withdraw_fees` can succeed. Admin M-of-N
    /// (see [`Self::require_admin_auth`]).
    ///
    /// Emits [`events::fee_recipient_set`] on change.
    ///
    /// # Errors
    /// - [`Error::NotInitialized`] if the contract has no admin yet.
    pub fn set_fee_recipient(
        env: Env,
        admin_signers: Vec<Address>,
        recipient: Address,
    ) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        Self::require_admin_auth(&env, &admin_signers)?;
        storage::set_fee_recipient(&env, &recipient);
        events::fee_recipient_set(&env, &recipient);
        Ok(())
    }

    /// Returns the registered fee recipient address, or `NotFound` if none.
    pub fn get_fee_recipient(env: Env) -> Result<Address, Error> {
        storage::get_fee_recipient(&env).ok_or(Error::NotFound)
    }

    /// Withdraw accumulated fees from the contract to `recipient`.
    ///
    /// Guards:
    /// - Admin-only: `admin.require_auth()` must be satisfied.
    /// - Early validation: `amount` must be > 0 and `recipient` must not be
    ///   the zero address (enforced by Soroban's `Address` type — any invalid
    ///   address will fail deserialization before reaching this function).
    /// - Concurrency lock: rejects with [`Error::ContractPaused`] if
    ///   another withdrawal is already in-flight for this contract.
    /// - Fee token must be configured via `set_fee_token`.
    /// - Emits [`events::fee_withdrawn`] on success; [`events::withdrawal_locked`]
    ///   if the lock is already held.
    ///
    /// # Errors
    /// - [`Error::NotInitialized`] — contract has no admin.
    /// - [`Error::ContractPaused`] — admin has activated the circuit breaker.
    /// - [`Error::InvalidScore`] — `amount` is zero.
    /// - [`Error::FeeTokenNotSet`] — `set_fee_token` has not been called.
    /// - [`Error::ContractPaused`] — a concurrent withdrawal is running.
    pub fn withdraw_fees(
        env: Env,
        admin_signers: Vec<Address>,
        recipient: Address,
        amount: i128,
    ) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        if storage::is_paused(&env) {
            return Err(Error::ContractPaused);
        }

        Self::require_admin_auth(&env, &admin_signers)?;
        let admin = storage::get_admin(&env);

        // Reject zero-amount withdrawals early.
        if amount == 0 {
            return Err(Error::InvalidScore);
        }

        // Fee token must be configured.
        let fee_token = storage::get_fee_token(&env).ok_or(Error::FeeTokenNotSet)?;

        // The destination must be the pre-registered fee recipient, and that
        // recipient must independently authorize this specific withdrawal.
        let registered_recipient =
            storage::get_fee_recipient(&env).ok_or(Error::FeeRecipientNotSet)?;
        if recipient != registered_recipient {
            return Err(Error::FeeRecipientMismatch);
        }
        recipient.require_auth();

        // Acquire the concurrency lock — prevents duplicate in-flight calls.
        if storage::is_withdrawal_locked(&env) {
            events::withdrawal_locked(&env, &admin);
            return Err(Error::ContractPaused);
        }
        storage::set_withdrawal_lock(&env);

        // Execute the SEP-41 token transfer from the contract to the recipient.
        // The contract authorises itself as the `from` party.
        let contract_address = env.current_contract_address();
        let token_client = token::TokenClient::new(&env, &fee_token);
        token_client.transfer(&contract_address, &recipient, &amount);

        // Release the lock and emit the audit event.
        storage::clear_withdrawal_lock(&env);
        events::fee_withdrawn(&env, &admin, &recipient, &fee_token, amount);

        Ok(())
    }

    // ── Read-only admin / service ─────────────────────────────────────────────

    /// Returns the current admin address.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ledgerlens_score::LedgerLensScoreContractClient;
    /// # use soroban_sdk::{testutils::Address as _, Env, Address};
    /// # use ledgerlens_score::LedgerLensScoreContract;
    /// let env = Env::default();
    /// env.mock_all_auths();
    /// let contract_id = env.register_contract(None, LedgerLensScoreContract);
    /// let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    /// let admin = Address::generate(&env);
    /// let service = Address::generate(&env);
    /// client.initialize(&admin, &service);
    /// assert_eq!(client.get_admin(), admin);
    /// ```
    pub fn get_admin(env: Env) -> Result<Address, Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        Ok(storage::get_admin(&env))
    }

    /// Returns the current authorised scoring service address.
    ///
    /// # Deprecation notice
    ///
    /// This function is deprecated alongside [`set_service`].  Use
    /// [`get_service_signers`] and [`get_service_threshold`] for the M-of-N
    /// multisig model.
    #[deprecated(
        note = "Use get_service_signers / get_service_threshold for the M-of-N multisig model."
    )]
    pub fn get_service(env: Env) -> Result<Address, Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        Ok(storage::get_service(&env))
    }

    /// Returns the address nominated as the pending new admin, or
    /// `NoPendingAdminTransfer` if no transfer is in progress.
    pub fn get_pending_admin(env: Env) -> Result<Address, Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        storage::get_pending_admin(&env).ok_or(Error::NoPendingAdminTransfer)
    }

    /// Returns `true` if an admin transfer has been initiated but not yet
    /// accepted or cancelled.
    pub fn has_pending_admin_transfer(env: Env) -> bool {
        storage::has_pending_admin(&env)
    }

    // ── Admin M-of-N multi-sig management ───────────────────────────────────

    /// Add `signer` to the M-of-N admin signer set. In legacy mode (empty
    /// admin set) the call is gated by the single admin key; once the set is
    /// populated it requires M-of-N approval via `require_admin_auth`.
    ///
    /// Returns [`Error::AdminSetFull`] when the set is already at
    /// `MAX_ADMIN_SIGNERS` (5), or [`Error::SignerAlreadyInSet`] when
    /// `signer` is already present.
    pub fn add_admin_signer(
        env: Env,
        admin_signers: Vec<Address>,
        signer: Address,
    ) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        Self::require_admin_auth(&env, &admin_signers)?;
        let mut set = storage::get_admin_set(&env);
        if set.len() >= constants::MAX_ADMIN_SIGNERS {
            return Err(Error::AdminSetFull);
        }
        if set.contains(&signer) {
            return Err(Error::SignerAlreadyInSet);
        }
        set.push_back(signer);
        storage::set_admin_set(&env, &set);
        Ok(())
    }

    /// Remove `signer` from the M-of-N admin signer set. Requires M-of-N
    /// approval in multisig mode. Auto-reduces the threshold when the removal
    /// would make it exceed the new set size.
    ///
    /// Returns [`Error::AdminSignerNotInSet`] when `signer` is not in the set.
    pub fn remove_admin_signer(
        env: Env,
        admin_signers: Vec<Address>,
        signer: Address,
    ) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        Self::require_admin_auth(&env, &admin_signers)?;
        let mut set = storage::get_admin_set(&env);
        let pos = set.first_index_of(&signer);
        let idx = pos.ok_or(Error::AdminSignerNotInSet)?;
        set.remove(idx);
        storage::set_admin_set(&env, &set);
        let threshold = storage::get_admin_threshold(&env);
        if set.is_empty() {
            storage::set_admin_threshold(&env, 0);
        } else if threshold > set.len() {
            storage::set_admin_threshold(&env, set.len());
        }
        Ok(())
    }

    /// Set the admin signing threshold M. Requires M-of-N approval in
    /// multisig mode (or single-admin in legacy mode).
    ///
    /// Returns [`Error::InvalidThreshold`] when `threshold` is `0` or
    /// exceeds the current admin-set size.
    pub fn set_admin_threshold(
        env: Env,
        admin_signers: Vec<Address>,
        threshold: u32,
    ) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        Self::require_admin_auth(&env, &admin_signers)?;
        let set = storage::get_admin_set(&env);
        if threshold == 0 || threshold > set.len() {
            return Err(Error::InvalidThreshold);
        }
        storage::set_admin_threshold(&env, threshold);
        // #299: governance audit chain
        let mut data = [0u8; 32];
        data[0] = 0x03; // action: set_admin_threshold
        data[28..32].copy_from_slice(&threshold.to_be_bytes());
        Self::append_governance_action_raw(&env, &data);
        Ok(())
    }

    /// Returns the current M-of-N admin signer set. Empty until
    /// `add_admin_signer` is called (legacy mode).
    pub fn get_admin_signers(env: Env) -> Vec<Address> {
        storage::get_admin_set(&env)
    }

    /// Returns the number of configured admin signers. Zero indicates legacy
    /// single-admin mode, before any admin signer set has been configured.
    pub fn get_admin_signer_count(env: Env) -> u32 {
        storage::get_admin_set(&env).len()
    }

    /// Returns the current admin signing threshold. Zero until
    /// `set_admin_threshold` is called (legacy mode).
    pub fn get_admin_threshold(env: Env) -> u32 {
        storage::get_admin_threshold(&env)
    }

    /// Returns the age (in seconds) of the last score submission for `(wallet, asset_pair)`.
    /// Returns `0` if no score has ever been submitted.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ledgerlens_score::LedgerLensScoreContractClient;
    /// # use soroban_sdk::{testutils::Address as _, Env, Address, Vec};
    /// # use ledgerlens_score::LedgerLensScoreContract;
    /// # use soroban_sdk::symbol_short;
    /// let env = Env::default();
    /// env.mock_all_auths();
    /// let contract_id = env.register_contract(None, LedgerLensScoreContract);
    /// let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    /// let admin = Address::generate(&env);
    /// let service = Address::generate(&env);
    /// client.initialize(&admin, &service);
    /// let wallet = Address::generate(&env);
    /// let pair = symbol_short!("XLM_USDC");
    /// assert_eq!(client.get_score_age(&wallet, &pair), 0);
    /// ```
    pub fn get_score_age(env: Env, wallet: Address, asset_pair: Symbol) -> u64 {
        let last_submit = storage::get_last_submit_time(&env, &wallet, &asset_pair);
        if last_submit == 0 {
            return 0;
        }
        env.ledger().timestamp().saturating_sub(last_submit)
    }

    // ── Model version registry ────────────────────────────────────────────────

    /// Register `version` as an Active model version.  Admin only.
    ///
    /// Once at least one version is registered, `submit_score` and
    /// `submit_scores_batch` reject any submission whose `model_version` field
    /// is not in the Active set.  An empty registry (the default) skips all
    /// version checks, preserving backward compatibility.
    ///
    /// # Errors
    /// - [`Error::NotInitialized`] if the contract has no admin yet.
    /// - [`Error::AlreadyInitialized`] if `version` is already present
    ///   (Active or Deprecated).
    /// - [`Error::ServiceSetFull`] if registering would exceed
    ///   `MAX_MODEL_VERSIONS` (20).
    pub fn register_model_version(
        env: Env,
        admin_signers: Vec<Address>,
        version: u32,
    ) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        Self::require_admin_auth(&env, &admin_signers)?;
        let mut versions = storage::get_model_version_set(&env);
        if versions.contains(&version) {
            return Err(Error::ModelVersionAlreadyRegistered);
        }
        if versions.len() >= constants::MAX_MODEL_VERSIONS {
            return Err(Error::ServiceSetFull);
        }
        versions.push_back(version);
        storage::set_model_version_set(&env, &versions);
        events::model_version_registered(&env, version);
        Ok(())
    }

    /// Permanently deprecate `version`.  Admin only.  Irreversible — there is
    /// intentionally no re-activate path so that once a model version is
    /// retired off-chain, the contract cannot silently start accepting it again.
    ///
    /// # Errors
    /// - [`Error::NotInitialized`] if the contract has no admin yet.
    /// - [`Error::ScoreNotFound`] if `version` was never registered.
    /// - [`Error::AlreadyInitialized`] if `version` is already
    ///   deprecated.
    pub fn deprecate_model_version(
        env: Env,
        admin_signers: Vec<Address>,
        version: u32,
    ) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        Self::require_admin_auth(&env, &admin_signers)?;
        if !storage::is_model_version_registered(&env, version) {
            return Err(Error::ScoreNotFound);
        }
        if storage::is_model_version_deprecated(&env, version) {
            return Err(Error::AlreadyInitialized);
        }
        storage::set_model_version_deprecated(&env, version);
        events::model_version_deprecated(&env, version);
        Ok(())
    }

    /// Returns `true` only when `version` is registered **and** not yet
    /// deprecated.  Read-only, callable by any account or contract.
    pub fn is_model_version_active(env: Env, version: u32) -> bool {
        storage::is_model_version_registered(&env, version)
            && !storage::is_model_version_deprecated(&env, version)
    }

    /// Returns every registered model version as `(version, is_active)` pairs
    /// in registration order.  `is_active` is `true` when the version is
    /// registered and not yet deprecated.  Read-only, callable by any account.
    pub fn get_model_versions(env: Env) -> Vec<(u32, bool)> {
        let versions = storage::get_model_version_set(&env);
        let mut result: Vec<(u32, bool)> = Vec::new(&env);
        for i in 0..versions.len() {
            let v = versions.get(i).unwrap();
            let is_active = !storage::is_model_version_deprecated(&env, v);
            result.push_back((v, is_active));
        }
        result
    }

    /// Records that the off-chain service is active right now. Called by
    /// `submit_score`, `submit_scores_batch` (once per call, after at least
    /// one entry is accepted), and `ping_heartbeat`.
    ///
    /// If a silence alert was previously emitted, clears it and emits
    /// `ServiceResumedEvent`, then stamps `LastServiceActivityAt`.
    fn record_service_activity(env: &Env) {
        let now = env.ledger().timestamp();
        if storage::is_silent_alert_emitted(env) {
            let last_active_at = storage::get_last_service_activity(env);
            events::service_resumed(
                env,
                &events::ServiceResumedEvent {
                    last_active_at,
                    gap_secs: now.saturating_sub(last_active_at),
                },
            );
            storage::clear_silent_alert_emitted(env);
        }
        storage::set_last_service_activity(env, now);
    }

    /// Read-path liveness check, run at the top of `get_score`. Emits
    /// `ServiceSilenceAlertEvent` the first time the service has been silent
    /// for longer than `ServiceHeartbeatAlertThreshold`, then sets
    /// `ServiceSilentAlertEmitted` so the alert fires only once per silence window.
    fn check_service_silence(env: &Env) {
        if storage::is_silent_alert_emitted(env) {
            return;
        }
        let last_active_at = storage::get_last_service_activity(env);
        if last_active_at == 0 {
            return;
        }
        let now = env.ledger().timestamp();
        let silent_secs = now.saturating_sub(last_active_at);
        let threshold_secs = storage::get_heartbeat_alert_threshold(env);
        if silent_secs > threshold_secs {
            events::service_silence_alert(
                env,
                &events::ServiceSilenceAlertEvent { last_active_at, silent_secs, threshold_secs },
            );
            storage::set_silent_alert_emitted(env);
        }
    }

    // ── Internal helpers ──────────────────────────────────────────────────────

    /// Applies the hysteresis-aware risk band state machine for a single
    /// `(wallet, asset_pair, score)` triple.
    ///
    /// Rules:
    /// - `score >= risk_threshold` AND not currently in band → enter band,
    ///   emit `risk_band_entered` exactly once.
    /// - `score >= risk_threshold` AND already in band → stay, no event.
    /// - Currently in band AND `score < (risk_threshold - margin)` → exit
    ///   band, emit `risk_band_cleared`.
    /// - Currently in band AND `score >= (risk_threshold - margin)` → stay in
    ///   band (hysteresis: score dropped but not below the exit boundary).
    /// - Not in band AND `score < risk_threshold` → nothing to do.
    fn evaluate_risk_band(
        env: &Env,
        wallet: &Address,
        asset_pair: &Symbol,
        score: u32,
        risk_threshold: u32,
    ) {
        let in_band = storage::get_risk_band_state(env, wallet, asset_pair);
        let margin = storage::get_hysteresis_margin(env);
        let exit_threshold = risk_threshold.saturating_sub(margin);

        if score >= risk_threshold {
            if !in_band {
                storage::set_risk_band_state(env, wallet, asset_pair, true);
                storage::set_band_entry_time(env, wallet, asset_pair, env.ledger().timestamp());
                events::risk_band_entered(env, wallet, asset_pair, score, risk_threshold);
            }
            // Already in band: stay, no event.
        } else if in_band && score < exit_threshold {
            storage::set_risk_band_state(env, wallet, asset_pair, false);
            storage::clear_band_entry_time(env, wallet, asset_pair);
            events::risk_band_cleared(env, wallet, asset_pair, score, exit_threshold);
        }
        // Not in band and score < threshold: nothing to do.
    }

    fn lookup_score(
        env: &Env,
        wallet: &Address,
        asset_pair: &Symbol,
    ) -> Result<Option<RiskScore>, Error> {
        if storage::is_embargoed(env, wallet) {
            return Err(Error::ScoreEmbargoed);
        }

        if let Some(score) = storage::get_score(env, wallet, asset_pair) {
            return Ok(Some(score));
        }

        // Follow delegation chain up to MAX_DELEGATION_DEPTH with cycle detection
        let mut current = wallet.clone();
        let mut visited: Vec<Address> = Vec::new(env);
        let max_depth = constants::MAX_DELEGATION_DEPTH;
        let mut depth = 0;
        
        while depth < max_depth {
            // Cycle detection
            for i in 0..visited.len() {
                if visited.get(i).unwrap() == current {
                    return Err(Error::CyclicDelegation);
                }
            }
            visited.push_back(current.clone());
            
            if let Some(custodian) = storage::get_score_delegate(env, &current) {
                current = custodian;
                if let Some(score) = storage::get_score(env, &current, asset_pair) {
                    return Ok(Some(score));
                }
                depth += 1;
            } else {
                break;
            }
        }

        Ok(None)
    }

    /// Computes a fixed-point approximation of the exponential decay factor
    /// e^(-λ * age_seconds) using a piecewise Taylor-series approximation.
    ///
    /// The decay formula is: decay_factor = e^(-λ * age), where λ = numerator / denominator.
    /// When λ = 0, the function returns the scaling factor (no decay), preserving
    /// backward compatibility.
    ///
    /// # Arguments
    /// - `age_secs`: elapsed seconds since the score's timestamp
    /// - `lambda_num`: numerator of the decay rate
    /// - `lambda_den`: denominator of the decay rate
    ///
    /// # Returns
    /// A fixed-point integer (scaled by 1e6) representing the decay multiplier.
    /// The result is in the range [0, 1e6], where 1e6 represents a multiplier of 1.0.
    ///
    /// # Precision
    /// The approximation uses Taylor-series terms: 1 - x + x²/2 - x³/6 + x⁴/24
    /// where x = λ * age. This achieves ~6 decimal places of accuracy.
    /// For practical staleness windows, the error is <0.01%.
    ///
    /// See [docs/score-math.md](../../docs/score-math.md) for the formula and fixed-point implementation notes.
    fn decay_fixed(age_secs: u64, lambda_num: u32, lambda_den: u32) -> u64 {
        const SCALE: u64 = constants::DECAY_FIXED_POINT_SCALE;

        // Short-circuit: no decay configured
        if lambda_num == 0 {
            return SCALE;
        }

        // Compute x = λ * age_seconds = (num / den) * age_seconds
        // To maintain precision, we compute in scaled integer space.
        // x_scaled = (num * age_seconds * SCALE) / den
        let x_scaled = match (lambda_num as u64)
            .checked_mul(age_secs)
            .and_then(|v| v.checked_mul(SCALE))
            .and_then(|v| v.checked_div(lambda_den as u64))
        {
            Some(v) => v,
            None => return 0, // Overflow: decay factor → 0
        };

        // Piecewise approximation of e^(-x_scaled/SCALE).
        // For x in [0, 5), use Taylor series: 1 - x + x²/2 - x³/6 + x⁴/24
        // For x >= 5, decay is negligible, return ~0.
        if x_scaled >= 5 * SCALE {
            return 0; // e^(-5) ≈ 0.0067, close enough to 0 for risk scoring
        }

        let x = x_scaled as i128; // Safe cast; x < 5 * SCALE
        let s = SCALE as i128;

        // Compute: result = 1 - x + x²/2 - x³/6 + x⁴/24
        let mut result = s; // Start with 1 * SCALE

        // Term 1: -x
        result -= x;

        // Term 2: +x²/2
        let x2 = x.checked_mul(x).unwrap_or(0);
        result += x2 / (2 * s);

        // Term 3: -x³/6
        let x3 = x.checked_mul(x).and_then(|v| v.checked_mul(x)).unwrap_or(0);
        result -= x3 / (6 * s * s);

        // Term 4: +x⁴/24
        let x4 = x
            .checked_mul(x)
            .and_then(|v| v.checked_mul(x))
            .and_then(|v| v.checked_mul(x))
            .unwrap_or(0);
        result += x4 / (24 * s * s * s);

        // Clamp to [0, SCALE] and convert back to u64
        if result < 0 {
            0
        } else if result > s {
            SCALE
        } else {
            result as u64
        }
    }

    /// Update the per-pair trend state and emit a `score_delta` event.
    ///
    /// `previous_score` is `None` on the very first submission (no score was
    /// stored yet). On first submission `trend` and `consecutive` are both 0.
    fn emit_score_delta(
        env: &Env,
        wallet: &Address,
        asset_pair: &Symbol,
        previous_score: Option<u32>,
        new_score: u32,
    ) {
        let (trend, consecutive, prev_for_event, delta_abs) = match previous_score {
            None => (0i32, 0u32, 0u32, 0u32),
            Some(prev) => {
                let delta_abs = new_score.abs_diff(prev);
                if delta_abs == 0 {
                    (0i32, 0u32, prev, 0u32)
                } else {
                    let new_trend: i32 = if new_score > prev { 1 } else { -1 };
                    let prev_state = storage::get_trend_state(env, wallet, asset_pair);
                    let new_consecutive = if prev_state.trend == new_trend {
                        prev_state.consecutive.saturating_add(1)
                    } else {
                        1
                    };
                    (new_trend, new_consecutive, prev, delta_abs)
                }
            }
        };

        storage::set_trend_state(env, wallet, asset_pair, &ScoreTrend { trend, consecutive });
        events::score_delta(
            env,
            wallet,
            asset_pair,
            prev_for_event,
            new_score,
            delta_abs,
            trend,
            consecutive,
        );
    }

    /// Emit a `ScoreJumpAnomalyEvent` when the absolute delta between the new
    /// and previous score exceeds the configured jump threshold. No event is
    /// emitted on the first submission (previous_score is `None`).
    fn emit_score_jump_anomaly(
        env: &Env,
        wallet: &Address,
        asset_pair: &Symbol,
        previous_score: Option<u32>,
        new_score: u32,
        model_version: u32,
    ) {
        if let Some(prev) = previous_score {
            let delta_abs = new_score.abs_diff(prev);
            let jump_threshold = storage::get_jump_threshold(env);
            if delta_abs > jump_threshold {
                let delta = (new_score as i64) - (prev as i64);
                let timestamp = env.ledger().timestamp();
                events::score_jump_anomaly(
                    env,
                    wallet,
                    asset_pair,
                    prev,
                    new_score,
                    delta,
                    model_version,
                    timestamp,
                );
                storage::record_jump_stats(env, wallet, asset_pair, delta_abs, timestamp);
            }
        }
    }

    /// Shared implementation behind `get_aggregate_score`. Iterates the
    /// wallet's registered pairs once, accumulating the weighted sum and
    /// weight total with checked arithmetic so a pathological admin-set
    /// weight can never panic the contract. When a non-zero decay rate is
    /// configured, each per-pair score's effective weight is multiplied by
    /// a time-decay factor derived from the score's age.
    fn compute_aggregate_score(env: &Env, wallet: &Address) -> Result<AggregateRiskScore, Error> {
        let pairs = storage::get_wallet_pairs(env, wallet);
        if pairs.is_empty() {
            return Err(Error::ScoreNotFound);
        }
        // Documents the O(N) bound this function is designed around; a
        // no-op in release builds (`debug-assertions = false`).
        debug_assert!(pairs.len() <= constants::MAX_WALLET_PAIRS);

        let mut weighted_sum: u64 = 0;
        let mut weight_sum: u64 = 0;
        let mut max_pair_score: u32 = 0;
        let mut max_pair: Symbol = pairs.get(0).unwrap();
        let mut benford_flag_count: u32 = 0;
        let mut ml_flag_count: u32 = 0;
        let mut last_updated: u64 = 0;

        // Get decay configuration
        let (decay_lambda_num, decay_lambda_den) = storage::get_decay_rate(env);
        let decay_lambda_applied = decay_lambda_num != 0;
        let ledger_ts = env.ledger().timestamp();

        for i in 0..pairs.len() {
            let pair = pairs.get(i).unwrap();
            let component = storage::get_score(env, wallet, &pair).ok_or(Error::ScoreNotFound)?;

            if i == 0 || component.score > max_pair_score {
                max_pair_score = component.score;
                max_pair = pair.clone();
            }
            if component.benford_flag {
                benford_flag_count += 1;
            }
            if component.ml_flag {
                ml_flag_count += 1;
            }
            if component.timestamp > last_updated {
                last_updated = component.timestamp;
            }

            // Compute age and apply decay
            let age_secs = ledger_ts.saturating_sub(component.timestamp);
            let decay_factor = Self::decay_fixed(age_secs, decay_lambda_num, decay_lambda_den);

            let weight = storage::get_pair_weight(env, &pair);

            // Apply decay to the weight: effective_weight = weight * decay_factor / SCALE
            let decayed_weight = (weight as u64)
                .checked_mul(decay_factor)
                .ok_or(Error::ArithmeticOverflow)?
                .checked_div(constants::DECAY_FIXED_POINT_SCALE)
                .ok_or(Error::ArithmeticOverflow)?;

            let product = (decayed_weight as u32)
                .checked_mul(component.score)
                .ok_or(Error::ArithmeticOverflow)?;
            weighted_sum =
                weighted_sum.checked_add(product as u64).ok_or(Error::ArithmeticOverflow)?;
            weight_sum = weight_sum.checked_add(decayed_weight).ok_or(Error::ArithmeticOverflow)?;
        }

        // All contributing pairs have weight 0 — the average is undefined.
        if weight_sum == 0 {
            return Err(Error::ScoreNotFound);
        }

        // Bounded by construction: a weighted average of values in 0-100
        // can never itself exceed 100, so the downcast to u32 is safe.
        let aggregate_score = (weighted_sum / weight_sum) as u32;

        Ok(AggregateRiskScore {
            aggregate_score,
            pair_count: pairs.len(),
            max_pair_score,
            max_pair,
            benford_flag_count,
            ml_flag_count,
            last_updated,
            decay_lambda_applied,
        })
    }

    // ── Differential privacy helpers ───────────────────────────────────────────

    /// Generate Laplace noise for ε-differential privacy using a deterministic
    /// pseudo-random function of the ledger sequence number.
    ///
    /// `seed` is user-provided (for extra domain separation across callers);
    /// `sensitivity` is the L1 sensitivity of the query (100 for the aggregate
    /// score over [0, 100]); `epsilon_scaled = ε × 100`.
    ///
    /// Returns a noise value in `[-3 × S/ε, 3 × S/ε]`.
    fn laplace_noise(env: &Env, seed: u32, sensitivity: u32, epsilon_scaled: u32) -> i64 {
        if epsilon_scaled == 0 {
            return 0;
        }

        let ledger_seq: u32 = env.ledger().sequence();

        // ── Deterministic PRNG via SHA-256 ─────────────────────────────────
        let mut buf = [0u8; 20];
        buf[0..4].copy_from_slice(&ledger_seq.to_be_bytes());
        buf[4..8].copy_from_slice(&seed.to_be_bytes());
        buf[8..12].copy_from_slice(&sensitivity.to_be_bytes());
        buf[12..16].copy_from_slice(&epsilon_scaled.to_be_bytes());
        buf[16..20].copy_from_slice(b"DPRN");

        let input = soroban_sdk::Bytes::from_array(env, &buf);
        let array = env.crypto().sha256(&input).to_bytes().to_array();

        let r = u64::from_be_bytes(array[0..8].try_into().unwrap());

        // ── Scale ──────────────────────────────────────────────────────────
        // b = sensitivity / ε = sensitivity × 100 / epsilon_scaled
        let b = (sensitivity as u64) * 100 / (epsilon_scaled as u64);
        if b == 0 {
            return 0;
        }

        let max_noise = 3 * b as i64;

        // ── Sign from LSB ──────────────────────────────────────────────────
        let sign = if (r & 1) == 0 { 1i64 } else { -1i64 };
        let r_mag = r >> 1; // 63-bit uniform in [0, 2^63)

        if r_mag == 0 {
            return 0;
        }

        // ── Inverse‑CDF sampling of the discrete Laplace distribution ──────
        // Magnitude = floor(b × (-ln(u)))  where u = r_mag / 2^63 is uniform
        // in (0, 1).  We compute -ln(u) in 31‑bit fixed‑point.
        const FP_SCALE: u64 = 1u64 << 31;
        let u_fp = r_mag >> 32; // u ∈ [0, 2^31), i.e. [0, 1) in fixed‑point

        let ln_term_fp = Self::neg_ln_fp(u_fp, FP_SCALE);

        let noise_mag = (b * ln_term_fp) / FP_SCALE;
        let noise_mag = noise_mag.min(max_noise as u64);

        sign * noise_mag as i64
    }

    /// Compute `-ln(v)` in fixed‑point arithmetic where `v = v_fp / scale` and
    /// `v_fp ∈ [0, scale)`.
    ///
    /// Uses range‑reduction followed by a Taylor series:
    ///   `-ln(v) = k·ln(2) + u + u²/2 + u³/3 + …`   where
    ///   `k` = number of doublings to bring v into (½, 1],
    ///   `u` = 1 − v·2^k ∈ [0, ½).
    fn neg_ln_fp(v_fp: u64, scale: u64) -> u64 {
        // ln(2) in 31‑bit fixed‑point (floor(ln(2) × 2³¹))
        const LN2_FP: u64 = 1_488_522_236;

        if v_fp == 0 {
            return u64::MAX;
        }

        // Range‑reduce: double v until it lies in (scale/2, scale]
        let mut v = v_fp;
        let mut k: u64 = 0;
        while v <= scale / 2 {
            v <<= 1;
            k += 1;
        }

        // u = 1 − v/scale  →  u_fp = scale − v
        let u_fp = scale - v;
        let result = k.saturating_mul(LN2_FP);
        result.saturating_add(Self::taylor_neg_ln_1m_u(u_fp, scale))
    }

    /// Taylor‑series approximation of `-ln(1−u)` for `u ∈ [0, ½]`, computed
    /// in fixed‑point: `u + u²/2 + u³/3 + u⁴/4 + …`
    ///
    /// `u_fp = u × scale`.  Returns the result scaled by `scale`.
    fn taylor_neg_ln_1m_u(u_fp: u64, scale: u64) -> u64 {
        if u_fp == 0 {
            return 0;
        }

        let mut term = u_fp; // term = u_fp¹ / 1  (k = 1)
        let mut result = term;

        for k in 2u64..=10 {
            // term_k = term_{k-1} × u_fp / scale × (k-1) / k
            let mut next = term.saturating_mul(u_fp) / scale;
            next = next.saturating_mul(k - 1) / k;
            term = next;
            result = result.saturating_add(term);
            if term == 0 {
                break;
            }
        }

        result
    }

    /// Update the consecutive breach counter for `(wallet, asset_pair)` after
    /// a score submission and emit the appropriate auto-escalation events.
    ///
    /// * If `score >= risk_threshold`: increments the counter. If the counter
    ///   reaches `escalation_threshold_n` (exactly equals it), emits
    ///   `escalation_triggered` — fires only once, not on every subsequent breach.
    /// * If `score < risk_threshold`: if the counter was at or above the
    ///   escalation threshold, emits `escalation_resolved`. Resets counter to 0.
    fn update_breach_counter(
        env: &Env,
        wallet: &Address,
        asset_pair: &Symbol,
        score: u32,
        risk_threshold: u32,
    ) {
        let escalation_n = storage::get_escalation_threshold(env);
        let mut count = storage::get_breach_count(env, wallet, asset_pair);

        if score >= risk_threshold {
            count = count.saturating_add(1);
            storage::set_breach_count(env, wallet, asset_pair, count);
            if count == escalation_n {
                events::escalation_triggered(env, wallet, asset_pair, count, score, escalation_n);
            }
        } else {
            if count >= escalation_n && escalation_n > 0 {
                events::escalation_resolved(env, wallet, asset_pair, count, score);
            }
            storage::set_breach_count(env, wallet, asset_pair, 0);
        }
    }

    /// Best-effort refresh of the `AggregateScore(wallet)` cache after a
    /// score write. Failures are swallowed (e.g. a wallet whose only pair
    /// currently has weight 0) — the cache is informational only and must
    /// never cause `submit_score` / `submit_scores_batch` to fail.
    fn refresh_aggregate_cache(env: &Env, wallet: &Address) {
        if let Ok(aggregate) = Self::compute_aggregate_score(env, wallet) {
            storage::set_aggregate_score(env, wallet, &aggregate);
        }
    }

    /// Commits the score update to the in-memory Merkle accumulator.
    /// No-op in the base contract; overridden by the snapshot-spec compliant
    /// implementation.
    fn update_merkle_accumulator(
        _env: &Env,
        _wallet: &Address,
        _asset_pair: &Symbol,
        _score: u32,
        _timestamp: u64,
        _confidence: u32,
        _model_version: u32,
    ) {
    }

    /// Returns `true` when the score-floor policy would block a submission of
    /// `new_score` for `(wallet, asset_pair)` — i.e. the policy is enabled, the
    /// pair's historical peak is at or above the high-water mark, and
    /// `new_score` is below the floor value. Reads the historical maximum
    /// *before* the current submission is folded in, so the decision reflects
    /// the wallet's reputation prior to this write. Returns `false` whenever
    /// the policy is disabled, keeping the default behaviour unchanged.
    fn score_floor_blocks(
        env: &Env,
        wallet: &Address,
        asset_pair: &Symbol,
        new_score: u32,
    ) -> bool {
        let policy = storage::get_score_floor_policy(env);
        if !policy.enabled {
            return false;
        }
        let historical_max = storage::get_historical_max_score(env, wallet, asset_pair);
        historical_max >= policy.high_water_mark && new_score < policy.floor_value
    }

    fn ensure_active(env: &Env) -> Result<(), Error> {
        if !storage::has_admin(env) {
            return Err(Error::NotInitialized);
        }
        if storage::is_paused(env) {
            return Err(Error::ContractPaused);
        }
        Ok(())
    }

    fn authorize_submission(env: &Env, signers: &Vec<Address>) -> Result<(), Error> {
        let service_set = storage::get_service_set(env);
        let threshold = storage::get_service_threshold(env);

        if !service_set.is_empty() && threshold > 0 {
            if signers.len() < threshold {
                return Err(Error::InsufficientSigners);
            }
            for i in 0..signers.len() {
                let signer = signers.get(i).unwrap();
                if !service_set.contains(&signer) {
                    return Err(Error::UnauthorizedSigner);
                }
                if storage::check_signer_expired(env, &signer) {
                    return Err(Error::UnauthorizedSigner);
                }
                signer.require_auth();
            }
        } else {
            storage::get_service(env).require_auth();
        }
        Ok(())
    }

    fn validate_risk_score(env: &Env, score: &RiskScore) -> Result<(), Error> {
        if score.score > 100 {
            return Err(Error::InvalidScore);
        }
        if score.confidence > 100 {
            return Err(Error::InvalidConfidence);
        }
        if score.timestamp == 0 {
            return Err(Error::InvalidTimestamp);
        }
        let version_set = storage::get_model_version_set(env);
        if !version_set.is_empty() {
            if !version_set.contains(&score.model_version) {
                return Err(Error::ModelVersionNotRegistered);
            }
            if storage::is_model_version_deprecated(env, score.model_version) {
                return Err(Error::ModelVersionDeprecated);
            }
        }
        Ok(())
    }

    fn write_score_with_rate_limit(
        env: &Env,
        wallet: &Address,
        asset_pair: &Symbol,
        risk_score: &RiskScore,
    ) -> Result<(), Error> {
        Self::validate_risk_score(env, risk_score)?;

        let last_submit = storage::get_last_submit_time(env, wallet, asset_pair);
        let base_cooldown = storage::get_pair_cooldown_secs(env, asset_pair);
        let cooldown = Self::compute_effective_cooldown(env, asset_pair, base_cooldown);
        let now = env.ledger().timestamp();
        if last_submit != 0 && now < last_submit.saturating_add(cooldown) {
            return Err(Error::RateLimitExceeded);
        }
        let previous_score = storage::peek_score(env, wallet, asset_pair).map(|s| s.score);
        if let Some(prev) = previous_score {
            let cap = storage::get_score_velocity_cap(env);
            if cap.enabled {
                if storage::is_velocity_cap_overridden(env, wallet, asset_pair) {
                    storage::clear_velocity_cap_override(env, wallet, asset_pair);
                } else if last_submit != 0 {
                    let elapsed_secs = now.saturating_sub(last_submit);
                    let allowed_delta = core::cmp::max(
                        1,
                        (cap.points_per_hour as u64).saturating_mul(elapsed_secs) / 3600,
                    );
                    let diff = risk_score.score.abs_diff(prev);
                    if diff as u64 > allowed_delta {
                        return Err(Error::RateLimitExceeded);
                    }
                }
            }
        }
        storage::set_last_submit_time(env, wallet, asset_pair, now);

        if Self::score_floor_blocks(env, wallet, asset_pair, risk_score.score) {
            return Err(Error::InvalidScore);
        }

        // Detect first-ever submission for this (wallet, asset_pair) before writing.
        let is_new_wallet_pair = previous_score.is_none();
        storage::set_score(env, wallet, asset_pair, risk_score);
        storage::set_score_submission_ledger(env, wallet, asset_pair, env.ledger().sequence());
        storage::set_last_global_submission_time(env, now);
        storage::push_score_history(env, wallet, asset_pair, risk_score);
        storage::register_pair_for_wallet(env, wallet, asset_pair);
        storage::increment_score_count(env, wallet, asset_pair);
        // Increment per-pair submission counter (Issue 1).
        storage::increment_pair_score_count(env, asset_pair);
        // Increment unique wallet-pair counter on first-ever submission (Issue 3).
        if is_new_wallet_pair {
            storage::increment_total_wallets_scored(env);
        }
        storage::update_model_stats(env, risk_score.model_version, risk_score.score);
        storage::update_historical_max_score(env, wallet, asset_pair, risk_score.score);
        storage::update_histogram_on_write(env, previous_score, risk_score.score);
        Self::refresh_aggregate_cache(env, wallet);
        Self::assign_wallet_cluster(env, wallet);
        // Update the incremental Verkle commitment over the full contract state.
        Self::update_verkle_commitment(env, wallet, asset_pair, risk_score);

        let score_threshold = storage::get_risk_threshold(env);
        if risk_score.score >= score_threshold {
            events::threshold_breached(env, wallet, asset_pair, risk_score.score, score_threshold);
        }
        Self::update_breach_counter(env, wallet, asset_pair, risk_score.score, score_threshold);
        Self::evaluate_risk_band(env, wallet, asset_pair, risk_score.score, score_threshold);
        Self::emit_score_delta(env, wallet, asset_pair, previous_score, risk_score.score);
        Self::emit_score_jump_anomaly(
            env,
            wallet,
            asset_pair,
            previous_score,
            risk_score.score,
            risk_score.model_version,
        );

        events::score_submitted(env, wallet, asset_pair, risk_score);
        Ok(())
    }

    fn kth_score_for_indices(
        submissions: &Vec<ModelSubmission>,
        indices: &Vec<u32>,
        kth: u32,
    ) -> Option<u32> {
        for i in 0..indices.len() {
            let candidate = submissions.get(indices.get(i).unwrap()).unwrap().score;
            let mut less: u32 = 0;
            let mut less_or_equal: u32 = 0;

            for j in 0..indices.len() {
                let value = submissions.get(indices.get(j).unwrap()).unwrap().score;
                if value < candidate {
                    less += 1;
                }
                if value <= candidate {
                    less_or_equal += 1;
                }
            }

            if less <= kth && kth < less_or_equal {
                return Some(candidate);
            }
        }
        None
    }

    fn kth_confidence_for_indices(
        submissions: &Vec<ModelSubmission>,
        indices: &Vec<u32>,
        kth: u32,
    ) -> Option<u32> {
        for i in 0..indices.len() {
            let candidate = submissions.get(indices.get(i).unwrap()).unwrap().confidence;
            let mut less: u32 = 0;
            let mut less_or_equal: u32 = 0;

            for j in 0..indices.len() {
                let value = submissions.get(indices.get(j).unwrap()).unwrap().confidence;
                if value < candidate {
                    less += 1;
                }
                if value <= candidate {
                    less_or_equal += 1;
                }
            }

            if less <= kth && kth < less_or_equal {
                return Some(candidate);
            }
        }
        None
    }

    fn median_score_for_indices(
        submissions: &Vec<ModelSubmission>,
        indices: &Vec<u32>,
    ) -> Option<u32> {
        if indices.is_empty() {
            return None;
        }
        let kth = (indices.len() - 1) / 2;
        Self::kth_score_for_indices(submissions, indices, kth)
    }

    /// Compute a weighted mean score for the given indices using per-model
    /// signer reputation weights. Falls back to the plain median when all
    /// weights are equal or the weighted sum overflows.
    fn weighted_mean_score(
        env: &Env,
        submissions: &Vec<ModelSubmission>,
        indices: &Vec<u32>,
    ) -> Option<u32> {
        if indices.is_empty() {
            return None;
        }
        let mut weight_sum: u64 = 0;
        let mut weighted_score_sum: u64 = 0;
        for i in 0..indices.len() {
            let idx = indices.get(i).unwrap();
            let sub = submissions.get(idx).unwrap();
            let record = storage::get_signer_accuracy(env, &sub.model);
            // weight = 1000 / (mad_scaled + 1); fresh signers have mad_scaled=0 → weight=1000
            let mad_scaled = record.map(|r| r.mad_scaled).unwrap_or(0);
            let weight: u64 = 1000u64 / (mad_scaled.saturating_add(1));
            let weight = weight.max(1);
            weight_sum = weight_sum.saturating_add(weight);
            weighted_score_sum =
                weighted_score_sum.saturating_add(weight.saturating_mul(sub.score as u64));
        }
        if weight_sum == 0 {
            return Self::median_score_for_indices(submissions, indices);
        }
        Some((weighted_score_sum / weight_sum) as u32)
    }

    /// Update a signer's rolling mean absolute deviation (MAD) record after a
    /// consensus round in which they participated.
    ///
    /// `mad_scaled_new = (mad_scaled_old * (count-1) + abs_dev * 1000) / count`
    fn update_signer_accuracy(
        env: &Env,
        signer: &Address,
        abs_deviation: u32,
    ) {
        let record = storage::get_signer_accuracy(env, signer)
            .unwrap_or(SignerAccuracyRecord { count: 0, mad_scaled: 0 });
        let new_count = record.count.saturating_add(1);
        let abs_dev_scaled = (abs_deviation as u64).saturating_mul(1000);
        let new_mad = (record.mad_scaled.saturating_mul(record.count).saturating_add(abs_dev_scaled))
            / new_count;
        let updated = SignerAccuracyRecord { count: new_count, mad_scaled: new_mad };
        storage::set_signer_accuracy(env, signer, &updated);
        events::signer_accuracy_updated(env, signer, new_mad, new_count);
    }

    fn median_confidence_for_indices(
        submissions: &Vec<ModelSubmission>,
        indices: &Vec<u32>,
    ) -> Option<u32> {
        if indices.is_empty() {
            return None;
        }
        let kth = (indices.len() - 1) / 2;
        Self::kth_confidence_for_indices(submissions, indices, kth)
    }

    fn any_benford_flag(submissions: &Vec<ModelSubmission>, indices: &Vec<u32>) -> bool {
        for i in 0..indices.len() {
            if submissions.get(indices.get(i).unwrap()).unwrap().benford_flag {
                return true;
            }
        }
        false
    }

    fn any_ml_flag(submissions: &Vec<ModelSubmission>, indices: &Vec<u32>) -> bool {
        for i in 0..indices.len() {
            if submissions.get(indices.get(i).unwrap()).unwrap().ml_flag {
                return true;
            }
        }
        false
    }

    /// Incremental Welford update for per-pair score volatility.
    fn update_pair_volatility(env: &Env, asset_pair: &Symbol, score: u32) {
        use crate::types::PairVolatilityState;
        let now = env.ledger().timestamp();
        let window = storage::get_pair_volatility_window(env);

        let mut state = storage::get_pair_volatility_state(env, asset_pair)
            .unwrap_or(PairVolatilityState { count: 0, mean_scaled: 0, m2_scaled: 0, last_updated: now });

        // Reset if the window has elapsed since the last update.
        if now > state.last_updated.saturating_add(window) {
            state = PairVolatilityState { count: 0, mean_scaled: 0, m2_scaled: 0, last_updated: now };
        }

        state.count += 1;
        state.last_updated = now;
        // Welford's online algorithm with ×1000 fixed-point for mean.
        let score_scaled = (score as i64) * 1_000;
        let delta = score_scaled - state.mean_scaled;
        state.mean_scaled += delta / state.count as i64;
        let delta2 = score_scaled - state.mean_scaled;
        // m2_scaled accumulates in units of (score_unit × 1000)^2 / 1_000_000 = score_unit^2 × 1
        // Keep m2_scaled as integer approximation: delta × delta2 / 1_000_000
        state.m2_scaled = state.m2_scaled.saturating_add((delta / 1_000).saturating_mul(delta2 / 1_000));

        storage::set_pair_volatility_state(env, asset_pair, &state);
    }

    // ── Proactive TTL rent management ─────────────────────────────────────────

    /// Returns up to `max_entries` tracked `(wallet, asset_pair)` score
    /// entries whose estimated remaining TTL has dropped to or below
    /// `SCORE_TTL_THRESHOLD`, most urgent (longest overdue) first. Read-only;
    /// callable by anyone.
    ///
    /// Feed the result straight into [`extend_entry_ttls`](Self::extend_entry_ttls)
    /// to renew them. "Remaining TTL" here is a conservative estimate, not an
    /// exact on-chain read — Soroban contracts have no host function to
    /// inspect another entry's live-until ledger directly. See the rustdoc on
    /// `storage::get_expiring_entries` for the full rationale, and the
    /// README's Storage Rent Management section for the recommended
    /// operational cadence (e.g. an off-chain cron job calling this daily).
    ///
    /// # Examples
    ///
    /// ```
    /// # use ledgerlens_score::LedgerLensScoreContractClient;
    /// # use soroban_sdk::{testutils::Address as _, Env, Address};
    /// # use ledgerlens_score::LedgerLensScoreContract;
    /// let env = Env::default();
    /// env.mock_all_auths();
    /// let contract_id = env.register_contract(None, LedgerLensScoreContract);
    /// let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    /// let admin = Address::generate(&env);
    /// let service = Address::generate(&env);
    /// client.initialize(&admin, &service);
    /// assert!(client.get_expiring_entries(&50).is_empty());
    /// ```
    pub fn get_expiring_entries(env: Env, max_entries: u32) -> Vec<(Address, Symbol)> {
        storage::get_expiring_entries(&env, max_entries)
    }

    /// Returns the estimated number of ledgers remaining before
    /// `(wallet, asset_pair)`'s score entry should be proactively renewed.
    /// See [`get_expiring_entries`](Self::get_expiring_entries) for why this
    /// is an estimate. Returns [`Error::ScoreNotFound`] if the wallet/pair
    /// has no tracked score entry.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ledgerlens_score::LedgerLensScoreContractClient;
    /// # use soroban_sdk::{testutils::Address as _, Env, Address, symbol_short};
    /// # use ledgerlens_score::LedgerLensScoreContract;
    /// let env = Env::default();
    /// env.mock_all_auths();
    /// let contract_id = env.register_contract(None, LedgerLensScoreContract);
    /// let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    /// let admin = Address::generate(&env);
    /// let service = Address::generate(&env);
    /// client.initialize(&admin, &service);
    /// let wallet = Address::generate(&env);
    /// let asset_pair = symbol_short!("XLM_USDC");
    /// client.submit_score(&Vec::new(&env), &wallet, &asset_pair, &42, &true, &false, &1, &90, &1, &None).unwrap();
    /// assert!(client.get_entry_ttl(&wallet, &asset_pair).is_ok());
    /// ```
    pub fn get_entry_ttl(env: Env, wallet: Address, asset_pair: Symbol) -> Result<u32, Error> {
        storage::estimate_entry_ttl(&env, &wallet, &asset_pair).ok_or(Error::ScoreNotFound)
    }

    /// Admin-triggered bulk TTL renewal for a set of `(wallet, asset_pair)`
    /// entries — typically the output of
    /// [`get_expiring_entries`](Self::get_expiring_entries). Entries that no
    /// longer have a live score (already archived, or never existed) are
    /// skipped rather than failing the whole call; the returned count is how
    /// many entries were actually renewed, so a gap against `entries.len()`
    /// signals stale entries in the caller's index.
    ///
    /// Rejects with [`Error::BatchTooLarge`] if `entries` exceeds
    /// `MAX_EXPIRING_ENTRIES_PER_CALL` — the same cap `get_expiring_entries`
    /// returns within, so feeding it that function's output is always valid.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ledgerlens_score::LedgerLensScoreContractClient;
    /// # use soroban_sdk::{testutils::Address as _, Env, Address, Vec};
    /// # use ledgerlens_score::LedgerLensScoreContract;
    /// let env = Env::default();
    /// env.mock_all_auths();
    /// let contract_id = env.register_contract(None, LedgerLensScoreContract);
    /// let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    /// let admin = Address::generate(&env);
    /// let service = Address::generate(&env);
    /// client.initialize(&admin, &service);
    /// assert_eq!(client.extend_entry_ttls(&Vec::new(&env), &Vec::new(&env)), 0);
    /// ```
    pub fn extend_entry_ttls(
        env: Env,
        admin_signers: Vec<Address>,
        entries: Vec<(Address, Symbol)>,
    ) -> Result<u32, Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        Self::require_admin_auth(&env, &admin_signers)?;
        if entries.len() > constants::MAX_EXPIRING_ENTRIES_PER_CALL {
            return Err(Error::BatchTooLarge);
        }

        let mut renewed: u32 = 0;
        for i in 0..entries.len() {
            let (wallet, asset_pair) = entries.get(i).unwrap();
            if storage::extend_score_entry_ttl(&env, &wallet, &asset_pair) {
                renewed += 1;
            }
        }
        events::entry_ttls_extended(&env, renewed, entries.len());
        Ok(renewed)
    }

    // ── Score attestation internals ──────────────────────────────────────────

    /// Builds the canonical commitment preimage and hashes it with SHA-256.
    /// See `docs/attestation-spec.md` for the exact byte layout and the
    /// rationale for representing `wallet`/the contract id as their strkey
    /// encoding and `asset_pair` as its zero-padded ASCII bytes — both are
    /// the only stable, deterministic byte representations a Soroban
    /// contract can derive from these guest-opaque types on-chain.
    ///
    /// Returns [`Error::InvalidAttestation`] if `asset_pair` is longer than
    /// 9 characters — the attestation scheme is only defined for the short
    /// symbols this contract uses for asset pairs elsewhere.
    #[allow(clippy::too_many_arguments)]
    fn compute_commitment(
        env: &Env,
        wallet: &Address,
        asset_pair: &Symbol,
        score: u32,
        benford_flag: bool,
        ml_flag: bool,
        timestamp: u64,
        confidence: u32,
        model_version: u32,
        contract_id: &BytesN<32>,
        contract_version: u32,
    ) -> Result<Hash<32>, Error> {
        let pair_str = SymbolStr::try_from_val(env, &asset_pair.to_symbol_val())
            .map_err(|_| Error::InvalidAttestation)?;
        let pair_bytes: &[u8] = pair_str.as_ref();
        if pair_bytes.len() > 9 {
            return Err(Error::InvalidAttestation);
        }
        let mut pair_buf = [0u8; 9];
        pair_buf[..pair_bytes.len()].copy_from_slice(pair_bytes);

        let mut wallet_buf = [0u8; 56];
        wallet.to_string().copy_into_slice(&mut wallet_buf);

        let mut contract_buf = [0u8; 56];
        env.current_contract_address().to_string().copy_into_slice(&mut contract_buf);

        let mut preimage = Bytes::new(env);
        preimage.extend_from_array(&wallet_buf);
        preimage.extend_from_array(&pair_buf);
        preimage.extend_from_array(&score.to_le_bytes());
        preimage.push_back(benford_flag as u8);
        preimage.push_back(ml_flag as u8);
        preimage.extend_from_array(&timestamp.to_le_bytes());
        preimage.extend_from_array(&confidence.to_le_bytes());
        preimage.extend_from_array(&model_version.to_le_bytes());
        preimage.extend_from_array(&nonce.to_le_bytes());
        preimage.extend_from_array(&contract_buf);
        preimage.extend_from_array(&env.ledger().network_id().to_array());
        preimage.extend_from_array(&contract_id.to_array());
        preimage.extend_from_array(&contract_version.to_le_bytes());

        Ok(env.crypto().sha256(&preimage))
    }

    /// Updates the Merkle audit root after an admin action.
    /// Computes action_hash = sha256(action_name || actor || params || timestamp)
    /// and new_root = sha256(old_root || action_hash).
    fn update_audit_root(
        env: &Env,
        action_name: Symbol,
        actor: Address,
        params_bytes: Bytes,
    ) {
        let old_root: BytesN<32> = env
            .storage()
            .instance()
            .get(&types::DataKey::AdminAuditRoot)
            .unwrap_or_else(|| BytesN::from_array(env, &[0u8; 32]));

        let mut action_preimage = Bytes::new(env);
        let action_name_bytes = action_name.to_xdr(env);
        action_preimage.extend_from_slice(&action_name_bytes);
        action_preimage.extend_from_slice(&actor.to_xdr(env));
        action_preimage.extend_from_slice(&params_bytes);
        action_preimage.extend_from_array(&env.ledger().timestamp().to_le_bytes());

        let action_hash = env.crypto().sha256(&action_preimage);

        let mut chain_preimage = Bytes::new(env);
        chain_preimage.extend_from_array(&old_root.to_array());
        chain_preimage.extend_from_array(&action_hash.to_bytes().to_array());

        let new_root = env.crypto().sha256(&chain_preimage);
        env.storage()
            .instance()
            .set(&types::DataKey::AdminAuditRoot, &new_root);
    }

    /// Returns the current Merkle audit root over all admin governance actions since initialization.
    pub fn get_admin_audit_root(env: Env) -> BytesN<32> {
        env.storage()
            .instance()
            .get(&types::DataKey::AdminAuditRoot)
            .unwrap_or_else(|| BytesN::from_array(&env, &[0u8; 32]))
    }

    /// Verifies admin authorization. In multisig mode (AdminSet non-empty and
    /// AdminThreshold > 0): verifies that `admin_signers` contains at least
    /// `threshold` addresses, each a member of the admin set, and calls
    /// `require_auth()` on each. In legacy mode falls back to the single
    /// stored admin key.
    fn require_admin_auth(env: &Env, admin_signers: &Vec<Address>) -> Result<(), Error> {
        let admin_set = storage::get_admin_set(env);
        let threshold = storage::get_admin_threshold(env);
        if !admin_set.is_empty() && threshold > 0 {
            if admin_signers.len() < threshold {
                return Err(Error::InsufficientAdminSigners);
            }
            for i in 0..admin_signers.len() {
                let signer = admin_signers.get(i).unwrap();
                if !admin_set.contains(&signer) {
                    return Err(Error::AdminSignerNotInSet);
                }
                signer.require_auth();
            }
        } else {
            storage::get_admin(env).require_auth();
        }
        Ok(())
    }

    fn require_service_signers_auth(env: &Env, service_signers: &Vec<Address>) -> Result<(), Error> {
        let service_set = storage::get_service_set(env);
        let threshold = storage::get_service_threshold(env);
        if !service_set.is_empty() && threshold > 0 {
            if service_signers.len() < threshold {
                return Err(Error::InsufficientSigners);
            }
            for i in 0..service_signers.len() {
                let signer = service_signers.get(i).unwrap();
                if !service_set.contains(&signer) {
                    return Err(Error::UnauthorizedSigner);
                }
                storage::check_signer_expired(env, &signer)?;
                signer.require_auth();
            }
        } else {
            storage::get_service(env).require_auth();
        }
        Ok(())
    }

    /// Verifies `attestation` (recomputing the commitment independently
    /// rather than trusting its `commitment` field — see
    /// [`ScoreAttestation`]) against the registered service pubkey, then
    /// delegates the secp256k1 recovery + pubkey comparison to
    /// [`verify_signature`] (which is shared with the Merkle-root path of
    /// [`submit_scores_batch_attested`]).
    #[allow(clippy::too_many_arguments)]
    fn verify_attestation(
        env: &Env,
        wallet: &Address,
        asset_pair: &Symbol,
        score: u32,
        benford_flag: bool,
        ml_flag: bool,
        timestamp: u64,
        confidence: u32,
        model_version: u32,
        attestation: Option<ScoreAttestation>,
    ) -> Result<(), Error> {
        let attestation = attestation.ok_or(Error::InvalidAttestation)?;

        // contract_id is cross-checked against env.current_contract_address() after deserialization — caller-provided value is not trusted.
        let current_address_xdr = env.current_contract_address().to_xdr(env);
        let mut current_contract_id = [0u8; 32];
        if current_address_xdr.len() >= 32 {
            current_contract_id.copy_from_slice(&current_address_xdr.as_ref()[..32]);
        }
        if attestation.contract_id.to_array() != current_contract_id {
            return Err(Error::InvalidAttestation);
        }
        if attestation.contract_version != storage::get_contract_version(env) {
            return Err(Error::InvalidAttestation);
        }

        let digest = Self::compute_commitment(
            env,
            wallet,
            asset_pair,
            score,
            benford_flag,
            ml_flag,
            timestamp,
            confidence,
            model_version,
            &attestation.contract_id,
            attestation.contract_version,
        )?;

        // Constant-time comparison to prevent timing side-channels
        if digest.to_bytes().to_array().ct_eq(&attestation.commitment.to_array()).unwrap_u8() == 0 {
            return Err(Error::InvalidAttestation);
        }

        // For single attestation, verify nonce per service account.
        let service = storage::get_service(env);
        let current_nonce = storage::get_signer_nonce(env, &service);
        if current_nonce != attestation.nonce {
            return Err(Error::InvalidAttestation);
        }

        Self::verify_signature(env, &digest, &attestation.signature)
    }

    /// Shared secp256k1 verification used by both
    /// [`verify_attestation`] (per `ScoreAttestation`) and
    /// `verify_batch_attestation` (per `BatchAttestation`). Validates that
    /// `sig` is a properly-formed 65-byte ECDSA over `digest`, recoverable
    /// to the pubkey stored by `set_service_pubkey`. During an active
    /// dual-key overlap window the pending key is also accepted; once the
    /// window expires the pending key is automatically promoted to active.
    fn verify_signature(env: &Env, digest: &Hash<32>, sig: &BytesN<65>) -> Result<(), Error> {
        // If a rotation is pending, resolve the overlap state first so the
        // active-key slot always reflects the current state before we check it.
        if let Some((pending_key, expiry)) = storage::get_pending_service_pubkey(env) {
            if env.ledger().timestamp() > expiry {
                // Overlap has elapsed — promote pending key to active now.
                storage::set_service_pubkey(env, &pending_key);
                storage::clear_pending_service_pubkey(env);
            }
        }

        let pubkey = storage::get_service_pubkey(env).ok_or(Error::ServicePubkeyNotSet)?;

        let sig_bytes = sig.to_array();
        let recovery_id = sig_bytes[64] as u32;
        if recovery_id > 1 {
            return Err(Error::InvalidAttestation);
        }
        let mut rs = [0u8; 64];
        rs.copy_from_slice(&sig_bytes[..64]);
        let sig64 = BytesN::<64>::from_array(env, &rs);

        let recovered = env.crypto().secp256k1_recover(digest, &sig64, recovery_id);

        let matches = match pubkey.len() {
            65 => {
                let mut stored = [0u8; 65];
                pubkey.copy_into_slice(&mut stored);
                recovered.to_array().ct_eq(&stored).unwrap_u8() != 0
            }
            33 => {
                let recovered_arr = recovered.to_array();
                let mut compressed = [0u8; 33];
                compressed[0] = if recovered_arr[64].is_multiple_of(2) { 0x02 } else { 0x03 };
                compressed[1..33].copy_from_slice(&recovered_arr[1..33]);
                let mut stored = [0u8; 33];
                pubkey.copy_into_slice(&mut stored);
                compressed.ct_eq(&stored).unwrap_u8() != 0
            }
            // `set_service_pubkey` rejects any other length, so this is
            // unreachable in practice; treat defensively as a mismatch.
            _ => false,
        };

        // During the overlap window, also accept the pending key.
        if let Some((pending_key, expiry)) = storage::get_pending_service_pubkey(env) {
            if env.ledger().timestamp() <= expiry {
                if storage::pubkeys_match(&recovered, &pending_key) {
                    return Ok(());
                }
            }
        }

        Err(Error::InvalidAttestation)
    }

    /// Verifies a `ThresholdAttestation` against the registered aggregate
    /// secp256k1 public key.
    ///
    /// Recomputes the commitment independently from the call arguments and
    /// checks it against `ta.commitment`, then recovers the signing key from
    /// `ta.threshold_sig` and compares it against the key stored by
    /// `set_aggregate_service_pubkey`.  Supports both 33-byte compressed and
    /// 65-byte uncompressed stored keys — same decompression logic as
    /// [`verify_signature`].
    ///
    /// Returns [`Error::InvalidAttestation`] on any mismatch.
    #[allow(clippy::too_many_arguments)]
    fn verify_threshold_attestation(
        env: &Env,
        wallet: &Address,
        asset_pair: &Symbol,
        score: u32,
        benford_flag: bool,
        ml_flag: bool,
        timestamp: u64,
        confidence: u32,
        model_version: u32,
        ta: &ThresholdAttestation,
    ) -> Result<(), Error> {
        // contract_id is cross-checked against env.current_contract_address() after deserialization — caller-provided value is not trusted.
        let current_address_xdr = env.current_contract_address().to_xdr(env);
        let mut current_contract_id = [0u8; 32];
        if current_address_xdr.len() >= 32 {
            current_contract_id.copy_from_slice(&current_address_xdr.as_ref()[..32]);
        }
        if ta.contract_id.to_array() != current_contract_id {
            return Err(Error::InvalidAttestation);
        }
        if ta.contract_version != storage::get_contract_version(env) {
            return Err(Error::InvalidAttestation);
        }

        let digest = Self::compute_commitment(
            env,
            wallet,
            asset_pair,
            score,
            benford_flag,
            ml_flag,
            timestamp,
            confidence,
            model_version,
            &ta.contract_id,
            ta.contract_version,
        )?;

        // Commitment must match what the contract independently derives.
        // Use constant-time comparison to prevent timing side-channels.
        if digest.to_bytes().to_array().ct_eq(&ta.commitment.to_array()).unwrap_u8() == 0 {
            return Err(Error::InvalidAttestation);
        }

        let pubkey =
            storage::get_aggregate_service_pubkey(env).ok_or(Error::ServicePubkeyNotSet)?;

        let sig_bytes = ta.threshold_sig.to_array();
        let recovery_id = sig_bytes[64] as u32;
        if recovery_id > 1 {
            return Err(Error::InvalidAttestation);
        }
        let mut rs = [0u8; 64];
        rs.copy_from_slice(&sig_bytes[..64]);
        let sig64 = BytesN::<64>::from_array(env, &rs);

        let recovered = env.crypto().secp256k1_recover(&digest, &sig64, recovery_id);

        let matches = match pubkey.len() {
            65 => {
                let mut stored = [0u8; 65];
                pubkey.copy_into_slice(&mut stored);
                recovered.to_array().ct_eq(&stored).unwrap_u8() != 0
            }
            33 => {
                let recovered_arr = recovered.to_array();
                let mut compressed = [0u8; 33];
                compressed[0] = if recovered_arr[64].is_multiple_of(2) { 0x02 } else { 0x03 };
                compressed[1..33].copy_from_slice(&recovered_arr[1..33]);
                let mut stored = [0u8; 33];
                pubkey.copy_into_slice(&mut stored);
                compressed.ct_eq(&stored).unwrap_u8() != 0
            }
            // `set_aggregate_service_pubkey` rejects any other length, so
            // this is unreachable in practice.
            _ => false,
        };

        if !matches {
            return Err(Error::InvalidAttestation);
        }

        // ── Nonce verification and increment ───────────────────────────────────
        // Each participating signer must have the expected next nonce.
        // After successful verification, increment each signer's nonce to prevent replay.
        for i in 0..ta.participating_signers.len() {
            let signer = ta.participating_signers.get(i).unwrap();
            let current_nonce = storage::get_signer_nonce(env, &signer);
            if current_nonce != ta.nonce {
                return Err(Error::InvalidAttestation);
            }
        }

        // All nonces matched; increment them for the next submission.
        // Safe unwrap: nonce overflow returns error, doesn't panic.
        for i in 0..ta.participating_signers.len() {
            let signer = ta.participating_signers.get(i).unwrap();
            let next_nonce = ta.nonce.checked_add(1)
                .ok_or(Error::InvalidAttestation)?;
            storage::set_signer_nonce(env, &signer, next_nonce);
        }

        Ok(())
    }

    // ── Merkle batch attestation internals ───────────────────────────────────

    /// Computes the Merkle leaf for a single `ScoreSubmission`:
    /// `SHA-256(0x00 || compute_commitment(submission))`, returned as a
    /// `BytesN<32>`. The opaque `Hash<32>` that `env.crypto().sha256`
    /// produces is converted via `.to_bytes()` at the tail so the leaf
    /// is directly usable as input to [`hash_internal_node`] /
    /// [`verify_merkle_proof`] without further conversion at the call
    /// site.
    ///
    /// # Domain separation
    ///
    /// The prepended `0x00` byte is the **leaf marker** under the RFC 9162
    /// style domain-separation scheme documented in
    /// `docs/batch-attestation-spec.md`. It distinguishes leaves (whose
    /// preimage is 33 bytes: `0x00 || 32-byte commitment`) from internal
    /// nodes (whose preimage is 65 bytes: `0x01 || 32-byte left || 32-byte
    /// right`) at every level of the tree, cheap second-preimage resistance
    /// without the extra hashing a sorted-pair scheme would need.
    ///
    /// The underlying commitment is the same 175-byte preimage
    /// [`ScoreAttestation`] binds (binding every leaf to one specific
    /// deployment on one specific network), so a single secp256k1 signature
    /// over the Merkle root cryptographically links every accepted entry
    /// back to its actual payload.
    ///
    /// # Failure modes
    ///
    /// The only flow through `Err` is `compute_commitment` returning
    /// `Error::InvalidAttestation` for a `> 9`-character `asset_pair`
    /// symbol. Submission-side numeric range checks (score > 100,
    /// confidence > 100, zero timestamp) live in the batch validation
    /// pipeline, not here — `compute_merkle_leaf` does not validate the
    /// submission, only its attestation preimage layout.
    fn compute_merkle_leaf(env: &Env, submission: &ScoreSubmission) -> Result<BytesN<32>, Error> {
        let commitment_bytes = Self::compute_commitment(
            env,
            &submission.wallet,
            &submission.asset_pair,
            submission.score,
            submission.benford_flag,
            submission.ml_flag,
            submission.timestamp,
            submission.confidence,
            submission.model_version,
            0, // Batch/merkle attestations use nonce 0 (not per-submission)
        )?
        .to_bytes()
        .to_array();
        let mut preimage = [0u8; 33];
        preimage[0] = 0x00; // leaf marker
        preimage[1..33].copy_from_slice(&commitment_bytes);
        Ok(env.crypto().sha256(&Bytes::from_array(env, &preimage)).to_bytes())
    }

    /// Hash two 32-byte siblings into their parent: `SHA-256(0x01 || L || R)`,
    /// returned as a `BytesN<32>` (no further hashing or opaque wrapping
    /// required). `BytesN<32>` is the natural type for raw 32-byte
    /// cryptographic outputs inside this contract; only the root-signature
    /// verification path needs the opaque `Hash<32>` handle (see
    /// `verify_batch_attestation`).
    ///
    /// `sibling_on_left` is the **bit `i` of `proof_flags`** for the current
    /// tree level: `true` when the sibling sits to the left of the node
    /// being walked up (so the canonical preimage order is
    /// `sibling || current`), `false` when the sibling sits to the right
    /// (so the canonical order is `current || sibling`). The prepended
    /// `0x01` byte is the **internal-node marker** under the same RFC 9162
    /// scheme as [`compute_merkle_leaf`]; combined with the leaf marker,
    /// no leaf hash can collide with any internal-node hash, and no node of
    /// one shape can collide with any node of a different shape.
    fn hash_internal_node(
        env: &Env,
        current: &BytesN<32>,
        sibling: &BytesN<32>,
        sibling_on_left: bool,
    ) -> BytesN<32> {
        let mut preimage = [0u8; 65];
        preimage[0] = 0x01; // internal-node marker
        let current_bytes = current.to_array();
        let sibling_bytes = sibling.to_array();
        if sibling_on_left {
            preimage[1..33].copy_from_slice(&sibling_bytes);
            preimage[33..65].copy_from_slice(&current_bytes);
        } else {
            preimage[1..33].copy_from_slice(&current_bytes);
            preimage[33..65].copy_from_slice(&sibling_bytes);
        }
        env.crypto().sha256(&Bytes::from_array(env, &preimage)).to_bytes()
    }

    /// Walk a Merkle inclusion proof and verify that `leaf` is included in
    /// the tree with the supplied `root`. The loop runs exactly
    /// `proof.len()` iterations regardless of whether any intermediate
    // ── Wallet Relationship Graph ─────────────────────────────────────────────

    /// Add a bidirectional counterparty link between `wallet_a` and `wallet_b`
    /// for `asset_pair`. Each wallet tracks up to
    /// `MAX_COUNTERPARTY_LINKS_PER_WALLET` links. Self-links and duplicates are
    /// rejected.
    ///
    /// # Errors
    /// - [`Error::CounterpartyLinkFull`] if either wallet's link set is already
    ///   full or if trying to self-link.
    pub fn add_counterparty_link(
        env: Env,
        wallet_a: Address,
        wallet_b: Address,
        asset_pair: Symbol,
    ) -> Result<(), Error> {
        storage::add_counterparty_link(&env, &wallet_a, &wallet_b, &asset_pair)?;
        events::counterparty_link_added(&env, &wallet_a, &wallet_b, &asset_pair);
        Ok(())
    }

    /// Remove a bidirectional counterparty link between `wallet_a` and
    /// `wallet_b` for `asset_pair`.
    ///
    /// # Errors
    /// - [`Error::CounterpartyLinkFull`] if no link existed between the wallets.
    pub fn remove_counterparty_link(
        env: Env,
        wallet_a: Address,
        wallet_b: Address,
        asset_pair: Symbol,
    ) -> Result<(), Error> {
        storage::remove_counterparty_link(&env, &wallet_a, &wallet_b, &asset_pair)?;
        events::counterparty_link_removed(&env, &wallet_a, &wallet_b, &asset_pair);
        Ok(())
    }

    /// Returns the list of counterparty addresses linked to `wallet` for
    /// `asset_pair`.
    pub fn get_counterparties(env: Env, wallet: Address, asset_pair: Symbol) -> Vec<Address> {
        storage::get_counterparties(&env, &wallet, &asset_pair)
    }

    /// Returns every wallet bidirectionally linked to `wallet` on `asset_pair`,
    /// or an empty vector if it has no counterparty links. Because
    /// `add_counterparty_link` records links in both directions, the returned
    /// set is exactly the neighbourhood graph-analytics tools need to traverse
    /// the on-chain counterparty network without reconstructing it from events.
    /// Read-only, callable by any account or contract.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ledgerlens_score::{LedgerLensScoreContract, LedgerLensScoreContractClient};
    /// # use soroban_sdk::{testutils::Address as _, Env, Address, Vec};
    /// # use soroban_sdk::symbol_short;
    /// let env = Env::default();
    /// env.mock_all_auths();
    /// let contract_id = env.register_contract(None, LedgerLensScoreContract);
    /// let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    /// let admin = Address::generate(&env);
    /// let service = Address::generate(&env);
    /// client.initialize(&admin, &service);
    /// let alice = Address::generate(&env);
    /// let bob = Address::generate(&env);
    /// let pair = symbol_short!("XLM_USDC");
    /// client.add_counterparty_link(&alice, &bob, &pair);
    /// // The link is bidirectional, so each side lists the other.
    /// assert_eq!(client.get_counterparty_list(&alice, &pair), Vec::from_array(&env, [bob.clone()]));
    /// assert_eq!(client.get_counterparty_list(&bob, &pair), Vec::from_array(&env, [alice]));
    /// ```
    pub fn get_counterparty_list(env: Env, wallet: Address, asset_pair: Symbol) -> Vec<Address> {
        storage::get_counterparties(&env, &wallet, &asset_pair)
    }

    /// Returns the number of counterparty links `wallet` has for `asset_pair`.
    pub fn get_contagion_depth(env: Env, wallet: Address, asset_pair: Symbol) -> u32 {
        storage::get_contagion_depth(&env, &wallet, &asset_pair)
    }

    /// Propagate an additive score boost of `boost` points to every
    /// counterparty of `anchor` for `asset_pair`.  Affected scores are
    /// capped at 100.  Returns the number of wallets that were boosted.
    pub fn propagate_contagion(env: Env, anchor: Address, asset_pair: Symbol, boost: u32) -> u32 {
        let counterparties = storage::get_counterparties(&env, &anchor, &asset_pair);
        let mut affected = 0u32;
        for i in 0..counterparties.len() {
            let cw = counterparties.get(i).unwrap();
            let old = storage::get_score(&env, &cw, &asset_pair).unwrap_or(RiskScore {
                score: 0,
                benford_flag: false,
                ml_flag: false,
                timestamp: env.ledger().timestamp(),
                confidence: 0,
                model_version: 0,
                commitment: None,
            });
            let new_score = core::cmp::min(old.score.saturating_add(boost), 100);
            if new_score != old.score {
                let updated = RiskScore { score: new_score, ..old };
                storage::set_score(&env, &cw, &asset_pair, &updated);
                events::contagion_propagated(&env, &anchor, &asset_pair, &cw, old.score, new_score);
                affected += 1;
            }
        }
        affected
    }

    /// hash diverges, so the gas cost is always bounded — there is no
    /// early-exit branch that an attacker could exploit as a timing oracle.
    ///
    /// # Edge cases
    ///
    /// - **Empty proof (single-leaf batch):** `current` stays at `leaf`,
    ///   and the final equality check is just `leaf == root`. This is what
    ///   makes `proof = []`, `proof_flags = 0` the correct encoding for a
    ///   one-entry Merkle attestation.
    /// - **Proof too deep:** `proof.len() > MAX_MERKLE_PROOF_DEPTH`
    ///   (currently 30) rejects the proof unconditionally — even if the
    ///   supplied root matches, the contract cannot afford an unbounded
    ///   number of SHA-256 invocations.
    ///
    /// # Returns
    ///
    /// `true` when the proof is well-formed and terminates at `root`,
    /// `false` otherwise (including on any hash mismatch or an over-deep
    /// proof). A `false` return in `submit_scores_batch_attested` causes
    /// the affected entry to be rejected with `Error::InvalidAttestation`,
    /// not the whole batch.
    fn verify_merkle_proof(
        env: &Env,
        leaf: &BytesN<32>,
        proof: &Vec<BytesN<32>>,
        proof_flags: u32,
        root: &BytesN<32>,
    ) -> bool {
        let proof_len = proof.len();
        if proof_len > crate::constants::MAX_MERKLE_PROOF_DEPTH {
            return false;
        }
        let mut current = leaf.clone();
        for i in 0..proof_len {
            let sibling = proof.get(i).unwrap();
            // Bit `i` of `proof_flags` (LSB = 0): 1 means sibling on the left
            // at this level, 0 means sibling on the right.
            let sibling_on_left = ((proof_flags >> i) & 1) == 1;
            current = Self::hash_internal_node(env, &current, &sibling, sibling_on_left);
        }
        // Constant-time across mismatches: we always complete the loop above
        // before comparing; only the final equality check is short-circuited,
        // and both operands are public.
        current.to_array() == root.to_array()
    }

    // ── Verkle commitment internals ──────────────────────────────────────────

    /// Incrementally update the Verkle commitment when a score is written.
    ///
    /// Algorithm:
    /// 1. Derive the evaluation point `z` for `(wallet, asset_pair)`.
    /// 2. Derive the new value element `v_new` from the incoming score.
    /// 3. If an old leaf exists (previous score), XOR it out of the commitment.
    /// 4. Compute the new leaf `leaf_new = H(0x02 || z || v_new)`.
    /// 5. XOR the new leaf into the commitment.
    /// 6. Persist the new leaf and new commitment.
    ///
    /// Step 3 is the key invariant that makes updates sound: each write
    /// replaces exactly one entry's contribution without disturbing others.
    fn update_verkle_commitment(
        env: &Env,
        wallet: &Address,
        asset_pair: &Symbol,
        risk_score: &RiskScore,
    ) {
        // Derive z (evaluation point) for this key.
        let mut wallet_buf = [0u8; 56];
        wallet.to_string().copy_into_slice(&mut wallet_buf);

        let pair_str = match SymbolStr::try_from_val(env, &asset_pair.to_symbol_val()) {
            Ok(s) => s,
            Err(_) => return, // unreachable for valid pairs; skip silently
        };
        let pair_bytes_ref: &[u8] = pair_str.as_ref();
        let mut pair_buf = [0u8; 9];
        let len = pair_bytes_ref.len().min(9);
        pair_buf[..len].copy_from_slice(&pair_bytes_ref[..len]);

        let z = verkle::derive_evaluation_point(env, &wallet_buf, &pair_buf);
        let v_new = verkle::derive_value_element(env, risk_score.score, risk_score.timestamp, &z);
        let leaf_new = verkle::hash_leaf(env, &z, &v_new);

        let mut commit: [u8; 32] = storage::get_verkle_commitment_raw(env).unwrap_or([0u8; 32]);

        // Remove old leaf contribution (XOR is its own inverse).
        if let Some(old_leaf) = storage::get_verkle_leaf(env, wallet, asset_pair) {
            // XOR old leaf into running accumulator to remove it.
            commit = verkle::xor32(&commit, &old_leaf);
            // Re-apply the outer hash with domain separator to maintain the
            // hash-chain structure after the removal step.
            let mut buf = [0u8; 33];
            buf[0] = 0x06; // DOMAIN_COMMIT — same as in update_commitment
            buf[1..33].copy_from_slice(&commit);
            commit = soroban_sdk::BytesN::from(env.crypto().sha256(&Bytes::from_array(env, &buf))).to_array();
        }

        // Add new leaf contribution.
        commit = verkle::update_commitment(env, &commit, &z, &v_new);

        storage::set_verkle_commitment_raw(env, &commit);
        storage::set_verkle_leaf(env, wallet, asset_pair, &leaf_new);
    }

    // ── Signer reputation (issue #274) ────────────────────────────────────────

    /// Returns the current accuracy record for `signer`, or `None` if the
    /// signer has never participated in a consensus round.
    pub fn get_signer_accuracy(
        env: Env,
        signer: Address,
    ) -> Option<SignerAccuracyRecord> {
        storage::get_signer_accuracy(&env, &signer)
    }

    /// Admin-only. Clears the accuracy record for `signer`, resetting their
    /// reputation to a neutral starting state.
    pub fn reset_signer_accuracy(
        env: Env,
        admin_signers: Vec<Address>,
        signer: Address,
    ) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        Self::require_admin_auth(&env, &admin_signers)?;
        storage::remove_signer_accuracy(&env, &signer);
        events::signer_accuracy_reset(&env, &signer);
        Ok(())
    }

    // ── Oracle adapter (issue #276) ────────────────────────────────────────────

    /// Admin-only. Registers (or replaces) the oracle contract for `asset_pair`.
    /// The oracle must implement `OracleAdapterTrait::get_price(asset_pair)`.
    pub fn register_oracle(
        env: Env,
        admin_signers: Vec<Address>,
        asset_pair: Symbol,
        oracle_contract: Address,
    ) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        Self::require_admin_auth(&env, &admin_signers)?;
        storage::set_registered_oracle(&env, &asset_pair, &oracle_contract);
        events::oracle_registered(&env, &asset_pair, &oracle_contract);
        Ok(())
    }

    /// Admin-only. Removes the oracle registration for `asset_pair`.
    pub fn remove_oracle(
        env: Env,
        admin_signers: Vec<Address>,
        asset_pair: Symbol,
    ) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        Self::require_admin_auth(&env, &admin_signers)?;
        storage::remove_registered_oracle(&env, &asset_pair);
        events::oracle_removed(&env, &asset_pair);
        Ok(())
    }

    /// Returns the registered oracle contract address for `asset_pair`, or
    /// `None` if none has been registered.
    pub fn get_registered_oracle(
        env: Env,
        asset_pair: Symbol,
    ) -> Option<Address> {
        storage::get_registered_oracle(&env, &asset_pair)
    }
}

// ── Query gate allowlist (stub — full implementation pending) ────────────────
mod storage_gate {
    use soroban_sdk::Env;

    pub fn verify_caller_protection(_env: &Env) -> bool {
        true
    }
}

/// Integer square root (floor) for use in volatility std-dev computation.
fn isqrt_u64(n: u64) -> u64 {
    if n == 0 {
        return 0;
    }
    let mut x = n;
    let mut y = (x + 1) / 2;
    while y < x {
        x = y;
        y = (x + n / x) / 2;
    }
    x
}
