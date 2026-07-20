#![no_std]

#[cfg(test)]
extern crate std;

#[cfg(test)]
mod test;

use soroban_sdk::{contract, contracterror, contractimpl, contracttype, symbol_short, vec, Address, Env, Symbol, Vec};
use ledgerlens_score::{RiskScore, AggregateRiskScore};

pub const MAX_SHARDS: usize = 10;

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConflictPolicy {
    HighestScore,
    MostRecent,
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    ShardNotFound = 3,
    ShardAlreadyExists = 4,
    ShardSetFull = 5,
    SelfShard = 6,
    ScoreNotFound = 7,
}

#[contract]
pub struct LedgerLensAggregator;

#[contractimpl]
impl LedgerLensAggregator {
    pub fn initialize(env: Env, admin: Address) -> Result<(), Error> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(Error::AlreadyInitialized);
        }
        env.storage().instance().set(&DataKey::Admin, &admin);
        Ok(())
    }

    pub fn get_admin(env: Env) -> Result<Address, Error> {
        env.storage().instance().get(&DataKey::Admin).ok_or(Error::NotInitialized)
    }

    pub fn get_decay_rate(_env: Env) -> (u64, u64) {
        const DECAY_NUMERATOR: u64 = 999;
        const DECAY_DENOMINATOR: u64 = 1000;
        (DECAY_NUMERATOR, DECAY_DENOMINATOR)
    }

    pub fn get_consensus_threshold_k(_env: Env) -> u32 {
        const CONSENSUS_THRESHOLD_K: u32 = 5;
        CONSENSUS_THRESHOLD_K
    }

    pub fn get_watchlist_status(_env: Env, _wallet: Address) -> bool {
        false
    }

    pub fn add_shard(env: Env, shard: Address) -> Result<(), Error> {
        let admin: Address = env.storage().instance().get(&DataKey::Admin).ok_or(Error::NotInitialized)?;
        admin.require_auth();
        if env.current_contract_address() == shard {
            return Err(Error::SelfShard);
        }
        let mut shards: Vec<Address> = env.storage().instance().get(&DataKey::Shards).unwrap_or_else(|| Vec::new(&env));
        for i in 0..shards.len() {
            if shards.get(i).unwrap() == shard {
                return Err(Error::ShardAlreadyExists);
            }
        }
        if shards.len() as usize >= MAX_SHARDS {
            return Err(Error::ShardSetFull);
        }
        shards.push_back(shard);
        env.storage().instance().set(&DataKey::Shards, &shards);
        Ok(())
    }

    pub fn remove_shard(env: Env, shard: Address) -> Result<(), Error> {
        let admin: Address = env.storage().instance().get(&DataKey::Admin).ok_or(Error::NotInitialized)?;
        admin.require_auth();
        let shards: Vec<Address> = env.storage().instance().get(&DataKey::Shards).unwrap_or_else(|| Vec::new(&env));
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
            return Err(Error::ShardNotFound);
        }
        env.storage().instance().set(&DataKey::Shards, &out);
        env.storage().instance().remove(&DataKey::ShardHealth(shard));
        Ok(())
    }

    pub fn get_shards(env: Env) -> Vec<Address> {
        env.storage().instance().get(&DataKey::Shards).unwrap_or_else(|| Vec::new(&env))
    }

    pub fn set_shard_health(env: Env, shard: Address, healthy: bool) -> Result<(), Error> {
        let admin: Address = env.storage().instance().get(&DataKey::Admin).ok_or(Error::NotInitialized)?;
        admin.require_auth();
        let shards: Vec<Address> = env.storage().instance().get(&DataKey::Shards).unwrap_or_else(|| Vec::new(&env));
        let mut found = false;
        for i in 0..shards.len() {
            if shards.get(i).unwrap() == shard {
                found = true;
                break;
            }
        }
        if !found {
            return Err(Error::ShardNotFound);
        }
        env.storage().instance().set(&DataKey::ShardHealth(shard), &healthy);
        Ok(())
    }

    pub fn get_shard_health(env: Env, shard: Address) -> bool {
        env.storage().instance().get(&DataKey::ShardHealth(shard)).unwrap_or(true)
    }

    pub fn query_risk_gate(env: Env, wallet: Address, asset_pair: Symbol, gate_threshold: u32) -> bool {
        let shards: Vec<Address> = env.storage().instance().get(&DataKey::Shards).unwrap_or_else(|| Vec::new(&env));
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
                Ok(Ok(res)) => {
                    if !res {
                        return false;
                    }
                }
                _ => return false,
            }
        }
        true
    }

    pub fn set_conflict_resolution_policy(env: Env, policy: ConflictPolicy) -> Result<(), Error> {
        let admin: Address = env.storage().instance().get(&DataKey::Admin).ok_or(Error::NotInitialized)?;
        admin.require_auth();
        env.storage().instance().set(&DataKey::ConflictPolicy, &policy);
        Ok(())
    }

    pub fn get_conflict_resolution_policy(env: Env) -> ConflictPolicy {
        env.storage().instance().get(&DataKey::ConflictPolicy).unwrap_or(ConflictPolicy::HighestScore)
    }

    pub fn get_score(env: Env, wallet: Address, asset_pair: Symbol) -> Result<RiskScore, Error> {
        let shards: Vec<Address> = env.storage().instance().get(&DataKey::Shards).unwrap_or_else(|| Vec::new(&env));
        let policy: ConflictPolicy = env.storage().instance().get(&DataKey::ConflictPolicy).unwrap_or(ConflictPolicy::HighestScore);
        let mut best: Option<RiskScore> = None;
        for i in 0..shards.len() {
            let shard = shards.get(i).unwrap();
            if !is_shard_healthy(&env, &shard) {
                continue;
            }
            let client = ledgerlens_score::LedgerLensScoreContractClient::new(&env, &shard);
            if let Ok(Ok(score)) = client.try_get_score(&wallet, &asset_pair) {
                match &best {
                    None => best = Some(score),
                    Some(b) => {
                        let replace = match policy {
                            ConflictPolicy::HighestScore => score.score > b.score,
                            ConflictPolicy::MostRecent => score.timestamp > b.timestamp,
                        };
                        if replace {
                            best = Some(score);
                        }
                    }
                }
            }
        }
        best.ok_or(Error::ScoreNotFound)
    }

    pub fn get_aggregate_score(env: Env, wallet: Address) -> Result<AggregateRiskScore, Error> {
        let shards: Vec<Address> = env.storage().instance().get(&DataKey::Shards).unwrap_or_else(|| Vec::new(&env));
        let policy: ConflictPolicy = env.storage().instance().get(&DataKey::ConflictPolicy).unwrap_or(ConflictPolicy::HighestScore);
        let mut best: Option<AggregateRiskScore> = None;
        for i in 0..shards.len() {
            let shard = shards.get(i).unwrap();
            if !is_shard_healthy(&env, &shard) {
                continue;
            }
            let client = ledgerlens_score::LedgerLensScoreContractClient::new(&env, &shard);
            if let Ok(Ok(agg)) = client.try_get_aggregate_score(&wallet) {
                match &best {
                    None => best = Some(agg),
                    Some(b) => {
                        let replace = match policy {
                            ConflictPolicy::HighestScore => agg.aggregate_score > b.aggregate_score,
                            ConflictPolicy::MostRecent => agg.last_updated > b.last_updated,
                        };
                        if replace {
                            best = Some(agg);
                        }
                    }
                }
            }
        }
        best.ok_or(Error::ScoreNotFound)
    }

    pub fn supports_interface(env: Env, capability: Symbol) -> bool {
        let caps = vec![&env, symbol_short!("score"), symbol_short!("gate"), symbol_short!("aggr"), symbol_short!("federated")];
        for i in 0..caps.len() {
            if caps.get(i).unwrap() == capability {
                return true;
            }
        }
        false
    }

    pub fn get_score_across_shards(env: Env, wallet: Address, asset_pair: Symbol) -> Vec<(Address, Option<RiskScore>)> {
        let shards: Vec<Address> = env.storage().instance().get(&DataKey::Shards).unwrap_or_else(|| Vec::new(&env));
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

    pub fn contagion_depth_across_shards(
        env: Env,
        wallet: Address,
        asset_pair: Symbol,
    ) -> u32 {
        let shards: Vec<Address> = env.storage().instance().get(&DataKey::Shards).unwrap_or_else(|| Vec::new(&env));
        let mut max_depth: u32 = 0;
        for i in 0..shards.len() {
            let shard = shards.get(i).unwrap();
            if !is_shard_healthy(&env, &shard) {
                continue;
            }
            let client = ledgerlens_score::LedgerLensScoreContractClient::new(&env, &shard);
            if let Ok(Ok(depth)) = client.try_get_contagion_depth(&wallet, &asset_pair) {
                if depth > max_depth {
                    max_depth = depth;
                }
            }
        }
        max_depth
    }
}

fn is_shard_healthy(env: &Env, shard: &Address) -> bool {
    env.storage().instance().get(&DataKey::ShardHealth(shard.clone())).unwrap_or(true)
}

#[contracttype]
#[derive(Clone)]
enum DataKey {
    Admin,
    Shards,
    ConflictPolicy,
    ShardHealth(Address),
}
