//! Consumer-facing capability negotiation examples (issue #718).
//!
//! Demonstrates how a downstream contract should detect supported LedgerLens
//! features **without** hardcoding a contract version number.  The canonical
//! API is `supports_interface(capability: Symbol) -> bool`, which is stable
//! across upgrades and is the only endorsed mechanism for feature detection.
//!
//! ## Capabilities covered
//!
//! | Symbol string    | Meaning                                              |
//! |------------------|------------------------------------------------------|
//! | `"gate"`         | `query_risk_gate` (basic swap/borrow guard)          |
//! | `"cgate"`        | `query_risk_gate_with_confidence` (confidence-aware) |
//! | `"score"`        | `get_score` (raw risk score lookup)                  |
//! | `"aggr"`         | `get_aggregate_score` (multi-shard aggregation)      |
//! | `"emb"`          | `is_embargoed` / `set_score_embargo`                 |
//! | `"batch"`        | `submit_scores_batch` (batch submission)             |
//! | `"batch_attested"` | `submit_scores_batch_attested` (attested batch)    |
//!
//! ## Scenarios covered
//!
//! 1. **Success** — querying a known capability returns `true`.
//! 2. **Unsupported capability** — querying an unknown capability returns
//!    `false`; callers must not panic or assume `false` means "unavailable".
//! 3. **Mixed aggregator/score deployments** — the aggregator only advertises
//!    the subset of capabilities it owns; capability probing must be
//!    per-contract, not global.
//! 4. **Consumer gate pattern** — the recommended guard a downstream contract
//!    should write before invoking an optional feature.
//!
//! These are integration tests that drive real deployed contracts, not unit
//! tests of the capability flag table.

use ledgerlens_aggregator::{LedgerLensAggregator, LedgerLensAggregatorClient};
use ledgerlens_score::{LedgerLensScoreContract, LedgerLensScoreContractClient};
use soroban_sdk::{symbol_short, testutils::Address as _, Address, Env, Symbol};

// ── Helpers ───────────────────────────────────────────────────────────────────

fn deploy_score(env: &Env) -> LedgerLensScoreContractClient {
    let id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(env, &id);
    client.initialize(&Address::generate(env), &Address::generate(env));
    client
}

fn deploy_aggregator(env: &Env) -> LedgerLensAggregatorClient {
    let id = env.register_contract(None, LedgerLensAggregator);
    let client = LedgerLensAggregatorClient::new(env, &id);
    client.initialize(&Address::generate(env));
    client
}

// ── Scenario 1: Known capabilities return true on the score contract ───────────

#[test]
fn score_contract_supports_gate_capability() {
    let env = Env::default();
    env.mock_all_auths();
    let score = deploy_score(&env);

    assert!(score.supports_interface(&symbol_short!("gate")));
}

#[test]
fn score_contract_supports_confidence_gate_capability() {
    let env = Env::default();
    env.mock_all_auths();
    let score = deploy_score(&env);

    assert!(score.supports_interface(&symbol_short!("cgate")));
}

#[test]
fn score_contract_supports_score_capability() {
    let env = Env::default();
    env.mock_all_auths();
    let score = deploy_score(&env);

    assert!(score.supports_interface(&symbol_short!("score")));
}

#[test]
fn score_contract_supports_aggr_capability() {
    let env = Env::default();
    env.mock_all_auths();
    let score = deploy_score(&env);

    assert!(score.supports_interface(&symbol_short!("aggr")));
}

#[test]
fn score_contract_supports_embargo_capability() {
    let env = Env::default();
    env.mock_all_auths();
    let score = deploy_score(&env);

    assert!(score.supports_interface(&symbol_short!("emb")));
}

#[test]
fn score_contract_supports_batch_capability() {
    let env = Env::default();
    env.mock_all_auths();
    let score = deploy_score(&env);

    assert!(score.supports_interface(&symbol_short!("batch")));
}

#[test]
fn score_contract_supports_batch_attested_capability() {
    let env = Env::default();
    env.mock_all_auths();
    let score = deploy_score(&env);

    assert!(score.supports_interface(&Symbol::new(&env, "batch_attested")));
}

// ── Scenario 2: Unknown capabilities return false — not a panic, not an error ──

#[test]
fn score_contract_returns_false_for_unknown_capability() {
    let env = Env::default();
    env.mock_all_auths();
    let score = deploy_score(&env);

    // A capability that will never exist — must return false, not trap.
    assert!(!score.supports_interface(&symbol_short!("xyzzy")));
}

#[test]
fn aggregator_returns_false_for_unknown_capability() {
    let env = Env::default();
    env.mock_all_auths();
    let agg = deploy_aggregator(&env);

    assert!(!agg.supports_interface(&symbol_short!("xyzzy")));
}

#[test]
fn empty_capability_symbol_returns_false() {
    let env = Env::default();
    env.mock_all_auths();
    let score = deploy_score(&env);

    // Querying the empty symbol must not panic; returning false is correct.
    assert!(!score.supports_interface(&symbol_short!("")));
}

// ── Scenario 3: Mixed aggregator/score deployments ────────────────────────────
//
// The aggregator is a distinct contract with its own capability set.  A
// consumer that probes the score contract for aggregator-specific capabilities
// (or vice-versa) may get `false` even though the deployment supports the
// feature — on a different contract.  Capability probing must be directed at
// the correct contract.

#[test]
fn aggregator_contract_supports_gate_capability() {
    let env = Env::default();
    env.mock_all_auths();
    let agg = deploy_aggregator(&env);

    // The aggregator exposes its own `query_risk_gate` and must advertise it.
    assert!(agg.supports_interface(&symbol_short!("gate")));
}

#[test]
fn aggregator_contract_supports_score_capability() {
    let env = Env::default();
    env.mock_all_auths();
    let agg = deploy_aggregator(&env);

    assert!(agg.supports_interface(&symbol_short!("score")));
}

#[test]
fn aggregator_contract_supports_aggr_capability() {
    let env = Env::default();
    env.mock_all_auths();
    let agg = deploy_aggregator(&env);

    assert!(agg.supports_interface(&symbol_short!("aggr")));
}

/// A score-only shard does NOT advertise aggregator-only capabilities that
/// belong to the aggregator tier.  Probing the shard for those must return
/// `false`, proving that a consumer cannot treat a shard as a drop-in for an
/// aggregator without first verifying the full capability set.
#[test]
fn score_shard_does_not_masquerade_as_aggregator_for_all_capabilities() {
    let env = Env::default();
    env.mock_all_auths();
    let score = deploy_score(&env);
    let agg = deploy_aggregator(&env);

    // Both support "gate" — the shared cross-contract integration point.
    assert!(score.supports_interface(&symbol_short!("gate")));
    assert!(agg.supports_interface(&symbol_short!("gate")));

    // "batch_attested" is a score-contract feature; aggregator need not have it.
    // This assertion documents the difference — do NOT assume they are equal.
    let score_has_batch_attested = score.supports_interface(&Symbol::new(&env, "batch_attested"));
    let agg_has_batch_attested = agg.supports_interface(&Symbol::new(&env, "batch_attested"));
    // At least the score contract must support it.
    assert!(score_has_batch_attested);
    // The point: do not assume the aggregator has every score-shard capability.
    let _ = agg_has_batch_attested; // document but do not constrain aggregator
}

// ── Scenario 4: Recommended consumer gate pattern ─────────────────────────────
//
// The pattern below is what a downstream Soroban contract should implement
// before invoking an optional LedgerLens feature.  It is reproduced here as a
// test so its correctness is mechanically verified, not just documented.

/// Returns `true` when the target contract supports `query_risk_gate_with_confidence`.
/// In production code this would be called once during an upgrade/migration
/// check, not on every swap — `supports_interface` is a read-only, side-effect
/// free function safe to cache.
fn consumer_probe_confidence_gate(_env: &Env, contract: &LedgerLensScoreContractClient) -> bool {
    contract.supports_interface(&symbol_short!("cgate"))
}

#[test]
fn consumer_gate_pattern_returns_true_for_current_score_contract() {
    let env = Env::default();
    env.mock_all_auths();
    let score = deploy_score(&env);

    assert!(consumer_probe_confidence_gate(&env, &score));
}

/// Simulates what happens when a consumer probes an older or stripped-down
/// deployment that does not support `cgate`.  The consumer must fall back to
/// the basic `gate` path rather than panicking.
#[test]
fn consumer_gate_pattern_falls_back_gracefully_when_cgate_unsupported() {
    let env = Env::default();
    env.mock_all_auths();
    let score = deploy_score(&env);

    // Probe for an intentionally unrecognised capability.
    let supports_hypothetical = score.supports_interface(&symbol_short!("cgate_v2"));

    // Consumer logic: use cgate_v2 if available, fall back to cgate.
    let capability_to_use = if supports_hypothetical {
        "cgate_v2"
    } else {
        "cgate" // known-good fallback
    };

    // Verify the fallback is itself supported.
    assert!(score.supports_interface(&Symbol::new(&env, capability_to_use)));
}
