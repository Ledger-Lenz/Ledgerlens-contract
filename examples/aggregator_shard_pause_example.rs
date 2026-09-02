//! Reference integration: gating a swap on `ledgerlens-aggregator` when the
//! aggregator sits in front of **several** shards, with explicit, documented
//! handling of the *degraded* case where one shard is paused mid-aggregation
//! (e.g. maintenance).
//!
//! `examples/aggregator_gate_example.rs` covers the normal path and the
//! "whole aggregator can't answer" fallback. This example goes one level
//! deeper, answering two questions an integrator will actually hit:
//! **what does the aggregator return when a single shard is down**, and
//! **how should a well-behaved consumer tell that apart from a real verdict**.
//!
//! ## What the aggregator actually returns when a shard is paused
//!
//! `LedgerLensAggregator::query_risk_gate` ANDs the result across every
//! registered, healthy shard. When one shard is globally paused, that shard
//! does **not** trap the cross-contract call — `ledgerlens-score`'s
//! `query_risk_gate_with_confidence` hits its own fail-closed branch and
//! returns a clean `Ok(false)`. From the aggregator's perspective that is
//! indistinguishable from a shard that genuinely judges the wallet
//! high-risk, so:
//!
//! - the whole query fails closed (`query_risk_gate() == false`), and
//! - `get_last_shard_failure` is **not** updated — it only records a shard
//!   whose cross-contract call *itself* failed (unreachable/trapped), so the
//!   before/after snapshot trick from `aggregator_gate_example.rs` will *not*
//!   catch a pause. Without extra care, a paused shard gets misreported as a
//!   genuine "high-risk wallet" rejection.
//!
//! (Concretely exercised in
//! `tests/composability/tests/aggregator_shard_pause.rs` and
//! `tests/composability/tests/aggregator_fallback_gate.rs`; background in
//! issue #411.)
//!
//! ## The pattern
//!
//! A well-behaved consumer must look at the raw shards, not just the
//! aggregated boolean:
//!
//! 1. `get_shards()` empty => nothing is registered to consult at all; treat
//!    this as **unavailable**, not as a verdict.
//! 2. Probe each registered shard's `is_paused()` *before* trusting any
//!    `query_risk_gate` result. If any shard is paused, the aggregated gate
//!    is guaranteed to collapse to `false` — but that `false` reflects a
//!    down shard, not the wallet's risk. Treat this as **degraded**: fail
//!    closed, and surface the paused shard(s) so an operator can alert /
//!    un-pause them.
//! 3. Only when no shard is paused does the normal-path logic apply:
//!    snapshot `get_last_shard_failure()` before/after the call to tell an
//!    **unreachable** shard from a genuine, risk-based rejection.
//!
//! ## Recommended fallback policy: fail closed
//!
//! Both `Unavailable` and `Degraded` mean "we cannot get a trustworthy
//! verdict right now"; the recommendation is to **fail closed** (refuse the
//! swap) while separately surfacing the degraded cause so an operator can
//! respond. This matches `aggregator_gate_example.rs`'s policy — the new
//! thing here is that the degraded cause is made visible instead of being
//! silently reported as a rejection.
//!
//! Build it as part of the workspace:
//!
//! ```text
//! cargo build --example aggregator_shard_pause_example -p ledgerlens-aggregator
//! ```

#![no_std]

use ledgerlens_aggregator::LedgerLensAggregatorClient;
use ledgerlens_score::LedgerLensScoreContractClient;
use soroban_sdk::{contract, contracterror, contractimpl, contracttype, Address, Env, Symbol, Vec};

/// Errors surfaced by the gated AMM. `PartialShardPause` is the degraded-mode
/// branch new to this example — one or more shards is paused, so the
/// aggregated result is unreliable (it fails closed). It is distinct from
/// `AggregatorUnavailable` (nothing registered, or an *unreachable* shard)
/// and from `UserHighRisk` (a genuine, risk-based rejection).
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum AmmError {
    /// No shards registered, or the shard's cross-contract call itself failed
    /// (unreachable/trapped). Fallback policy applied here: fail closed.
    AggregatorUnavailable = 1,
    /// The aggregator's healthy shards agree this wallet does not clear
    /// `gate_threshold` — a genuine, risk-based rejection.
    UserHighRisk = 2,
    /// One or more registered shards is paused, so the aggregated gate fails
    /// closed for reasons unrelated to the wallet's risk. Fail closed; an
    /// operator should alert on this so the shard(s) can be un-paused.
    PartialShardPause = 3,
}

/// The rich verdict a degraded-aware consumer should build out of the
/// aggregator's public API plus per-shard `is_paused` probes. This is the
/// "detect" half of the pattern.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GateOutcome {
    /// Every healthy shard agreed the wallet clears the gate.
    Passed,
    /// No shard is paused or unreachable; the healthy shards agree the
    /// wallet is too risky. A genuine rejection.
    RejectedHighRisk,
    /// One or more shards is paused. The aggregated gate necessarily returns
    /// `false`, but that `false` is not a trustworthy verdict. Fails closed;
    /// `paused` names the offending shard(s) for operator visibility.
    Degraded(Vec<Address>),
    /// No shards registered, or a shard's cross-contract call failed outright
    /// (recorded via `get_last_shard_failure` — unreachable/trapped).
    Unavailable,
}

#[contract]
pub struct PauseAwareGatedAmm;

#[contractimpl]
impl PauseAwareGatedAmm {
    /// Evaluate the risk gate, distinguishing a genuine rejection from the
    /// aggregator being unavailable **or** degraded by a paused shard.
    pub fn evaluate_gate(
        env: Env,
        user: Address,
        asset_pair: Symbol,
        aggregator_id: Address,
        gate_threshold: u32,
    ) -> GateOutcome {
        let aggregator = LedgerLensAggregatorClient::new(&env, &aggregator_id);

        // 1. Nothing registered to consult at all: unavailable, not a verdict.
        let shards = aggregator.get_shards();
        if shards.is_empty() {
            return GateOutcome::Unavailable;
        }

        // 2. Degraded-mode detection. A globally paused shard makes
        // `query_risk_gate` fail closed to `false` *without* updating
        // `get_last_shard_failure` (it returns a clean `Ok(false)`, not a
        // trap). So the only reliable way for a consumer to see a pause is to
        // probe each registered shard's own `is_paused()`.
        let mut paused: Vec<Address> = Vec::new(&env);
        for i in 0..shards.len() {
            let shard = shards.get(i).unwrap();
            let shard_client = LedgerLensScoreContractClient::new(&env, &shard);
            if shard_client.is_paused() {
                paused.push_back(shard);
            }
        }
        if !paused.is_empty() {
            // A paused shard forces the aggregated gate to `false`; that
            // `false` is NOT a trustworthy verdict. Fail closed, but surface
            // the cause so the caller can alert rather than treat it as a
            // rejection.
            return GateOutcome::Degraded(paused);
        }

        // 3. No shard paused: normal path. Snapshot the failure marker so we
        // can tell whether *this* call tripped it (an unreachable/trapped
        // shard) versus a genuine rejection from healthy shards.
        let failure_before = aggregator.get_last_shard_failure();
        if aggregator.query_risk_gate(&user, &asset_pair, &gate_threshold) {
            return GateOutcome::Passed;
        }
        let failure_after = aggregator.get_last_shard_failure();
        if failure_after != failure_before {
            // This exact call caused a *new* shard failure — the `false`
            // reflects an unreachable/trapped shard, not the wallet's risk.
            GateOutcome::Unavailable
        } else {
            // Otherwise it's a genuine, risk-based rejection.
            GateOutcome::RejectedHighRisk
        }
    }

    /// Execute a swap gated on `ledgerlens-aggregator`. "Gracefully handle"
    /// here means: proceed only on a trustworthy `Passed`; fail closed on
    /// every other outcome, and let the caller distinguish *why* it refused
    /// so it can alert an operator when the failure is operational rather
    /// than a risk verdict.
    pub fn swap(
        env: Env,
        user: Address,
        asset_pair: Symbol,
        amount_in: u64,
        aggregator_id: Address,
        gate_threshold: u32,
    ) -> Result<u64, AmmError> {
        let outcome = Self::evaluate_gate(env, user, asset_pair, aggregator_id, gate_threshold);
        match outcome {
            GateOutcome::Passed => {
                // (In a real AMM this would include pool checks, reserve
                // calculations, etc.)
                Ok((amount_in * 997) / 1000)
            }
            GateOutcome::RejectedHighRisk => Err(AmmError::UserHighRisk),
            GateOutcome::Degraded(_) => Err(AmmError::PartialShardPause),
            GateOutcome::Unavailable => Err(AmmError::AggregatorUnavailable),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ledgerlens_aggregator::{LedgerLensAggregator, LedgerLensAggregatorClient};
    use ledgerlens_score::{LedgerLensScoreContract, LedgerLensScoreContractClient};
    use soroban_sdk::{
        symbol_short,
        testutils::{Address as _, Ledger as _},
        Vec,
    };

    const GATE_THRESHOLD: u32 = 75;

    struct Fixture<'a> {
        env: Env,
        aggregator_id: Address,
        shard_a: LedgerLensScoreContractClient<'a>,
        shard_b: LedgerLensScoreContractClient<'a>,
        amm: PauseAwareGatedAmmClient<'a>,
        user: Address,
        pair: Symbol,
    }

    /// Registers an aggregator, two real `LedgerLensScoreContract` shards, and
    /// the example AMM, but does **not** add the shards to the aggregator —
    /// each test opts in via `add_both_shards` so the no-shards scenario is
    /// easy to set up.
    fn setup<'a>() -> Fixture<'a> {
        let env = Env::default();
        env.mock_all_auths();
        env.ledger().with_mut(|l| l.timestamp = 100_000);

        let aggregator_id = env.register_contract(None, LedgerLensAggregator);
        let aggregator = LedgerLensAggregatorClient::new(&env, &aggregator_id);
        aggregator.initialize(&Address::generate(&env));

        let shard_a_id = env.register_contract(None, LedgerLensScoreContract);
        let shard_a = LedgerLensScoreContractClient::new(&env, &shard_a_id);
        shard_a.initialize(&Address::generate(&env), &Address::generate(&env));

        let shard_b_id = env.register_contract(None, LedgerLensScoreContract);
        let shard_b = LedgerLensScoreContractClient::new(&env, &shard_b_id);
        shard_b.initialize(&Address::generate(&env), &Address::generate(&env));

        let amm_id = env.register_contract(None, PauseAwareGatedAmm);
        let amm = PauseAwareGatedAmmClient::new(&env, &amm_id);

        let user = Address::generate(&env);
        let pair = symbol_short!("XLM_USDC");

        Fixture { env, aggregator_id, shard_a, shard_b, amm, user, pair }
    }

    fn add_both_shards(f: &Fixture) {
        let aggregator = LedgerLensAggregatorClient::new(&f.env, &f.aggregator_id);
        aggregator.add_shard(&f.shard_a.address);
        aggregator.add_shard(&f.shard_b.address);
    }

    fn submit_score(f: &Fixture, shard: &LedgerLensScoreContractClient, score: u32) {
        shard.submit_score(
            &Vec::new(&f.env),
            &f.user,
            &f.pair,
            &score,
            &false,
            &false,
            &f.env.ledger().timestamp(),
            &90, // comfortably above any confidence floor
            &1,
            &None,
        );
    }

    #[test]
    fn all_shards_healthy_low_risk_passes() {
        let f = setup();
        add_both_shards(&f);
        submit_score(&f, &f.shard_a, 10);
        submit_score(&f, &f.shard_b, 10);

        assert_eq!(
            f.amm.evaluate_gate(&f.user, &f.pair, &f.aggregator_id, &GATE_THRESHOLD),
            GateOutcome::Passed
        );
        assert_eq!(
            f.amm.try_swap(&f.user, &f.pair, &1_000_000, &f.aggregator_id, &GATE_THRESHOLD),
            Ok(Ok(997_000))
        );
    }

    #[test]
    fn all_shards_healthy_high_risk_is_genuine_rejection() {
        let f = setup();
        add_both_shards(&f);
        submit_score(&f, &f.shard_a, 90);
        submit_score(&f, &f.shard_b, 90);

        assert_eq!(
            f.amm.evaluate_gate(&f.user, &f.pair, &f.aggregator_id, &GATE_THRESHOLD),
            GateOutcome::RejectedHighRisk
        );
        assert_eq!(
            f.amm.try_swap(&f.user, &f.pair, &1_000_000, &f.aggregator_id, &GATE_THRESHOLD),
            Err(Ok(AmmError::UserHighRisk))
        );
    }

    #[test]
    fn no_shards_registered_is_unavailable() {
        let f = setup(); // no add_both_shards()
        assert_eq!(
            f.amm.evaluate_gate(&f.user, &f.pair, &f.aggregator_id, &GATE_THRESHOLD),
            GateOutcome::Unavailable
        );
        assert_eq!(
            f.amm.try_swap(&f.user, &f.pair, &1_000_000, &f.aggregator_id, &GATE_THRESHOLD),
            Err(Ok(AmmError::AggregatorUnavailable))
        );
    }

    /// Core degraded-mode scenario. Both shards hold a *low-risk* score
    /// (`10`) for the user, so the gate would genuinely pass — but shard_a is
    /// globally paused (maintenance). `query_risk_gate` fails closed to
    /// `false`, and — because a paused shard returns a clean `Ok(false)`
    /// rather than a trap — `get_last_shard_failure` is NOT updated. So an
    /// integrator using only the boolean, or even the before/after
    /// failure-marker trick from `aggregator_gate_example.rs`, would
    /// misreport this as a genuine `UserHighRisk` rejection. Probing each
    /// shard's `is_paused()` catches it as `Degraded`.
    #[test]
    fn paused_shard_is_detected_as_degraded_not_rejection() {
        let f = setup();
        add_both_shards(&f);
        submit_score(&f, &f.shard_a, 10);
        submit_score(&f, &f.shard_b, 10);

        f.shard_a.pause(&Vec::new(&f.env));
        assert!(f.shard_a.is_paused());
        assert!(!f.shard_b.is_paused());

        // Baseline sanity: the aggregated gate does fail closed, exactly as
        // documented in tests/composability/tests/aggregator_shard_pause.rs...
        let aggregator = LedgerLensAggregatorClient::new(&f.env, &f.aggregator_id);
        assert!(!aggregator.query_risk_gate(&f.user, &f.pair, &GATE_THRESHOLD));
        // ...and that `false` is NOT attributable to any shard failure marker
        // (a pause is not a trap), which is why the degraded-aware consumer
        // must probe `is_paused()` per shard:
        assert_eq!(aggregator.get_last_shard_failure(), None);

        let mut expected_paused = Vec::new(&f.env);
        expected_paused.push_back(f.shard_a.address.clone());
        assert_eq!(
            f.amm.evaluate_gate(&f.user, &f.pair, &f.aggregator_id, &GATE_THRESHOLD),
            GateOutcome::Degraded(expected_paused)
        );
        assert_eq!(
            f.amm.try_swap(&f.user, &f.pair, &1_000_000, &f.aggregator_id, &GATE_THRESHOLD),
            Err(Ok(AmmError::PartialShardPause))
        );

        // Recovery: un-pausing the shard restores a genuine (passing) verdict.
        f.shard_a.unpause(&Vec::new(&f.env));
        assert!(!f.shard_a.is_paused());
        assert_eq!(
            f.amm.evaluate_gate(&f.user, &f.pair, &f.aggregator_id, &GATE_THRESHOLD),
            GateOutcome::Passed
        );
    }
}
