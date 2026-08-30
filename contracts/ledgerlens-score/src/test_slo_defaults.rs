//! Guards the numeric defaults documented in `docs/slo-operational-targets.md`.
//!
//! If someone changes a default in `constants.rs` without updating the SLO
//! doc (or vice versa), these tests fail loudly instead of silently drifting.

use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    Address, Env,
};

use crate::{constants, LedgerLensScoreContract, LedgerLensScoreContractClient};

// Values below are copied from docs/slo-operational-targets.md — keep in sync.
const DOC_HEARTBEAT_ALERT_THRESHOLD_SECS: u64 = 3_600;
const DOC_STALENESS_WINDOW_SECS: u64 = 604_800;
const DOC_ORACLE_STALENESS_THRESHOLD_SECS: u64 = 3_600;
const DOC_FAILOVER_STALENESS_WINDOW_SECS: u64 = 3_600;

#[test]
fn documented_freshness_defaults_match_constants() {
    assert_eq!(
        constants::DEFAULT_HEARTBEAT_ALERT_THRESHOLD_SECS,
        DOC_HEARTBEAT_ALERT_THRESHOLD_SECS,
        "docs/slo-operational-targets.md heartbeat threshold is stale"
    );
    assert_eq!(
        constants::DEFAULT_STALENESS_WINDOW_SECS,
        DOC_STALENESS_WINDOW_SECS,
        "docs/slo-operational-targets.md score staleness window is stale"
    );
    assert_eq!(
        constants::DEFAULT_ORACLE_STALENESS_THRESHOLD_SECS,
        DOC_ORACLE_STALENESS_THRESHOLD_SECS,
        "docs/slo-operational-targets.md oracle staleness threshold is stale"
    );
    assert_eq!(
        constants::FAILOVER_STALENESS_WINDOW,
        DOC_FAILOVER_STALENESS_WINDOW_SECS,
        "docs/slo-operational-targets.md failover staleness window is stale"
    );
}

#[test]
fn fresh_service_reports_alive_and_gate_reports_unpaused() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|l| l.timestamp = 1_700_000_000);

    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let service = Address::generate(&env);
    client.initialize(&admin, &service);

    // Freshness SLI: a service that just initialized (heartbeat = now) must
    // read as alive, and the risk gate must start open (unpaused).
    assert!(client.is_service_alive());
    assert!(!client.is_paused());
}

#[test]
fn service_silent_past_heartbeat_threshold_reports_dead() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|l| l.timestamp = 1_700_000_000);

    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let service = Address::generate(&env);
    client.initialize(&admin, &service);
    client.ping_heartbeat();

    // Advance past the documented/default heartbeat alert threshold with no
    // further service activity — the freshness SLI must flip to "not alive"
    // so the paging alert in docs/slo-operational-targets.md has a real signal.
    env.ledger().with_mut(|l| {
        l.timestamp += constants::DEFAULT_HEARTBEAT_ALERT_THRESHOLD_SECS + 1;
    });

    assert!(!client.is_service_alive());
}
