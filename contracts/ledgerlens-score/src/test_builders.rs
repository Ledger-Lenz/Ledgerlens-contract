use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Ledger as _},
    Address, Env, Symbol, Vec,
};

use crate::{LedgerLensScoreContract, LedgerLensScoreContractClient};

pub(crate) struct BuiltState<'a> {
    pub env: Env,
    pub client: LedgerLensScoreContractClient<'a>,
    pub admin: Address,
    pub service: Address,
    pub wallet: Address,
    pub pair: Symbol,
    pub service_signers: Vec<Address>,
    pub admin_signers: Vec<Address>,
}

#[derive(Default)]
pub(crate) struct ContractStateBuilder {
    service_signer_count: u32,
    service_threshold: Option<u32>,
    admin_signer_count: u32,
    admin_threshold: Option<u32>,
    finality_buffer: u64,
    paused: bool,
    score_history: &'static [(u64, u32, u32)],
}

impl ContractStateBuilder {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn with_service_multisig(mut self, signer_count: u32, threshold: u32) -> Self {
        self.service_signer_count = signer_count;
        self.service_threshold = Some(threshold);
        self
    }

    pub(crate) fn with_admin_multisig(mut self, signer_count: u32, threshold: u32) -> Self {
        self.admin_signer_count = signer_count;
        self.admin_threshold = Some(threshold);
        self
    }

    pub(crate) fn with_finality_buffer(mut self, finality_buffer: u64) -> Self {
        self.finality_buffer = finality_buffer;
        self
    }

    pub(crate) fn paused(mut self, paused: bool) -> Self {
        self.paused = paused;
        self
    }

    pub(crate) fn with_score_history(mut self, score_history: &'static [(u64, u32, u32)]) -> Self {
        self.score_history = score_history;
        self
    }

    pub(crate) fn build<'a>(self) -> BuiltState<'a> {
        let env = Env::default();
        env.mock_all_auths();
        env.budget().reset_unlimited();
        env.ledger().with_mut(|ledger| ledger.timestamp = 100_000);

        let contract_id = env.register_contract(None, LedgerLensScoreContract);
        let client = LedgerLensScoreContractClient::new(&env, &contract_id);
        let admin = Address::generate(&env);
        let service = Address::generate(&env);
        let wallet = Address::generate(&env);
        let pair = symbol_short!("XLM_USDC");

        client.initialize(&admin, &service);

        let legacy_admin = Vec::new(&env);

        let mut service_signers = Vec::new(&env);
        for _ in 0..self.service_signer_count {
            let signer = Address::generate(&env);
            client.add_service_signer(&legacy_admin, &signer);
            service_signers.push_back(signer);
        }
        if let Some(threshold) = self.service_threshold {
            client.set_service_threshold(&legacy_admin, &threshold);
        }

        let mut admin_signers = Vec::new(&env);
        for _ in 0..self.admin_signer_count {
            let signer = Address::generate(&env);
            client.add_admin_signer(&legacy_admin, &signer);
            admin_signers.push_back(signer);
        }
        if let Some(threshold) = self.admin_threshold {
            client.set_admin_threshold(&legacy_admin, &threshold);
        }

        if self.finality_buffer > 0 {
            client.set_finality_buffer(&legacy_admin, &self.finality_buffer);
        }

        if self.paused {
            client.pause(&legacy_admin);
        }

        for (timestamp, score, confidence) in self.score_history.iter().copied() {
            env.ledger().with_mut(|ledger| ledger.timestamp = timestamp);
            client.submit_score(
                &Vec::new(&env),
                &wallet,
                &pair,
                &score,
                &false,
                &false,
                &timestamp,
                &confidence,
                &1,
                &None,
            );
        }

        BuiltState { env, client, admin, service, wallet, pair, service_signers, admin_signers }
    }
}
