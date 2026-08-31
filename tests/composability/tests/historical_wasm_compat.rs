//! Historical-WASM compatibility suite for the stable consumer surface.
//!
//! The frozen fixture is a locked release build of commit `8336828`, the final
//! v3 main-line revision before the v4 storage and ABI changes.
//! Tests exercise compatibility in both directions:
//!
//! - current mock AMM/lending consumers call the historical WASM;
//! - the client generated from the historical WASM calls the current contract.
//!
//! No production entry point, storage key, event, or error discriminant is
//! changed by this suite.

use ledgerlens_score::{LedgerLensScoreContract, LedgerLensScoreContractClient, RiskScore};
use mock_amm::{FailPolicy as AmmFailPolicy, MockAmm, MockAmmClient, MockAmmError};
use mock_lending::{MockLending, MockLendingClient, MockLendingError};
use sha2::{Digest, Sha256};
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, EnvTestConfig, Events as _, Ledger as _},
    xdr::{
        ContractDataDurability, LedgerKey, Limits, ReadXdr, ScSpecEntry, ScSpecTypeDef, ScVal,
        WriteXdr,
    },
    Address, Env, IntoVal, TryFromVal, Val, Vec,
};

const HISTORICAL_WASM: &[u8] =
    include_bytes!("../../fixtures/historical/ledgerlens-score-v3-8336828.wasm");
const HISTORICAL_LOCK: &[u8] =
    include_bytes!("../../fixtures/historical/ledgerlens-score-v3-8336828.Cargo.lock");
const HISTORICAL_MANIFEST: &str =
    include_str!("../../fixtures/historical/ledgerlens-score-v3-8336828.manifest");
const HISTORICAL_ABI_GOLDEN: &str =
    include_str!("../../fixtures/historical/ledgerlens-score-v3-8336828.abi");
const CURRENT_ABI_GOLDEN: &str =
    include_str!("../../fixtures/historical/ledgerlens-score-current.abi");

const MAX_FIXTURE_BYTES: usize = 512 * 1024;
const MAX_SPEC_BYTES: usize = 256 * 1024;
const MAX_MANIFEST_BYTES: usize = 4 * 1024;
const MAX_GATE_CPU_INSNS: u64 = 25_000_000;
const MAX_GATE_MEMORY_BYTES: u64 = 4_500_000;
const MAX_SILENCE_EVENT_PAYLOAD_BYTES: usize = 1_024;
const GATE_THRESHOLD: u32 = 75;
const MIN_CONFIDENCE: u32 = 50;
const HISTORICAL_SOURCE_COMMIT: &str = "8336828159b7e7ff05d018200ce7f7a385bdade5";

#[allow(clippy::too_many_arguments)]
mod historical_client {
    soroban_sdk::contractimport!(file = "../fixtures/historical/ledgerlens-score-v3-8336828.wasm");
}

#[derive(Debug, Eq, PartialEq)]
enum EncodedSizeError {
    Empty,
    TooLarge,
}

fn validate_encoded_size(size: usize, maximum: usize) -> Result<(), EncodedSizeError> {
    if size == 0 {
        return Err(EncodedSizeError::Empty);
    }
    if size > maximum {
        return Err(EncodedSizeError::TooLarge);
    }
    Ok(())
}

fn manifest_value(key: &str) -> &str {
    assert!(HISTORICAL_MANIFEST.len() <= MAX_MANIFEST_BYTES, "fixture manifest must stay bounded");
    let prefix = format!("{key}=");
    let mut matches = HISTORICAL_MANIFEST.lines().filter_map(|line| line.strip_prefix(&prefix));
    let value = matches.next().unwrap_or_else(|| panic!("manifest key `{key}` is missing"));
    assert!(matches.next().is_none(), "manifest key `{key}` is duplicated");
    value
}

fn xdr_string(bytes: &[u8]) -> String {
    std::str::from_utf8(bytes).expect("contract spec names must be UTF-8").to_owned()
}

fn type_name(type_: &ScSpecTypeDef) -> String {
    match type_ {
        ScSpecTypeDef::Val => "val".into(),
        ScSpecTypeDef::Bool => "bool".into(),
        ScSpecTypeDef::Void => "void".into(),
        ScSpecTypeDef::Error => "error".into(),
        ScSpecTypeDef::U32 => "u32".into(),
        ScSpecTypeDef::I32 => "i32".into(),
        ScSpecTypeDef::U64 => "u64".into(),
        ScSpecTypeDef::I64 => "i64".into(),
        ScSpecTypeDef::Timepoint => "timepoint".into(),
        ScSpecTypeDef::Duration => "duration".into(),
        ScSpecTypeDef::U128 => "u128".into(),
        ScSpecTypeDef::I128 => "i128".into(),
        ScSpecTypeDef::U256 => "u256".into(),
        ScSpecTypeDef::I256 => "i256".into(),
        ScSpecTypeDef::Bytes => "bytes".into(),
        ScSpecTypeDef::String => "string".into(),
        ScSpecTypeDef::Symbol => "symbol".into(),
        ScSpecTypeDef::Address => "address".into(),
        ScSpecTypeDef::Option(option) => {
            format!("option<{}>", type_name(&option.value_type))
        }
        ScSpecTypeDef::Result(result) => {
            format!("result<{},{}>", type_name(&result.ok_type), type_name(&result.error_type))
        }
        ScSpecTypeDef::Vec(vec) => format!("vec<{}>", type_name(&vec.element_type)),
        ScSpecTypeDef::Map(map) => {
            format!("map<{},{}>", type_name(&map.key_type), type_name(&map.value_type))
        }
        ScSpecTypeDef::Tuple(tuple) => {
            let values = tuple.value_types.iter().map(type_name).collect::<std::vec::Vec<_>>();
            format!("tuple<{}>", values.join(","))
        }
        ScSpecTypeDef::BytesN(bytes) => format!("bytes_n<{}>", bytes.n),
        ScSpecTypeDef::Udt(udt) => xdr_string(udt.name.as_ref()),
    }
}

fn stable_abi_golden(entries: &[ScSpecEntry]) -> String {
    const FUNCTIONS: [&str; 5] = [
        "get_version",
        "initialize",
        "query_risk_gate",
        "query_risk_gate_with_confidence",
        "supports_interface",
    ];

    let mut lines = std::vec::Vec::new();

    for expected_name in FUNCTIONS {
        let function = entries
            .iter()
            .find_map(|entry| match entry {
                ScSpecEntry::FunctionV0(function)
                    if xdr_string(function.name.0.as_ref()) == expected_name =>
                {
                    Some(function)
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("stable function `{expected_name}` is missing"));
        let inputs = function
            .inputs
            .iter()
            .map(|input| format!("{}:{}", xdr_string(input.name.as_ref()), type_name(&input.type_)))
            .collect::<std::vec::Vec<_>>()
            .join(",");
        let output = function.outputs.first().map(type_name).unwrap_or_else(|| "void".into());
        lines.push(format!("function {expected_name}({inputs})->{output}"));
    }

    let risk_score = entries
        .iter()
        .find_map(|entry| match entry {
            ScSpecEntry::UdtStructV0(struct_)
                if xdr_string(struct_.name.as_ref()) == "RiskScore" =>
            {
                Some(struct_)
            }
            _ => None,
        })
        .expect("RiskScore spec must be present");
    let fields = risk_score
        .fields
        .iter()
        .map(|field| format!("{}:{}", xdr_string(field.name.as_ref()), type_name(&field.type_)))
        .collect::<std::vec::Vec<_>>()
        .join(",");
    lines.push(format!("struct RiskScore({fields})"));

    format!("{}\n", lines.join("\n"))
}

fn historical_abi_golden() -> String {
    let entries = soroban_spec::read::from_wasm(HISTORICAL_WASM)
        .expect("historical contract spec must parse");
    stable_abi_golden(&entries)
}

fn current_abi_golden() -> String {
    fn entry(bytes: impl AsRef<[u8]>) -> ScSpecEntry {
        ScSpecEntry::from_xdr(bytes.as_ref(), Limits::none())
            .expect("current native spec XDR must decode")
    }

    let entries = [
        entry(LedgerLensScoreContract::spec_xdr_get_version()),
        entry(LedgerLensScoreContract::spec_xdr_initialize()),
        entry(LedgerLensScoreContract::spec_xdr_query_risk_gate()),
        entry(LedgerLensScoreContract::spec_xdr_query_risk_gate_with_confidence()),
        entry(LedgerLensScoreContract::spec_xdr_supports_interface()),
        entry(RiskScore::spec_xdr()),
    ];
    stable_abi_golden(&entries)
}

struct HistoricalFixture<'a> {
    env: Env,
    score: LedgerLensScoreContractClient<'a>,
    amm: MockAmmClient<'a>,
    lending: MockLendingClient<'a>,
}

fn test_env() -> Env {
    Env::new_with_config(EnvTestConfig { capture_snapshot_at_drop: false })
}

fn setup_historical<'a>() -> HistoricalFixture<'a> {
    let env = test_env();
    env.mock_all_auths();
    // Uploading the 209 KiB fixture is harness setup rather than the operation
    // under measurement. Individual resource tests reset the tracker after
    // deployment and before invoking a consumer.
    env.budget().reset_unlimited();
    env.ledger().with_mut(|ledger| {
        ledger.sequence_number = 100;
        ledger.timestamp = 1_700_000_000;
    });

    let score_id = env.register_contract_wasm(None, HISTORICAL_WASM);
    let score = LedgerLensScoreContractClient::new(&env, &score_id);
    let admin = Address::generate(&env);
    let service = Address::generate(&env);
    score.initialize(&admin, &service);
    assert_eq!(score.get_version(), 3, "fixture must remain the v3 compatibility baseline");

    let amm_id = env.register_contract(None, MockAmm);
    let amm = MockAmmClient::new(&env, &amm_id);
    amm.initialize(&admin, &score_id, &GATE_THRESHOLD);
    amm.set_liquidity_gate_config(
        &admin,
        &GATE_THRESHOLD,
        &MIN_CONFIDENCE,
        &AmmFailPolicy::FailClosed,
        &604_800,
        &0,
    );

    let lending_id = env.register_contract(None, MockLending);
    let lending = MockLendingClient::new(&env, &lending_id);
    lending.initialize(&admin, &score_id, &GATE_THRESHOLD, &MIN_CONFIDENCE);

    HistoricalFixture { env, score, amm, lending }
}

fn submit_score(fixture: &HistoricalFixture<'_>, wallet: &Address, score: u32, confidence: u32) {
    fixture.env.ledger().with_mut(|ledger| {
        ledger.timestamp += 3_601;
        ledger.sequence_number += 1;
    });
    fixture.score.submit_score(
        &Vec::new(&fixture.env),
        wallet,
        &symbol_short!("XLM_USDC"),
        &score,
        &false,
        &false,
        &fixture.env.ledger().timestamp(),
        &confidence,
        &1,
        &None,
    );
}

fn persistent_entries(env: &Env) -> std::vec::Vec<String> {
    let mut entries = env
        .to_ledger_snapshot()
        .ledger_entries
        .into_iter()
        .filter_map(|(key, entry)| match key.as_ref() {
            LedgerKey::ContractData(data)
                if data.durability == ContractDataDurability::Persistent =>
            {
                Some(format!("{key:?}={entry:?}"))
            }
            LedgerKey::ContractCode(_) => Some(format!("{key:?}={entry:?}")),
            _ => None,
        })
        .collect::<std::vec::Vec<_>>();
    entries.sort();
    entries
}

fn event_payload_bytes(env: &Env, topics: &Vec<Val>, data: &Val) -> usize {
    let topics_val: Val = topics.clone().into_val(env);
    let topics = ScVal::try_from_val(env, &topics_val).expect("event topics must convert to XDR");
    let data = ScVal::try_from_val(env, data).expect("event data must convert to XDR");
    topics.to_xdr(Limits::none()).unwrap().len() + data.to_xdr(Limits::none()).unwrap().len()
}

fn assert_gate_budget(label: &str, env: &Env) -> (u64, u64) {
    let cpu = env.budget().cpu_instruction_cost();
    let memory = env.budget().memory_bytes_cost();
    assert!(cpu <= MAX_GATE_CPU_INSNS, "{label} CPU regression: {cpu} > {MAX_GATE_CPU_INSNS}");
    assert!(
        memory <= MAX_GATE_MEMORY_BYTES,
        "{label} memory regression: {memory} > {MAX_GATE_MEMORY_BYTES}"
    );
    (cpu, memory)
}

#[test]
fn fixture_manifest_hash_size_and_spec_are_reproducible() {
    assert_eq!(manifest_value("source_commit"), HISTORICAL_SOURCE_COMMIT);
    assert_eq!(manifest_value("source_contract_version"), "3");
    assert_eq!(manifest_value("soroban_sdk"), "21.7.7");
    validate_encoded_size(HISTORICAL_WASM.len(), MAX_FIXTURE_BYTES).unwrap();
    assert_eq!(HISTORICAL_WASM.len(), manifest_value("wasm_bytes").parse::<usize>().unwrap());
    assert_eq!(MAX_FIXTURE_BYTES, manifest_value("wasm_max_bytes").parse::<usize>().unwrap());

    let digest = format!("{:x}", Sha256::digest(HISTORICAL_WASM));
    assert_eq!(digest, manifest_value("wasm_sha256"));
    assert_eq!(HISTORICAL_LOCK.len(), manifest_value("lock_bytes").parse::<usize>().unwrap());
    let lock_digest = format!("{:x}", Sha256::digest(HISTORICAL_LOCK));
    assert_eq!(lock_digest, manifest_value("lock_sha256"));

    let raw_spec =
        soroban_spec::read::raw_from_wasm(HISTORICAL_WASM).expect("fixture needs contractspecv0");
    validate_encoded_size(raw_spec.len(), MAX_SPEC_BYTES).unwrap();
    assert_eq!(raw_spec.len(), manifest_value("spec_bytes").parse::<usize>().unwrap());
    assert_eq!(MAX_SPEC_BYTES, manifest_value("spec_max_bytes").parse::<usize>().unwrap());
}

#[test]
fn encoded_size_boundaries_are_explicit() {
    assert_eq!(validate_encoded_size(0, MAX_FIXTURE_BYTES), Err(EncodedSizeError::Empty));
    assert_eq!(validate_encoded_size(1, MAX_FIXTURE_BYTES), Ok(()));
    assert_eq!(validate_encoded_size(MAX_FIXTURE_BYTES, MAX_FIXTURE_BYTES), Ok(()));
    assert_eq!(
        validate_encoded_size(MAX_FIXTURE_BYTES + 1, MAX_FIXTURE_BYTES),
        Err(EncodedSizeError::TooLarge)
    );

    assert_eq!(validate_encoded_size(0, MAX_SPEC_BYTES), Err(EncodedSizeError::Empty));
    assert_eq!(validate_encoded_size(1, MAX_SPEC_BYTES), Ok(()));
    assert_eq!(validate_encoded_size(MAX_SPEC_BYTES, MAX_SPEC_BYTES), Ok(()));
    assert_eq!(
        validate_encoded_size(MAX_SPEC_BYTES + 1, MAX_SPEC_BYTES),
        Err(EncodedSizeError::TooLarge)
    );
}

#[test]
fn historical_abi_matches_the_reviewed_golden_surface() {
    assert_eq!(historical_abi_golden(), HISTORICAL_ABI_GOLDEN);
}

#[test]
fn current_abi_matches_the_reviewed_golden_surface() {
    assert_eq!(current_abi_golden(), CURRENT_ABI_GOLDEN);
}

#[test]
fn current_amm_consumer_accepts_and_rejects_historical_wasm_scores() {
    let fixture = setup_historical();
    let safe = Address::generate(&fixture.env);
    let risky = Address::generate(&fixture.env);
    let unknown = Address::generate(&fixture.env);
    submit_score(&fixture, &safe, 10, 90);
    submit_score(&fixture, &risky, 90, 90);

    assert_eq!(fixture.amm.try_swap(&safe, &symbol_short!("XLM_USDC"), &1_000), Ok(Ok(())));
    assert_eq!(
        fixture.amm.try_swap(&risky, &symbol_short!("XLM_USDC"), &1_000),
        Err(Ok(MockAmmError::HighRiskWallet))
    );
    assert_eq!(
        fixture.amm.try_swap(&unknown, &symbol_short!("XLM_USDC"), &1_000),
        Err(Ok(MockAmmError::HighRiskWallet))
    );
}

#[test]
fn current_lending_consumer_preserves_confidence_semantics_on_historical_wasm() {
    let fixture = setup_historical();
    let safe = Address::generate(&fixture.env);
    let low_confidence = Address::generate(&fixture.env);
    submit_score(&fixture, &safe, 10, 90);
    submit_score(&fixture, &low_confidence, 10, 20);

    assert_eq!(fixture.lending.try_borrow(&safe, &symbol_short!("XLM_USDC"), &1_000), Ok(Ok(())));
    assert_eq!(
        fixture.lending.try_borrow(&low_confidence, &symbol_short!("XLM_USDC"), &1_000),
        Err(Ok(MockLendingError::RiskGateRejected))
    );
}

#[test]
fn historical_client_calls_the_current_stable_gate_surface() {
    let env = test_env();
    env.mock_all_auths();
    env.ledger().with_mut(|ledger| {
        ledger.sequence_number = 100;
        ledger.timestamp = 1_700_000_000;
    });
    let current_id = env.register_contract(None, LedgerLensScoreContract);
    let current = LedgerLensScoreContractClient::new(&env, &current_id);
    let admin = Address::generate(&env);
    let service = Address::generate(&env);
    current.initialize(&admin, &service);
    let wallet = Address::generate(&env);
    env.ledger().with_mut(|ledger| {
        ledger.sequence_number += 1;
        ledger.timestamp += 3_601;
    });
    current.submit_score(
        &Vec::new(&env),
        &wallet,
        &symbol_short!("XLM_USDC"),
        &10,
        &false,
        &false,
        &env.ledger().timestamp(),
        &90,
        &1,
        &None,
    );

    let legacy = historical_client::Client::new(&env, &current_id);
    assert!(legacy.query_risk_gate(&wallet, &symbol_short!("XLM_USDC"), &GATE_THRESHOLD));
    assert!(legacy.query_risk_gate_with_confidence(
        &wallet,
        &symbol_short!("XLM_USDC"),
        &GATE_THRESHOLD,
        &MIN_CONFIDENCE,
    ));
}

#[test]
fn direct_score_consumer_gate_stays_bounded_and_has_no_persistent_side_effect() {
    let fixture = setup_historical();
    let wallet = Address::generate(&fixture.env);
    submit_score(&fixture, &wallet, 74, 50);

    let persistent_before = persistent_entries(&fixture.env);
    let event_count_before = fixture.env.events().all().len();
    let total_entries_before = fixture.env.to_ledger_snapshot().ledger_entries.len();

    fixture.env.budget().reset_unlimited();
    fixture.env.budget().reset_tracker();
    assert_eq!(fixture.amm.try_swap(&wallet, &symbol_short!("XLM_USDC"), &i128::MAX), Ok(Ok(())));
    let (cpu, memory) = assert_gate_budget("historical direct-score AMM gate", &fixture.env);
    let total_entries_after = fixture.env.to_ledger_snapshot().ledger_entries.len();
    eprintln!(
        "historical AMM gate resources: cpu={cpu}, memory={memory}, ledger_entry_delta={}",
        total_entries_after as i64 - total_entries_before as i64
    );

    assert_eq!(
        persistent_entries(&fixture.env),
        persistent_before,
        "consumer gate must not write persistent state"
    );
    assert_eq!(
        fixture.env.events().all().len(),
        event_count_before,
        "successful gate emits no events"
    );
    assert_eq!(
        total_entries_after, total_entries_before,
        "active-service gate path must not add temporary or persistent entries"
    );
}

#[test]
fn delegated_consumer_gate_is_the_bounded_read_only_worst_case() {
    let fixture = setup_historical();
    let wallet = Address::generate(&fixture.env);
    let custodian = Address::generate(&fixture.env);
    submit_score(&fixture, &custodian, 74, 50);
    fixture.score.set_score_delegate(&wallet, &custodian);

    let persistent_before = persistent_entries(&fixture.env);
    let event_count_before = fixture.env.events().all().len();
    let total_entries_before = fixture.env.to_ledger_snapshot().ledger_entries.len();

    fixture.env.budget().reset_unlimited();
    fixture.env.budget().reset_tracker();
    assert_eq!(fixture.amm.try_swap(&wallet, &symbol_short!("XLM_USDC"), &i128::MAX), Ok(Ok(())));
    let (cpu, memory) = assert_gate_budget("historical delegated AMM gate", &fixture.env);
    let total_entries_after = fixture.env.to_ledger_snapshot().ledger_entries.len();
    eprintln!(
        "historical delegated AMM gate resources: cpu={cpu}, memory={memory}, ledger_entry_delta={}",
        total_entries_after as i64 - total_entries_before as i64
    );

    assert_eq!(persistent_entries(&fixture.env), persistent_before);
    assert_eq!(fixture.env.events().all().len(), event_count_before);
    assert_eq!(total_entries_after, total_entries_before);
}

#[test]
fn silence_transition_has_bounded_resource_write_and_event_behavior() {
    let fixture = setup_historical();
    let wallet = Address::generate(&fixture.env);
    submit_score(&fixture, &wallet, 74, 50);
    fixture.env.ledger().with_mut(|ledger| {
        ledger.timestamp += 3_601;
        ledger.sequence_number += 1;
    });

    let persistent_before = persistent_entries(&fixture.env);
    let events_before = fixture.env.events().all();
    let total_entries_before = fixture.env.to_ledger_snapshot().ledger_entries.len();

    fixture.env.budget().reset_unlimited();
    fixture.env.budget().reset_tracker();
    assert_eq!(fixture.amm.try_swap(&wallet, &symbol_short!("XLM_USDC"), &i128::MAX), Ok(Ok(())));
    let (cpu, memory) = assert_gate_budget("historical silence-transition AMM gate", &fixture.env);
    let events_after = fixture.env.events().all();
    let total_entries_after = fixture.env.to_ledger_snapshot().ledger_entries.len();
    let (_, topics, data) =
        events_after.get(events_before.len()).expect("silence transition must emit one event");
    let event_bytes = event_payload_bytes(&fixture.env, &topics, &data);
    eprintln!(
        "historical silence-transition resources: cpu={cpu}, memory={memory}, \
         ledger_entry_delta={}, event_payload_bytes={event_bytes}",
        total_entries_after as i64 - total_entries_before as i64
    );

    assert_eq!(events_after.len(), events_before.len() + 1);
    assert!(
        event_bytes <= MAX_SILENCE_EVENT_PAYLOAD_BYTES,
        "silence event payload regression: {event_bytes} > {MAX_SILENCE_EVENT_PAYLOAD_BYTES}"
    );
    assert_ne!(
        persistent_entries(&fixture.env),
        persistent_before,
        "first silence transition must persist its one-shot alert flag"
    );
    assert_eq!(
        total_entries_after, total_entries_before,
        "silence flag updates the existing contract instance entry"
    );

    let persistent_after_transition = persistent_entries(&fixture.env);
    fixture.env.budget().reset_unlimited();
    fixture.env.budget().reset_tracker();
    assert_eq!(fixture.amm.try_swap(&wallet, &symbol_short!("XLM_USDC"), &i128::MAX), Ok(Ok(())));
    assert_gate_budget("historical repeated-silence AMM gate", &fixture.env);
    assert_eq!(fixture.env.events().all().len(), events_after.len());
    assert_eq!(persistent_entries(&fixture.env), persistent_after_transition);
}
