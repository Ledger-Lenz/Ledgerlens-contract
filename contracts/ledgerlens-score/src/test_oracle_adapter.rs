#![cfg(test)]

use soroban_sdk::{
    contract, contractimpl, contracttype,
    symbol_short,
    testutils::{Address as _, Ledger as _},
    Address, Env, Symbol, Vec,
};

use crate::{LedgerLensScoreContract, LedgerLensScoreContractClient};

const START_TS: u64 = 1_700_000_000;

// ── Minimal mock oracle ───────────────────────────────────────────────────────

#[contracttype]
pub enum OracleKey {
    Price(Symbol),
}

#[contract]
pub struct MockOracle;

#[contractimpl]
impl MockOracle {
    pub fn set_price(env: Env, asset_pair: Symbol, price: i128) {
        env.storage().instance().set(&OracleKey::Price(asset_pair), &price);
    }
    pub fn get_price(env: Env, asset_pair: Symbol) -> i128 {
        env.storage().instance().get(&OracleKey::Price(asset_pair)).unwrap_or(0i128)
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn setup<'a>() -> (Env, LedgerLensScoreContractClient<'a>) {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|l| l.timestamp = START_TS);
    let cid = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &cid);
    client.initialize(&Address::generate(&env), &Address::generate(&env));
    (env, client)
}

fn deploy_oracle(env: &Env) -> Address {
    env.register_contract(None, MockOracle)
}

fn set_oracle_price(env: &Env, oracle: &Address, pair: &Symbol, price: i128) {
    let client = MockOracleClient::new(env, oracle);
    client.set_price(pair, &price);
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[test]
fn test_register_oracle_stores_address() {
    let (env, client) = setup();
    let oracle = deploy_oracle(&env);
    let pair = symbol_short!("XLM_USDC");

    assert!(client.get_registered_oracle(&pair).is_none());
    client.register_oracle(&Vec::new(&env), &pair, &oracle);
    assert_eq!(client.get_registered_oracle(&pair), Some(oracle));
}

#[test]
fn test_remove_oracle_clears_registration() {
    let (env, client) = setup();
    let oracle = deploy_oracle(&env);
    let pair = symbol_short!("XLM_USDC");

    client.register_oracle(&Vec::new(&env), &pair, &oracle);
    client.remove_oracle(&Vec::new(&env), &pair);
    assert!(client.get_registered_oracle(&pair).is_none());
}

#[test]
fn test_no_oracle_zero_confidence_floor() {
    let (env, client) = setup();
    let wallet = Address::generate(&env);
    let pair = symbol_short!("XLM_USDC");

    client.submit_score(&Vec::new(&env), &wallet, &pair, &50, &false, &false, &START_TS, &80, &1, &None);

    let eff = client.get_effective_score(&wallet, &pair).unwrap();
    assert_eq!(eff.confidence_floor, 0);
}

#[test]
fn test_oracle_zero_price_zero_floor() {
    let (env, client) = setup();
    let oracle = deploy_oracle(&env);
    let pair = symbol_short!("XLM_USDC");
    set_oracle_price(&env, &oracle, &pair, 0i128);
    client.register_oracle(&Vec::new(&env), &pair, &oracle);

    let wallet = Address::generate(&env);
    client.submit_score(&Vec::new(&env), &wallet, &pair, &50, &false, &false, &START_TS, &80, &1, &None);

    let eff = client.get_effective_score(&wallet, &pair).unwrap();
    assert_eq!(eff.confidence_floor, 0);
}

#[test]
fn test_oracle_high_price_raises_floor() {
    let (env, client) = setup();
    let oracle = deploy_oracle(&env);
    let pair = symbol_short!("XLM_USDC");
    // 1_000_000 / 20_000 = 50 (cap)
    set_oracle_price(&env, &oracle, &pair, 1_000_000i128);
    client.register_oracle(&Vec::new(&env), &pair, &oracle);

    let wallet = Address::generate(&env);
    client.submit_score(&Vec::new(&env), &wallet, &pair, &40, &false, &false, &START_TS, &90, &1, &None);

    let eff = client.get_effective_score(&wallet, &pair).unwrap();
    assert_eq!(eff.confidence_floor, 50);
    assert_eq!(eff.original_score, 40);
    assert_eq!(eff.original_confidence, 90);
}

#[test]
fn test_oracle_moderate_price_proportional_floor() {
    let (env, client) = setup();
    let oracle = deploy_oracle(&env);
    let pair = symbol_short!("XLM_USDC");
    // 200_000 / 20_000 = 10
    set_oracle_price(&env, &oracle, &pair, 200_000i128);
    client.register_oracle(&Vec::new(&env), &pair, &oracle);

    let wallet = Address::generate(&env);
    client.submit_score(&Vec::new(&env), &wallet, &pair, &60, &false, &false, &START_TS, &85, &1, &None);

    let eff = client.get_effective_score(&wallet, &pair).unwrap();
    assert_eq!(eff.confidence_floor, 10);
}
