#[cfg(test)]
mod test_event_schema {
    use soroban_sdk::{
        symbol_short,
        testutils::{Address as _, Events as _},
        Address, Env, IntoVal, Symbol, Vec,
    };

    use crate::{events::EVENT_VERSION, LedgerLensScoreContract, LedgerLensScoreContractClient};

    #[test]
    fn test_all_events_carry_schema_version() {
        let env = Env::default();
        env.mock_all_auths();

        let contract_id = env.register_contract(None, LedgerLensScoreContract);
        let client = LedgerLensScoreContractClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let service = Address::generate(&env);

        // This triggers a subset of events (initialization, watch, etc.)
        client.initialize(&admin, &service);
        let wallet = Address::generate(&env);
        client.set_watchlist(&Vec::new(&env), &wallet, &true);

        let all_events = env.events().all();

        // Assert every single event emitted by this contract has EVENT_VERSION as its second topic.
        for (addr, topics, _data) in all_events.iter() {
            if addr == contract_id {
                assert!(topics.len() >= 2, "event topic array too short to contain schema version");
                // First topic is event name, second is schema version
                let version_topic: u32 = topics.get(1).unwrap().into_val(&env);
                assert_eq!(
                    version_topic, EVENT_VERSION,
                    "event missing correct schema version in topics"
                );
            }
        }
    }

    /// Regression test for the `pair_weight_reset` gap documented in
    /// `docs/invariants.md` §4b: every other event in this contract carries
    /// `EVENT_VERSION` as the second topic element, but `pair_weight_reset`'s
    /// topic tuple is `(name, asset_pair)` — missing the version entirely.
    ///
    /// `test_all_events_carry_schema_version` above never catches this
    /// because it never triggers `bulk_reset_pair_weight` (the only caller of
    /// `pair_weight_reset`). This test triggers that path and pins the
    /// corrected versioned topic shape so future changes cannot silently
    /// break indexers.
    #[test]
    fn test_pair_weight_reset_carries_schema_version() {
        let env = Env::default();
        env.mock_all_auths();

        let contract_id = env.register_contract(None, LedgerLensScoreContract);
        let client = LedgerLensScoreContractClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let service = Address::generate(&env);
        client.initialize(&admin, &service);

        let pair = symbol_short!("XLM_USDC");
        client.set_pair_weight(&Vec::new(&env), &pair, &3);

        let mut pairs = Vec::new(&env);
        pairs.push_back(pair.clone());
        client.bulk_reset_pair_weight(&Vec::new(&env), &pairs);

        let all_events = env.events().all();
        let mut found_reset_event = false;
        for (addr, topics, _data) in all_events.iter() {
            if addr != contract_id {
                continue;
            }
            let name: Symbol = topics.get(0).unwrap().into_val(&env);
            if name != symbol_short!("pw_rst") {
                continue;
            }
            found_reset_event = true;
            assert!(
                topics.len() >= 2,
                "pair_weight_reset event topic array too short to contain schema version \
                 (found {} topics, expected at least 2)",
                topics.len()
            );
            let version_topic: u32 = topics.get(1).unwrap().into_val(&env);
            assert_eq!(
                version_topic, EVENT_VERSION,
                "pair_weight_reset event missing EVENT_VERSION in topics"
            );
        }
        assert!(
            found_reset_event,
            "bulk_reset_pair_weight did not emit a pair_weight_reset event as expected"
        );
    }
}
