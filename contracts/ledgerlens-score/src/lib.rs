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
mod test_hysteresis;

use soroban_sdk::{
    contract, contractimpl, crypto::Hash, symbol_short, Address, Bytes, BytesN, Env, Symbol,
    SymbolStr, TryFromVal, Vec,
};

pub use errors::Error;
pub use types::{
    AggregateRiskScore, BatchEntryResult, BatchResult, RiskScore, ScoreAttestation,
    ScoreSubmission, UpgradeProposal,
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
    /// assert_eq!(client.get_version(), 2);
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
    /// Returns `ContractPaused` if the admin has activated the circuit breaker.
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

        let service_set = storage::get_service_set(&env);
        let threshold = storage::get_service_threshold(&env);

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
        } else {
            // Legacy single-service path.
            let service = storage::get_service(&env);
            service.require_auth();
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

        let risk_score =
            RiskScore { score, benford_flag, ml_flag, timestamp, confidence, model_version };

        storage::set_score(&env, &wallet, &asset_pair, &risk_score);
        storage::push_score_history(&env, &wallet, &asset_pair, &risk_score);
        storage::register_pair_for_wallet(&env, &wallet, &asset_pair);
        storage::increment_score_count(&env, &wallet, &asset_pair);
        Self::refresh_aggregate_cache(&env, &wallet);

        let score_threshold = storage::get_risk_threshold(&env);
        if score >= score_threshold {
            events::threshold_breached(&env, &wallet, &asset_pair, score, score_threshold);
        }
        Self::evaluate_risk_band(&env, &wallet, &asset_pair, score, score_threshold);

        events::score_submitted(&env, &wallet, &asset_pair, &risk_score);
        Ok(())
    }

    /// Submit multiple risk scores in a single invocation.  The service
    /// account authorises once for the whole batch.  Returns a `BatchResult`
    /// that lists every entry's outcome so the caller knows exactly which
    /// entries succeeded and why any failed, without needing to re-query
    /// each (wallet, pair) individually.
    ///
    /// Entries with out-of-range `score` or `confidence`, zero `timestamp`,
    /// or that arrive before their `(wallet, asset_pair)`'s submission
    /// cooldown has elapsed, are recorded as rejected in the result with an
    /// appropriate `rejection_code`. Two entries for the same pair within
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

            if sub.score > 100 {
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

                    if sub.score >= threshold {
                        events::threshold_breached(
                            &env,
                            &sub.wallet,
                            &sub.asset_pair,
                            sub.score,
                            threshold,
                        );
                    }
                    Self::evaluate_risk_band(
                        &env,
                        &sub.wallet,
                        &sub.asset_pair,
                        sub.score,
                        threshold,
                    );

                    events::score_submitted(&env, &sub.wallet, &sub.asset_pair, &risk_score);
                    accepted = true;
                    accepted_count += 1;
                }
            }

            results.push_back(BatchEntryResult { index: i, accepted, rejection_code });
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
        storage::get_score(&env, &wallet, &asset_pair).ok_or(Error::ScoreNotFound)
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
    /// client.set_history_max_depth(&20).unwrap();
    /// assert_eq!(client.get_history_max_depth(), 20);
    /// ```
    ///
    /// # Errors
    /// - [`Error::NotInitialized`] if the contract has no admin yet.
    /// - [`Error::InvalidHistoryDepth`] if `depth` is `0` or above
    ///   `MAX_HISTORY_DEPTH` (50).
    pub fn set_history_max_depth(env: Env, depth: u32) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        if depth == 0 || depth > constants::MAX_HISTORY_DEPTH {
            return Err(Error::InvalidHistoryDepth);
        }
        let admin = storage::get_admin(&env);
        admin.require_auth();
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
    /// client.set_pair_weight(&pair, &3).unwrap();
    /// assert_eq!(client.get_pair_weight(&pair), 3);
    /// ```
    pub fn set_pair_weight(env: Env, asset_pair: Symbol, weight: u32) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        storage::get_admin(&env).require_auth();
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
        // A wallet currently in the high-risk band fails the gate even if its
        // raw score has dipped below gate_threshold — the hysteresis state is
        // sticky until the score crosses the full exit boundary.
        // Uses peek (no TTL extension) to remain side-effect free per the
        // ILedgerLensScore interface contract.
        if storage::peek_risk_band_state(&env, &wallet, &asset_pair) {
            return false;
        }
        match storage::peek_score(&env, &wallet, &asset_pair) {
            Some(risk) => risk.score < gate_threshold,
            None => false,
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
    pub fn add_service_signer(env: Env, signer: Address) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        storage::get_admin(&env).require_auth();

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
    pub fn remove_service_signer(env: Env, signer: Address) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        storage::get_admin(&env).require_auth();

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
    pub fn set_service_threshold(env: Env, threshold: u32) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        storage::get_admin(&env).require_auth();

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
    pub fn set_service_pubkey(env: Env, pubkey: Bytes) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        if pubkey.len() != 33 && pubkey.len() != 65 {
            return Err(Error::InvalidPubkeyLength);
        }
        storage::get_admin(&env).require_auth();
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
    pub fn transfer_admin(env: Env, new_admin: Address) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        let admin = storage::get_admin(&env);
        admin.require_auth();
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
    /// client.transfer_admin(&new_admin);
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
    /// client.transfer_admin(&new_admin);
    /// client.cancel_admin_transfer();
    /// assert_eq!(client.get_admin(), admin);
    /// ```
    pub fn cancel_admin_transfer(env: Env) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        if !storage::has_pending_admin(&env) {
            return Err(Error::NoPendingAdminTransfer);
        }
        let admin = storage::get_admin(&env);
        admin.require_auth();
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
    /// client.pause();
    /// assert!(client.is_paused());
    /// ```
    pub fn pause(env: Env) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        let admin = storage::get_admin(&env);
        admin.require_auth();
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
    /// client.pause();
    /// assert!(client.is_paused());
    /// client.unpause();
    /// assert!(!client.is_paused());
    /// ```
    pub fn unpause(env: Env) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        let admin = storage::get_admin(&env);
        admin.require_auth();
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
    pub fn propose_upgrade(env: Env, new_wasm_hash: BytesN<32>) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        let admin = storage::get_admin(&env);
        admin.require_auth();

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
    pub fn execute_upgrade(env: Env) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        let admin = storage::get_admin(&env);
        admin.require_auth();

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
    pub fn veto_upgrade(env: Env) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        let admin = storage::get_admin(&env);
        admin.require_auth();

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
    pub fn set_upgrade_delay(env: Env, delay_secs: u64) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        if !(constants::MIN_UPGRADE_DELAY_SECS..=constants::MAX_UPGRADE_DELAY_SECS)
            .contains(&delay_secs)
        {
            return Err(Error::InvalidUpgradeDelay);
        }
        let admin = storage::get_admin(&env);
        admin.require_auth();
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
    /// client.set_watchlist(&wallet, &true);
    /// assert!(client.is_watchlisted(&wallet));
    /// ```
    pub fn set_watchlist(env: Env, wallet: Address, flagged: bool) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        storage::get_admin(&env).require_auth();
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
    /// client.set_risk_threshold(&80);
    /// assert_eq!(client.get_risk_threshold(), 80);
    /// ```
    pub fn set_risk_threshold(env: Env, threshold: u32) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        if threshold > 100 {
            return Err(Error::InvalidScore);
        }
        let admin = storage::get_admin(&env);
        admin.require_auth();
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
    /// The value is rejected with [`Error::InvalidHysteresisMargin`] when it
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
    pub fn set_staleness_window(env: Env, window_secs: u64) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        if window_secs == 0 {
            return Err(Error::InvalidStalenessWindow);
        }
        let admin = storage::get_admin(&env);
        admin.require_auth();
        storage::set_staleness_window(&env, window_secs);
        Ok(())
    }

    /// Returns the current staleness window in seconds. Defaults to
    /// `DEFAULT_STALENESS_WINDOW_SECS` (7 days) until configured.
    pub fn get_staleness_window(env: Env) -> u64 {
        storage::get_staleness_window(&env)
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
    /// client.set_cooldown(&120);
    /// assert_eq!(client.get_cooldown(), 120);
    /// ```
    ///
    /// # Errors
    /// - [`Error::NotInitialized`] if the contract has no admin yet.
    /// - [`Error::InvalidCooldown`] if `secs` is outside the bounds.
    pub fn set_cooldown(env: Env, secs: u64) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        if !(constants::MIN_COOLDOWN_SECS..=constants::MAX_COOLDOWN_SECS).contains(&secs) {
            return Err(Error::InvalidCooldown);
        }
        let admin = storage::get_admin(&env);
        admin.require_auth();
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
    pub fn override_rate_limit(env: Env, wallet: Address, asset_pair: Symbol) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        let admin = storage::get_admin(&env);
        admin.require_auth();
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
    pub fn clear_score_history(env: Env, wallet: Address, asset_pair: Symbol) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        let admin = storage::get_admin(&env);
        admin.require_auth();
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
    pub fn clear_score(env: Env, wallet: Address, asset_pair: Symbol) -> Result<(), Error> {
        if !storage::has_admin(&env) {
            return Err(Error::NotInitialized);
        }
        let admin = storage::get_admin(&env);
        admin.require_auth();
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

    // ── Internal helpers ─────────────────────────────────────────────────────

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
                events::risk_band_entered(env, wallet, asset_pair, score, risk_threshold);
            }
            // Already in band: stay, no event.
        } else if in_band {
            if score < exit_threshold {
                storage::set_risk_band_state(env, wallet, asset_pair, false);
                events::risk_band_cleared(env, wallet, asset_pair, score, exit_threshold);
            }
            // score >= exit_threshold: hysteresis holds, stay in band, no event.
        }
        // Not in band and score < threshold: nothing to do.
    }

    /// Shared implementation behind `get_aggregate_score`. Iterates the
    /// wallet's registered pairs once, accumulating the weighted sum and
    /// weight total with checked arithmetic so a pathological admin-set
    /// weight can never panic the contract.
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

            let weight = storage::get_pair_weight(env, &pair);
            let product = weight.checked_mul(component.score).ok_or(Error::ArithmeticOverflow)?;
            weighted_sum =
                weighted_sum.checked_add(product as u64).ok_or(Error::ArithmeticOverflow)?;
            weight_sum = weight_sum.checked_add(weight as u64).ok_or(Error::ArithmeticOverflow)?;
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
}
