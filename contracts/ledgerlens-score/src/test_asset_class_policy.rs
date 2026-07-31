//! Fixtures for #725: per-asset-class risk-threshold policy profiles.
//!
//! Proves that `get_effective_risk_threshold` deterministically resolves to
//! the asset class's override when one exists, and safely falls back to the
//! global default when the pair is unclassified or its class has no override.

use soroban_sdk::{symbol_short, testutils::Address as _, Address, Env, Vec};

use crate::{Error, LedgerLensScoreContract, LedgerLensScoreContractClient};

fn setup<'a>() -> (Env, LedgerLensScoreContractClient<'a>, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let service = Address::generate(&env);
    client.initialize(&admin, &service);
    (env, client, admin)
}

#[test]
fn test_unclassified_pair_falls_back_to_global_default() {
    let (env, client, _admin) = setup();
    let pair = symbol_short!("XLM_USDC");
    assert_eq!(client.get_pair_asset_class(&pair), None);
    assert_eq!(client.get_effective_risk_threshold(&pair), client.get_risk_threshold());
}

#[test]
fn test_classified_pair_without_class_override_falls_back_to_global_default() {
    let (env, client, admin) = setup();
    let pair = symbol_short!("XLM_USDC");
    let stable = symbol_short!("stable");
    client.set_pair_asset_class(&Vec::from_array(&env, [admin.clone()]), &pair, &stable);
    assert_eq!(client.get_pair_asset_class(&pair), Some(stable));
    assert_eq!(client.get_effective_risk_threshold(&pair), client.get_risk_threshold());
}

#[test]
fn test_classified_pair_uses_class_override_deterministically() {
    let (env, client, admin) = setup();
    let pair = symbol_short!("XLM_USDC");
    let volatile = symbol_short!("volatile");
    let signers = Vec::from_array(&env, [admin.clone()]);
    client.set_pair_asset_class(&signers, &pair, &volatile);
    client.set_asset_class_policy(&signers, &volatile, &45);

    assert_eq!(client.get_effective_risk_threshold(&pair), 45);
    // Repeated lookups are deterministic.
    assert_eq!(client.get_effective_risk_threshold(&pair), 45);
}

#[test]
fn test_class_override_does_not_affect_other_classes_or_unclassified_pairs() {
    let (env, client, admin) = setup();
    let stable_pair = symbol_short!("USDC_USDT");
    let other_pair = symbol_short!("XLM_USDC");
    let stable = symbol_short!("stable");
    let signers = Vec::from_array(&env, [admin.clone()]);

    client.set_pair_asset_class(&signers, &stable_pair, &stable);
    client.set_asset_class_policy(&signers, &stable, &10);

    assert_eq!(client.get_effective_risk_threshold(&stable_pair), 10);
    assert_eq!(client.get_effective_risk_threshold(&other_pair), client.get_risk_threshold());
}

#[test]
fn test_set_asset_class_policy_rejects_threshold_above_100() {
    let (env, client, admin) = setup();
    let signers = Vec::from_array(&env, [admin.clone()]);
    let hivalue = symbol_short!("hivalue");
    assert_eq!(
        client.try_set_asset_class_policy(&signers, &hivalue, &101),
        Err(Ok(Error::InvalidScore))
    );
}
