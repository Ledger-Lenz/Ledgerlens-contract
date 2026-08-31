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
//!
//! Production integrations should add a freshness bound and fail closed when
//! the score is stale, the oracle is silent, or the protocol's own pause state
//! is active.

use ledgerlens_score::LedgerLensScoreContractClient;
use soroban_sdk::{contract, contracterror, contractimpl, contracttype, Address, Env, Symbol};

#[contracttype]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum FailPolicy {
    FailClosed = 0,
    FailOpen = 1,
}

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
    OracleUnavailable = 4,
    StaleScore = 5,
    UnsupportedVersion = 6,
    Unauthorized = 7,
}

#[contracttype]
enum DataKey {
    Admin,
    /// Contract ID of the LedgerLens score registry this market trusts.
    LedgerLens,
    /// Risk-gate threshold (0-100) this market enforces on borrows.
    GateThreshold,
    /// Minimum confidence (0-100) this market requires of the score backing
    /// a borrow decision.
    MinConfidence,
    FailPolicy,
    MaxStalenessSecs,
    RequiredOracleVersion,
    ExpandedRiskScore,
}

#[contract]
pub struct MockLending;

#[contractimpl]
impl MockLending {
    /// One-time wiring: record the LedgerLens deployment plus the risk
    /// threshold and confidence floor this market enforces.
    pub fn initialize(
        env: Env,
        admin: Address,
        ledgerlens: Address,
        gate_threshold: u32,
        min_confidence: u32,
    ) {
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::LedgerLens, &ledgerlens);
        env.storage().instance().set(&DataKey::GateThreshold, &gate_threshold);
        env.storage().instance().set(&DataKey::MinConfidence, &min_confidence);
        env.storage().instance().set(&DataKey::FailPolicy, &FailPolicy::FailClosed);
        env.storage().instance().set(&DataKey::MaxStalenessSecs, &604_800u64);
        env.storage().instance().set(&DataKey::RequiredOracleVersion, &0u32);
        let client = LedgerLensScoreContractClient::new(&env, &ledgerlens);
        let expanded_score = matches!(client.try_get_version(), Ok(Ok(version)) if version >= 5);
        env.storage().instance().set(&DataKey::ExpandedRiskScore, &expanded_score);
    }

    pub fn set_borrow_gate_config(
        env: Env,
        admin: Address,
        gate_threshold: u32,
        min_confidence: u32,
        fail_policy: FailPolicy,
        max_staleness_secs: u64,
        required_oracle_version: u32,
    ) -> Result<(), MockLendingError> {
        Self::require_admin(&env, &admin)?;
        env.storage().instance().set(&DataKey::GateThreshold, &gate_threshold);
        env.storage().instance().set(&DataKey::MinConfidence, &min_confidence);
        env.storage().instance().set(&DataKey::FailPolicy, &fail_policy);
        env.storage().instance().set(&DataKey::MaxStalenessSecs, &max_staleness_secs);
        env.storage().instance().set(&DataKey::RequiredOracleVersion, &required_oracle_version);
        Ok(())
    }

    fn require_admin(env: &Env, admin: &Address) -> Result<(), MockLendingError> {
        let configured: Address =
            env.storage().instance().get(&DataKey::Admin).ok_or(MockLendingError::NotConfigured)?;
        if &configured != admin {
            return Err(MockLendingError::Unauthorized);
        }
        admin.require_auth();
        Ok(())
    }

    fn allow_on_unavailable(policy: FailPolicy) -> Result<(), MockLendingError> {
        match policy {
            FailPolicy::FailOpen => Ok(()),
            FailPolicy::FailClosed => Err(MockLendingError::OracleUnavailable),
        }
    }

    /// Attempt a borrow for `user` against `asset_pair`. Rejected with
    /// `RiskGateRejected` whenever LedgerLens's
    /// `query_risk_gate_with_confidence` says the wallet's score is too
    /// risky, missing, or not backed by enough confidence — even if the raw
    /// risk score itself would otherwise pass. Callers that care about
    /// detection lag should apply a max-age check before calling this.
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
        let fail_policy: FailPolicy =
            env.storage().instance().get(&DataKey::FailPolicy).unwrap_or(FailPolicy::FailClosed);
        let max_staleness_secs: u64 =
            env.storage().instance().get(&DataKey::MaxStalenessSecs).unwrap_or(604_800);
        let required_oracle_version: u32 =
            env.storage().instance().get(&DataKey::RequiredOracleVersion).unwrap_or(0);

        let client = LedgerLensScoreContractClient::new(&env, &ledgerlens);
        if required_oracle_version > 0 {
            match client.try_get_contract_version() {
                Ok(Ok(version)) if version >= required_oracle_version => {}
                Ok(Ok(_)) => return Err(MockLendingError::UnsupportedVersion),
                _ => return Self::allow_on_unavailable(fail_policy),
            }
        }
        let is_safe = match client.try_query_risk_gate_with_confidence(
            &user,
            &asset_pair,
            &gate_threshold,
            &min_confidence,
        ) {
            Ok(Ok(v)) => v,
            _ => return Self::allow_on_unavailable(fail_policy),
        };
        if !is_safe {
            return Err(MockLendingError::RiskGateRejected);
        }
        // RiskScore expanded in contract version 5. Older deployments remain
        // compatible through the stable confidence-gate surface, but their
        // smaller score value cannot be decoded by the current generated client.
        let expanded_score =
            env.storage().instance().get(&DataKey::ExpandedRiskScore).unwrap_or(false);
        if expanded_score {
            if let Ok(Ok(score)) = client.try_get_score(&user, &asset_pair) {
                if env.ledger().timestamp().saturating_sub(score.timestamp) > max_staleness_secs {
                    return Err(MockLendingError::StaleScore);
                }
            }
        }

        Ok(())
    }
}
