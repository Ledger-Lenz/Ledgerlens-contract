#![cfg(test)]

use soroban_sdk::{
    contract, contractimpl, contracttype, symbol_short,
    testutils::{Address as _, Events as _, Ledger as _},
    Address, Env, IntoVal, Symbol, Vec,
};

use crate::{Error, LedgerLensScoreContract, LedgerLensScoreContractClient, RiskScore};

#[contracttype]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum HookMode {
    Succeed,
    Panic,
    Expensive,
    Reenter,
}

#[contracttype]
#[derive(Clone)]
enum HookKey {
    LedgerLens,
    Mode,
    Calls,
    ReentryBlocked,
    WorkResult,
}

#[contract]
struct TestScoreHook;

#[contractimpl]
impl TestScoreHook {
    pub fn initialize(env: Env, ledgerlens: Address, mode: HookMode) {
        env.storage().instance().set(&HookKey::LedgerLens, &ledgerlens);
        env.storage().instance().set(&HookKey::Mode, &mode);
        env.storage().instance().set(&HookKey::Calls, &0u32);
    }

    pub fn set_mode(env: Env, mode: HookMode) {
        env.storage().instance().set(&HookKey::Mode, &mode);
    }

    pub fn calls(env: Env) -> u32 {
        env.storage().instance().get(&HookKey::Calls).unwrap_or(0)
    }

    pub fn reentry_was_blocked(env: Env) -> bool {
        env.storage().instance().get(&HookKey::ReentryBlocked).unwrap_or(false)
    }

    pub fn on_score_change(
        env: Env,
        wallet: Address,
        asset_pair: Symbol,
        _score: RiskScore,
        _score_count: u32,
    ) {
        let calls: u32 = env.storage().instance().get(&HookKey::Calls).unwrap_or(0);
        env.storage().instance().set(&HookKey::Calls, &calls.saturating_add(1));
        let mode: HookMode = env.storage().instance().get(&HookKey::Mode).unwrap();
        match mode {
            HookMode::Succeed => {}
            HookMode::Panic => panic!("intentional hook failure"),
            HookMode::Expensive => {
                let mut value = 0u64;
                for i in 0..10_000u64 {
                    value = value.wrapping_add(i.rotate_left((i % 63) as u32));
                }
                env.storage().instance().set(&HookKey::WorkResult, &value);
            }
            HookMode::Reenter => {
                let ledgerlens: Address = env.storage().instance().get(&HookKey::LedgerLens).unwrap();
                let client = LedgerLensScoreContractClient::new(&env, &ledgerlens);
                let result = client.try_submit_score(
                    &Vec::new(&env),
                    &wallet,
                    &asset_pair,
                    &99,
                    &false,
                    &false,
                    &env.ledger().timestamp().saturating_add(1),
                    &90,
                    &1,
                    &None,
                );
                let blocked = matches!(result, Err(Ok(Error::ScoreHookDispatchInProgress)));
                env.storage().instance().set(&HookKey::ReentryBlocked, &blocked);
            }
        }
    }
}

struct Setup {
    env: Env,
    client: LedgerLensScoreContractClient<'static>,
    contract_id: Address,
    service: Address,
}

fn setup() -> Setup {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let service = Address::generate(&env);
    client.initialize(&admin, &service);
    let client: LedgerLensScoreContractClient<'static> =
        unsafe { core::mem::transmute(client) };
    Setup { env, client, contract_id, service }
}

fn add_hook(setup: &Setup, wallet: &Address, pair: &Symbol, mode: HookMode) -> Address {
    let hook_id = setup.env.register_contract(None, TestScoreHook);
    let hook = TestScoreHookClient::new(&setup.env, &hook_id);
    hook.initialize(&setup.contract_id, &mode);
    setup.client.register_score_change_hook(&hook_id, wallet, pair).unwrap();
    hook_id
}

fn submit(setup: &Setup, wallet: &Address, pair: &Symbol, score: u32) -> Result<(), Error> {
    setup.client.try_submit_score(
        &Vec::new(&setup.env),
        wallet,
        pair,
        &score,
        &false,
        &false,
        &setup.env.ledger().timestamp().saturating_add(1),
        &90,
        &1,
        &None,
    )
    .map_err(|error| error.unwrap())
}

fn dispatch(setup: &Setup, wallet: &Address, pair: &Symbol) -> Result<u32, Error> {
    setup.client
        .try_dispatch_score_change_hooks(&Address::generate(&setup.env), wallet, pair)
        .map_err(|error| error.unwrap())
}

#[test]
fn panic_hook_isolated_and_authoritative_submission_state_unchanged() {
    let setup = setup();
    let wallet = Address::generate(&setup.env);
    let pair = symbol_short!("XLM_USDC");
    let hook_id = add_hook(&setup, &wallet, &pair, HookMode::Panic);

    assert_eq!(submit(&setup, &wallet, &pair, 42), Ok(()));
    let before = setup.client.get_score(&wallet, &pair);
    assert_eq!(dispatch(&setup, &wallet, &pair), Ok(1));

    assert_eq!(setup.client.get_score(&wallet, &pair), before);
    assert_eq!(setup.client.get_score_count(&wallet, &pair), 1);
    assert_eq!(TestScoreHookClient::new(&setup.env, &hook_id).calls(), 0);
    assert_eq!(setup.client.get_score_change_hooks(&wallet, &pair).get(0).unwrap().consecutive_failures, 1);
}

#[test]
fn expensive_hook_is_bounded_to_one_call_per_dispatch() {
    let setup = setup();
    let pair = symbol_short!("XLM_USDC");
    let first_wallet = Address::generate(&setup.env);
    let second_wallet = Address::generate(&setup.env);
    let first_hook = add_hook(&setup, &first_wallet, &pair, HookMode::Expensive);
    let second_hook = add_hook(&setup, &second_wallet, &pair, HookMode::Expensive);

    assert_eq!(submit(&setup, &first_wallet, &pair, 21), Ok(()));
    assert_eq!(submit(&setup, &second_wallet, &pair, 22), Ok(()));
    assert_eq!(dispatch(&setup, &first_wallet, &pair), Ok(1));
    assert_eq!(TestScoreHookClient::new(&setup.env, &first_hook).calls(), 1);
    assert_eq!(TestScoreHookClient::new(&setup.env, &second_hook).calls(), 0);
    assert_eq!(dispatch(&setup, &second_wallet, &pair), Ok(1));
    assert_eq!(TestScoreHookClient::new(&setup.env, &second_hook).calls(), 1);
}

#[test]
fn reentrant_dispatch_is_blocked_without_changing_score() {
    let setup = setup();
    let wallet = Address::generate(&setup.env);
    let pair = symbol_short!("XLM_USDC");
    let hook_id = add_hook(&setup, &wallet, &pair, HookMode::Reenter);
    assert_eq!(submit(&setup, &wallet, &pair, 57), Ok(()));
    let before = setup.client.get_score(&wallet, &pair);

    assert_eq!(dispatch(&setup, &wallet, &pair), Ok(1));
    assert!(TestScoreHookClient::new(&setup.env, &hook_id).reentry_was_blocked());
    assert_eq!(setup.client.get_score(&wallet, &pair), before);
    assert_eq!(setup.client.get_score_count(&wallet, &pair), 1);
}

#[test]
fn hook_auto_disables_on_exact_consecutive_failure_threshold_and_emits_event() {
    let setup = setup();
    setup.client.set_score_hook_failure_threshold(&Vec::new(&setup.env), &2).unwrap();
    let wallet = Address::generate(&setup.env);
    let pair = symbol_short!("XLM_USDC");
    let hook_id = add_hook(&setup, &wallet, &pair, HookMode::Panic);

    assert_eq!(submit(&setup, &wallet, &pair, 30), Ok(()));
    assert_eq!(dispatch(&setup, &wallet, &pair), Ok(1));
    let after_first = setup.client.get_score_change_hooks(&wallet, &pair).get(0).unwrap();
    assert!(after_first.enabled);
    assert_eq!(after_first.consecutive_failures, 1);

    setup.env.ledger().with_mut(|ledger| ledger.timestamp += 3_601);
    assert_eq!(submit(&setup, &wallet, &pair, 31), Ok(()));
    assert_eq!(dispatch(&setup, &wallet, &pair), Ok(1));
    let after_second = setup.client.get_score_change_hooks(&wallet, &pair).get(0).unwrap();
    assert!(!after_second.enabled);
    assert_eq!(after_second.consecutive_failures, 2);
    assert_eq!(TestScoreHookClient::new(&setup.env, &hook_id).calls(), 0);

    let expected_topics = (symbol_short!("hook_off"), 1u32, hook_id.clone()).into_val(&setup.env);
    assert!(setup.env.events().all().iter().any(|(_, topics, _)| topics == expected_topics));
}

#[test]
fn pair_and_global_registration_limits_are_hard() {
    let setup = setup();
    let pair = symbol_short!("XLM_USDC");
    let wallet = Address::generate(&setup.env);
    for _ in 0..crate::constants::MAX_SCORE_CHANGE_HOOKS_PER_PAIR {
        add_hook(&setup, &wallet, &pair, HookMode::Succeed);
    }
    let overflow_hook = setup.env.register_contract(None, TestScoreHook);
    TestScoreHookClient::new(&setup.env, &overflow_hook).initialize(&setup.contract_id, &HookMode::Succeed);
    assert_eq!(
        setup.client.try_register_score_change_hook(&overflow_hook, &wallet, &pair),
        Err(Ok(Error::ScoreHookLimitReached)),
    );

    let global_setup = setup();
    let global_pair = symbol_short!("XLM_USDC");
    let global_hook = global_setup.env.register_contract(None, TestScoreHook);
    TestScoreHookClient::new(&global_setup.env, &global_hook)
        .initialize(&global_setup.contract_id, &HookMode::Succeed);
    for _ in 0..crate::constants::MAX_SCORE_CHANGE_HOOKS_GLOBAL {
        let unique_wallet = Address::generate(&global_setup.env);
        global_setup
            .client
            .register_score_change_hook(&global_hook, &unique_wallet, &global_pair)
            .unwrap();
    }
    let final_wallet = Address::generate(&global_setup.env);
    assert_eq!(
        global_setup
            .client
            .try_register_score_change_hook(&global_hook, &final_wallet, &global_pair),
        Err(Ok(Error::ScoreHookLimitReached)),
    );
}

#[test]
fn registration_requires_consumer_authentication() {
    let setup = setup();
    let consumer = Address::generate(&setup.env);
    let wallet = Address::generate(&setup.env);
    let pair = symbol_short!("XLM_USDC");
    let result = setup
        .client
        .mock_auths(&[])
        .try_register_score_change_hook(&consumer, &wallet, &pair);
    assert!(result.is_err());
}