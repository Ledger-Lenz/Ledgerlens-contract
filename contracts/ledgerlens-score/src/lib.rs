#![no_std]
#![allow(deprecated)] // Required: contractimpl macro calls spec_xdr_* for all fns including deprecated ones

mod constants;
mod errors;
mod events;
mod storage;
mod types;

#[cfg(test)]
mod test;

#[cfg(test)]
mod test_upgrade;

#[cfg(test)]
mod test_interface;

#[cfg(test)]
mod test_rate_limit;

#[cfg(test)]
mod test_attestation;

#[cfg(test)]
mod test_decay;

#[cfg(test)]
mod test_embargo;

#[cfg(test)]
mod test_score_delta;

#[cfg(test)]
mod test_finality_buffer;

#[cfg(test)]
mod test_heartbeat;

use soroban_sdk::{
    contract, contractimpl, crypto::Hash, symbol_short, token, xdr::ToXdr, Address, Bytes, BytesN,
    Env, Symbol, SymbolStr, TryFromVal, Vec,
};

pub use errors::Error;
pub use events::{ServiceResumedEvent, ServiceSilenceAlertEvent};
pub use types::{
    AggregateRiskScore, BatchEntryResult, BatchResult, PendingScoreEntry, RiskScore,
    ScoreAttestation, ScoreSubmission, ScoreTrend, SnapshotRecord, UpgradeProposal,
};

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
        attestation: Option<ScoreAttestation>,
    ) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        if storage::is_paused(&env) {
            return Err(Error::ContractPaused);
        }
        if storage::is_pair_paused(&env, &asset_pair) {
            return Err(Error::PairPaused);
        }

        let service_set = storage::get_service_set(&env);
        let threshold = storage::get_service_threshold(&env);

        // Tracked for `PendingScoreEntry::submitted_by` when the finality
        // buffer is active; in the multi-sig path this is the first
        // verified co-signer.
        let submitted_by: Address;
        if !service_set.is_empty() && threshold > 0 {
            // Multi-sig path: verify count, membership, then require_auth each.
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
            submitted_by = signers.get(0).unwrap();
        } else {
            // Legacy single-service path.
            let service = storage::get_service(&env);
            service.require_auth();
            submitted_by = service;
        }

        // Cryptographic payload attestation — opt-in. Once the admin has
        // configured a service pubkey, every submission must carry a valid
        // attestation; until then, `attestation` is ignored entirely so
        // existing integrations are unaffected. See `set_service_pubkey`.
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
                attestation,
            )?;
        }

        if score > 100 {
            return Err(Error::InvalidScore);
        }
        if confidence > 100 {
            return Err(Error::InvalidConfidence);
        }
        if timestamp == 0 {
            return Err(Error::InvalidTimestamp);
        }

        let last_submit = storage::get_last_submit_time(&env, &wallet, &asset_pair);
        let cooldown = storage::get_cooldown_secs(&env);
        let now = env.ledger().timestamp();
        // `last_submit == 0` means "never accepted" (see get_last_submit_time) —
        // not a real submission at the epoch — so the cooldown doesn't apply yet.
        if last_submit != 0 && now < last_submit.saturating_add(cooldown) {
            return Err(Error::RateLimitExceeded);
        }
        storage::set_last_submit_time(&env, &wallet, &asset_pair, now);
        Self::record_service_activity(&env);

        let buffer = storage::get_finality_buffer_secs(&env);
        if buffer == 0 {
            // Disabled — commit straight to live storage, exactly as before
            // the finality buffer existed.
            let previous_score = storage::peek_score(&env, &wallet, &asset_pair).map(|s| s.score);

            let risk_score =
                RiskScore { score, benford_flag, ml_flag, timestamp, confidence, model_version };

            storage::set_score(&env, &wallet, &asset_pair, &risk_score);
            storage::push_score_history(&env, &wallet, &asset_pair, &risk_score);
            storage::register_pair_for_wallet(&env, &wallet, &asset_pair);
            storage::increment_score_count(&env, &wallet, &asset_pair);
            Self::refresh_aggregate_cache(&env, &wallet);
            Self::update_merkle_accumulator(
                &env,
                &wallet,
                &asset_pair,
                score,
                timestamp,
                confidence,
                model_version,
            );

            let score_threshold = storage::get_risk_threshold(&env);
            if score >= score_threshold {
                events::threshold_breached(&env, &wallet, &asset_pair, score, score_threshold);
            }

            Self::emit_score_delta(&env, &wallet, &asset_pair, previous_score, score);
            events::score_submitted(&env, &wallet, &asset_pair, &risk_score);
        } else {
            // Buffer active — hold the score in `PendingScore` instead of
            // writing to live storage. `get_score` / `query_risk_gate` never
            // read this key, so the submission has no observable effect on
            // any consumer until `commit_pending_score` runs. A second
            // submission while one is pending overwrites it rather than
            // queuing alongside it.
            let commit_after = now.saturating_add(buffer);
            let pending = PendingScoreEntry {
                score,
                benford_flag,
                ml_flag,
                timestamp,
                confidence,
                model_version,
                submitted_at: now,
                commit_after,
                submitted_by,
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
    /// client.set_finality_buffer(&Vec::new(&env), &300);
    /// assert_eq!(client.get_finality_buffer(), 300);
    /// ```
    ///
    /// # Errors
    /// - [`Error::NotInitialized`] if the contract has no admin yet.
    /// - [`Error::InvalidFinalityBuffer`] if `secs` exceeds
    ///   `MAX_FINALITY_BUFFER_SECS`.
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
    /// assert_eq!(client.get_finality_buffer(), 0);
    /// ```
    pub fn get_finality_buffer(env: Env) -> u64 {
        storage::get_finality_buffer_secs(&env)
    }

    /// Commits a pending score to live storage once its hold window has
    /// elapsed. Callable by **anyone** — there is nothing secret about a
    /// pending score, so no authorization is required; the only gate is
    /// `commit_after <= now`.
    ///
    /// On success this performs exactly the side effects `submit_score`
    /// would have performed immediately had the buffer been disabled:
    /// writing `Score`, appending to `ScoreHistory`, registering the pair,
    /// incrementing the submission counter, refreshing the aggregate cache,
    /// updating the Merkle accumulator, and emitting `threshold_breached` /
    /// the score-delta event — then clears the pending entry and emits
    /// `ScoreCommittedEvent`.
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
    /// client.set_finality_buffer(&Vec::new(&env), &300);
    /// let wallet = Address::generate(&env);
    /// let asset_pair = symbol_short!("XLM_USDC");
    /// client.submit_score(&Vec::new(&env), &wallet, &asset_pair, &90, &true, &true, &1, &95, &1, &None);
    /// assert!(client.try_get_score(&wallet, &asset_pair).is_err()); // still pending, invisible
    /// env.ledger().with_mut(|l| l.timestamp += 300);
    /// client.commit_pending_score(&wallet, &asset_pair);
    /// assert_eq!(client.get_score(&wallet, &asset_pair).score, 90);
    /// ```
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
        };

        storage::set_score(&env, &wallet, &asset_pair, &risk_score);
        storage::push_score_history(&env, &wallet, &asset_pair, &risk_score);
        storage::register_pair_for_wallet(&env, &wallet, &asset_pair);
        storage::increment_score_count(&env, &wallet, &asset_pair);
        Self::refresh_aggregate_cache(&env, &wallet);
        Self::update_merkle_accumulator(
            &env,
            &wallet,
            &asset_pair,
            pending.score,
            pending.timestamp,
            pending.confidence,
            pending.model_version,
        );

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
    /// to provide: a compromised service key may still pass attestation,
    /// multisig, and the velocity cap, but an adversarial score can be
    /// vetoed here during the hold window.
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
    /// client.set_finality_buffer(&Vec::new(&env), &300);
    /// let wallet = Address::generate(&env);
    /// let asset_pair = symbol_short!("XLM_USDC");
    /// client.submit_score(&Vec::new(&env), &wallet, &asset_pair, &90, &true, &true, &1, &95, &1, &None);
    /// client.cancel_pending_score(&Vec::new(&env), &wallet, &asset_pair);
    /// assert!(client.get_pending_score(&wallet, &asset_pair).is_none());
    /// ```
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

    /// Read-only lookup of the pending score held for `(wallet, asset_pair)`,
    /// if any. Lets the admin (or any observer) inspect a submission during
    /// its hold window before deciding whether to let it commit or to
    /// `cancel_pending_score` it. Returns `None` once the buffer is disabled,
    /// once it has been committed, or once it has been cancelled.
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
    /// client.set_finality_buffer(&Vec::new(&env), &300);
    /// let wallet = Address::generate(&env);
    /// let asset_pair = symbol_short!("XLM_USDC");
    /// client.submit_score(&Vec::new(&env), &wallet, &asset_pair, &90, &true, &true, &1, &95, &1, &None);
    /// let pending = client.get_pending_score(&wallet, &asset_pair).unwrap();
    /// assert_eq!(pending.score, 90);
    /// ```
    pub fn get_pending_score(
        env: Env,
        wallet: Address,
        asset_pair: Symbol,
    ) -> Option<PendingScoreEntry> {
        storage::get_pending_score(&env, &wallet, &asset_pair)
    }

    /// Submit multiple risk scores in a single invocation.  The service
    /// account authorises once for the whole batch.  Returns a `BatchResult`
    /// that lists every entry's outcome so the caller knows exactly which
    /// entries succeeded and why any failed, without needing to re-query
    /// each (wallet, pair) individually.
    ///
    /// Entries targeting a paused pair (`PairPaused`), with out-of-range
    /// `score` or `confidence`, a zero `timestamp`, or that arrive before
    /// their `(wallet, asset_pair)`'s submission cooldown has elapsed, are
    /// recorded as rejected in the result with an appropriate
    /// `rejection_code` — the rest of the batch is still processed. The
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
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        if storage::is_paused(&env) {
            return Err(Error::ContractPaused);
        }

        let service = storage::get_service(&env);
        service.require_auth();

        if submissions.is_empty() {
            return Err(Error::EmptyBatch);
        }
        if submissions.len() > constants::MAX_BATCH_SIZE {
            return Err(Error::BatchTooLarge);
        }

        let threshold = storage::get_risk_threshold(&env);
        let cooldown = storage::get_cooldown_secs(&env);
        let now = env.ledger().timestamp();
        let mut accepted_count: u32 = 0;
        let mut results: Vec<BatchEntryResult> = Vec::new(&env);

        for i in 0..submissions.len() {
            let sub = submissions.get(i).unwrap();
            let mut accepted = false;
            let mut rejection_code: u32 = 0;

            if storage::is_pair_paused(&env, &sub.asset_pair) {
                rejection_code = Error::PairPaused as u32;
            } else if sub.score > 100 {
                rejection_code = Error::InvalidScore as u32;
            } else if sub.confidence > 100 {
                rejection_code = Error::InvalidConfidence as u32;
            } else if sub.timestamp == 0 {
                rejection_code = Error::InvalidTimestamp as u32;
            } else {
                let last_submit = storage::get_last_submit_time(&env, &sub.wallet, &sub.asset_pair);
                if last_submit != 0 && now < last_submit.saturating_add(cooldown) {
                    rejection_code = Error::RateLimitExceeded as u32;
                } else {
                    let previous_score =
                        storage::peek_score(&env, &sub.wallet, &sub.asset_pair).map(|s| s.score);

                    storage::set_last_submit_time(&env, &sub.wallet, &sub.asset_pair, now);

                    let risk_score = RiskScore {
                        score: sub.score,
                        benford_flag: sub.benford_flag,
                        ml_flag: sub.ml_flag,
                        timestamp: sub.timestamp,
                        confidence: sub.confidence,
                        model_version: sub.model_version,
                    };

                    storage::set_score(&env, &sub.wallet, &sub.asset_pair, &risk_score);
                    storage::push_score_history(&env, &sub.wallet, &sub.asset_pair, &risk_score);
                    storage::register_pair_for_wallet(&env, &sub.wallet, &sub.asset_pair);
                    storage::increment_score_count(&env, &sub.wallet, &sub.asset_pair);
                    Self::refresh_aggregate_cache(&env, &sub.wallet);
                    Self::update_merkle_accumulator(
                        &env,
                        &sub.wallet,
                        &sub.asset_pair,
                        sub.score,
                        sub.timestamp,
                        sub.confidence,
                        sub.model_version,
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

                    Self::emit_score_delta(
                        &env,
                        &sub.wallet,
                        &sub.asset_pair,
                        previous_score,
                        sub.score,
                    );
                    events::score_submitted(&env, &sub.wallet, &sub.asset_pair, &risk_score);
                    accepted = true;
                    accepted_count += 1;
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
        if storage::is_embargoed(&env, &wallet) {
            return Err(Error::ScoreEmbargoed);
        }
        match storage::get_score(&env, &wallet, &asset_pair) {
            Some(score) => Ok(score),
            None => {
                if let Some(custodian) = storage::get_score_delegate(&env, &wallet) {
                    storage::get_score(&env, &custodian, &asset_pair).ok_or(Error::ScoreNotFound)
                } else {
                    Err(Error::ScoreNotFound)
                }
            }
        }
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

        if sub_wallet == custodian {
            return Err(Error::CyclicDelegation);
        }
        if let Some(custodian_delegate) = storage::get_score_delegate(&env, &custodian) {
            if custodian_delegate == sub_wallet {
                return Err(Error::CyclicDelegation);
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
            return Err(Error::DelegateNotFound);
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
    /// # Example (caller side)
    ///
    /// ```ignore
    /// let client = LedgerLensScoreContractClient::new(&env, &llens_id);
    /// if !client.query_risk_gate(&user, &symbol_short!("XLM_USDC"), &75) {
    ///     return Err(MyError::HighRiskWallet);
    /// }
    /// ```
    pub fn query_risk_gate(
        env: Env,
        wallet: Address,
        asset_pair: Symbol,
        gate_threshold: u32,
    ) -> bool {
        if storage::is_embargoed(&env, &wallet) {
            return false;
        }
        match storage::peek_score(&env, &wallet, &asset_pair) {
            Some(risk) => risk.score < gate_threshold,
            None => {
                if let Some(custodian) = storage::peek_score_delegate(&env, &wallet) {
                    if let Some(risk) = storage::peek_score(&env, &custodian, &asset_pair) {
                        return risk.score < gate_threshold;
                    }
                }
                false
            }
        }
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
    /// | Symbol      | Backing functionality                              |
    /// |-------------|----------------------------------------------------|
    /// | `score`     | `get_score` / `submit_score`                       |
    /// | `history`   | `get_score_history`                                |
    /// | `batch`     | `submit_scores_batch`                              |
    /// | `gate`      | `query_risk_gate`                                  |
    /// | `aggr`      | `get_aggregate_score` (cross-asset aggregate risk) |
    /// | `count`     | `get_score_count`                                  |
    ///
    /// Any unrecognised `capability` returns `false`.
    pub fn supports_interface(_env: Env, capability: Symbol) -> bool {
        capability == symbol_short!("score")
            || capability == symbol_short!("history")
            || capability == symbol_short!("batch")
            || capability == symbol_short!("gate")
            || capability == symbol_short!("aggr")
            || capability == symbol_short!("count")
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
        events::signer_added(&env, &signer);
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

    /// Returns the current signing threshold.
    pub fn get_service_threshold(env: Env) -> u32 {
        storage::get_service_threshold(&env)
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
    /// assert_eq!(client.get_last_service_activity(), 0);
    /// let wallet = Address::generate(&env);
    /// let asset_pair = symbol_short!("XLM_USDC");
    /// client.submit_score(&Vec::new(&env), &wallet, &asset_pair, &10, &false, &false, &1, &50, &1, &None).unwrap();
    /// assert!(client.get_last_service_activity() > 0);
    /// ```
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
    ///
    /// # Examples
    ///
    /// ```
    /// # use ledgerlens_score::LedgerLensScoreContractClient;
    /// # use soroban_sdk::{testutils::{Address as _, Ledger as _}, Env, Address};
    /// # use ledgerlens_score::LedgerLensScoreContract;
    /// let env = Env::default();
    /// env.mock_all_auths();
    /// let contract_id = env.register_contract(None, LedgerLensScoreContract);
    /// let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    /// let admin = Address::generate(&env);
    /// let service = Address::generate(&env);
    /// client.initialize(&admin, &service);
    /// assert!(client.is_service_alive()); // never active yet — alive by default
    /// client.ping_heartbeat().unwrap();
    /// assert!(client.is_service_alive());
    /// env.ledger().with_mut(|l| l.timestamp += 3_601); // past the 1-hour default threshold
    /// assert!(!client.is_service_alive());
    /// ```
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
    /// `is_service_alive`, and before the next `get_score` call emits a
    /// `ServiceSilenceAlertEvent`. Admin only.
    ///
    /// Defaults to `DEFAULT_HEARTBEAT_ALERT_THRESHOLD_SECS` (1 hour) until
    /// this is called.
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
    /// client.set_heartbeat_alert_threshold(&Vec::new(&env), &7_200).unwrap();
    /// assert_eq!(client.get_heartbeat_alert_threshold(), 7_200);
    /// ```
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
    /// `DEFAULT_HEARTBEAT_ALERT_THRESHOLD_SECS` (1 hour) until
    /// `set_heartbeat_alert_threshold` is called.
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
    /// assert_eq!(client.get_heartbeat_alert_threshold(), 3_600);
    /// ```
    pub fn get_heartbeat_alert_threshold(env: Env) -> u64 {
        storage::get_heartbeat_alert_threshold(&env)
    }

    /// Proves off-chain service liveness without submitting a score.
    /// Callable only by the configured service account. Updates
    /// `LastServiceActivityAt` and, if a silence alert was previously
    /// emitted, clears it and emits `ServiceResumedEvent` — treated
    /// equivalently to a successful submission for heartbeat purposes.
    /// Useful when no new scores need to be submitted but the service wants
    /// to prove it is still up.
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
    /// client.ping_heartbeat().unwrap();
    /// assert!(client.get_last_service_activity() > 0);
    /// ```
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
    /// // submit_score for this pair now returns Error::PairPaused, while
    /// // every other pair is unaffected.
    /// client.set_pair_paused(&pair, &false);
    /// assert!(!client.is_pair_paused(&pair));
    /// ```
    ///
    /// # Errors
    /// - [`Error::NotInitialized`] if the contract has no admin yet.
    /// - [`Error::PausedPairIndexFull`] if `asset_pair` is not already paused
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
                return Err(Error::PausedPairIndexFull);
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

        events::upgrade_proposed(&env, &new_wasm_hash, executable_after);
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

    // ── Score embargo (regulatory hold) ─────────────────────────────────────

    /// Place a per-wallet score embargo, blocking all read-path access to the
    /// wallet's scores without erasing any data.  Admin only.
    ///
    /// `expiry` sets the automatic lift time as a ledger timestamp:
    /// - `None` — indefinite; only `lift_score_embargo` can remove it.
    /// - `Some(ts)` — auto-lifted once `env.ledger().timestamp() >= ts`.
    ///
    /// While active:
    /// - `get_score` → `Err(ScoreEmbargoed)`
    /// - `query_risk_gate` → `false` (conservative deny, no error)
    /// - `get_score_history` → empty `Vec`
    /// - `get_aggregate_score` → `Err(ScoreEmbargoed)`
    /// - `submit_score` / `submit_scores_batch` → unaffected (writes succeed)
    ///
    /// # Examples
    ///
    /// ```
    /// # use ledgerlens_score::{LedgerLensScoreContract, LedgerLensScoreContractClient, Error};
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
    /// assert!(!client.is_embargoed(&wallet));
    /// client.set_score_embargo(&wallet, &None).unwrap();
    /// assert!(client.is_embargoed(&wallet));
    /// ```
    ///
    /// # Errors
    /// - [`Error::NotInitialized`] if the contract has no admin yet.
    pub fn set_score_embargo(env: Env, wallet: Address, expiry: Option<u64>) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        storage::get_admin(&env).require_auth();
        storage::set_score_embargo(&env, &wallet, &expiry);
        events::embargo_set(&env, &wallet, &expiry);
        Ok(())
    }

    /// Explicitly lift a per-wallet score embargo.  Admin only.
    ///
    /// This is required for indefinite embargoes (`expiry = None`). For
    /// time-limited embargoes it is also available as an early-lift mechanism.
    /// After this call `is_embargoed` returns `false` and all read-path
    /// functions resume their normal behaviour.
    ///
    /// # Errors
    /// - [`Error::NotInitialized`] if the contract has no admin yet.
    pub fn lift_score_embargo(env: Env, wallet: Address) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        let admin = storage::get_admin(&env);
        admin.require_auth();
        storage::remove_score_embargo(&env, &wallet);
        events::embargo_lifted(&env, &wallet, &admin);
        Ok(())
    }

    /// Returns `true` when `wallet` is under an active, non-expired embargo.
    /// Returns `false` when no embargo exists or when a time-limited embargo
    /// has passed its expiry timestamp.  Read-only, callable by any account.
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
    /// let wallet = Address::generate(&env);
    /// assert!(!client.is_embargoed(&wallet));
    /// ```
    pub fn is_embargoed(env: Env, wallet: Address) -> bool {
        storage::is_embargoed(&env, &wallet)
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
    /// - [`Error::InvalidDecayRate`] if the ratio exceeds MAX_DECAY_LAMBDA.
    ///
    /// # Examples
    ///
    /// Set λ to 0.001 per second (half-life ~693 seconds):
    /// ```text
    /// client.set_decay_rate(&1, &1000);
    /// ```
    ///
    /// With a 7-day staleness window, a score from 7 days ago decays by factor:
    /// ```text
    /// decay = e^(-0.001 * 604800) ≈ e^(-604.8) ≈ 0 (fully decayed)
    /// ```
    ///
    /// A score from 1 day ago decays by:
    /// ```text
    /// decay = e^(-0.001 * 86400) ≈ e^(-86.4) ≈ 0 (nearly fully decayed)
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
            return Err(Error::InvalidDecayRate);
        }

        // Validate the ratio is within bounds
        // Check: numerator / denominator <= MAX_DECAY_LAMBDA
        // Equivalently: numerator * MAX_DEN <= MAX_NUM * denominator
        let max_num = constants::MAX_DECAY_LAMBDA_NUM as u64;
        let max_den = constants::MAX_DECAY_LAMBDA_DEN as u64;
        let num = numerator as u64;
        let den = denominator as u64;

        if num.checked_mul(max_den).map(|v| v > max_num.saturating_mul(den)).unwrap_or(true) {
            return Err(Error::InvalidDecayRate);
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

    /// Emergency re-score path: immediately clears the submission cooldown
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
    ) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        Self::require_admin_auth(&env, &admin_signers)?;
        let admin = storage::get_admin(&env);
        storage::clear_last_submit_time(&env, &wallet, &asset_pair);
        events::rate_limit_overridden(&env, &admin, &wallet, &asset_pair);
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
        storage::clear_score(&env, &wallet, &asset_pair);
        events::score_cleared(&env, &wallet, &asset_pair);
        Ok(())
    }

    /// Returns the ledger timestamp of the last accepted submission for
    /// `(wallet, asset_pair)`, or `0` if none has ever been accepted (or it
    /// was cleared by `override_rate_limit`).
    pub fn get_last_submit_time(env: Env, wallet: Address, asset_pair: Symbol) -> u64 {
        storage::get_last_submit_time(&env, &wallet, &asset_pair)
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

    /// Returns the configured fee token address, or `FeeTokenNotSet` if none.
    pub fn get_fee_token(env: Env) -> Result<Address, Error> {
        storage::get_fee_token(&env).ok_or(Error::FeeTokenNotSet)
    }

    /// Withdraw accumulated fees from the contract to `recipient`.
    ///
    /// Guards:
    /// - Admin-only: `admin.require_auth()` must be satisfied.
    /// - Early validation: `amount` must be > 0 and `recipient` must not be
    ///   the zero address (enforced by Soroban's `Address` type — any invalid
    ///   address will fail deserialization before reaching this function).
    /// - Concurrency lock: rejects with [`Error::WithdrawalInProgress`] if
    ///   another withdrawal is already in-flight for this contract.
    /// - Fee token must be configured via `set_fee_token`.
    /// - Emits [`fee_withdrawn`] on success; [`withdrawal_locked`] if the
    ///   lock is already held.
    ///
    /// # Errors
    /// - [`Error::NotInitialized`] — contract has no admin.
    /// - [`Error::ContractPaused`] — admin has activated the circuit breaker.
    /// - [`Error::InvalidWithdrawalAmount`] — `amount` is zero.
    /// - [`Error::FeeTokenNotSet`] — `set_fee_token` has not been called.
    /// - [`Error::WithdrawalInProgress`] — a concurrent withdrawal is running.
    pub fn withdraw_fees(env: Env, recipient: Address, amount: i128) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        if storage::is_paused(&env) {
            return Err(Error::ContractPaused);
        }

        let admin = storage::get_admin(&env);
        admin.require_auth();

        // Reject zero-amount withdrawals early.
        if amount == 0 {
            return Err(Error::InvalidWithdrawalAmount);
        }

        // Fee token must be configured.
        let fee_token = storage::get_fee_token(&env).ok_or(Error::FeeTokenNotSet)?;

        // Acquire the concurrency lock — prevents duplicate in-flight calls.
        if storage::is_withdrawal_locked(&env) {
            events::withdrawal_locked(&env, &admin);
            return Err(Error::WithdrawalInProgress);
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
        Ok(())
    }

    /// Returns the current M-of-N admin signer set. Empty until
    /// `add_admin_signer` is called (legacy mode).
    pub fn get_admin_signers(env: Env) -> Vec<Address> {
        storage::get_admin_set(&env)
    }

    /// Returns the current admin signing threshold. Zero until
    /// `set_admin_threshold` is called (legacy mode).
    pub fn get_admin_threshold(env: Env) -> u32 {
        storage::get_admin_threshold(&env)
    }

    // ── Internal helpers ──────────────────────────────────────────────────────

    /// Records that the off-chain service is active right now. Called by
    /// `submit_score`, `submit_scores_batch` (once per call, after at least
    /// one entry is accepted), and `ping_heartbeat`.
    ///
    /// If a silence alert was previously emitted (`ServiceSilentAlertEmitted
    /// == true`), this is the first activity since that alert fired: it
    /// emits `ServiceResumedEvent` with the gap since the last recorded
    /// activity and clears the flag, before stamping `LastServiceActivityAt`
    /// with the current ledger time.
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
    /// `ServiceSilenceAlertEvent` (without otherwise affecting the read) the
    /// first time the service has been silent for longer than
    /// `ServiceHeartbeatAlertThreshold`, then sets `ServiceSilentAlertEmitted`
    /// so the alert fires only once per silence window. A `LastServiceActivityAt`
    /// of `0` (the service has never been active) is never treated as
    /// silence — see `is_service_alive`.
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

    /// Best-effort refresh of the `AggregateScore(wallet)` cache after a
    /// score write. Failures are swallowed (e.g. a wallet whose only pair
    /// currently has weight 0) — the cache is informational only and must
    /// never cause `submit_score` / `submit_scores_batch` to fail.
    fn refresh_aggregate_cache(env: &Env, wallet: &Address) {
        if let Ok(aggregate) = Self::compute_aggregate_score(env, wallet) {
            storage::set_aggregate_score(env, wallet, &aggregate);
        }
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
        preimage.extend_from_array(&contract_buf);
        preimage.extend_from_array(&env.ledger().network_id().to_array());

        Ok(env.crypto().sha256(&preimage))
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

    /// Verifies `attestation` (recomputing the commitment independently
    /// rather than trusting its `commitment` field — see
    /// [`ScoreAttestation`]) against the registered service pubkey, then
    /// recovers the secp256k1 signer and compares it. Supports both
    /// compressed (33-byte) and uncompressed (65-byte) registered pubkeys:
    /// `secp256k1_recover` always yields the uncompressed SEC-1 form, so a
    /// compressed registered key is compared against the recovered key's
    /// compressed form instead (parity byte + x-coordinate — no elliptic-
    /// curve math needed since the full point is already known).
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
        let pubkey = storage::get_service_pubkey(env).ok_or(Error::ServicePubkeyNotSet)?;
        let attestation = attestation.ok_or(Error::InvalidAttestation)?;

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
        )?;

        if digest.to_bytes().to_array() != attestation.commitment.to_array() {
            return Err(Error::InvalidAttestation);
        }

        let sig_bytes = attestation.signature.to_array();
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
                recovered.to_array() == stored
            }
            33 => {
                let recovered_arr = recovered.to_array();
                let mut compressed = [0u8; 33];
                compressed[0] = if recovered_arr[64].is_multiple_of(2) { 0x02 } else { 0x03 };
                compressed[1..33].copy_from_slice(&recovered_arr[1..33]);
                let mut stored = [0u8; 33];
                pubkey.copy_into_slice(&mut stored);
                compressed == stored
            }
            // `set_service_pubkey` rejects any other length, so this is
            // unreachable in practice; treat defensively as a mismatch.
            _ => false,
        };

        if !matches {
            return Err(Error::InvalidAttestation);
        }
        Ok(())
    }

    /// Enable the Merkle accumulator. Admin-only.
    pub fn enable_merkle_accumulator(
        env: Env,
        admin_signers: Vec<Address>,
    ) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        Self::require_admin_auth(&env, &admin_signers)?;
        storage::set_merkle_enabled(&env, true);
        Ok(())
    }

    /// Disable the Merkle accumulator. Admin-only.
    pub fn disable_merkle_accumulator(
        env: Env,
        admin_signers: Vec<Address>,
    ) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        Self::require_admin_auth(&env, &admin_signers)?;
        storage::set_merkle_enabled(&env, false);
        Ok(())
    }

    /// Commit the current Merkle root to the SnapshotHistory ring buffer.
    /// Public — anyone may call.
    pub fn commit_snapshot(env: Env) -> Result<BytesN<32>, Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        if !storage::is_merkle_enabled(&env) {
            return Err(Error::MerkleAccumulatorDisabled);
        }
        let root = storage::get_merkle_root(&env)
            .unwrap_or_else(|| BytesN::from_array(&env, &[0u8; 32]));
        let leaf_count = storage::get_merkle_leaf_count(&env);
        let now = env.ledger().timestamp();
        let committed_by = env.current_contract_address();

        let mut history = storage::get_snapshot_history(&env);
        let record = SnapshotRecord {
            root: root.clone(),
            leaf_count,
            committed_at: now,
            committed_by: committed_by.clone(),
        };


        history.push_back(record);

        // history ring buffer depth:
        let depth = storage::get_snapshot_history_depth(&env);
        while history.len() > depth {
            history.remove(0);
        }

        storage::set_snapshot_history(&env, &history);
        events::snapshot_committed(&env, &root, leaf_count, now, &committed_by);

        Ok(root)
    }

    /// Get the current Merkle root of the score accumulator.
    pub fn get_current_merkle_root(env: Env) -> Result<BytesN<32>, Error> {
        if !storage::is_merkle_enabled(&env) {
            return Err(Error::MerkleAccumulatorDisabled);
        }
        Ok(storage::get_merkle_root(&env)
            .unwrap_or_else(|| BytesN::from_array(&env, &[0u8; 32])))
    }

    /// Get the history of committed score Merkle root snapshots.
    pub fn get_snapshot_history(env: Env) -> Vec<SnapshotRecord> {
        storage::get_snapshot_history(&env)
    }

    /// Set the depth (max capacity) of the snapshot history ring buffer. Admin-only.
    pub fn set_snapshot_history_depth(
        env: Env,
        admin_signers: Vec<Address>,
        depth: u32,
    ) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        Self::require_admin_auth(&env, &admin_signers)?;
        if depth == 0 || depth > 100 {
            return Err(Error::InvalidHistoryDepth);
        }
        storage::set_snapshot_history_depth(&env, depth);
        Ok(())
    }

    fn update_merkle_accumulator(
        env: &Env,
        wallet: &Address,
        asset_pair: &Symbol,
        score: u32,
        timestamp: u64,
        confidence: u32,
        model_version: u32,
    ) {
        if !storage::is_merkle_enabled(env) {
            return;
        }

        // Compute leaf = sha256(wallet_bytes || asset_pair_bytes || score || timestamp || confidence || model_version)
        let mut preimage = Bytes::new(env);
        preimage.append(&wallet.to_xdr(env));
        preimage.append(&asset_pair.to_xdr(env));
        preimage.extend_from_array(&score.to_be_bytes());
        preimage.extend_from_array(&timestamp.to_be_bytes());
        preimage.extend_from_array(&confidence.to_be_bytes());
        preimage.extend_from_array(&model_version.to_be_bytes());

        let leaf = env.crypto().sha256(&preimage);
        let leaf_bytes = leaf.to_bytes();


        // new_root = sha256(old_root || leaf)
        let old_root = storage::get_merkle_root(env)
            .unwrap_or_else(|| BytesN::from_array(env, &[0u8; 32]));
        let mut root_preimage = Bytes::new(env);
        root_preimage.extend_from_array(&old_root.to_array());
        root_preimage.extend_from_array(&leaf_bytes.to_array());

        let new_root = env.crypto().sha256(&root_preimage);

        let new_root_bytes = new_root.to_bytes();
        storage::set_merkle_root(env, &new_root_bytes);

        storage::set_merkle_leaf_count(env, storage::get_merkle_leaf_count(env) + 1);
    }
}
