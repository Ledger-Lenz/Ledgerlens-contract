//! Tests for the time-locked upgrade governance mechanism.
//!
//! Time is simulated with `env.ledger().with_mut(|l| l.timestamp = ...)`; the
//! contract derives every deadline from `env.ledger().timestamp()`, which is
//! deterministic and cannot be set by the caller on-chain.
//!
//! `execute_upgrade` invokes the real Soroban primitive
//! `env.deployer().update_current_contract_wasm(hash)`, which requires the
//! target WASM to already be uploaded to the ledger. We therefore upload a
//! real contract WASM fixture (this contract's own compiled output) and
//! propose *its* hash in the execute-path tests.

use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    Address, Bytes, BytesN, Env, Vec,
};

use crate::{
    constants::{DEFAULT_UPGRADE_DELAY_SECS, MAX_UPGRADE_DELAY_SECS, MIN_UPGRADE_DELAY_SECS},
    storage, Error, LedgerLensScoreContract, LedgerLensScoreContractClient,
};

/// Ledger timestamp the tests start from (an arbitrary fixed instant).
const START_TS: u64 = 1_700_000_000;

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

/// An arbitrary 32-byte hash for tests that never reach `execute_upgrade`
/// (propose/veto/getter paths don't require the WASM to actually exist).
fn dummy_hash(env: &Env) -> BytesN<32> {
    BytesN::from_array(env, &[7u8; 32])
}

/// Uploads a zero-byte WASM and returns its hash. The test host permits a
/// zero-byte upload (see `soroban_env_host` lifecycle) so `wasm_exists`
/// returns true and `update_current_contract_wasm` succeeds, without ever
/// instantiating the module — exactly what's needed to exercise the
/// `execute_upgrade` success path. We do not invoke the contract through the
/// client after upgrading (which would try to instantiate the empty module);
/// post-upgrade state is read directly via `env.as_contract`.
fn upload_uploadable_wasm(env: &Env) -> BytesN<32> {
    env.deployer().upload_contract_wasm(Bytes::new(env))
}

fn advance_to(env: &Env, ts: u64) {
    env.ledger().with_mut(|l| l.timestamp = ts);
}

// ── propose ────────────────────────────────────────────────────────────────────

#[test]
fn test_propose_upgrade_stores_proposal() {
    let (env, client, admin) = setup();
    let hash = dummy_hash(&env);

    client.propose_upgrade(&Vec::new(&env), &hash);

    let proposal = client.get_pending_upgrade();
    assert_eq!(proposal.new_wasm_hash, hash);
    assert_eq!(proposal.proposed_at, START_TS);
    assert_eq!(proposal.executable_after, START_TS + DEFAULT_UPGRADE_DELAY_SECS);
    assert_eq!(proposal.proposed_by, admin);
}

#[test]
fn test_double_propose_rejected() {
    let (env, client, _admin) = setup();
    let hash = dummy_hash(&env);

    client.propose_upgrade(&Vec::new(&env), &hash);
    let result = client.try_propose_upgrade(&Vec::new(&env), &hash);
    assert_eq!(result, Err(Ok(Error::UpgradeAlreadyPending)));
}

// ── execute ─────────────────────────────────────────────────────────────────────

#[test]
fn test_execute_before_delay_rejected() {
    let (env, client, _admin) = setup();
    let hash = dummy_hash(&env);

    client.propose_upgrade(&Vec::new(&env), &hash);

    // Still at START_TS, well before executable_after.
    let result = client.try_execute_upgrade(&Vec::new(&env));
    assert_eq!(result, Err(Ok(Error::UpgradeNotReady)));

    // One second short of the deadline is still not ready.
    advance_to(&env, START_TS + DEFAULT_UPGRADE_DELAY_SECS - 1);
    let result = client.try_execute_upgrade(&Vec::new(&env));
    assert_eq!(result, Err(Ok(Error::UpgradeNotReady)));
}

#[test]
fn test_execute_after_delay_succeeds() {
    let (env, client, _admin) = setup();

    let wasm_hash = upload_uploadable_wasm(&env);
    client.propose_upgrade(&Vec::new(&env), &wasm_hash);

    advance_to(&env, START_TS + DEFAULT_UPGRADE_DELAY_SECS);
    client.execute_upgrade(&Vec::new(&env)); // must not panic / error
}

#[test]
fn test_execute_upgrade_clears_proposal() {
    let (env, client, _admin) = setup();

    let wasm_hash = upload_uploadable_wasm(&env);
    client.propose_upgrade(&Vec::new(&env), &wasm_hash);

    advance_to(&env, START_TS + DEFAULT_UPGRADE_DELAY_SECS);
    client.execute_upgrade(&Vec::new(&env));

    // The contract's executable is now the (empty) upgrade target, so we read
    // storage directly inside the contract context rather than re-invoking the
    // client, which would try to instantiate the empty module.
    let cleared = env.as_contract(&client.address, || storage::get_pending_upgrade(&env).is_none());
    assert!(cleared, "PendingUpgrade must be cleared after execute_upgrade");
}

#[test]
fn test_execute_without_pending_rejected() {
    let (env, client, _admin) = setup();
    let result = client.try_execute_upgrade(&Vec::new(&env));
    assert_eq!(result, Err(Ok(Error::NoPendingUpgrade)));
}

// ── veto ─────────────────────────────────────────────────────────────────────────

#[test]
fn test_veto_clears_pending_upgrade() {
    let (env, client, _admin) = setup();
    let hash = dummy_hash(&env);

    client.propose_upgrade(&Vec::new(&env), &hash);
    client.veto_upgrade(&Vec::new(&env));

    let result = client.try_get_pending_upgrade();
    assert_eq!(result, Err(Ok(Error::NoPendingUpgrade)));
}

#[test]
fn test_veto_without_pending_rejected() {
    let (env, client, _admin) = setup();
    let result = client.try_veto_upgrade(&Vec::new(&env));
    assert_eq!(result, Err(Ok(Error::NoPendingUpgrade)));
}

#[test]
fn test_can_repropose_after_veto() {
    let (env, client, _admin) = setup();
    let hash = dummy_hash(&env);

    client.propose_upgrade(&Vec::new(&env), &hash);
    client.veto_upgrade(&Vec::new(&env));
    // Vetoing frees the slot, so a fresh proposal is accepted.
    client.propose_upgrade(&Vec::new(&env), &hash);
    assert_eq!(client.get_pending_upgrade().new_wasm_hash, hash);
}

// ── get_pending_upgrade ──────────────────────────────────────────────────────────

#[test]
fn test_get_pending_upgrade_no_proposal() {
    let (_env, client, _admin) = setup();
    let result = client.try_get_pending_upgrade();
    assert_eq!(result, Err(Ok(Error::NoPendingUpgrade)));
}

// ── delay configuration ──────────────────────────────────────────────────────────

#[test]
fn test_default_upgrade_delay_is_min() {
    let (_env, client, _admin) = setup();
    assert_eq!(client.get_upgrade_delay(), DEFAULT_UPGRADE_DELAY_SECS);
    assert_eq!(DEFAULT_UPGRADE_DELAY_SECS, MIN_UPGRADE_DELAY_SECS);
}

#[test]
fn test_set_upgrade_delay_within_bounds() {
    let (env, client, _admin) = setup();

    // Min, max, and an interior value are all accepted.
    client.set_upgrade_delay(&Vec::new(&env), &MIN_UPGRADE_DELAY_SECS);
    assert_eq!(client.get_upgrade_delay(), MIN_UPGRADE_DELAY_SECS);

    client.set_upgrade_delay(&Vec::new(&env), &MAX_UPGRADE_DELAY_SECS);
    assert_eq!(client.get_upgrade_delay(), MAX_UPGRADE_DELAY_SECS);

    let mid = (MIN_UPGRADE_DELAY_SECS + MAX_UPGRADE_DELAY_SECS) / 2;
    client.set_upgrade_delay(&Vec::new(&env), &mid);
    assert_eq!(client.get_upgrade_delay(), mid);
}

#[test]
fn test_upgrade_delay_below_min_rejected() {
    let (env, client, _admin) = setup();
    assert_eq!(
        client.try_set_upgrade_delay(&Vec::new(&env), &0),
        Err(Ok(Error::InvalidUpgradeDelay))
    );
    assert_eq!(
        client.try_set_upgrade_delay(&Vec::new(&env), &(MIN_UPGRADE_DELAY_SECS - 1)),
        Err(Ok(Error::InvalidUpgradeDelay))
    );
}

#[test]
fn test_upgrade_delay_above_max_rejected() {
    let (env, client, _admin) = setup();
    assert_eq!(
        client.try_set_upgrade_delay(&Vec::new(&env), &(MAX_UPGRADE_DELAY_SECS + 1)),
        Err(Ok(Error::InvalidUpgradeDelay))
    );
}

#[test]
fn test_configured_delay_applied_to_proposal() {
    let (env, client, _admin) = setup();
    let hash = dummy_hash(&env);

    // Raise the delay, then confirm a new proposal uses the new value.
    client.set_upgrade_delay(&Vec::new(&env), &MAX_UPGRADE_DELAY_SECS);
    client.propose_upgrade(&Vec::new(&env), &hash);

    let proposal = client.get_pending_upgrade();
    assert_eq!(proposal.executable_after, START_TS + MAX_UPGRADE_DELAY_SECS);
}
