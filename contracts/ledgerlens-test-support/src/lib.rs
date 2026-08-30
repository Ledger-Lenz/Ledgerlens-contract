use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    Address, Env,
};

pub fn test_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

pub fn test_env_with_unlimited_budget() -> Env {
    let env = test_env();
    env.budget().reset_unlimited();
    env
}

pub fn generate_score_roles(env: &Env) -> (Address, Address) {
    let admin = Address::generate(env);
    let service = Address::generate(env);
    (admin, service)
}

pub fn set_ledger_timestamp(env: &Env, timestamp: u64) {
    env.ledger().with_mut(|ledger| ledger.timestamp = timestamp);
}
