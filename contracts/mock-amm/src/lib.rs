#![no_std]

//! Minimal mock AMM used to exercise LedgerLens's composability primitives
//! (`docs/interface-spec.md` §1.1) from a genuinely separate, independently
//! deployed contract.
//!
//! This is **not** a real AMM — there are no reserves, no pricing curve, no
//! transfers. It exists solely to prove that `swap` can call
//! `query_risk_gate` on a LedgerLens deployment and refuse the swap when the
//! wallet is too risky, mirroring the pattern in `examples/amm_gate.rs`.

use ledgerlens_score::LedgerLensScoreContractClient;
use soroban_sdk::{contract, contracterror, contractimpl, contracttype, Address, Env, Symbol};

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum MockAmmError {
    /// `initialize` has not been called yet.
    NotConfigured = 1,
    /// LedgerLens's `query_risk_gate` returned `false` for this wallet.
    HighRiskWallet = 2,
    /// Swap amount must be positive.
    InvalidAmount = 3,
}

#[contracttype]
enum DataKey {
    /// Contract ID of the LedgerLens score registry this AMM trusts.
    LedgerLens,
    /// Risk-gate threshold (0-100) this AMM enforces on swaps.
    GateThreshold,
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

        let client = LedgerLensScoreContractClient::new(&env, &ledgerlens);
        let is_safe = client.query_risk_gate(&user, &asset_pair, &gate_threshold);
        if !is_safe {
            return Err(MockAmmError::HighRiskWallet);
        }

        Ok(())
    }
}
