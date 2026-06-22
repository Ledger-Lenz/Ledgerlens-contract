#![no_std]

use soroban_sdk::{contract, contractimpl, symbol_short, Address, Env, Symbol, Vec, TryFromVal};
use ledgerlens_score::{RiskScore, AggregateRiskScore, Error as ScoreError};

pub const MAX_SHARDS: usize = 10;

#[contract]
pub struct LedgerLensAggregator;

#[contractimpl]
impl LedgerLensAggregator {
    pub fn initialize(env: Env, admin: Address) -> Result<(), ScoreError> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(ScoreError::AlreadyInitialized);
        }
        env.storage().instance().set(&DataKey::Admin, &admin);
        Ok(())
    }

    pub fn get_admin(env: Env) -> Result<Address, ScoreError> {
        env.storage().instance().get(&DataKey::Admin).ok_or(ScoreError::NotInitialized)
    }

    pub fn add_shard(env: Env, shard: Address) -> Result<(), ScoreError> {
        let admin: Address = env.storage().instance().get(&DataKey::Admin).ok_or(ScoreError::NotInitialized)?;
        admin.require_auth();
        // Prevent self-reference
        let me = env.current_contract_address();
        if Address::Contract(me.clone()) == shard {
            return Err(ScoreError::InvalidAttestation); // reuse an error for self-ref guard
        }
        let mut shards: Vec<Address> = env.storage().instance().get(&DataKey::Shards).unwrap_or_else(|| Vec::new(&env));
        // Check duplicate
        for i in 0..shards.len() {
            if shards.get(i).unwrap() == shard {
                return Err(ScoreError::Unauthorized); // reuse
            }
        }
        if shards.len() as usize >= MAX_SHARDS {
            return Err(ScoreError::ServiceSetFull); // reuse
        }
        shards.push_back(shard);
        env.storage().instance().set(&DataKey::Shards, &shards);
        Ok(())
    }

    pub fn remove_shard(env: Env, shard: Address) -> Result<(), ScoreError> {
        let admin: Address = env.storage().instance().get(&DataKey::Admin).ok_or(ScoreError::NotInitialized)?;
        admin.require_auth();
        let mut shards: Vec<Address> = env.storage().instance().get(&DataKey::Shards).unwrap_or_else(|| Vec::new(&env));
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
            return Err(ScoreError::SignerNotInSet); // reuse
        }
        env.storage().instance().set(&DataKey::Shards, &out);
        Ok(())
    }

    pub fn get_shards(env: Env) -> Vec<Address> {
        env.storage().instance().get(&DataKey::Shards).unwrap_or_else(|| Vec::new(&env))
    }

    pub fn query_risk_gate(env: Env, wallet: Address, asset_pair: Symbol, gate_threshold: u32) -> bool {
        let shards: Vec<Address> = env.storage().instance().get(&DataKey::Shards).unwrap_or_else(|| Vec::new(&env));
        if shards.is_empty() {
            return false;
        }
        for i in 0..shards.len() {
            let shard = shards.get(i).unwrap();
            // build client
            let client = ledgerlens_score::LedgerLensScoreContractClient::new(&env, &shard);
            // try call
            let ok: Result<bool, _> = client.try_query_risk_gate(&wallet, &asset_pair, &gate_threshold);
            match ok {
                Ok(res) => {
                    if !res { return false; }
                }
                Err(_) => { return false; }
            }
        }
        true
    }

    pub fn get_score(env: Env, wallet: Address, asset_pair: Symbol) -> Result<RiskScore, ScoreError> {
        let shards: Vec<Address> = env.storage().instance().get(&DataKey::Shards).unwrap_or_else(|| Vec::new(&env));
        let mut best: Option<RiskScore> = None;
        for i in 0..shards.len() {
            let shard = shards.get(i).unwrap();
            let client = ledgerlens_score::LedgerLensScoreContractClient::new(&env, &shard);
            let res: Result<RiskScore, _> = client.try_get_score(&wallet, &asset_pair);
            if let Ok(score) = res {
                match &best {
                    None => best = Some(score),
                    Some(b) => {
                        if score.score > b.score {
                            best = Some(score);
                        }
                    }
                }
            }
        }
        best.ok_or(ScoreError::ScoreNotFound)
    }

    pub fn get_aggregate_score(env: Env, wallet: Address) -> Result<AggregateRiskScore, ScoreError> {
        let shards: Vec<Address> = env.storage().instance().get(&DataKey::Shards).unwrap_or_else(|| Vec::new(&env));
        let mut best: Option<AggregateRiskScore> = None;
        for i in 0..shards.len() {
            let shard = shards.get(i).unwrap();
            let client = ledgerlens_score::LedgerLensScoreContractClient::new(&env, &shard);
            let res: Result<AggregateRiskScore, _> = client.try_get_aggregate_score(&wallet);
            if let Ok(agg) = res {
                match &best {
                    None => best = Some(agg),
                    Some(b) => {
                        if agg.aggregate_score > b.aggregate_score {
                            best = Some(agg);
                        }
                    }
                }
            }
        }
        best.ok_or(ScoreError::ScoreNotFound)
    }

    pub fn supports_interface(env: Env, capability: Symbol) -> bool {
        let caps = vec![symbol_short!("score"), symbol_short!("gate"), symbol_short!("aggr"), symbol_short!("federated")];
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
            let client = ledgerlens_score::LedgerLensScoreContractClient::new(&env, &shard);
            let res: Result<RiskScore, _> = client.try_get_score(&wallet, &asset_pair);
            if let Ok(score) = res {
                out.push_back((shard.clone(), Some(score)));
            } else {
                out.push_back((shard.clone(), None));
            }
        }
        out
    }

    /// Queries the contagion depth across all shards, returning the maximum depth found.
    ///
    /// Returns the highest counterparty count for the wallet/pair across all registered shards.
    pub fn get_contagion_depth_across_shards(
        env: Env,
        wallet: Address,
        asset_pair: Symbol,
    ) -> u32 {
        let shards: Vec<Address> = env.storage().instance().get(&DataKey::Shards).unwrap_or_else(|| Vec::new(&env));
        let mut max_depth: u32 = 0;
        for i in 0..shards.len() {
            let shard = shards.get(i).unwrap();
            let client = ledgerlens_score::LedgerLensScoreContractClient::new(&env, &shard);
            let res: Result<u32, _> = client.try_get_contagion_depth(&wallet, &asset_pair);
            if let Ok(depth) = res {
                if depth > max_depth {
                    max_depth = depth;
                }
            }
        }
        max_depth
    }
}

#[contracttype]
#[derive(Clone)]
enum DataKey {
    Admin,
    Shards,
}
