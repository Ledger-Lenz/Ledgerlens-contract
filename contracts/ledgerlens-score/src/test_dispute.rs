//! Tests for the stake-backed score dispute mechanism:
//! `open_score_dispute`, `resolve_dispute_admin`, `resolve_dispute_timeout`,
//! and `get_open_disputes`.
//!
//! Escrow uses `env.register_stellar_asset_contract_v2` so the contract
//! exercises the real SEP-41 `token::TokenClient::transfer` path for both the
//! inbound bond and the outbound bond/bonus payout.

use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Events as _, Ledger as _},
    token::{StellarAssetClient, TokenClient},
    Address, Bytes, Env, Symbol, Vec,
};

use crate::{Error, LedgerLensScoreContract, LedgerLensScoreContractClient};

const CHALLENGE_PERIOD_SECS: u64 = 604_800; // DISPUTE_CHALLENGE_PERIOD_SECS
const BONUS_PCT: i128 = 10; // DISPUTE_BONUS_PCT

struct Fixture<'a> {
    env: Env,
    client: LedgerLensScoreContractClient<'a>,
    admin: Address,
    service: Address,
    challenger: Address,
    token: Address,
    contract_id: Address,
    pair: Symbol,
}

/// Spins up the contract with a configured fee token. The contract is funded
/// with `contract_reserve` stroops (the fee reserve that backs timeout bonuses)
/// and the challenger wallet is funded with `challenger_funds` stroops.
fn setup<'a>(contract_reserve: i128, challenger_funds: i128) -> Fixture<'a> {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let service = Address::generate(&env);
    client.initialize(&admin, &service);

    let issuer = Address::generate(&env);
    let sac = env.register_stellar_asset_contract_v2(issuer);
    let token = sac.address();
    client.set_fee_token(&token);

    let minter = StellarAssetClient::new(&env, &token);
    if contract_reserve > 0 {
        minter.mint(&contract_id, &contract_reserve);
    }
    let challenger = Address::generate(&env);
    if challenger_funds > 0 {
        minter.mint(&challenger, &challenger_funds);
    }

    Fixture {
        env,
        client,
        admin,
        service,
        challenger,
        token,
        contract_id,
        pair: symbol_short!("XLM_USDC"),
    }
}

/// Helper to execute commit-reveal and open a dispute for a challenger & wallet & pair.
fn open_dispute(f: &Fixture, challenger: &Address, wallet: &Address, pair: &Symbol, bond: i128) {
    let salt = Bytes::from_array(&f.env, &[1u8; 16]);
    let mut commit_bytes = Bytes::new(&f.env);
    commit_bytes.extend_from_slice(&bond.to_le_bytes());
    commit_bytes.extend_from_slice(&[1u8; 16]);
    f.client.commit_dispute_bond(challenger, wallet, pair, &commit_bytes);
    f.client.open_score_dispute(challenger, wallet, pair, &bond, &salt);
}

/// Helper to try to commit-reveal and open a dispute, returning the Result.
fn try_open_dispute(
    f: &Fixture,
    challenger: &Address,
    wallet: &Address,
    pair: &Symbol,
    bond: i128,
) -> Result<Result<(), soroban_sdk::ConversionError>, Result<Error, soroban_sdk::InvokeError>> {
    let salt = Bytes::from_array(&f.env, &[1u8; 16]);
    let mut commit_bytes = Bytes::new(&f.env);
    commit_bytes.extend_from_slice(&bond.to_le_bytes());
    commit_bytes.extend_from_slice(&[1u8; 16]);
    let _ = f.client.try_commit_dispute_bond(challenger, wallet, pair, &commit_bytes);
    f.client.try_open_score_dispute(challenger, wallet, pair, &bond, &salt)
}

/// Seeds a score for `(challenger, pair)` so there is something to dispute.
fn seed_score(f: &Fixture, score: u32) {
    f.client.submit_score(
        &Vec::new(&f.env),
        &f.challenger,
        &f.pair,
        &score,
        &false,
        &false,
        &1,
        &90,
        &1,
        &None,
    );
}

// ── open_score_dispute ──────────────────────────────────────────────────────

#[test]
fn test_open_dispute_escrows_bond_and_lists_it() {
    let f = setup(1_000_000, 1_000_000);
    seed_score(&f, 80);
    let token = TokenClient::new(&f.env, &f.token);
    let bond: i128 = 10_000;

    open_dispute(&f, &f.challenger, &f.challenger, &f.pair, bond);

    // Bond moved from challenger into the contract escrow.
    assert_eq!(token.balance(&f.challenger), 1_000_000 - bond);
    assert_eq!(token.balance(&f.contract_id), 1_000_000 + bond);

    // Dispute is listed with the correct deadline.
    let open = f.client.get_open_disputes();
    assert_eq!(open.len(), 1);
    let (w, p, deadline) = open.get(0).unwrap();
    assert_eq!(w, f.challenger);
    assert_eq!(p, f.pair);
    assert_eq!(deadline, f.env.ledger().timestamp() + CHALLENGE_PERIOD_SECS);
}

#[test]
fn test_open_dispute_zero_bond_rejected() {
    let f = setup(1_000_000, 1_000_000);
    let res = try_open_dispute(&f, &f.challenger, &f.challenger, &f.pair, 0);
    assert_eq!(res, Err(Ok(Error::InvalidDisputeBond)));
}

#[test]
fn test_open_dispute_negative_bond_rejected() {
    let f = setup(1_000_000, 1_000_000);
    let res = try_open_dispute(&f, &f.challenger, &f.challenger, &f.pair, -5);
    assert_eq!(res, Err(Ok(Error::InvalidDisputeBond)));
}

#[test]
fn test_open_dispute_duplicate_rejected() {
    let f = setup(1_000_000, 1_000_000);
    open_dispute(&f, &f.challenger, &f.challenger, &f.pair, 10_000);
    let res = try_open_dispute(&f, &f.challenger, &f.challenger, &f.pair, 10_000);
    assert_eq!(res, Err(Ok(Error::DisputeAlreadyOpen)));
}

#[test]
fn test_open_dispute_fee_token_not_set() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let service = Address::generate(&env);
    client.initialize(&admin, &service);
    let wallet = Address::generate(&env);

    let salt = Bytes::from_array(&env, &[1u8; 16]);
    let mut commit_bytes = Bytes::new(&env);
    commit_bytes.extend_from_slice(&10_000i128.to_le_bytes());
    commit_bytes.extend_from_slice(&[1u8; 16]);
    let _ =
        client.try_commit_dispute_bond(&wallet, &wallet, &symbol_short!("XLM_USDC"), &commit_bytes);
    let res =
        client.try_open_score_dispute(&wallet, &wallet, &symbol_short!("XLM_USDC"), &10_000, &salt);
    assert_eq!(res, Err(Ok(Error::FeeTokenNotSet)));
}

#[test]
fn test_commit_dispute_bond_rejects_oversized_preimage_without_mutation() {
    let f = setup(1_000_000, 1_000_000);
    let oversized_pair = Symbol::new(&f.env, "PAIR123456");
    let oversized_preimage = Bytes::from_array(
        &f.env,
        &[9u8; crate::constants::MAX_DISPUTE_BOND_PREIMAGE_BYTES as usize + 1],
    );

    let commit_res = f.client.try_commit_dispute_bond(
        &f.challenger,
        &f.challenger,
        &oversized_pair,
        &oversized_preimage,
    );
    assert_eq!(commit_res, Err(Ok(Error::InvalidAttestation)));

    let reveal_salt = Bytes::from_array(&f.env, &[1u8; 16]);
    let reveal_res = f.client.try_open_score_dispute(
        &f.challenger,
        &f.challenger,
        &oversized_pair,
        &10_000,
        &reveal_salt,
    );
    assert_eq!(reveal_res, Err(Ok(Error::InvalidAttestation)));
}

#[test]
fn test_open_dispute_rejects_oversized_reveal_input_without_clearing_commitment() {
    let f = setup(1_000_000, 1_000_000);
    seed_score(&f, 80);

    let valid_salt = Bytes::from_array(&f.env, &[1u8; 16]);
    let mut commit_bytes = Bytes::new(&f.env);
    commit_bytes.extend_from_slice(&10_000i128.to_le_bytes());
    commit_bytes.extend_from_slice(&[1u8; 16]);
    f.client.commit_dispute_bond(&f.challenger, &f.challenger, &f.pair, &commit_bytes);

    let oversized_salt = Bytes::from_array(
        &f.env,
        &[2u8; crate::constants::MAX_DISPUTE_BOND_SALT_BYTES as usize + 1],
    );
    let oversized_res = f.client.try_open_score_dispute(
        &f.challenger,
        &f.challenger,
        &f.pair,
        &10_000,
        &oversized_salt,
    );
    assert_eq!(oversized_res, Err(Ok(Error::InvalidAttestation)));

    f.client.open_score_dispute(&f.challenger, &f.challenger, &f.pair, &10_000, &valid_salt);
    assert_eq!(f.client.get_open_disputes().len(), 1);
}

#[test]
fn test_open_dispute_per_actor_cap_exceeded_rejected() {
    let f = setup(10_000_000, 10_000_000);

    let pairs = [
        symbol_short!("PAIR1"),
        symbol_short!("PAIR2"),
        symbol_short!("PAIR3"),
        symbol_short!("PAIR4"),
        symbol_short!("PAIR5"),
        symbol_short!("PAIR6"),
        symbol_short!("PAIR7"),
    ];

    // Challenger 1 opens 5 (MAX_DISPUTES_PER_ACTOR) disputes across different pairs.
    for pair in pairs.iter().take(5) {
        open_dispute(&f, &f.challenger, &f.challenger, pair, 1_000);
    }
    assert_eq!(f.client.get_open_disputes().len(), 5);

    // 6th dispute from Challenger 1 is rejected with ActorDisputeLimitExceeded (RateLimitExceeded).
    let res = try_open_dispute(&f, &f.challenger, &f.challenger, &pairs[5], 1_000);
    assert_eq!(res, Err(Ok(Error::RateLimitExceeded)));

    // Challenger 2 (distinct actor) can still open a dispute on PAIR6.
    let challenger2 = Address::generate(&f.env);
    StellarAssetClient::new(&f.env, &f.token).mint(&challenger2, &1_000_000);
    open_dispute(&f, &challenger2, &challenger2, &pairs[5], 1_000);
    assert_eq!(f.client.get_open_disputes().len(), 6);

    // Resolving 1 of Challenger 1's disputes liberates a slot for Challenger 1.
    f.client.resolve_dispute_admin(&Vec::new(&f.env), &f.challenger, &pairs[0], &25);
    assert_eq!(f.client.get_open_disputes().len(), 5);

    // Challenger 1 can now open a 5th dispute on PAIR7.
    open_dispute(&f, &f.challenger, &f.challenger, &pairs[6], 1_000);
    assert_eq!(f.client.get_open_disputes().len(), 6);
}

// ── resolve_dispute_admin ─────────────────────────────────────────────────────

#[test]
fn test_resolve_admin_returns_bond_and_corrects_score() {
    let f = setup(1_000_000, 1_000_000);
    seed_score(&f, 80);
    let token = TokenClient::new(&f.env, &f.token);
    let bond: i128 = 10_000;

    open_dispute(&f, &f.challenger, &f.challenger, &f.pair, bond);
    f.client.resolve_dispute_admin(&Vec::new(&f.env), &f.challenger, &f.pair, &25);

    // Bond fully returned, no bonus.
    assert_eq!(token.balance(&f.challenger), 1_000_000);
    assert_eq!(token.balance(&f.contract_id), 1_000_000);

    // Corrected score is live and the dispute is closed.
    assert_eq!(f.client.get_score(&f.challenger, &f.pair).score, 25);
    assert_eq!(f.client.get_open_disputes().len(), 0);
}

#[test]
fn test_resolve_admin_nonexistent_dispute_rejected() {
    let f = setup(1_000_000, 1_000_000);
    let res = f.client.try_resolve_dispute_admin(&Vec::new(&f.env), &f.challenger, &f.pair, &25);
    assert_eq!(res, Err(Ok(Error::DisputeNotFound)));
}

#[test]
fn test_resolve_admin_invalid_score_rejected() {
    let f = setup(1_000_000, 1_000_000);
    open_dispute(&f, &f.challenger, &f.challenger, &f.pair, 10_000);
    let res = f.client.try_resolve_dispute_admin(&Vec::new(&f.env), &f.challenger, &f.pair, &101);
    assert_eq!(res, Err(Ok(Error::InvalidScore)));
}

#[test]
fn test_resolve_admin_requires_m_of_n_auth() {
    let f = setup(1_000_000, 1_000_000);

    // Configure a 2-of-N admin set.
    let signer_a = Address::generate(&f.env);
    let signer_b = Address::generate(&f.env);
    f.client.add_admin_signer(&Vec::new(&f.env), &signer_a);
    f.client.add_admin_signer(&Vec::new(&f.env), &signer_b);
    f.client.set_admin_threshold(&Vec::new(&f.env), &2);

    open_dispute(&f, &f.challenger, &f.challenger, &f.pair, 10_000);

    // Too few signers → rejected even with mock_all_auths (count is checked
    // before any require_auth).
    let mut one = Vec::new(&f.env);
    one.push_back(signer_a.clone());
    let res = f.client.try_resolve_dispute_admin(&one, &f.challenger, &f.pair, &25);
    assert_eq!(res, Err(Ok(Error::InsufficientAdminSigners)));

    // Full quorum succeeds.
    let mut both = Vec::new(&f.env);
    both.push_back(signer_a);
    both.push_back(signer_b);
    f.client.resolve_dispute_admin(&both, &f.challenger, &f.pair, &25);
    assert_eq!(f.client.get_open_disputes().len(), 0);
}

// ── resolve_dispute_timeout ───────────────────────────────────────────────────

#[test]
fn test_resolve_timeout_returns_bond_with_bonus() {
    let f = setup(1_000_000, 1_000_000);
    seed_score(&f, 80);
    let token = TokenClient::new(&f.env, &f.token);
    let bond: i128 = 10_000;
    let bonus = bond * BONUS_PCT / 100;

    open_dispute(&f, &f.challenger, &f.challenger, &f.pair, bond);

    // Advance past the deadline.
    f.env.ledger().with_mut(|l| l.timestamp += CHALLENGE_PERIOD_SECS + 1);

    // Anyone can settle — not the challenger or admin.
    f.client.resolve_dispute_timeout(&f.challenger, &f.pair);

    // Challenger nets the bonus; the contract reserve funds it.
    assert_eq!(token.balance(&f.challenger), 1_000_000 + bonus);
    assert_eq!(token.balance(&f.contract_id), 1_000_000 - bonus);
    assert_eq!(f.client.get_open_disputes().len(), 0);
}

#[test]
fn test_resolve_timeout_before_deadline_rejected() {
    let f = setup(1_000_000, 1_000_000);
    open_dispute(&f, &f.challenger, &f.challenger, &f.pair, 10_000);

    let res = f.client.try_resolve_dispute_timeout(&f.challenger, &f.pair);
    assert_eq!(res, Err(Ok(Error::DisputeNotYetTimedOut)));
}

#[test]
fn test_resolve_timeout_nonexistent_dispute_rejected() {
    let f = setup(1_000_000, 1_000_000);
    let res = f.client.try_resolve_dispute_timeout(&f.challenger, &f.pair);
    assert_eq!(res, Err(Ok(Error::DisputeNotFound)));
}

// ── get_open_disputes ─────────────────────────────────────────────────────────

#[test]
fn test_get_open_disputes_empty_by_default() {
    let f = setup(1_000_000, 1_000_000);
    assert_eq!(f.client.get_open_disputes().len(), 0);
}

#[test]
fn test_get_open_disputes_tracks_multiple_pairs() {
    let f = setup(1_000_000, 1_000_000);
    let other = symbol_short!("BTC_USDC");

    open_dispute(&f, &f.challenger, &f.challenger, &f.pair, 10_000);
    open_dispute(&f, &f.challenger, &f.challenger, &other, 5_000);
    assert_eq!(f.client.get_open_disputes().len(), 2);

    // Resolving one removes only that entry.
    f.client.resolve_dispute_admin(&Vec::new(&f.env), &f.challenger, &f.pair, &25);
    let remaining = f.client.get_open_disputes();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining.get(0).unwrap().1, other);
}

#[test]
fn test_dispute_emits_events() {
    let f = setup(1_000_000, 1_000_000);
    open_dispute(&f, &f.challenger, &f.challenger, &f.pair, 10_000);
    // An event was published for the open.
    assert!(!f.env.events().all().is_empty());

    f.env.ledger().with_mut(|l| l.timestamp += CHALLENGE_PERIOD_SECS + 1);
    f.client.resolve_dispute_timeout(&f.challenger, &f.pair);
    assert!(!f.env.events().all().is_empty());

    // Silence unused-field warnings for fixture members not asserted here.
    let _ = (&f.admin, &f.service);
}
