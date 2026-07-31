use anyhow::{anyhow, bail, Context, Result};
use ledgerlens_aggregator::{LedgerLensAggregator, LedgerLensAggregatorClient};
use ledgerlens_score::{LedgerLensScoreContract, LedgerLensScoreContractClient};
use mock_amm::{MockAmm, MockAmmClient};
use mock_lending::{MockLending, MockLendingClient};
use serde::{Deserialize, Serialize};
use soroban_sdk::{
    testutils::{Address as _, EnvTestConfig, Events as _, Ledger as _},
    Address, Env, IntoVal, Symbol, Val, Vec as SorobanVec,
};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

pub const FORMAT_VERSION: u32 = 1;
pub const MAX_OPERATIONS: usize = 16;
pub const MAX_RAW_ARGUMENTS: usize = 8;
pub const MAX_SYMBOL_BYTES: usize = 32;
pub const MAX_WIRE_STRING_BYTES: usize = 64;
pub const MAX_ENCODED_CAMPAIGN_BYTES: usize = 16 * 1024;
pub const DEFAULT_CASES: usize = 64;
pub const MAX_CASES: usize = 512;

const START_TIMESTAMP: u64 = 1_700_000_000;
const GATE_THRESHOLD: u32 = 75;
const MIN_CONFIDENCE: u32 = 50;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Campaign {
    pub version: u32,
    pub name: String,
    pub seed: u64,
    pub operations: Vec<Operation>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Operation {
    SubmitScore {
        score: u32,
        confidence: u32,
        advance_seconds: u64,
    },
    AmmSwap {
        #[serde(with = "i128_decimal")]
        amount: i128,
        asset_pair: String,
    },
    AmmLiquidity {
        #[serde(with = "i128_decimal")]
        amount: i128,
    },
    LendingBorrow {
        #[serde(with = "i128_decimal")]
        amount: i128,
        asset_pair: String,
    },
    AggregatorGate {
        threshold: u32,
        asset_pair: String,
    },
    RotateAmmToUnavailable,
    RawInvoke {
        target: InvocationTarget,
        function: String,
        args: Vec<WireValue>,
    },
}

impl Operation {
    fn kind(&self) -> &'static str {
        match self {
            Self::SubmitScore { .. } => "submit_score",
            Self::AmmSwap { .. } => "amm_swap",
            Self::AmmLiquidity { .. } => "amm_liquidity",
            Self::LendingBorrow { .. } => "lending_borrow",
            Self::AggregatorGate { .. } => "aggregator_gate",
            Self::RotateAmmToUnavailable => "rotate_amm_unavailable",
            Self::RawInvoke { .. } => "raw_invoke",
        }
    }

    fn must_preserve_score_and_events(&self) -> bool {
        !matches!(self, Self::SubmitScore { .. })
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InvocationTarget {
    Score,
    Amm,
    Lending,
    Aggregator,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum WireValue {
    Wallet,
    I128(#[serde(with = "i128_decimal")] i128),
    U32(u32),
    U64(u64),
    Bool(bool),
    Symbol(String),
}

mod i128_decimal {
    use serde::{de::Error as _, Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(value: &i128, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&value.to_string())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<i128, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        value.parse().map_err(D::Error::custom)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ScoreFingerprint {
    pub score: u32,
    pub confidence: u32,
    pub timestamp: u64,
    pub model_version: u32,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Ord, PartialOrd, Serialize)]
pub struct Observation {
    pub operation: String,
    pub outcome: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ResourceReport {
    pub cpu_instructions: u64,
    pub memory_bytes: u64,
    pub encoded_input_bytes: usize,
    pub operations: usize,
    pub raw_arguments: usize,
    pub logical_score_reads: usize,
    pub logical_score_writes: usize,
    pub contract_events: u32,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CampaignReport {
    pub name: String,
    pub seed: u64,
    pub observations: Vec<Observation>,
    pub final_score: Option<ScoreFingerprint>,
    pub resources: ResourceReport,
}

impl CampaignReport {
    fn deterministic_projection(&self) -> (&[Observation], &Option<ScoreFingerprint>) {
        (&self.observations, &self.final_score)
    }

    fn coverage(&self) -> impl Iterator<Item = Observation> + '_ {
        self.observations.iter().cloned()
    }
}

struct Fixture<'a> {
    env: Env,
    wallet: Address,
    score: LedgerLensScoreContractClient<'a>,
    amm: MockAmmClient<'a>,
    lending: MockLendingClient<'a>,
    aggregator: LedgerLensAggregatorClient<'a>,
    score_id: Address,
    amm_id: Address,
    lending_id: Address,
    aggregator_id: Address,
}

fn setup<'a>() -> Fixture<'a> {
    // The corpus JSON is the intentionally small replay artifact. Disable the
    // SDK's multi-megabyte full-ledger snapshots so fuzz runs never dirty the
    // source tree or create a second, unstable fixture format.
    let env = Env::new_with_config(EnvTestConfig { capture_snapshot_at_drop: false });
    env.mock_all_auths();
    env.budget().reset_unlimited();
    env.ledger().with_mut(|ledger| ledger.timestamp = START_TIMESTAMP);

    let score_id = env.register_contract(None, LedgerLensScoreContract);
    let score = LedgerLensScoreContractClient::new(&env, &score_id);
    let admin = Address::generate(&env);
    let service = Address::generate(&env);
    score.initialize(&admin, &service);

    let amm_id = env.register_contract(None, MockAmm);
    let amm = MockAmmClient::new(&env, &amm_id);
    amm.initialize(&score_id, &GATE_THRESHOLD);
    amm.set_liquidity_gate_config(&GATE_THRESHOLD, &MIN_CONFIDENCE);

    let lending_id = env.register_contract(None, MockLending);
    let lending = MockLendingClient::new(&env, &lending_id);
    lending.initialize(&score_id, &GATE_THRESHOLD, &MIN_CONFIDENCE);

    let aggregator_id = env.register_contract(None, LedgerLensAggregator);
    let aggregator = LedgerLensAggregatorClient::new(&env, &aggregator_id);
    aggregator.initialize(&admin);
    aggregator.add_shard(&score_id);

    let wallet = Address::generate(&env);
    env.budget().reset_tracker();

    Fixture {
        env,
        wallet,
        score,
        amm,
        lending,
        aggregator,
        score_id,
        amm_id,
        lending_id,
        aggregator_id,
    }
}

pub fn validate_campaign(campaign: &Campaign) -> Result<usize> {
    if campaign.version != FORMAT_VERSION {
        bail!("unsupported corpus version {}; expected {}", campaign.version, FORMAT_VERSION);
    }
    if campaign.name.is_empty() || campaign.name.len() > MAX_WIRE_STRING_BYTES {
        bail!("campaign name must contain 1..={MAX_WIRE_STRING_BYTES} bytes");
    }
    if campaign.operations.len() > MAX_OPERATIONS {
        bail!(
            "campaign contains {} operations; maximum is {}",
            campaign.operations.len(),
            MAX_OPERATIONS
        );
    }

    for operation in &campaign.operations {
        match operation {
            Operation::AmmSwap { asset_pair, .. }
            | Operation::LendingBorrow { asset_pair, .. }
            | Operation::AggregatorGate { asset_pair, .. } => {
                validate_wire_string(asset_pair)?;
            }
            Operation::RawInvoke { function, args, .. } => {
                validate_wire_string(function)?;
                if args.len() > MAX_RAW_ARGUMENTS {
                    bail!(
                        "raw invocation contains {} arguments; maximum is {}",
                        args.len(),
                        MAX_RAW_ARGUMENTS
                    );
                }
                for arg in args {
                    if let WireValue::Symbol(value) = arg {
                        validate_wire_string(value)?;
                    }
                }
            }
            _ => {}
        }
    }

    let encoded = serde_json::to_vec(campaign).context("encoding campaign")?;
    if encoded.len() > MAX_ENCODED_CAMPAIGN_BYTES {
        bail!(
            "encoded campaign is {} bytes; maximum is {}",
            encoded.len(),
            MAX_ENCODED_CAMPAIGN_BYTES
        );
    }
    Ok(encoded.len())
}

fn validate_wire_string(value: &str) -> Result<()> {
    if value.len() > MAX_WIRE_STRING_BYTES {
        bail!("wire string is {} bytes; maximum is {}", value.len(), MAX_WIRE_STRING_BYTES);
    }
    Ok(())
}

fn symbol(env: &Env, value: &str) -> Result<Symbol> {
    if value.is_empty() || value.len() > MAX_SYMBOL_BYTES {
        bail!("symbol must contain 1..={MAX_SYMBOL_BYTES} bytes");
    }
    if !value.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'_') {
        bail!("symbol contains unsupported bytes");
    }
    Ok(Symbol::new(env, value))
}

fn score_fingerprint(fixture: &Fixture<'_>) -> Option<ScoreFingerprint> {
    let pair = Symbol::new(&fixture.env, "XLM_USDC");
    match fixture.score.try_get_score(&fixture.wallet, &pair) {
        Ok(Ok(score)) => Some(ScoreFingerprint {
            score: score.score,
            confidence: score.confidence,
            timestamp: score.timestamp,
            model_version: score.model_version,
        }),
        _ => None,
    }
}

fn raw_target<'a>(fixture: &'a Fixture<'_>, target: InvocationTarget) -> &'a Address {
    match target {
        InvocationTarget::Score => &fixture.score_id,
        InvocationTarget::Amm => &fixture.amm_id,
        InvocationTarget::Lending => &fixture.lending_id,
        InvocationTarget::Aggregator => &fixture.aggregator_id,
    }
}

fn raw_args(fixture: &Fixture<'_>, values: &[WireValue]) -> Result<SorobanVec<Val>> {
    let mut args = SorobanVec::new(&fixture.env);
    for value in values {
        let val = match value {
            WireValue::Wallet => fixture.wallet.clone().into_val(&fixture.env),
            WireValue::I128(value) => value.into_val(&fixture.env),
            WireValue::U32(value) => value.into_val(&fixture.env),
            WireValue::U64(value) => value.into_val(&fixture.env),
            WireValue::Bool(value) => value.into_val(&fixture.env),
            WireValue::Symbol(value) => symbol(&fixture.env, value)?.into_val(&fixture.env),
        };
        args.push_back(val);
    }
    Ok(args)
}

fn invoke(fixture: &Fixture<'_>, operation: &Operation) -> Result<String> {
    match operation {
        Operation::SubmitScore { score, confidence, advance_seconds } => {
            let now = fixture
                .env
                .ledger()
                .timestamp()
                .checked_add(*advance_seconds)
                .ok_or_else(|| anyhow!("ledger timestamp overflow"))?;
            fixture.env.ledger().with_mut(|ledger| ledger.timestamp = now);
            let result = fixture.score.try_submit_score(
                &SorobanVec::new(&fixture.env),
                &fixture.wallet,
                &Symbol::new(&fixture.env, "XLM_USDC"),
                score,
                &false,
                &false,
                &now,
                confidence,
                &1,
                &None,
            );
            Ok(format!("{result:?}"))
        }
        Operation::AmmSwap { amount, asset_pair } => {
            let pair = symbol(&fixture.env, asset_pair)?;
            Ok(format!("{:?}", fixture.amm.try_swap(&fixture.wallet, &pair, amount)))
        }
        Operation::AmmLiquidity { amount } => {
            Ok(format!("{:?}", fixture.amm.try_provide_liquidity_gated(&fixture.wallet, amount)))
        }
        Operation::LendingBorrow { amount, asset_pair } => {
            let pair = symbol(&fixture.env, asset_pair)?;
            Ok(format!("{:?}", fixture.lending.try_borrow(&fixture.wallet, &pair, amount)))
        }
        Operation::AggregatorGate { threshold, asset_pair } => {
            let pair = symbol(&fixture.env, asset_pair)?;
            Ok(format!(
                "{:?}",
                fixture.aggregator.try_query_risk_gate(&fixture.wallet, &pair, threshold)
            ))
        }
        Operation::RotateAmmToUnavailable => {
            let unavailable = Address::generate(&fixture.env);
            Ok(format!("{:?}", fixture.amm.try_set_risk_oracle(&unavailable)))
        }
        Operation::RawInvoke { target, function, args } => {
            let function = symbol(&fixture.env, function)?;
            let args = raw_args(fixture, args)?;
            let result = fixture.env.try_invoke_contract::<Val, soroban_sdk::Error>(
                raw_target(fixture, *target),
                &function,
                args,
            );
            Ok(format!("{result:?}"))
        }
    }
}

pub fn execute_campaign(campaign: &Campaign) -> Result<CampaignReport> {
    let encoded_input_bytes = validate_campaign(campaign)?;
    let fixture = setup();
    let mut observations = Vec::with_capacity(campaign.operations.len());
    let mut raw_arguments = 0usize;
    let mut logical_score_reads = 0usize;
    let mut logical_score_writes = 0usize;

    for (index, operation) in campaign.operations.iter().enumerate() {
        let before_score = score_fingerprint(&fixture);
        let before_events = fixture.env.events().all().len();
        logical_score_reads += 1;
        if let Operation::RawInvoke { args, .. } = operation {
            raw_arguments = raw_arguments
                .checked_add(args.len())
                .ok_or_else(|| anyhow!("raw argument counter overflow"))?;
        }

        let outcome = match invoke(&fixture, operation) {
            Ok(outcome) => outcome,
            Err(error) => format!("harness_rejected:{error}"),
        };

        let after_score = score_fingerprint(&fixture);
        let after_events = fixture.env.events().all().len();
        logical_score_reads += 1;

        if operation.must_preserve_score_and_events() {
            if before_score != after_score {
                bail!(
                    "invariant violation at operation {index} ({}): non-submission operation changed score state",
                    operation.kind()
                );
            }
            if before_events != after_events {
                bail!(
                    "invariant violation at operation {index} ({}): non-submission operation emitted a contract event",
                    operation.kind()
                );
            }
        } else if matches!(operation, Operation::SubmitScore { .. }) {
            logical_score_writes += usize::from(before_score != after_score);
        }

        observations.push(Observation { operation: operation.kind().to_owned(), outcome });
    }

    let final_score = score_fingerprint(&fixture);
    logical_score_reads += 1;
    let resources = ResourceReport {
        cpu_instructions: fixture.env.budget().cpu_instruction_cost(),
        memory_bytes: fixture.env.budget().memory_bytes_cost(),
        encoded_input_bytes,
        operations: campaign.operations.len(),
        raw_arguments,
        logical_score_reads,
        logical_score_writes,
        contract_events: fixture.env.events().all().len(),
    };

    Ok(CampaignReport {
        name: campaign.name.clone(),
        seed: campaign.seed,
        observations,
        final_score,
        resources,
    })
}

pub fn replay_campaign(campaign: &Campaign) -> Result<CampaignReport> {
    let first = execute_campaign(campaign)?;
    let second = execute_campaign(campaign)?;
    if first.deterministic_projection() != second.deterministic_projection() {
        bail!(
            "deterministic replay mismatch for campaign '{}' seed {}",
            campaign.name,
            campaign.seed
        );
    }
    Ok(first)
}

pub fn load_campaign(path: &Path) -> Result<Campaign> {
    let bytes = fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    let campaign =
        serde_json::from_slice(&bytes).with_context(|| format!("decoding {}", path.display()))?;
    validate_campaign(&campaign)?;
    Ok(campaign)
}

pub fn load_corpus(directory: &Path) -> Result<Vec<Campaign>> {
    let mut paths: Vec<PathBuf> = fs::read_dir(directory)
        .with_context(|| format!("reading corpus directory {}", directory.display()))?
        .map(|entry| entry.map(|item| item.path()))
        .collect::<std::io::Result<_>>()?;
    paths.retain(|path| path.extension().is_some_and(|extension| extension == "json"));
    paths.sort();
    if paths.is_empty() {
        bail!("corpus directory {} contains no JSON fixtures", directory.display());
    }
    paths.iter().map(|path| load_campaign(path)).collect()
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FuzzSummary {
    pub seed: u64,
    pub regression_cases: usize,
    pub generated_cases: usize,
    pub retained_cases: usize,
    pub behavior_signatures: usize,
    pub max_resources: ResourceReport,
}

#[derive(Clone, Copy)]
struct XorShift64(u64);

impl XorShift64 {
    fn new(seed: u64) -> Self {
        Self(if seed == 0 { 0x9e37_79b9_7f4a_7c15 } else { seed })
    }

    fn next(&mut self) -> u64 {
        let mut value = self.0;
        value ^= value << 13;
        value ^= value >> 7;
        value ^= value << 17;
        self.0 = value;
        value
    }

    fn index(&mut self, upper: usize) -> usize {
        (self.next() as usize) % upper
    }
}

fn random_amount(rng: &mut XorShift64) -> i128 {
    const VALUES: [i128; 7] = [i128::MIN, -1, 0, 1, 10, 1_000, i128::MAX];
    VALUES[rng.index(VALUES.len())]
}

fn random_pair(rng: &mut XorShift64) -> String {
    const VALUES: [&str; 5] = [
        "XLM_USDC",
        "BTC_USDC",
        "",
        "12345678901234567890123456789012",
        "123456789012345678901234567890123",
    ];
    VALUES[rng.index(VALUES.len())].to_owned()
}

fn random_operation(rng: &mut XorShift64) -> Operation {
    match rng.index(7) {
        0 => {
            const SCORES: [u32; 6] = [0, 74, 75, 100, 101, u32::MAX];
            const CONFIDENCE: [u32; 5] = [0, 49, 50, 100, 101];
            Operation::SubmitScore {
                score: SCORES[rng.index(SCORES.len())],
                confidence: CONFIDENCE[rng.index(CONFIDENCE.len())],
                advance_seconds: [0, 1, 3_600, 3_601, u64::MAX][rng.index(5)],
            }
        }
        1 => Operation::AmmSwap { amount: random_amount(rng), asset_pair: random_pair(rng) },
        2 => Operation::AmmLiquidity { amount: random_amount(rng) },
        3 => Operation::LendingBorrow { amount: random_amount(rng), asset_pair: random_pair(rng) },
        4 => Operation::AggregatorGate {
            threshold: [0, 74, 75, 100, 101, u32::MAX][rng.index(6)],
            asset_pair: random_pair(rng),
        },
        5 => Operation::RotateAmmToUnavailable,
        _ => Operation::RawInvoke {
            target: [
                InvocationTarget::Score,
                InvocationTarget::Amm,
                InvocationTarget::Lending,
                InvocationTarget::Aggregator,
            ][rng.index(4)],
            function: ["swap", "borrow", "query_risk_gate", "missing"][rng.index(4)].to_owned(),
            args: vec![
                WireValue::Wallet,
                WireValue::Symbol(random_pair(rng)),
                WireValue::I128(random_amount(rng)),
            ],
        },
    }
}

fn mutate(parent: &Campaign, rng: &mut XorShift64, index: usize) -> Campaign {
    let mut child = parent.clone();
    child.name = format!("generated-{index}");
    child.seed = rng.next();

    match rng.index(5) {
        0 if child.operations.len() < MAX_OPERATIONS => {
            let position = rng.index(child.operations.len() + 1);
            child.operations.insert(position, random_operation(rng));
        }
        1 if !child.operations.is_empty() => {
            let position = rng.index(child.operations.len());
            child.operations[position] = random_operation(rng);
        }
        2 if child.operations.len() > 1 => {
            let position = rng.index(child.operations.len());
            child.operations.remove(position);
        }
        3 if child.operations.len() > 1 => {
            let first = rng.index(child.operations.len());
            let second = rng.index(child.operations.len());
            child.operations.swap(first, second);
        }
        _ if !child.operations.is_empty() && child.operations.len() < MAX_OPERATIONS => {
            let position = rng.index(child.operations.len());
            let duplicate = child.operations[position].clone();
            child.operations.insert(position, duplicate);
        }
        _ => child.operations.push(random_operation(rng)),
    }
    child
}

fn update_maximum(maximum: &mut ResourceReport, current: &ResourceReport) {
    maximum.cpu_instructions = maximum.cpu_instructions.max(current.cpu_instructions);
    maximum.memory_bytes = maximum.memory_bytes.max(current.memory_bytes);
    maximum.encoded_input_bytes = maximum.encoded_input_bytes.max(current.encoded_input_bytes);
    maximum.operations = maximum.operations.max(current.operations);
    maximum.raw_arguments = maximum.raw_arguments.max(current.raw_arguments);
    maximum.logical_score_reads = maximum.logical_score_reads.max(current.logical_score_reads);
    maximum.logical_score_writes = maximum.logical_score_writes.max(current.logical_score_writes);
    maximum.contract_events = maximum.contract_events.max(current.contract_events);
}

pub fn run_fuzz(corpus: Vec<Campaign>, seed: u64, cases: usize) -> Result<FuzzSummary> {
    if cases == 0 || cases > MAX_CASES {
        bail!("cases must be within 1..={MAX_CASES}");
    }
    if corpus.is_empty() {
        bail!("at least one regression fixture is required");
    }

    let regression_cases = corpus.len();
    let mut queue = corpus;
    let mut coverage = BTreeSet::new();
    let mut maximum = ResourceReport {
        cpu_instructions: 0,
        memory_bytes: 0,
        encoded_input_bytes: 0,
        operations: 0,
        raw_arguments: 0,
        logical_score_reads: 0,
        logical_score_writes: 0,
        contract_events: 0,
    };

    for campaign in &queue {
        let report = replay_campaign(campaign)
            .with_context(|| format!("regression fixture '{}' failed", campaign.name))?;
        coverage.extend(report.coverage());
        update_maximum(&mut maximum, &report.resources);
    }

    let mut rng = XorShift64::new(seed);
    for index in 0..cases {
        let parent = queue[rng.index(queue.len())].clone();
        let child = mutate(&parent, &mut rng, index);
        let report = match replay_campaign(&child) {
            Ok(report) => report,
            Err(error) => {
                let minimized =
                    shrink_campaign(child, |candidate| replay_campaign(candidate).is_err());
                let path = persist_failure(&minimized)?;
                return Err(error).with_context(|| {
                    format!(
                        "generated case {index} failed; minimized replay saved to {}; replay with: cargo run -p invocation-fuzzer --locked -- replay {}",
                        path.display(),
                        path.display(),
                    )
                });
            }
        };
        update_maximum(&mut maximum, &report.resources);
        let discovered = report.coverage().filter(|key| coverage.insert(key.clone())).count();
        if discovered > 0 {
            queue.push(child);
        }
    }

    Ok(FuzzSummary {
        seed,
        regression_cases,
        generated_cases: cases,
        retained_cases: queue.len(),
        behavior_signatures: coverage.len(),
        max_resources: maximum,
    })
}

pub fn shrink_campaign<F>(mut campaign: Campaign, mut still_fails: F) -> Campaign
where
    F: FnMut(&Campaign) -> bool,
{
    let mut index = 0usize;
    while index < campaign.operations.len() {
        let mut candidate = campaign.clone();
        candidate.operations.remove(index);
        if still_fails(&candidate) {
            campaign = candidate;
        } else {
            index += 1;
        }
    }

    for index in 0..campaign.operations.len() {
        let simplified = match &campaign.operations[index] {
            Operation::SubmitScore { .. } => {
                Some(Operation::SubmitScore { score: 0, confidence: 0, advance_seconds: 0 })
            }
            Operation::AmmSwap { .. } => {
                Some(Operation::AmmSwap { amount: 0, asset_pair: "XLM_USDC".to_owned() })
            }
            Operation::AmmLiquidity { .. } => Some(Operation::AmmLiquidity { amount: 0 }),
            Operation::LendingBorrow { .. } => {
                Some(Operation::LendingBorrow { amount: 0, asset_pair: "XLM_USDC".to_owned() })
            }
            Operation::AggregatorGate { .. } => {
                Some(Operation::AggregatorGate { threshold: 0, asset_pair: "XLM_USDC".to_owned() })
            }
            Operation::RawInvoke { target, .. } => Some(Operation::RawInvoke {
                target: *target,
                function: "missing".to_owned(),
                args: Vec::new(),
            }),
            Operation::RotateAmmToUnavailable => None,
        };
        if let Some(simplified) = simplified {
            let mut candidate = campaign.clone();
            candidate.operations[index] = simplified;
            if still_fails(&candidate) {
                campaign = candidate;
            }
        }
    }
    campaign
}

fn persist_failure(campaign: &Campaign) -> Result<PathBuf> {
    let directory = PathBuf::from("target/invocation-fuzzer/failures");
    fs::create_dir_all(&directory).with_context(|| format!("creating {}", directory.display()))?;
    let path = directory.join(format!("seed-{}.json", campaign.seed));
    let bytes = serde_json::to_vec_pretty(campaign).context("encoding minimized failure")?;
    fs::write(&path, bytes).with_context(|| format!("writing {}", path.display()))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn campaign(operations: Vec<Operation>) -> Campaign {
        Campaign { version: FORMAT_VERSION, name: "unit".to_owned(), seed: 7, operations }
    }

    #[test]
    fn deterministic_replay_matches_observable_state() {
        let input = campaign(vec![
            Operation::SubmitScore { score: 10, confidence: 90, advance_seconds: 1 },
            Operation::AmmSwap { amount: 1, asset_pair: "XLM_USDC".to_owned() },
        ]);
        let report = replay_campaign(&input).expect("campaign should replay");
        assert_eq!(report.observations.len(), 2);
        assert_eq!(report.final_score.expect("score").score, 10);
    }

    #[test]
    fn shrink_removes_irrelevant_operations_and_simplifies_counterexample() {
        let input = campaign(vec![
            Operation::AmmLiquidity { amount: 1 },
            Operation::AmmSwap { amount: 99, asset_pair: "XLM_USDC".to_owned() },
            Operation::LendingBorrow { amount: 1, asset_pair: "XLM_USDC".to_owned() },
        ]);
        let shrunk = shrink_campaign(input, |candidate| {
            candidate
                .operations
                .iter()
                .any(|operation| matches!(operation, Operation::AmmSwap { .. }))
        });
        assert_eq!(
            shrunk.operations,
            vec![Operation::AmmSwap { amount: 0, asset_pair: "XLM_USDC".to_owned() }]
        );
    }

    #[test]
    fn bounds_reject_maximum_plus_one() {
        let mut input = campaign(Vec::new());
        input.operations =
            (0..=MAX_OPERATIONS).map(|_| Operation::AmmLiquidity { amount: 1 }).collect();
        assert!(validate_campaign(&input)
            .expect_err("maximum plus one must fail")
            .to_string()
            .contains("maximum"));
    }

    #[test]
    fn raw_argument_and_wire_string_bounds_reject_maximum_plus_one() {
        let too_many_arguments = campaign(vec![Operation::RawInvoke {
            target: InvocationTarget::Amm,
            function: "swap".to_owned(),
            args: (0..=MAX_RAW_ARGUMENTS).map(|_| WireValue::U32(0)).collect(),
        }]);
        assert!(validate_campaign(&too_many_arguments)
            .expect_err("maximum plus one raw argument must fail")
            .to_string()
            .contains("raw invocation"));

        let too_long = "x".repeat(MAX_WIRE_STRING_BYTES + 1);
        let too_long_wire_string = campaign(vec![Operation::RawInvoke {
            target: InvocationTarget::Amm,
            function: too_long,
            args: Vec::new(),
        }]);
        assert!(validate_campaign(&too_long_wire_string)
            .expect_err("maximum plus one wire byte must fail")
            .to_string()
            .contains("wire string"));
    }

    #[test]
    fn invalid_symbol_is_classified_without_soroban_allocation() {
        let input = campaign(vec![Operation::AmmSwap {
            amount: 1,
            asset_pair: "x".repeat(MAX_SYMBOL_BYTES + 1),
        }]);
        let report = replay_campaign(&input).expect("invalid symbol is a contained outcome");
        assert!(report.observations[0].outcome.contains("harness_rejected:symbol must contain"));
        assert_eq!(report.final_score, None);
    }

    #[test]
    fn empty_corpus_fails_safe() {
        assert!(run_fuzz(Vec::new(), 1, 1)
            .expect_err("empty corpus must be rejected")
            .to_string()
            .contains("at least one"));
    }

    #[test]
    fn invalid_and_unavailable_calls_preserve_score_oracle() {
        let input = campaign(vec![
            Operation::SubmitScore { score: 10, confidence: 90, advance_seconds: 1 },
            Operation::RotateAmmToUnavailable,
            Operation::AmmSwap { amount: 1, asset_pair: "XLM_USDC".to_owned() },
            Operation::RawInvoke {
                target: InvocationTarget::Amm,
                function: "missing".to_owned(),
                args: Vec::new(),
            },
        ]);
        let report = replay_campaign(&input).expect("adversarial campaign should be contained");
        assert_eq!(report.final_score.expect("score").score, 10);
    }
}
