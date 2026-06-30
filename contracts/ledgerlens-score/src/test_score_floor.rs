//! Tests for the configurable per-wallet score submission floor (issue:
//! "Configurable Score Submission Floor for High-Risk Wallets").
//!
//! The floor blocks a compromised or colluding signer from laundering a
//! known high-risk wallet's reputation by submitting an artificially low
//! score once the wallet's historical peak has crossed a danger level.

use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Ledger as _},
    Address, Env, Symbol, Vec,
};

use crate::{Error, LedgerLensScoreContract, LedgerLensScoreContractClient};

const START_TS: u64 = 1_700_000_000;
const COOLDOWN: u64 = 3_601; // just past the 1-hour default cooldown

fn setup<'a>() -> (Env, LedgerLensScoreContractClient<'a>, Address) {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|l| l.timestamp = START_TS);

    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let service = Address::generate(&env);
    client.initialize(&admin, &service);

    (env, client, admin)
}

/// Submits a score using the current ledger timestamp.
fn submit(
    env: &Env,
    client: &LedgerLensScoreContractClient,
    wallet: &Address,
    pair: &Symbol,
    score: u32,
) {
    client.submit_score(
        &Vec::new(env),
        wallet,
        pair,
        &score,
        &false,
        &false,
        &env.ledger().timestamp(),
        &90,
        &1,
        &None,
    );
}

/// `try_submit_score` variant returning the raw result for assertions.
fn try_submit(
    env: &Env,
    client: &LedgerLensScoreContractClient,
    wallet: &Address,
    pair: &Symbol,
    score: u32,
) -> Result<Result<(), soroban_sdk::ConversionError>, Result<Error, soroban_sdk::InvokeError>> {
    client.try_submit_score(
        &Vec::new(env),
        wallet,
        pair,
        &score,
        &false,
        &false,
        &env.ledger().timestamp(),
        &90,
        &1,
        &None,
    )
}

fn advance(env: &Env) {
    env.ledger().with_mut(|l| l.timestamp += COOLDOWN);
}

// ── Defaults ──────────────────────────────────────────────────────────────────

#[test]
fn test_default_policy_is_disabled() {
    let (_, client, _) = setup();
    let policy = client.get_score_floor_policy();
    assert!(!policy.enabled);
    assert_eq!(policy.high_water_mark, 80);
    assert_eq!(policy.floor_value, 20);
}

#[test]
fn test_historical_max_unset_returns_none() {
    let (env, client, _) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");
    assert_eq!(client.get_historical_max_score(&wallet, &pair), None);
}

// ── Historical max tracking ───────────────────────────────────────────────────

#[test]
fn test_historical_max_tracks_running_peak() {
    let (env, client, _) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    submit(&env, &client, &wallet, &pair, 40);
    assert_eq!(client.get_historical_max_score(&wallet, &pair), Some(40));

    advance(&env);
    submit(&env, &client, &wallet, &pair, 90);
    assert_eq!(client.get_historical_max_score(&wallet, &pair), Some(90));

    // A subsequent lower score must not lower the recorded peak.
    advance(&env);
    submit(&env, &client, &wallet, &pair, 55);
    assert_eq!(client.get_historical_max_score(&wallet, &pair), Some(90));
}

// ── Issue #245 — Option<u32> historical max getter ──────────────────────────────

#[test]
fn test_historical_max_none_until_first_score_then_some() {
    let (env, client, _) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    // Unset → None (distinguishable from a recorded peak).
    assert_eq!(client.get_historical_max_score(&wallet, &pair), None);

    // After the first accepted score → Some(peak).
    submit(&env, &client, &wallet, &pair, 60);
    assert_eq!(client.get_historical_max_score(&wallet, &pair), Some(60));
}

// ── Policy validation ─────────────────────────────────────────────────────────

#[test]
fn test_set_policy_rejects_low_high_water_mark() {
    let (env, client, _) = setup();
    let result = client.try_set_score_floor_policy(&Vec::new(&env), &true, &49, &20);
    assert_eq!(result, Err(Ok(Error::InvalidScoreFloorPolicy)));
}

#[test]
fn test_set_policy_rejects_high_high_water_mark() {
    let (env, client, _) = setup();
    let result = client.try_set_score_floor_policy(&Vec::new(&env), &true, &101, &20);
    assert_eq!(result, Err(Ok(Error::InvalidScoreFloorPolicy)));
}

#[test]
fn test_set_policy_rejects_floor_at_or_above_high_water_mark() {
    let (env, client, _) = setup();
    // floor == high_water_mark is invalid (must be strictly below).
    let result = client.try_set_score_floor_policy(&Vec::new(&env), &true, &80, &80);
    assert_eq!(result, Err(Ok(Error::InvalidScoreFloorPolicy)));
}

#[test]
fn test_set_policy_accepts_boundary_values() {
    let (env, client, _) = setup();
    // high_water_mark at both bounds, floor just below it.
    client.set_score_floor_policy(&Vec::new(&env), &true, &50, &49);
    assert_eq!(client.get_score_floor_policy().high_water_mark, 50);
    client.set_score_floor_policy(&Vec::new(&env), &true, &100, &0);
    let policy = client.get_score_floor_policy();
    assert_eq!(policy.high_water_mark, 100);
    assert_eq!(policy.floor_value, 0);
}

// ── Floor disabled (acceptance criterion) ─────────────────────────────────────

#[test]
fn test_floor_disabled_allows_zeroing() {
    let (env, client, _) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    // Drive the historical peak above the default danger level...
    submit(&env, &client, &wallet, &pair, 95);
    // ...but the policy is disabled, so a zeroing submission is accepted.
    advance(&env);
    assert_eq!(try_submit(&env, &client, &wallet, &pair, 0), Ok(Ok(())));
    assert_eq!(client.get_score(&wallet, &pair).score, 0);
}

// ── Below high-water mark (acceptance criterion) ──────────────────────────────

#[test]
fn test_below_high_water_mark_no_floor() {
    let (env, client, _) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    client.set_score_floor_policy(&Vec::new(&env), &true, &80, &20);

    // Peak only ever reaches 70 — below the 80 high-water mark, so the floor
    // never applies and a low score is accepted.
    submit(&env, &client, &wallet, &pair, 70);
    advance(&env);
    assert_eq!(try_submit(&env, &client, &wallet, &pair, 5), Ok(Ok(())));
    assert_eq!(client.get_score(&wallet, &pair).score, 5);
}

// ── Above high-water mark, compliant score (acceptance criterion) ─────────────

#[test]
fn test_above_high_water_mark_compliant_accepted() {
    let (env, client, _) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    client.set_score_floor_policy(&Vec::new(&env), &true, &80, &20);

    submit(&env, &client, &wallet, &pair, 90); // peak crosses the danger level
    advance(&env);
    // 25 >= floor (20): a downward revision that respects the floor is allowed.
    assert_eq!(try_submit(&env, &client, &wallet, &pair, 25), Ok(Ok(())));
    assert_eq!(client.get_score(&wallet, &pair).score, 25);
}

#[test]
fn test_floor_value_itself_is_accepted() {
    let (env, client, _) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    client.set_score_floor_policy(&Vec::new(&env), &true, &80, &20);
    submit(&env, &client, &wallet, &pair, 88);
    advance(&env);
    // Exactly the floor value is permitted — only values *below* it are blocked.
    assert_eq!(try_submit(&env, &client, &wallet, &pair, 20), Ok(Ok(())));
}

// ── Above high-water mark, sub-floor score (acceptance criterion) ─────────────

#[test]
fn test_above_high_water_mark_sub_floor_rejected() {
    let (env, client, _) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    client.set_score_floor_policy(&Vec::new(&env), &true, &80, &20);

    submit(&env, &client, &wallet, &pair, 90); // historical peak = 90
    advance(&env);

    // Attempt to launder the wallet by submitting 1 — below the floor of 20.
    assert_eq!(try_submit(&env, &client, &wallet, &pair, 1), Err(Ok(Error::BelowScoreFloor)));
    // The stored score is unchanged; the laundering attempt did not land.
    assert_eq!(client.get_score(&wallet, &pair).score, 90);
}

#[test]
fn test_rejected_submission_leaves_cooldown_untouched() {
    let (env, client, _) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    client.set_score_floor_policy(&Vec::new(&env), &true, &80, &20);
    submit(&env, &client, &wallet, &pair, 90);
    let last_submit = client.get_last_submit_time(&wallet, &pair);

    advance(&env);
    // Blocked by the floor — must not advance the last-submit timestamp.
    assert_eq!(try_submit(&env, &client, &wallet, &pair, 0), Err(Ok(Error::BelowScoreFloor)));
    assert_eq!(client.get_last_submit_time(&wallet, &pair), last_submit);
}

// ── Emergency override (acceptance criterion) ─────────────────────────────────

#[test]
fn test_override_allows_sub_floor_submission() {
    let (env, client, admin) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    client.set_score_floor_policy(&Vec::new(&env), &true, &80, &20);
    submit(&env, &client, &wallet, &pair, 90);

    advance(&env);
    // Sub-floor blocked first...
    assert_eq!(try_submit(&env, &client, &wallet, &pair, 0), Err(Ok(Error::BelowScoreFloor)));

    // ...admin authorises an emergency override...
    let _ = admin;
    client.override_score_floor(&Vec::new(&env), &wallet, &pair);
    assert_eq!(client.get_historical_max_score(&wallet, &pair), None);

    // ...and the next sub-floor submission is now accepted.
    advance(&env);
    assert_eq!(try_submit(&env, &client, &wallet, &pair, 0), Ok(Ok(())));
    assert_eq!(client.get_score(&wallet, &pair).score, 0);
}

// ── Per-pair isolation ────────────────────────────────────────────────────────

#[test]
fn test_floor_is_per_wallet_pair() {
    let (env, client, _) = setup();
    let wallet = Address::generate(&env);
    let high_pair = symbol_short!("XLM_USDC");
    let fresh_pair = symbol_short!("XLM_BTC");

    client.set_score_floor_policy(&Vec::new(&env), &true, &80, &20);
    submit(&env, &client, &wallet, &high_pair, 90);

    // A different pair for the same wallet has no high-water history, so a
    // low score is accepted there.
    assert_eq!(try_submit(&env, &client, &wallet, &fresh_pair, 0), Ok(Ok(())));
}

// ── Batch enforcement ─────────────────────────────────────────────────────────

#[test]
fn test_batch_rejects_sub_floor_entry() {
    use crate::ScoreSubmission;

    let (env, client, _) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    client.set_score_floor_policy(&Vec::new(&env), &true, &80, &20);
    submit(&env, &client, &wallet, &pair, 90); // peak = 90
    advance(&env);

    let mut batch: Vec<ScoreSubmission> = Vec::new(&env);
    batch.push_back(ScoreSubmission {
        wallet: wallet.clone(),
        asset_pair: pair.clone(),
        score: 2, // below the floor of 20
        benford_flag: false,
        ml_flag: false,
        timestamp: env.ledger().timestamp(),
        confidence: 90,
        model_version: 1,
    });

    let result = client.submit_scores_batch(&batch);
    assert_eq!(result.accepted_count, 0);
    assert_eq!(result.rejected_count, 1);
    assert_eq!(result.results.get(0).unwrap().rejection_code, Error::BelowScoreFloor as u32);
    // Stored score unchanged.
    assert_eq!(client.get_score(&wallet, &pair).score, 90);
}
