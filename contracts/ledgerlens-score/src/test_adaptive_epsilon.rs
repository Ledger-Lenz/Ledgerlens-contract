use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Ledger as _},
    Address, Env, Vec,
};

use crate::{LedgerLensScoreContract, LedgerLensScoreContractClient};

fn setup<'a>() -> (Env, LedgerLensScoreContractClient<'a>, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|l| l.timestamp = 1_700_000_000);

    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let service = Address::generate(&env);
    client.initialize(&admin, &service);

    (env, client, admin, service)
}

/// Submit a score for the given wallet/pair, advancing timestamp by 3601s
/// each time to clear the rate-limit cooldown.
fn submit_at(
    env: &Env,
    client: &LedgerLensScoreContractClient,
    wallet: &Address,
    pair: &soroban_sdk::Symbol,
    score: u32,
    ts: u64,
) {
    env.ledger().with_mut(|l| l.timestamp = ts);
    client.submit_score(
        &Vec::new(env),
        wallet,
        pair,
        &score,
        &false,
        &false,
        &ts,
        &80,
        &1,
        &None,
    );
}

// ── Disabled by default ──────────────────────────────────────────────────────

#[test]
fn test_get_effective_epsilon_disabled_returns_base() {
    let (env, client, _admin, _service) = setup();
    let pair = symbol_short!("XLM_USDC");
    // Adaptive epsilon off by default → returns base (DEFAULT_CONSENSUS_EPSILON = 5).
    assert_eq!(client.get_effective_epsilon(&pair), 5);
    let _ = env;
}

// ── set_adaptive_epsilon / toggle ────────────────────────────────────────────

#[test]
fn test_set_adaptive_epsilon_disabled_returns_base() {
    let (env, client, _admin, _service) = setup();
    let pair = symbol_short!("XLM_USDC");
    client.set_adaptive_epsilon(&false, &200);
    assert_eq!(client.get_effective_epsilon(&pair), 5);
}

#[test]
fn test_set_adaptive_epsilon_enabled_zero_scale_returns_base() {
    let (env, client, _admin, _service) = setup();
    let pair = symbol_short!("XLM_USDC");
    client.set_adaptive_epsilon(&true, &0);
    // scale = 0 → addend = 0 → result is base epsilon
    assert_eq!(client.get_effective_epsilon(&pair), 5);
}

// ── Low-variance scenario ────────────────────────────────────────────────────

#[test]
fn test_low_variance_epsilon_near_base() {
    // All scores identical → stddev = 0 → effective_epsilon = base = 5.
    let (env, client, _admin, _service) = setup();
    let pair = symbol_short!("XLM_USDC");
    client.set_adaptive_epsilon(&true, &500);

    let wallet1 = Address::generate(&env);
    let wallet2 = Address::generate(&env);
    let ts_base: u64 = 1_700_000_000;

    submit_at(&env, &client, &wallet1, &pair, 50, ts_base);
    submit_at(&env, &client, &wallet2, &pair, 50, ts_base + 3_601);

    // stddev = 0 → addend = 0
    assert_eq!(client.get_effective_epsilon(&pair), 5);
}

// ── High-variance scenario ───────────────────────────────────────────────────

#[test]
fn test_high_variance_epsilon_above_base() {
    // Scores spread widely: 0 and 100.
    // mean = 50, variance = (50² + 50²) / 2 = 2500, stddev = 50.
    // scale = 100 → addend = 100 * 50 / 1000 = 5.
    // effective = 5 + 5 = 10.
    let (env, client, _admin, _service) = setup();
    let pair = symbol_short!("XLM_USDC");
    client.set_adaptive_epsilon(&true, &100);

    let wallet1 = Address::generate(&env);
    let wallet2 = Address::generate(&env);
    let ts_base: u64 = 1_700_000_000;

    submit_at(&env, &client, &wallet1, &pair, 0, ts_base);
    submit_at(&env, &client, &wallet2, &pair, 100, ts_base + 3_601);

    let effective = client.get_effective_epsilon(&pair);
    assert!(effective > 5, "expected effective > base, got {effective}");
}

// ── Cap at 100 ───────────────────────────────────────────────────────────────

#[test]
fn test_effective_epsilon_capped_at_100() {
    // Large scale factor forces the addend to overflow → result capped at 100.
    let (env, client, _admin, _service) = setup();
    let pair = symbol_short!("XLM_USDC");
    client.set_adaptive_epsilon(&true, &u32::MAX);

    let wallet1 = Address::generate(&env);
    let wallet2 = Address::generate(&env);
    let ts_base: u64 = 1_700_000_000;

    submit_at(&env, &client, &wallet1, &pair, 0, ts_base);
    submit_at(&env, &client, &wallet2, &pair, 100, ts_base + 3_601);

    assert_eq!(client.get_effective_epsilon(&pair), 100);
}

// ── No history → returns base ─────────────────────────────────────────────────

#[test]
fn test_effective_epsilon_no_history_returns_base() {
    let (env, client, _admin, _service) = setup();
    let pair = symbol_short!("XLM_ETH");
    client.set_adaptive_epsilon(&true, &500);
    // No submissions for this pair → stddev = 0 → base returned.
    assert_eq!(client.get_effective_epsilon(&pair), 5);
    let _ = env;
}

// ── get_effective_epsilon for different pair is independent ──────────────────

#[test]
fn test_effective_epsilon_independent_per_pair() {
    let (env, client, _admin, _service) = setup();
    let pair_a = symbol_short!("XLM_USDC");
    let pair_b = symbol_short!("XLM_BTC");
    client.set_adaptive_epsilon(&true, &200);

    // Only pair_a has scores.
    let wallet = Address::generate(&env);
    submit_at(&env, &client, &wallet, &pair_a, 10, 1_700_000_000);
    env.ledger().with_mut(|l| l.timestamp += 3_601);
    submit_at(&env, &client, &wallet, &pair_a, 90, 1_700_003_601);

    let eps_a = client.get_effective_epsilon(&pair_a);
    let eps_b = client.get_effective_epsilon(&pair_b);
    // pair_b has no history → stddev = 0 → eps_b = base = 5
    assert_eq!(eps_b, 5);
    // pair_a has high variance → eps_a > base
    assert!(eps_a > 5, "expected eps_a > 5, got {eps_a}");
}
