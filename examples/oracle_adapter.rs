//! Reference integration: registering a price-feed oracle with LedgerLens and
//! reading oracle-adjusted effective risk scores.
//!
//! This example demonstrates:
//! 1. Deploying a minimal oracle contract that implements `get_price(asset_pair) -> i128`.
//! 2. Registering it with LedgerLens via `register_oracle`.
//! 3. Calling `get_effective_score` and observing that the returned
//!    `confidence_floor` is elevated when the oracle reports a high price,
//!    signalling increased market uncertainty to the caller.
//!
//! Build it as part of the workspace:
//!
//! ```text
//! cargo build --example oracle_adapter -p ledgerlens-score
//! ```
//!
//! The key insight: `confidence_floor` in [`EffectiveRiskScore`] lets a
//! composable protocol know *at query time* that current market conditions
//! reduce confidence in the static stored score.  It is advisory — the stored
//! score and confidence are never mutated.

#![no_std]

use soroban_sdk::{contract, contractimpl, contracttype, Address, Env, Symbol};

/// Storage key used by the example oracle.
#[contracttype]
pub enum OracleKey {
    Price(Symbol),
}

/// Minimal price-feed oracle.
///
/// A production oracle would pull prices from an off-chain feed signed by a
/// trusted key. This example stores a price that the admin pushes on-chain,
/// sufficient to demonstrate the LedgerLens integration.
///
/// **The only requirement** LedgerLens places on a registered oracle is that
/// it exposes a `get_price(asset_pair: Symbol) -> i128` function callable via
/// cross-contract `invoke_contract`. This contract satisfies that interface.
#[contract]
pub struct ExamplePriceFeedOracle;

#[contractimpl]
impl ExamplePriceFeedOracle {
    /// Admin sets the latest price for an asset pair (production: feed via
    /// authenticated off-chain relayer).
    pub fn set_price(env: Env, asset_pair: Symbol, price: i128) {
        env.storage().instance().set(&OracleKey::Price(asset_pair), &price);
    }

    /// Returns the stored price for `asset_pair`, or `0` if none has been set.
    ///
    /// This is the function LedgerLens calls via cross-contract invocation
    /// when computing the oracle-adjusted `confidence_floor` in
    /// `get_effective_score`.
    pub fn get_price(env: Env, asset_pair: Symbol) -> i128 {
        env.storage()
            .instance()
            .get(&OracleKey::Price(asset_pair))
            .unwrap_or(0i128)
    }
}

// ── Integration sketch (pseudo-code, requires a test environment) ─────────────
//
// 1. Deploy LedgerLens and the oracle:
//
//    let ll_id  = env.register_contract(None, LedgerLensScoreContract);
//    let orc_id = env.register_contract(None, ExamplePriceFeedOracle);
//    let ll  = LedgerLensScoreContractClient::new(&env, &ll_id);
//    let orc = ExamplePriceFeedOracleClient::new(&env, &orc_id);
//
// 2. Push a price into the oracle:
//
//    orc.set_price(&symbol_short!("XLM_USDC"), &500_000i128);
//
// 3. Register the oracle with LedgerLens (admin call):
//
//    ll.register_oracle(&admin_signers, &symbol_short!("XLM_USDC"), &orc_id);
//
// 4. Submit a risk score for a wallet:
//
//    ll.submit_score(&signers, &wallet, &symbol_short!("XLM_USDC"),
//                    &55, &false, &false, &ts, &90, &1, &None);
//
// 5. Query the effective score — confidence_floor will reflect the oracle price:
//
//    let eff = ll.get_effective_score(&wallet, &symbol_short!("XLM_USDC")).unwrap();
//    // price=500_000 → confidence_floor = 500_000 / 20_000 = 25
//    assert_eq!(eff.confidence_floor, 25);
//    assert_eq!(eff.original_score, 55);   // stored score unchanged
//    assert_eq!(eff.original_confidence, 90); // stored confidence unchanged
//
// 6. Callers should treat the effective confidence as:
//    effective_confidence = original_confidence.saturating_sub(confidence_floor)
//    A composable protocol can refuse a swap/borrow if effective_confidence
//    falls below its required minimum.
