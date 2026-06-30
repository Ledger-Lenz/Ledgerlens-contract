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

fn submit(
    env: &Env,
    client: &LedgerLensScoreContractClient,
    wallet: &Address,
    pair: &soroban_sdk::Symbol,
    score: u32,
) {
    client.submit_score(
        &Vec::new(env),
        wallet,
        pair,
        &score,
        &false,
        &false,
        &1_700_000_000,
        &80,
        &1,
        &None,
    );
}

// ── get_cluster_boundaries ───────────────────────────────────────────────────

#[test]
fn test_get_cluster_boundaries_default_empty() {
    let (env, client, _admin, _service) = setup();
    let bounds = client.get_cluster_boundaries();
    assert_eq!(bounds.len(), 0);
    let _ = env;
}

// ── set_cluster_boundaries validation ───────────────────────────────────────

#[test]
fn test_set_cluster_boundaries_empty_rejected() {
    let (env, client, _admin, _service) = setup();
    let empty: Vec<u32> = Vec::new(&env);
    let result = client.try_set_cluster_boundaries(&Vec::new(&env), &empty);
    assert!(result.is_err());
}

#[test]
fn test_set_cluster_boundaries_non_ascending_rejected() {
    let (env, client, _admin, _service) = setup();
    let mut bad = Vec::new(&env);
    bad.push_back(50u32);
    bad.push_back(30u32); // not ascending
    let result = client.try_set_cluster_boundaries(&Vec::new(&env), &bad);
    assert!(result.is_err());
}

#[test]
fn test_set_cluster_boundaries_zero_rejected() {
    let (env, client, _admin, _service) = setup();
    let mut bad = Vec::new(&env);
    bad.push_back(0u32);
    bad.push_back(50u32);
    let result = client.try_set_cluster_boundaries(&Vec::new(&env), &bad);
    assert!(result.is_err());
}

#[test]
fn test_set_cluster_boundaries_above_100_rejected() {
    let (env, client, _admin, _service) = setup();
    let mut bad = Vec::new(&env);
    bad.push_back(50u32);
    bad.push_back(101u32);
    let result = client.try_set_cluster_boundaries(&Vec::new(&env), &bad);
    assert!(result.is_err());
}

#[test]
fn test_set_cluster_boundaries_valid_stored() {
    let (env, client, _admin, _service) = setup();
    let mut bounds = Vec::new(&env);
    bounds.push_back(33u32);
    bounds.push_back(66u32);
    bounds.push_back(100u32);
    client.set_cluster_boundaries(&Vec::new(&env), &bounds);
    let stored = client.get_cluster_boundaries();
    assert_eq!(stored, bounds);
}

// ── get_wallet_cluster – no score / no boundaries ───────────────────────────

#[test]
fn test_get_wallet_cluster_none_without_boundaries() {
    let (env, client, _admin, _service) = setup();
    let wallet = Address::generate(&env);
    assert!(client.get_wallet_cluster(&wallet).is_none());
}

// ── cluster assignment on submit_score ──────────────────────────────────────

#[test]
fn test_cluster_assigned_low_score_cluster_0() {
    let (env, client, _admin, _service) = setup();
    let pair = symbol_short!("XLM_USDC");

    // Boundaries: [33, 66, 100]  →  clusters 0/1/2
    let mut bounds = Vec::new(&env);
    bounds.push_back(33u32);
    bounds.push_back(66u32);
    bounds.push_back(100u32);
    client.set_cluster_boundaries(&Vec::new(&env), &bounds);

    let wallet = Address::generate(&env);
    submit(&env, &client, &wallet, &pair, 20); // score 20 ≤ 33 → cluster 0

    assert_eq!(client.get_wallet_cluster(&wallet), Some(0));
}

#[test]
fn test_cluster_assigned_mid_score_cluster_1() {
    let (env, client, _admin, _service) = setup();
    let pair = symbol_short!("XLM_USDC");

    let mut bounds = Vec::new(&env);
    bounds.push_back(33u32);
    bounds.push_back(66u32);
    bounds.push_back(100u32);
    client.set_cluster_boundaries(&Vec::new(&env), &bounds);

    let wallet = Address::generate(&env);
    submit(&env, &client, &wallet, &pair, 50); // 33 < 50 ≤ 66 → cluster 1

    assert_eq!(client.get_wallet_cluster(&wallet), Some(1));
}

#[test]
fn test_cluster_assigned_high_score_cluster_2() {
    let (env, client, _admin, _service) = setup();
    let pair = symbol_short!("XLM_USDC");

    let mut bounds = Vec::new(&env);
    bounds.push_back(33u32);
    bounds.push_back(66u32);
    bounds.push_back(100u32);
    client.set_cluster_boundaries(&Vec::new(&env), &bounds);

    let wallet = Address::generate(&env);
    submit(&env, &client, &wallet, &pair, 80); // 66 < 80 ≤ 100 → cluster 2

    assert_eq!(client.get_wallet_cluster(&wallet), Some(2));
}

#[test]
fn test_cluster_boundary_wallet_exact_threshold() {
    // A wallet whose aggregate score is exactly on the boundary lands in the
    // lower cluster (score ≤ boundary[i] → cluster i).
    let (env, client, _admin, _service) = setup();
    let pair = symbol_short!("XLM_USDC");

    let mut bounds = Vec::new(&env);
    bounds.push_back(50u32);
    bounds.push_back(100u32);
    client.set_cluster_boundaries(&Vec::new(&env), &bounds);

    let wallet = Address::generate(&env);
    submit(&env, &client, &wallet, &pair, 50); // exactly on boundary → cluster 0

    assert_eq!(client.get_wallet_cluster(&wallet), Some(0));
}

#[test]
fn test_cluster_transition_on_score_change() {
    // First submission puts wallet in cluster 0; second (after cooldown) moves
    // it to cluster 1.
    let (env, client, _admin, _service) = setup();
    let pair = symbol_short!("XLM_USDC");

    let mut bounds = Vec::new(&env);
    bounds.push_back(40u32);
    bounds.push_back(100u32);
    client.set_cluster_boundaries(&Vec::new(&env), &bounds);

    let wallet = Address::generate(&env);
    submit(&env, &client, &wallet, &pair, 20); // cluster 0
    assert_eq!(client.get_wallet_cluster(&wallet), Some(0));

    // Advance time past the 1-hour cooldown.
    env.ledger().with_mut(|l| l.timestamp += 3_601);

    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &75,
        &false,
        &false,
        &1_700_003_601,
        &80,
        &1,
        &None,
    );
    // 75 > 40 → cluster 1
    assert_eq!(client.get_wallet_cluster(&wallet), Some(1));
}
