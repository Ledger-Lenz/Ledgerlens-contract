#![no_std]

#[cfg(test)]
extern crate std;

#[cfg(test)]
mod test;
const REQUIRED_SHARD_CAPABILITIES: [&str; 4] = ["score", "gate", "aggr", "arch"];
use ledgerlens_score::{AggregateRiskScore, Error as ScoreError, RiskScore};
use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, vec, Address, Env, Symbol,
    TryFromVal, Vec,
};

pub const MAX_SHARDS: usize = 10;
const FAILURE_TRANSPORT: u32 = 0;
const FAILURE_CONTRACT_ERROR: u32 = 1;

/// Errors surfaced directly by `LedgerLensAggregator`'s own bookkeeping
/// (shard registry, admin gating). `query_risk_gate` itself is infallible —
/// see its doc comment — and reports every failure case by returning `false`
/// rather than one of these variants. Errors that originate from a specific
/// shard's `ledgerlens-score` deployment (e.g. an incompatible interface) are
/// reported as their own `ledgerlens_score::Error` (`ScoreError`) value
/// instead of being wrapped here.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    SelfReference = 3,
    ShardAlreadyRegistered = 4,
    ShardLimitReached = 5,
    ShardNotRegistered = 6,
    /// A candidate shard does not advertise every capability
    /// `REQUIRED_SHARD_CAPABILITIES` requires.
    IncompatibleInterface = 7,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SplitBrainStatus {
    NoShards,
    Aligned,
    SplitBrain,
    QuorumLost,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ShardProbeStatus {
    Aligned,
    ConfigMismatch,
    Unavailable,
    Stale,
    Unhealthy,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AggregatorConfigFingerprint {
    pub decay_num: u64,
    pub decay_den: u64,
    pub staleness_window: u64,
    pub global_min_confidence: u32,
    pub consensus_k: u32,
    pub consensus_epsilon: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MaybeAggregatorConfigFingerprint {
    None,
    Some(AggregatorConfigFingerprint),
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShardConfigDiagnostic {
    pub shard: Address,
    pub status: ShardProbeStatus,
    pub fingerprint: MaybeAggregatorConfigFingerprint,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SplitBrainReport {
    pub status: SplitBrainStatus,
    pub canonical: MaybeAggregatorConfigFingerprint,
    pub shard_count: u32,
    pub healthy_count: u32,
    pub available_count: u32,
    pub stale_count: u32,
    pub unavailable_count: u32,
    pub mismatch_count: u32,
    pub quorum_count: u32,
    pub required_quorum: u32,
    pub diagnostics: Vec<ShardConfigDiagnostic>,
}

/// Capabilities of the `ILedgerLensScore` interface (interface version 2, see
/// `docs/interface-spec.md`) that this aggregator invokes on every registered
/// shard: `query_risk_gate` (`gate`), `get_score` (`score`), and
/// `get_aggregate_score` (`aggr`). `add_shard` requires a candidate shard to
/// advertise all of them via `supports_interface`, so a shard whose interface
/// has drifted is rejected at registration time instead of failing silently
/// during a later cross-contract call.
const MAX_ASSET_PAIR_BYTES: u32 = 9;

/// Returns `true` only when `shard` reports support for every capability in
/// [`REQUIRED_SHARD_CAPABILITIES`]. A shard that omits one, reports `false`, or
/// does not expose `supports_interface` at all (an older or drifted build) is
/// treated as incompatible.
fn shard_supports_required_interface(env: &Env, shard: &Address) -> bool {
    let client = ledgerlens_score::LedgerLensScoreContractClient::new(env, shard);

    // 1. Verify standard capability flags
    for capability in REQUIRED_SHARD_CAPABILITIES {
        match client.try_supports_interface(&Symbol::new(env, capability)) {
            Ok(Ok(true)) => {}
            _ => return false,
        }
    }

    // 2. Interface validation: Verify getters exist and are callable
    if client.try_get_arch_owner().is_err() {
        return false;
    }

    if client.try_get_mandatory_reviewers().is_err() {
        return false;
    }

    true
}

fn asset_pair_is_bounded(env: &Env, asset_pair: &Symbol) -> bool {
    let pair = soroban_sdk::SymbolStr::try_from_val(env, &asset_pair.to_symbol_val());
    match pair {
        Ok(pair) => pair.len() as u32 <= MAX_ASSET_PAIR_BYTES,
        Err(_) => false,
    }
}

#[contract]
pub struct LedgerLensAggregator;

#[contractimpl]
impl LedgerLensAggregator {
    pub fn initialize(env: Env, admin: Address) -> Result<(), Error> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(Error::AlreadyInitialized);
        }
        // Bind first initialization to the nominated administrator. Without
        // this authorization, any invoker could front-run deployment and take
        // control of shard registration.
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        Ok(())
    }

    pub fn get_admin(env: Env) -> Result<Address, Error> {
        env.storage().instance().get(&DataKey::Admin).ok_or(Error::NotInitialized)
    }

    /// Returns the fixed-point exponential decay lambda as (numerator, denominator).
    ///
    /// Aggregation policy: singleton configuration is read from the primary
    /// shard, defined as the first registered shard. If shards diverge, the
    /// aggregator does not average or reconcile the values; operators must keep
    /// shard configuration aligned or intentionally choose the primary shard's
    /// value for integrators.
    ///
    /// Example:
    /// ```ignore
    /// let (num, den) = env.invoke_contract(&contract_id, &symbol_short!("get_decay_rate"), ());
    /// // decay_factor = num / den  (e.g. 999 / 1000 = 0.999)
    /// ```
    pub fn get_decay_rate(env: Env) -> Result<(u64, u64), ScoreError> {
        let shards: Vec<Address> =
            env.storage().instance().get(&DataKey::Shards).unwrap_or_else(|| Vec::new(&env));
        let primary = shards.get(0).ok_or(ScoreError::ScoreNotFound)?;
        let client = ledgerlens_score::LedgerLensScoreContractClient::new(&env, &primary);

        match client.try_get_decay_rate() {
            Ok(Ok(rate)) => Ok(rate),
            _ => Err(ScoreError::ScoreNotFound),
        }
    }

    pub fn get_consensus_threshold_k(_env: Env) -> u32 {
        // Adjust this value based on your actual consensus parameters
        const CONSENSUS_THRESHOLD_K: u32 = 5; // Minimum agreeing models required

        CONSENSUS_THRESHOLD_K
    }

    /// Returns whether the given wallet is currently on any shard's monitoring watchlist.
    ///
    /// Aggregation policy: watchlist is a conservative risk signal, so shard
    /// results are OR'd. A wallet is considered watchlisted if any registered
    /// shard reports `true`.
    ///
    /// Example:
    /// ```ignore
    /// let is_watched = env.invoke_contract(&contract_id, &symbol_short!("get_watchlist_status"), vec![&env, wallet]);
    /// ```
    pub fn get_watchlist_status(env: Env, wallet: Address) -> bool {
        let shards: Vec<Address> =
            env.storage().instance().get(&DataKey::Shards).unwrap_or_else(|| Vec::new(&env));
        for i in 0..shards.len() {
            let shard = shards.get(i).unwrap();
            let client = ledgerlens_score::LedgerLensScoreContractClient::new(&env, &shard);
            if let Ok(Ok(true)) = client.try_is_watchlisted(&wallet) {
                return true;
            }
        }
        false
    }

    pub fn add_shard(env: Env, shard: Address) -> Result<(), Error> {
        let admin: Address =
            env.storage().instance().get(&DataKey::Admin).ok_or(Error::NotInitialized)?;
        admin.require_auth();
        if env.current_contract_address() == shard {
            return Err(Error::SelfReference);
        }
        let mut shards: Vec<Address> =
            env.storage().instance().get(&DataKey::Shards).unwrap_or_else(|| Vec::new(&env));
        // Check duplicate
        for i in 0..shards.len() {
            if shards.get(i).unwrap() == shard {
                return Err(Error::ShardAlreadyRegistered);
            }
        }
        if shards.len() as usize >= MAX_SHARDS {
            return Err(Error::ShardLimitReached);
        }
        if !shard_supports_required_interface(&env, &shard) {
            return Err(Error::IncompatibleInterface);
        }
        // Record capability snapshot at registration time so operators and
        // tests can detect post-registration capability downgrades.
        let snapshot = probe_capabilities(&env, &shard);
        env.storage().instance().set(&DataKey::ShardCapabilities(shard.clone()), &snapshot);
        shards.push_back(shard);
        env.storage().instance().set(&DataKey::Shards, &shards);
        Ok(())
    }

    pub fn remove_shard(env: Env, shard: Address) -> Result<(), Error> {
        let admin: Address =
            env.storage().instance().get(&DataKey::Admin).ok_or(Error::NotInitialized)?;
        admin.require_auth();
        let shards: Vec<Address> =
            env.storage().instance().get(&DataKey::Shards).unwrap_or_else(|| Vec::new(&env));
        let mut found = false;
        let mut out: Vec<Address> = Vec::new(&env);
        for i in 0..shards.len() {
            let a = shards.get(i).unwrap();
            if a == shard {
                found = true;
            } else {
                out.push_back(a);
            }
        }
        if !found {
            return Err(Error::ShardNotRegistered);
        }
        env.storage().instance().set(&DataKey::Shards, &out);
        env.storage().instance().remove(&DataKey::ShardHealth(shard.clone()));
        env.storage().instance().remove(&DataKey::ShardCapabilities(shard));
        Ok(())
    }

    pub fn get_shards(env: Env) -> Vec<Address> {
        env.storage().instance().get(&DataKey::Shards).unwrap_or_else(|| Vec::new(&env))
    }

    /// Returns the capability snapshot recorded when `shard` was registered via
    /// `add_shard`, or an empty `Vec` if no snapshot exists (e.g. the shard was
    /// registered before this feature was deployed).
    pub fn get_shard_capabilities(env: Env, shard: Address) -> Vec<Symbol> {
        env.storage()
            .instance()
            .get(&DataKey::ShardCapabilities(shard))
            .unwrap_or_else(|| Vec::new(&env))
    }

    /// Returns `true` if `shard` no longer advertises all the capabilities it
    /// reported at registration time — i.e. a post-registration downgrade is
    /// detected.  Returns `false` when the shard is healthy and still passes
    /// every capability check, or when no snapshot exists.
    ///
    /// This is a read-only probe intended for monitoring and governance flows.
    pub fn shard_capabilities_downgraded(env: Env, shard: Address) -> bool {
        let snapshot: Vec<Symbol> = env
            .storage()
            .instance()
            .get(&DataKey::ShardCapabilities(shard.clone()))
            .unwrap_or_else(|| Vec::new(&env));
        if snapshot.is_empty() {
            return false;
        }
        let client = ledgerlens_score::LedgerLensScoreContractClient::new(&env, &shard);
        for i in 0..snapshot.len() {
            let cap = snapshot.get(i).unwrap();
            match client.try_supports_interface(&cap) {
                Ok(Ok(true)) => {}
                _ => return true,
            }
        }
        false
    }

    /// Infallible, side-effect-free (beyond recording `LastShardFailure`) gate
    /// check, mirroring `ledgerlens-score`'s own `query_risk_gate`: every
    /// registered, healthy shard must agree the wallet clears `gate_threshold`
    /// (an AND across shards), and any shard that is unhealthy is skipped, but
    /// a shard whose cross-contract call itself fails (unreachable contract,
    /// trap, etc.) fails the *whole* query closed — see
    /// `tests/composability/tests/aggregator_shard_pause.rs` (issue #411).
    pub fn query_risk_gate(
        env: Env,
        wallet: Address,
        asset_pair: Symbol,
        gate_threshold: u32,
    ) -> bool {
        if !asset_pair_is_bounded(&env, &asset_pair) {
            return false;
        }
        let shards: Vec<Address> =
            env.storage().instance().get(&DataKey::Shards).unwrap_or_else(|| Vec::new(&env));
        if shards.is_empty() {
            return false;
        }
        for i in 0..shards.len() {
            let shard = shards.get(i).unwrap();
            if !is_shard_healthy(&env, &shard) {
                continue;
            }
            let client = ledgerlens_score::LedgerLensScoreContractClient::new(&env, &shard);
            match client.try_query_risk_gate(&wallet, &asset_pair, &gate_threshold) {
                Ok(Ok(true)) => {}
                Ok(Ok(false)) => return false,
                _ => {
                    env.storage()
                        .instance()
                        .set(&DataKey::LastShardFailure, &(shard.clone(), FAILURE_TRANSPORT));
                    return false;
                }
            }
        }
        true
    }

    pub fn get_score(
        env: Env,
        wallet: Address,
        asset_pair: Symbol,
    ) -> Result<RiskScore, ScoreError> {
        if !asset_pair_is_bounded(&env, &asset_pair) {
            return Err(ScoreError::InvalidAttestation);
        }
        let shards: Vec<Address> =
            env.storage().instance().get(&DataKey::Shards).unwrap_or_else(|| Vec::new(&env));
        let mut best: Option<RiskScore> = None;
        for i in 0..shards.len() {
            let shard = shards.get(i).unwrap();
            if !is_shard_healthy(&env, &shard) {
                continue;
            }
            let client = ledgerlens_score::LedgerLensScoreContractClient::new(&env, &shard);
            match client.try_get_score(&wallet, &asset_pair) {
                Ok(Ok(score)) => match &best {
                    None => best = Some(score),
                    Some(b) => {
                        if score.score > b.score {
                            best = Some(score);
                        }
                    }
                },
                Ok(Err(_conv_err)) => {
                    env.storage()
                        .instance()
                        .set(&DataKey::LastShardFailure, &(shard.clone(), FAILURE_CONTRACT_ERROR));
                }
                Err(_) => {
                    env.storage()
                        .instance()
                        .set(&DataKey::LastShardFailure, &(shard.clone(), FAILURE_TRANSPORT));
                }
            }
        }
        best.ok_or(ScoreError::ScoreNotFound)
    }

    pub fn get_aggregate_score(
        env: Env,
        wallet: Address,
    ) -> Result<AggregateRiskScore, ScoreError> {
        let shards: Vec<Address> =
            env.storage().instance().get(&DataKey::Shards).unwrap_or_else(|| Vec::new(&env));
        let mut best: Option<AggregateRiskScore> = None;
        for i in 0..shards.len() {
            let shard = shards.get(i).unwrap();
            if !is_shard_healthy(&env, &shard) {
                continue;
            }
            let client = ledgerlens_score::LedgerLensScoreContractClient::new(&env, &shard);
            match client.try_get_aggregate_score(&wallet) {
                Ok(Ok(agg)) => match &best {
                    None => best = Some(agg),
                    Some(b) => {
                        if agg.aggregate_score > b.aggregate_score {
                            best = Some(agg);
                        }
                    }
                },
                Ok(Err(_conv_err)) => {
                    env.storage()
                        .instance()
                        .set(&DataKey::LastShardFailure, &(shard.clone(), FAILURE_CONTRACT_ERROR));
                }
                Err(_) => {
                    env.storage()
                        .instance()
                        .set(&DataKey::LastShardFailure, &(shard.clone(), FAILURE_TRANSPORT));
                }
            }
        }
        best.ok_or(ScoreError::ScoreNotFound)
    }

    pub fn supports_interface(env: Env, capability: Symbol) -> bool {
        let caps = vec![
            &env,
            symbol_short!("score"),
            symbol_short!("gate"),
            symbol_short!("aggr"),
            symbol_short!("federated"),
            symbol_short!("sbrain"),
            symbol_short!("health"),
        ];
        for i in 0..caps.len() {
            if caps.get(i).unwrap() == capability {
                return true;
            }
        }
        false
    }

    pub fn get_score_across_shards(
        env: Env,
        wallet: Address,
        asset_pair: Symbol,
    ) -> Vec<(Address, Option<RiskScore>)> {
        if !asset_pair_is_bounded(&env, &asset_pair) {
            return Vec::new(&env);
        }
        let shards: Vec<Address> =
            env.storage().instance().get(&DataKey::Shards).unwrap_or_else(|| Vec::new(&env));
        let mut out: Vec<(Address, Option<RiskScore>)> = Vec::new(&env);
        for i in 0..shards.len() {
            let shard = shards.get(i).unwrap();
            if !is_shard_healthy(&env, &shard) {
                continue;
            }
            let client = ledgerlens_score::LedgerLensScoreContractClient::new(&env, &shard);
            match client.try_get_score(&wallet, &asset_pair) {
                Ok(Ok(score)) => out.push_back((shard.clone(), Some(score))),
                _ => out.push_back((shard.clone(), None)),
            }
        }
        out
    }

    /// Queries the contagion depth across all shards, returning the maximum depth found.
    ///
    /// Returns the highest counterparty count for the wallet/pair across all registered shards.
    pub fn contagion_depth_across_shards(env: Env, wallet: Address, asset_pair: Symbol) -> u32 {
        if !asset_pair_is_bounded(&env, &asset_pair) {
            return 0;
        }
        let shards: Vec<Address> =
            env.storage().instance().get(&DataKey::Shards).unwrap_or_else(|| Vec::new(&env));
        let mut max_depth: u32 = 0;
        for i in 0..shards.len() {
            let shard = shards.get(i).unwrap();
            if !is_shard_healthy(&env, &shard) {
                continue;
            }
            let client = ledgerlens_score::LedgerLensScoreContractClient::new(&env, &shard);
            match client.try_get_contagion_depth(&wallet, &asset_pair) {
                Ok(Ok(depth)) => {
                    if depth > max_depth {
                        max_depth = depth;
                    }
                }
                _ => {
                    env.storage()
                        .instance()
                        .set(&DataKey::LastShardFailure, &(shard.clone(), FAILURE_TRANSPORT));
                }
            }
        }
        max_depth
    }

    pub fn get_last_shard_failure(env: Env) -> Option<(Address, u32)> {
        env.storage().instance().get(&DataKey::LastShardFailure)
    }

    pub fn set_shard_health(env: Env, shard: Address, healthy: bool) -> Result<(), Error> {
        let admin: Address =
            env.storage().instance().get(&DataKey::Admin).ok_or(Error::NotInitialized)?;
        admin.require_auth();
        if !is_shard_registered(&env, &shard) {
            return Err(Error::ShardNotRegistered);
        }
        env.storage().instance().set(&DataKey::ShardHealth(shard.clone()), &healthy);
        env.events().publish((symbol_short!("sh_health"), shard), healthy);
        Ok(())
    }

    pub fn get_shard_health(env: Env, shard: Address) -> Result<bool, Error> {
        if !env.storage().instance().has(&DataKey::Admin) {
            return Err(Error::NotInitialized);
        }
        if !is_shard_registered(&env, &shard) {
            return Err(Error::ShardNotRegistered);
        }
        Ok(is_shard_healthy(&env, &shard))
    }

    pub fn detect_split_brain(env: Env, wallet: Address, asset_pair: Symbol) -> SplitBrainReport {
        let shards: Vec<Address> =
            env.storage().instance().get(&DataKey::Shards).unwrap_or_else(|| Vec::new(&env));
        let shard_count = shards.len();
        let mut diagnostics: Vec<ShardConfigDiagnostic> = Vec::new(&env);
        let mut available: Vec<(Address, AggregatorConfigFingerprint)> = Vec::new(&env);
        let mut healthy_count = 0u32;
        let mut stale_count = 0u32;
        let mut unavailable_count = 0u32;

        for i in 0..shards.len() {
            let shard = shards.get(i).unwrap();
            if !is_shard_healthy(&env, &shard) {
                diagnostics.push_back(ShardConfigDiagnostic {
                    shard,
                    status: ShardProbeStatus::Unhealthy,
                    fingerprint: MaybeAggregatorConfigFingerprint::None,
                });
                continue;
            }
            healthy_count += 1;
            let client = ledgerlens_score::LedgerLensScoreContractClient::new(&env, &shard);
            match client.try_get_score(&wallet, &asset_pair) {
                Ok(Ok(_)) => match client.try_is_score_stale(&wallet, &asset_pair) {
                    Ok(Ok(true)) => {
                        stale_count += 1;
                        diagnostics.push_back(ShardConfigDiagnostic {
                            shard,
                            status: ShardProbeStatus::Stale,
                            fingerprint: MaybeAggregatorConfigFingerprint::None,
                        });
                        continue;
                    }
                    Ok(Ok(false)) => {}
                    _ => {
                        unavailable_count += 1;
                        diagnostics.push_back(ShardConfigDiagnostic {
                            shard,
                            status: ShardProbeStatus::Unavailable,
                            fingerprint: MaybeAggregatorConfigFingerprint::None,
                        });
                        continue;
                    }
                },
                Err(Ok(ScoreError::ScoreNotFound)) => {}
                Ok(Err(_)) => {
                    unavailable_count += 1;
                    diagnostics.push_back(ShardConfigDiagnostic {
                        shard,
                        status: ShardProbeStatus::Unavailable,
                        fingerprint: MaybeAggregatorConfigFingerprint::None,
                    });
                    continue;
                }
                Err(_) => {
                    unavailable_count += 1;
                    diagnostics.push_back(ShardConfigDiagnostic {
                        shard,
                        status: ShardProbeStatus::Unavailable,
                        fingerprint: MaybeAggregatorConfigFingerprint::None,
                    });
                    continue;
                }
            }

            match read_config_fingerprint(&env, &shard) {
                Some(fingerprint) => {
                    available.push_back((shard.clone(), fingerprint.clone()));
                    diagnostics.push_back(ShardConfigDiagnostic {
                        shard,
                        status: ShardProbeStatus::Aligned,
                        fingerprint: MaybeAggregatorConfigFingerprint::Some(fingerprint),
                    });
                }
                None => {
                    unavailable_count += 1;
                    diagnostics.push_back(ShardConfigDiagnostic {
                        shard,
                        status: ShardProbeStatus::Unavailable,
                        fingerprint: MaybeAggregatorConfigFingerprint::None,
                    });
                }
            }
        }

        let required_quorum = (healthy_count / 2) + 1;
        let (canonical, quorum_count) = select_canonical_fingerprint(&available);
        let mut mismatch_count = 0u32;
        let mut final_diagnostics: Vec<ShardConfigDiagnostic> = Vec::new(&env);

        for i in 0..diagnostics.len() {
            let mut diagnostic = diagnostics.get(i).unwrap();
            if let (Some(canon), MaybeAggregatorConfigFingerprint::Some(fingerprint)) =
                (&canonical, &diagnostic.fingerprint)
            {
                if fingerprint != canon {
                    diagnostic.status = ShardProbeStatus::ConfigMismatch;
                    mismatch_count += 1;
                }
            }
            final_diagnostics.push_back(diagnostic);
        }

        let status = if shard_count == 0 {
            SplitBrainStatus::NoShards
        } else if quorum_count < required_quorum {
            SplitBrainStatus::QuorumLost
        } else if mismatch_count > 0 {
            SplitBrainStatus::SplitBrain
        } else {
            SplitBrainStatus::Aligned
        };

        SplitBrainReport {
            status,
            canonical: match canonical {
                Some(fingerprint) => MaybeAggregatorConfigFingerprint::Some(fingerprint),
                None => MaybeAggregatorConfigFingerprint::None,
            },
            shard_count,
            healthy_count,
            available_count: available.len(),
            stale_count,
            unavailable_count,
            mismatch_count,
            quorum_count,
            required_quorum,
            diagnostics: final_diagnostics,
        }
    }
}

/// Queries the shard for every capability in the known universe and returns
/// those it acknowledges.  Used to build the snapshot stored at registration
/// time.  Unknown/future capabilities are simply absent from the snapshot.
fn probe_capabilities(env: &Env, shard: &Address) -> Vec<Symbol> {
    let all_caps = vec![
        env,
        Symbol::new(env, "score"),
        Symbol::new(env, "gate"),
        Symbol::new(env, "aggr"),
        Symbol::new(env, "arch"),
        Symbol::new(env, "federated"),
        Symbol::new(env, "sbrain"),
        Symbol::new(env, "health"),
        Symbol::new(env, "batch_attested"),
    ];
    let client = ledgerlens_score::LedgerLensScoreContractClient::new(env, shard);
    let mut found: Vec<Symbol> = Vec::new(env);
    for i in 0..all_caps.len() {
        let cap = all_caps.get(i).unwrap();
        if let Ok(Ok(true)) = client.try_supports_interface(&cap) {
            found.push_back(cap);
        }
    }
    found
}

fn is_shard_healthy(env: &Env, shard: &Address) -> bool {
    env.storage().instance().get(&DataKey::ShardHealth(shard.clone())).unwrap_or(true)
}

fn is_shard_registered(env: &Env, shard: &Address) -> bool {
    let shards: Vec<Address> =
        env.storage().instance().get(&DataKey::Shards).unwrap_or_else(|| Vec::new(env));
    for i in 0..shards.len() {
        if shards.get(i).unwrap() == *shard {
            return true;
        }
    }
    false
}

fn read_config_fingerprint(env: &Env, shard: &Address) -> Option<AggregatorConfigFingerprint> {
    let client = ledgerlens_score::LedgerLensScoreContractClient::new(env, shard);
    let decay = match client.try_get_decay_rate() {
        Ok(Ok(rate)) => rate,
        _ => return None,
    };
    let staleness_window = match client.try_get_staleness_window() {
        Ok(Ok(window)) => window,
        _ => return None,
    };
    let global_min_confidence = match client.try_get_global_min_confidence() {
        Ok(Ok(confidence)) => confidence,
        _ => return None,
    };
    let consensus = match client.try_get_consensus_config() {
        Ok(Ok(config)) => config,
        _ => return None,
    };
    Some(AggregatorConfigFingerprint {
        decay_num: decay.0,
        decay_den: decay.1,
        staleness_window,
        global_min_confidence,
        consensus_k: consensus.0,
        consensus_epsilon: consensus.1,
    })
}

fn fingerprint_less(a: &AggregatorConfigFingerprint, b: &AggregatorConfigFingerprint) -> bool {
    if a.decay_num != b.decay_num {
        return a.decay_num < b.decay_num;
    }
    if a.decay_den != b.decay_den {
        return a.decay_den < b.decay_den;
    }
    if a.staleness_window != b.staleness_window {
        return a.staleness_window < b.staleness_window;
    }
    if a.global_min_confidence != b.global_min_confidence {
        return a.global_min_confidence < b.global_min_confidence;
    }
    if a.consensus_k != b.consensus_k {
        return a.consensus_k < b.consensus_k;
    }
    a.consensus_epsilon < b.consensus_epsilon
}

fn select_canonical_fingerprint(
    available: &Vec<(Address, AggregatorConfigFingerprint)>,
) -> (Option<AggregatorConfigFingerprint>, u32) {
    let mut best: Option<AggregatorConfigFingerprint> = None;
    let mut best_count = 0u32;

    for i in 0..available.len() {
        let candidate = available.get(i).unwrap().1;
        let mut count = 0u32;
        for j in 0..available.len() {
            if available.get(j).unwrap().1 == candidate {
                count += 1;
            }
        }
        match &best {
            None => {
                best = Some(candidate);
                best_count = count;
            }
            Some(current) => {
                if count > best_count
                    || (count == best_count && fingerprint_less(&candidate, current))
                {
                    best = Some(candidate);
                    best_count = count;
                }
            }
        }
    }

    (best, best_count)
}

#[contracttype]
#[derive(Clone)]
enum DataKey {
    Admin,
    Shards,
    LastShardFailure,
    ShardHealth(Address),
    /// Capability snapshot recorded at `add_shard` time.
    /// Stores a `Vec<Symbol>` of the capabilities the shard advertised via
    /// `supports_interface` when it was registered.  Used by
    /// `get_shard_capabilities` and downgrade-detection tests.
    ShardCapabilities(Address),
}
