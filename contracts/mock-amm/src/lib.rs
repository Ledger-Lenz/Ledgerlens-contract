#![no_std]

//! Minimal mock AMM used to exercise LedgerLens's composability primitives
//! (`docs/interface-spec.md` §1.1–§1.2) from a genuinely separate, independently
//! deployed contract.
//!
//! This is **not** a real AMM — there are no reserves, no pricing curve, no
//! transfers. It exists solely to prove that `swap` / `provide_liquidity_gated`
//! can call LedgerLens gate functions and refuse risky wallets, mirroring the
//! patterns in `examples/amm_gate.rs` and `examples/amm_gate_example.rs`.
//!
//! The mock intentionally focuses on confidence-gated access control. Real
//! integrations should layer their own max-age and pause-state checks on top
//! of the LedgerLens gate so a stale-but-safe score cannot bypass a high-value
//! action during detection lag.

use ledgerlens_score::LedgerLensScoreContractClient;
use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, Env, Symbol,
};

#[contracttype]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum FailPolicy {
    FailClosed = 0,
    FailOpen = 1,
}

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
    /// The configured oracle call trapped or could not be decoded.
    OracleUnavailable = 5,
    /// The stored score is older than the configured fixture freshness window.
    StaleScore = 6,
    /// The configured oracle does not expose the required contract version.
    UnsupportedVersion = 7,
    /// Caller is not the configured fixture admin.
    Unauthorized = 8,
}

#[contracttype]
enum DataKey {
    Admin,
    /// Contract ID of the LedgerLens score registry this AMM trusts.
    LedgerLens,
    /// Risk-gate threshold (0-100) this AMM enforces.
    GateThreshold,
    /// Minimum confidence (0-100) required of the score backing a gate decision.
    MinConfidence,
    FailPolicy,
    MaxStalenessSecs,
    RequiredOracleVersion,
    ExpandedRiskScore,
}

#[contract]
pub struct MockAmm;

#[contractimpl]
impl MockAmm {
    /// One-time wiring: record the LedgerLens deployment, admin, version
    /// expectation, bounded freshness window, and failure policy enforced by
    /// this SDK conformance fixture.
    pub fn initialize(env: Env, admin: Address, ledgerlens: Address, gate_threshold: u32) {
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::LedgerLens, &ledgerlens);
        env.storage().instance().set(&DataKey::GateThreshold, &gate_threshold);
        env.storage().instance().set(&DataKey::MinConfidence, &0u32);
        env.storage().instance().set(&DataKey::FailPolicy, &FailPolicy::FailClosed);
        env.storage().instance().set(&DataKey::MaxStalenessSecs, &604_800u64);
        env.storage().instance().set(&DataKey::RequiredOracleVersion, &0u32);
        let expanded_score = Self::oracle_has_expanded_score(&env, &ledgerlens);
        env.storage().instance().set(&DataKey::ExpandedRiskScore, &expanded_score);
    }

    /// Register or rotate the LedgerLens oracle this AMM consults for gate checks.
    pub fn set_risk_oracle(env: Env, admin: Address, oracle: Address) -> Result<(), MockAmmError> {
        Self::require_admin(&env, &admin)?;
        env.storage().instance().set(&DataKey::LedgerLens, &oracle);
        let expanded_score = Self::oracle_has_expanded_score(&env, &oracle);
        env.storage().instance().set(&DataKey::ExpandedRiskScore, &expanded_score);
        Ok(())
    }

    /// Configure the score and confidence floors enforced by
    /// `provide_liquidity_gated`.
    pub fn set_liquidity_gate_config(
        env: Env,
        admin: Address,
        gate_threshold: u32,
        min_confidence: u32,
        fail_policy: FailPolicy,
        max_staleness_secs: u64,
        required_oracle_version: u32,
    ) -> Result<(), MockAmmError> {
        Self::require_admin(&env, &admin)?;
        env.storage().instance().set(&DataKey::GateThreshold, &gate_threshold);
        env.storage().instance().set(&DataKey::MinConfidence, &min_confidence);
        env.storage().instance().set(&DataKey::FailPolicy, &fail_policy);
        env.storage().instance().set(&DataKey::MaxStalenessSecs, &max_staleness_secs);
        env.storage().instance().set(&DataKey::RequiredOracleVersion, &required_oracle_version);
        Ok(())
    }

    fn require_admin(env: &Env, admin: &Address) -> Result<(), MockAmmError> {
        let configured: Address =
            env.storage().instance().get(&DataKey::Admin).ok_or(MockAmmError::NotConfigured)?;
        if &configured != admin {
            return Err(MockAmmError::Unauthorized);
        }
        admin.require_auth();
        Ok(())
    }

    fn gate_config(env: &Env) -> Result<(Address, u32, u32, FailPolicy, u64, u32), MockAmmError> {
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
        let min_confidence: u32 =
            env.storage().instance().get(&DataKey::MinConfidence).unwrap_or(0);
        let fail_policy: FailPolicy =
            env.storage().instance().get(&DataKey::FailPolicy).unwrap_or(FailPolicy::FailClosed);
        let max_staleness_secs: u64 =
            env.storage().instance().get(&DataKey::MaxStalenessSecs).unwrap_or(604_800);
        let required_oracle_version: u32 =
            env.storage().instance().get(&DataKey::RequiredOracleVersion).unwrap_or(0);
        Ok((
            ledgerlens,
            gate_threshold,
            min_confidence,
            fail_policy,
            max_staleness_secs,
            required_oracle_version,
        ))
    }

    fn allow_on_unavailable(policy: FailPolicy) -> Result<(), MockAmmError> {
        match policy {
            FailPolicy::FailOpen => Ok(()),
            FailPolicy::FailClosed => Err(MockAmmError::OracleUnavailable),
        }
    }

    fn oracle_has_expanded_score(env: &Env, oracle: &Address) -> bool {
        let client = LedgerLensScoreContractClient::new(env, oracle);
        matches!(client.try_get_version(), Ok(Ok(version)) if version >= 5)
    }

    /// Attempt a swap for `user` on `asset_pair`. Rejected with
    /// `HighRiskWallet` whenever LedgerLens's `query_risk_gate` says the
    /// wallet is not safe — note there is no `try_query_risk_gate` and no
    /// `?`, since the gate is infallible by design. Callers that need
    /// freshness guarantees must add their own max-age bound before invoking
    /// this method.
    pub fn swap(
        env: Env,
        user: Address,
        asset_pair: Symbol,
        amount: i128,
    ) -> Result<(), MockAmmError> {
        if amount <= 0 {
            return Err(MockAmmError::InvalidAmount);
        }

        let (ledgerlens, gate_threshold, _, fail_policy, max_staleness_secs, required_version) =
            Self::gate_config(&env)?;

        let client = LedgerLensScoreContractClient::new(&env, &ledgerlens);
        if required_version > 0 {
            match client.try_get_contract_version() {
                Ok(Ok(version)) if version >= required_version => {}
                Ok(Ok(_)) => return Err(MockAmmError::UnsupportedVersion),
                _ => return Self::allow_on_unavailable(fail_policy),
            }
        }
        let is_safe = match client.try_query_risk_gate(&user, &asset_pair, &gate_threshold) {
            Ok(Ok(v)) => v,
            _ => return Self::allow_on_unavailable(fail_policy),
        };
        if !is_safe {
            return Err(MockAmmError::HighRiskWallet);
        }
        // RiskScore gained additional fields in contract version 5. Older
        // oracles still implement the stable gate surface, but decoding their
        // smaller RiskScore with the current client would abort in the host.
        let expanded_score =
            env.storage().instance().get(&DataKey::ExpandedRiskScore).unwrap_or(false);
        if expanded_score {
            // A successful primary gate may have delegated to a configured
            // failover. In that case the primary has no local score to decode,
            // and the gate has already enforced the failover freshness window.
            if let Ok(Ok(score)) = client.try_get_score(&user, &asset_pair) {
                if env.ledger().timestamp().saturating_sub(score.timestamp) > max_staleness_secs {
                    return Err(MockAmmError::StaleScore);
                }
            }
        }

        Ok(())
    }

    /// Provide liquidity for `provider`, gated by LedgerLens risk score and
    /// confidence. The gate check runs **before** any state changes — no funds
    /// are moved until the provider clears the oracle. Real deployments should
    /// also cap score age and respect their own pause state before allowing a
    /// deposit through.
    ///
    /// When no score exists for the provider, the gate fails closed (same as
    /// `query_risk_gate_with_confidence` returning `false`) and the call is
    /// rejected with `HighRiskWallet`.
    pub fn provide_liquidity_gated(
        env: Env,
        provider: Address,
        amount: i128,
    ) -> Result<(), MockAmmError> {
        if amount <= 0 {
            return Err(MockAmmError::InvalidAmount);
        }

        let (
            ledgerlens,
            gate_threshold,
            min_confidence,
            fail_policy,
            max_staleness_secs,
            required_version,
        ) = Self::gate_config(&env)?;
        let asset_pair = symbol_short!("XLM_USDC");

        let client = LedgerLensScoreContractClient::new(&env, &ledgerlens);
        if required_version > 0 {
            match client.try_get_contract_version() {
                Ok(Ok(version)) if version >= required_version => {}
                Ok(Ok(_)) => return Err(MockAmmError::UnsupportedVersion),
                _ => return Self::allow_on_unavailable(fail_policy),
            }
        }
        let is_safe = match client.try_query_risk_gate_with_confidence(
            &provider,
            &asset_pair,
            &gate_threshold,
            &min_confidence,
        ) {
            Ok(Ok(v)) => v,
            _ => return Self::allow_on_unavailable(fail_policy),
        };
        if !is_safe {
            let expanded_score =
                env.storage().instance().get(&DataKey::ExpandedRiskScore).unwrap_or(false);
            if expanded_score {
                match client.try_get_score(&provider, &asset_pair) {
                    Ok(Ok(score)) if score.confidence < min_confidence => {
                        return Err(MockAmmError::LowConfidence);
                    }
                    _ => {}
                }
            }
            return Err(MockAmmError::HighRiskWallet);
        }
        let expanded_score =
            env.storage().instance().get(&DataKey::ExpandedRiskScore).unwrap_or(false);
        if expanded_score {
            if let Ok(Ok(score)) = client.try_get_score(&provider, &asset_pair) {
                if env.ledger().timestamp().saturating_sub(score.timestamp) > max_staleness_secs {
                    return Err(MockAmmError::StaleScore);
                }
            }
        }

        Ok(())
    }
}
