#![cfg(test)]

use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Ledger as _},
    Address, Env, Vec,
};

use crate::{Error, LedgerLensScoreContract, LedgerLensScoreContractClient};
use proptest::prelude::*;

fn setup_fresh_env<'a>() -> (Env, LedgerLensScoreContractClient<'a>, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    env.budget().reset_unlimited();

    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let service = Address::generate(&env);

    env.ledger().with_mut(|l| l.timestamp = 100_000);
    client.initialize(&admin, &service);

    (env, client, admin, service)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1000))]

    /// Property 1: For any score sequence with velocity cap V, every accepted consecutive delta is <= V.
    #[test]
    fn prop_velocity_cap_accepts_only_valid_deltas(
        cap_v in 1u32..=50,
        initial_score in 0u32..=100,
        score_delta in 0u32..=100
    ) {
        let (env, client, admin, _service) = setup_fresh_env();

        client.set_score_velocity_cap(&Vec::from_array(&env, [admin.clone()]), &true, &cap_v);

        let wallet = Address::generate(&env);
        let asset_pair = symbol_short!("XLM_USDC");

        // Submit initial score
        client.submit_score(
            &Vec::new(&env),
            &wallet,
            &asset_pair,
            &initial_score,
            &false,
            &false,
            &1,
            &90,
            &1,
            &None,
        );

        // Advance exactly 1 hour: now cap_v points are allowed
        env.ledger().with_mut(|l| l.timestamp += 3600);

        let delta = if score_delta >= initial_score {
            score_delta - initial_score
        } else {
            initial_score - score_delta
        };

        if delta <= cap_v {
            // Should be accepted
            client.submit_score(
                &Vec::new(&env),
                &wallet,
                &asset_pair,
                &score_delta,
                &false,
                &false,
                &2,
                &90,
                &1,
                &None,
            );
            prop_assert_eq!(client.get_score(&wallet, &asset_pair).score, score_delta);
        } else {
            // Should be rejected
            let result = client.try_submit_score(
                &Vec::new(&env),
                &wallet,
                &asset_pair,
                &score_delta,
                &false,
                &false,
                &2,
                &90,
                &1,
                &None,
            );
            prop_assert_eq!(result, Err(Ok(Error::ScoreVelocityExceeded)));
        }
    }

    /// Property 2: A score submission that exceeds the velocity cap is rejected with ScoreVelocityExceeded.
    #[test]
    fn prop_velocity_cap_rejects_excessive_deltas(
        cap_v in 1u32..=49,
        initial_score in 0u32..=50
    ) {
        let (env, client, admin, _service) = setup_fresh_env();

        client.set_score_velocity_cap(&Vec::from_array(&env, [admin.clone()]), &true, &cap_v);

        let wallet = Address::generate(&env);
        let asset_pair = symbol_short!("XLM_USDC");

        // Submit initial score
        client.submit_score(
            &Vec::new(&env),
            &wallet,
            &asset_pair,
            &initial_score,
            &false,
            &false,
            &1,
            &90,
            &1,
            &None,
        );

        // Advance exactly 1 hour
        env.ledger().with_mut(|l| l.timestamp += 3600);

        // Calculate a score that exceeds the cap
        let new_score = (initial_score + cap_v + 1).min(100);
        prop_assume!(new_score != initial_score); // Ensure we have a delta

        let result = client.try_submit_score(
            &Vec::new(&env),
            &wallet,
            &asset_pair,
            &new_score,
            &false,
            &false,
            &2,
            &90,
            &1,
            &None,
        );
        prop_assert_eq!(result, Err(Ok(Error::ScoreVelocityExceeded)));
    }

    /// Property 3: After override_score_velocity_cap, the next submission succeeds regardless of the cap.
    #[test]
    fn prop_velocity_cap_override_allows_next_submission(
        cap_v in 1u32..=49,
        initial_score in 1u32..=50
    ) {
        let (env, client, admin, _service) = setup_fresh_env();

        client.set_score_velocity_cap(&Vec::from_array(&env, [admin.clone()]), &true, &cap_v);

        let wallet = Address::generate(&env);
        let asset_pair = symbol_short!("XLM_USDC");

        // Submit initial score
        client.submit_score(
            &Vec::new(&env),
            &wallet,
            &asset_pair,
            &initial_score,
            &false,
            &false,
            &1,
            &90,
            &1,
            &None,
        );

        env.ledger().with_mut(|l| l.timestamp += 3600);

        // Calculate a score that would normally exceed the cap
        let new_score = (initial_score + cap_v + 1).min(100);
        prop_assume!(new_score != initial_score);

        // Set override to bypass the cap
        client.override_score_velocity_cap(
            &Vec::from_array(&env, [admin.clone()]),
            &wallet,
            &asset_pair,
        );

        // Submit score that would exceed cap but is overridden
        client.submit_score(
            &Vec::new(&env),
            &wallet,
            &asset_pair,
            &new_score,
            &false,
            &false,
            &2,
            &90,
            &1,
            &None,
        );

        prop_assert_eq!(client.get_score(&wallet, &asset_pair).score, new_score);
    }
}

#[test]
fn test_velocity_cap_boundary_100_with_cap_1() {
    let env = Env::default();
    env.mock_all_auths();
    env.budget().reset_unlimited();
    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let service = Address::generate(&env);
    env.ledger().with_mut(|l| l.timestamp = 100_000);
    client.initialize(&admin, &service);

    client.set_score_velocity_cap(&Vec::from_array(&env, [admin.clone()]), &true, &1);

    let wallet = Address::generate(&env);
    let asset_pair = symbol_short!("XLM_USDC");

    // Set score to 100
    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &asset_pair,
        &100,
        &false,
        &false,
        &100_000,
        &90,
        &1,
        &None,
    );

    env.ledger().with_mut(|l| l.timestamp += 3600);

    // Security edge case: submitting the same score at the boundary (100 with cap 1) must not produce an overflow or rejection.
    // Resubmit same score (delta = 0, which is <= cap of 1)
    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &asset_pair,
        &100,
        &false,
        &false,
        &100_001,
        &90,
        &1,
        &None,
    );

    assert_eq!(client.get_score(&wallet, &asset_pair).score, 100);
}
