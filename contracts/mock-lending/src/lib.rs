#![no_std]

//! Minimal mock lending protocol used to exercise LedgerLens's
//! confidence-aware composability primitive (`docs/interface-spec.md` §1.2)
//! from a genuinely separate, independently deployed contract.
//!
//! This is **not** a real lending market — there is no collateral, no
//! interest, no liquidation. It exists solely to prove that `borrow` can
//! call `query_risk_gate_with_confidence` with its own `min_confidence`
//! floor and refuse the borrow when the wallet is too risky or the score
//! isn't backed by enough confidence.

use ledgerlens_score::LedgerLensScoreContractClient;
use soroban_sdk::{contract, contracterror, contractimpl, contracttype, Address, Env, Symbol};

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum MockLendingError {
    /// `initialize` has not been called yet.
    NotConfigured = 1,
    /// LedgerLens's `query_risk_gate_with_confidence` returned `false` for
    /// this wallet — either too risky, no score, or insufficient confidence.
    RiskGateRejected = 2,
    /// Borrow amount must be positive.
    InvalidAmount = 3,
}

#[contracttype]
enum DataKey {
    /// Contract ID of the LedgerLens score registry this market trusts.
    LedgerLens,
    /// Risk-gate threshold (0-100) this market enforces on borrows.
    GateThreshold,
    /// Minimum confidence (0-100) this market requires of the score backing
    /// a borrow decision.
    MinConfidence,
}

#[contract]
pub struct MockLending;

#[contractimpl]
impl MockLending {
    /// One-time wiring: record the LedgerLens deployment plus the risk
    /// threshold and confidence floor this market enforces. No admin auth —
    /// this is a test fixture, not a production contract.
    pub fn initialize(env: Env, ledgerlens: Address, gate_threshold: u32, min_confidence: u32) {
        env.storage().instance().set(&DataKey::LedgerLens, &ledgerlens);
        env.storage().instance().set(&DataKey::GateThreshold, &gate_threshold);
        env.storage().instance().set(&DataKey::MinConfidence, &min_confidence);
    }

    /// Attempt a borrow for `user` against `asset_pair`. Rejected with
    /// `RiskGateRejected` whenever LedgerLens's
    /// `query_risk_gate_with_confidence` says the wallet's score is too
    /// risky, missing, or not backed by enough confidence — even if the raw
    /// risk score itself would otherwise pass.
    pub fn borrow(
        env: Env,
        user: Address,
        asset_pair: Symbol,
        amount: i128,
    ) -> Result<(), MockLendingError> {
        if amount <= 0 {
            return Err(MockLendingError::InvalidAmount);
        }

        let ledgerlens: Address = env
            .storage()
            .instance()
            .get(&DataKey::LedgerLens)
            .ok_or(MockLendingError::NotConfigured)?;
        let gate_threshold: u32 = env
            .storage()
            .instance()
            .get(&DataKey::GateThreshold)
            .ok_or(MockLendingError::NotConfigured)?;
        let min_confidence: u32 = env
            .storage()
            .instance()
            .get(&DataKey::MinConfidence)
            .ok_or(MockLendingError::NotConfigured)?;

        let client = LedgerLensScoreContractClient::new(&env, &ledgerlens);
        let is_safe = client.query_risk_gate_with_confidence(
            &user,
            &asset_pair,
            &gate_threshold,
            &min_confidence,
        );
        if !is_safe {
            return Err(MockLendingError::RiskGateRejected);
        }

        Ok(())
    }
}
