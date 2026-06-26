#![no_std]

//! Minimal mock AMM used to exercise LedgerLens's composability primitives
//! (`docs/interface-spec.md` §1.1–§1.2) from a genuinely separate, independently
//! deployed contract.
//!
//! This is **not** a real AMM — there are no reserves, no pricing curve, no
//! transfers. It exists solely to prove that `swap` / `provide_liquidity_gated`
//! can call LedgerLens gate functions and refuse risky wallets, mirroring the
//! patterns in `examples/amm_gate.rs` and `examples/amm_gate_example.rs`.

use ledgerlens_score::LedgerLensScoreContractClient;
use soroban_sdk::{contract, contracterror, contractimpl, contracttype, symbol_short, Address, Env, Symbol};

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum MockAmmError {
    /// `initialize` / `set_risk_oracle` has not been called yet.
    NotConfigured = 1,
    /// LedgerLens's gate returned `false` because the provider's risk score is
    /// at or above the configured threshold, or no score exists (fail closed).
    HighRiskWallet = 2,
    /// Liquidity amount must be positive.
    InvalidAmount = 3,
    /// LedgerLens's gate returned `false` because the score's confidence is
    /// below the configured minimum.
    LowConfidence = 4,
}

#[contracttype]
enum DataKey {
    /// Contract ID of the LedgerLens score registry this AMM trusts.
    LedgerLens,
    /// Risk-gate threshold (0-100) this AMM enforces.
    GateThreshold,
    /// Minimum confidence (0-100) required of the score backing a gate decision.
    MinConfidence,
}

#[contract]
pub struct MockAmm;

#[contractimpl]
impl MockAmm {
    /// One-time wiring: record the LedgerLens deployment and the risk
    /// threshold this AMM enforces. No admin auth — this is a test fixture,
    /// not a production contract.
    pub fn initialize(env: Env, ledgerlens: Address, gate_threshold: u32) {
        env.storage().instance().set(&DataKey::LedgerLens, &ledgerlens);
        env.storage().instance().set(&DataKey::GateThreshold, &gate_threshold);
        env.storage().instance().set(&DataKey::MinConfidence, &0u32);
    }

    /// Register or rotate the LedgerLens oracle this AMM consults for gate checks.
    pub fn set_risk_oracle(env: Env, oracle: Address) {
        env.storage().instance().set(&DataKey::LedgerLens, &oracle);
    }

    /// Configure the score and confidence floors enforced by
    /// `provide_liquidity_gated`.
    pub fn set_liquidity_gate_config(env: Env, gate_threshold: u32, min_confidence: u32) {
        env.storage().instance().set(&DataKey::GateThreshold, &gate_threshold);
        env.storage().instance().set(&DataKey::MinConfidence, &min_confidence);
    }

    fn gate_config(env: &Env) -> Result<(Address, u32, u32), MockAmmError> {
        let ledgerlens: Address = env
            .storage()
            .instance()
            .get(&DataKey::LedgerLens)
            .ok_or(MockAmmError::NotConfigured)?;
        let gate_threshold: u32 = env
            .storage()
            .instance()
            .get(&DataKey::GateThreshold)
            .ok_or(MockAmmError::NotConfigured)?;
        let min_confidence: u32 = env
            .storage()
            .instance()
            .get(&DataKey::MinConfidence)
            .unwrap_or(0);
        Ok((ledgerlens, gate_threshold, min_confidence))
    }

    /// Attempt a swap for `user` on `asset_pair`. Rejected with
    /// `HighRiskWallet` whenever LedgerLens's `query_risk_gate` says the
    /// wallet is not safe — note there is no `try_query_risk_gate` and no
    /// `?`, since the gate is infallible by design.
    pub fn swap(
        env: Env,
        user: Address,
        asset_pair: Symbol,
        amount: i128,
    ) -> Result<(), MockAmmError> {
        if amount <= 0 {
            return Err(MockAmmError::InvalidAmount);
        }

        let (ledgerlens, gate_threshold, _) = Self::gate_config(&env)?;

        let client = LedgerLensScoreContractClient::new(&env, &ledgerlens);
        let is_safe = client.query_risk_gate(&user, &asset_pair, &gate_threshold);
        if !is_safe {
            return Err(MockAmmError::HighRiskWallet);
        }

        Ok(())
    }

    /// Provide liquidity for `provider`, gated by LedgerLens risk score and
    /// confidence. The gate check runs **before** any state changes — no funds
    /// are moved until the provider clears the oracle.
    ///
    /// When no score exists for the provider, the gate fails closed (same as
    /// `query_risk_gate_with_confidence` returning `false`) and the call is
    /// rejected with `HighRiskWallet`.
    pub fn provide_liquidity_gated(env: Env, provider: Address, amount: i128) -> Result<(), MockAmmError> {
        if amount <= 0 {
            return Err(MockAmmError::InvalidAmount);
        }

        let (ledgerlens, gate_threshold, min_confidence) = Self::gate_config(&env)?;
        let asset_pair = symbol_short!("XLM_USDC");

        let client = LedgerLensScoreContractClient::new(&env, &ledgerlens);
        let is_safe = client.query_risk_gate_with_confidence(
            &provider,
            &asset_pair,
            &gate_threshold,
            &min_confidence,
        );
        if !is_safe {
            match client.try_get_score(&provider, &asset_pair) {
                Ok(Ok(score)) if score.confidence < min_confidence => {
                    return Err(MockAmmError::LowConfidence);
                }
                _ => return Err(MockAmmError::HighRiskWallet),
            }
        }

        Ok(())
    }
}
