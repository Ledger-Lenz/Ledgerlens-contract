//! Chaos test harness: random pause/unpause interleaved with submissions
//! (issue #306).
//!
//! The harness runs 1000 randomised sequences of:
//!   - `pause_pair`   — freeze a randomly chosen pair
//!   - `unpause_pair` — unfreeze a randomly chosen pair
//!   - `submit`       — single submit_score for a random (wallet, pair)
//!   - `batch_submit` — 1–5 entries covering random (wallet, pair) combinations
//!
//! After every operation, a set of invariants is checked:
//!   I1. A paused pair NEVER has a score written after the ledger timestamp
//!       at which the pause took effect.
//!   I2. Storage invariants: every score stored is in [0, 100]; confidence
//!       in [0, 100]; timestamp > 0.
//!   I3. `get_score` for any (wallet, pair) that has never been submitted to
//!       (while it was live) returns `ScoreNotFound` — no phantom data.
//!   I4. `get_admin()` always returns the same address (no corruption).
//!
//! Implementation note: to avoid relying on an in-memory shadow of which
//! (wallet, pair) combinations actually hold live scores we track a small
//! oracle set of pairs that have *ever* received an accepted submission.

#![cfg(test)]

use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    Address, Env, Symbol, Vec,
};

use crate::{Error, LedgerLensScoreContract, LedgerLensScoreContractClient, ScoreSubmission};

const START_TS: u64 = 1_700_000_000;
/// Number of chaos sequences to run.
const SEQUENCES: u32 = 1_000;
/// Operations per sequence.
const OPS_PER_SEQ: u32 = 20;

// ── Deterministic PRNG ────────────────────────────────────────────────────────

struct Xorshift64(u64);
impl Xorshift64 {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    fn range(&mut self, lo: u64, hi: u64) -> u64 {
        lo + self.next() % (hi - lo + 1)
    }
}

// ── Pair / wallet pool ────────────────────────────────────────────────────────

const NUM_PAIRS: u32 = 4;
const NUM_WALLETS: u32 = 6;

fn make_pairs(env: &Env) -> [Symbol; NUM_PAIRS as usize] {
    [
        Symbol::new(env, "PA"),
        Symbol::new(env, "PB"),
        Symbol::new(env, "PC"),
        Symbol::new(env, "PD"),
    ]
}

fn make_wallets(env: &Env) -> Vec<Address> {
    let mut v = Vec::new(env);
    for _ in 0..NUM_WALLETS {
        v.push_back(Address::generate(env));
    }
    v
}

// ── Harness setup ─────────────────────────────────────────────────────────────

fn setup<'a>() -> (Env, LedgerLensScoreContractClient<'a>, Address) {
    let env = Env::default();
    env.mock_all_auths();
    env.budget().reset_unlimited();
    env.ledger().with_mut(|l| l.timestamp = START_TS);
    let id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &id);
    let admin = Address::generate(&env);
    let service = Address::generate(&env);
    client.initialize(&admin, &service);
    (env, client, admin)
}

// ── Oracle ────────────────────────────────────────────────────────────────────

/// Minimal oracle: tracks what the harness believes to be true about the world.
struct Oracle {
    /// For each pair index: true iff currently paused.
    paused: [bool; NUM_PAIRS as usize],
    /// Timestamp at which each pair last became paused (0 = never / currently live).
    pause_ts: [u64; NUM_PAIRS as usize],
    /// For each (wallet_idx, pair_idx): the most recently accepted score, if any.
    scores: [[Option<u32>; NUM_PAIRS as usize]; NUM_WALLETS as usize],
    /// True iff (wallet_idx, pair_idx) has ever had an accepted submission.
    ever_submitted: [[bool; NUM_PAIRS as usize]; NUM_WALLETS as usize],
}

impl Oracle {
    fn new() -> Self {
        Self {
            paused: [false; NUM_PAIRS as usize],
            pause_ts: [0; NUM_PAIRS as usize],
            scores: [[None; NUM_PAIRS as usize]; NUM_WALLETS as usize],
            ever_submitted: [[false; NUM_PAIRS as usize]; NUM_WALLETS as usize],
        }
    }
    fn pause(&mut self, pi: usize, ts: u64) {
        self.paused[pi] = true;
        self.pause_ts[pi] = ts;
    }
    fn unpause(&mut self, pi: usize) {
        self.paused[pi] = false;
        // Timestamp intentionally kept so we can detect stale writes.
    }
    fn record_accept(&mut self, wi: usize, pi: usize, score: u32) {
        self.scores[wi][pi] = Some(score);
        self.ever_submitted[wi][pi] = true;
    }
}

// ── Invariant checker ─────────────────────────────────────────────────────────

fn check_invariants(
    env: &Env,
    client: &LedgerLensScoreContractClient,
    oracle: &Oracle,
    pairs: &[Symbol; NUM_PAIRS as usize],
    wallets: &Vec<Address>,
    admin: &Address,
    seq: u32,
    op: u32,
) {
    // I4: admin is intact.
    assert_eq!(client.get_admin(), *admin, "seq={seq} op={op}: admin corrupted");

    for pi in 0..NUM_PAIRS as usize {
        let pair = &pairs[pi];
        for wi in 0..NUM_WALLETS as usize {
            let wallet = wallets.get(wi as u32).unwrap();

            let stored = match client.try_get_score(&wallet, pair) {
                Ok(s) => Some(s),
                Err(Ok(Error::ScoreNotFound)) => None,
                Err(Ok(e)) => panic!("seq={seq} op={op}: unexpected error {:?}", e),
                Err(Err(_)) => panic!("seq={seq} op={op}: host panic on get_score"),
            };

            // I3: phantom data check.
            if !oracle.ever_submitted[wi][pi] {
                assert!(
                    stored.is_none(),
                    "seq={seq} op={op}: phantom score for wi={wi} pi={pi}: {:?}", stored
                );
            }

            // I2: stored score values are in valid ranges.
            if let Some(ref s) = stored {
                assert!(s.score <= 100, "seq={seq} op={op}: score {} > 100", s.score);
                assert!(s.confidence <= 100, "seq={seq} op={op}: confidence {} > 100", s.confidence);
                assert!(s.timestamp > 0, "seq={seq} op={op}: timestamp == 0");
            }

            // I1: if pair is paused, no new score after pause_ts.
            if oracle.paused[pi] {
                if let Some(ref s) = stored {
                    if let Some(oracle_score) = oracle.scores[wi][pi] {
                        assert_eq!(
                            s.score, oracle_score,
                            "seq={seq} op={op}: score changed for paused pair wi={wi} pi={pi}"
                        );
                    }
                    // score.timestamp must be <= pause_ts (was written before pause).
                    assert!(
                        s.timestamp <= oracle.pause_ts[pi] || oracle.pause_ts[pi] == 0,
                        "seq={seq} op={op}: score timestamp {} > pause_ts {} \
                         for paused pair wi={wi} pi={pi}",
                        s.timestamp, oracle.pause_ts[pi]
                    );
                }
            }
        }
    }
}

// ── Main chaos test ───────────────────────────────────────────────────────────

#[test]
fn chaos_pause_unpause_submit_1000_sequences() {
    let mut rng = Xorshift64(0xC8A0_5FED_1234_5678);

    for seq in 0..SEQUENCES {
        let (env, client, admin) = setup();
        let pairs = make_pairs(&env);
        let wallets = make_wallets(&env);
        let mut oracle = Oracle::new();
        // Per-pair cooldown tracker: last_submit_ts[wi][pi].
        let mut last_submit: [[u64; NUM_PAIRS as usize]; NUM_WALLETS as usize] =
            [[0; NUM_PAIRS as usize]; NUM_WALLETS as usize];

        for op in 0..OPS_PER_SEQ {
            // Advance time 0–3700 seconds.
            let advance = rng.range(0, 3_700) as u64;
            env.ledger().with_mut(|l| l.timestamp = l.timestamp.saturating_add(advance));
            let now = env.ledger().timestamp();

            let op_kind = rng.range(0, 3);
            match op_kind {
                0 => {
                    // pause_pair
                    let pi = rng.range(0, NUM_PAIRS as u64 - 1) as usize;
                    client.set_pair_paused(&pairs[pi], &true);
                    oracle.pause(pi, now);
                }
                1 => {
                    // unpause_pair
                    let pi = rng.range(0, NUM_PAIRS as u64 - 1) as usize;
                    client.set_pair_paused(&pairs[pi], &false);
                    oracle.unpause(pi);
                }
                2 => {
                    // single submit
                    let wi = rng.range(0, NUM_WALLETS as u64 - 1) as usize;
                    let pi = rng.range(0, NUM_PAIRS as u64 - 1) as usize;
                    let wallet = wallets.get(wi as u32).unwrap();
                    let score = rng.range(0, 100) as u32;
                    let ts = now.max(1);
                    let res = client.try_submit_score(
                        &Vec::new(&env),
                        &wallet, &pairs[pi],
                        &score, &false, &false, &ts, &80, &1, &None,
                    );
                    match res {
                        Ok(()) => {
                            oracle.record_accept(wi, pi, score);
                            last_submit[wi][pi] = now;
                        }
                        Err(Ok(Error::PairPaused)) => {
                            assert!(oracle.paused[pi],
                                "seq={seq} op={op}: PairPaused but oracle says not paused pi={pi}");
                        }
                        Err(Ok(Error::RateLimitExceeded)) => {
                            // Rate limit is expected when cooldown has not elapsed
                            // or velocity cap fires — both are valid rejections.
                        }
                        Err(Ok(_)) => {} // other rejections acceptable
                        Err(Err(_)) => panic!("seq={seq} op={op}: host panic on submit"),
                    }
                }
                _ => {
                    // batch submit (2–4 entries)
                    let batch_size = rng.range(2, 4) as usize;
                    let mut batch: Vec<ScoreSubmission> = Vec::new(&env);
                    // Track wi, pi, score for each entry in a fixed-size array.
                    let mut wi_arr = [0usize; 4];
                    let mut pi_arr = [0usize; 4];
                    let mut sc_arr = [0u32; 4];
                    for idx in 0..batch_size {
                        let wi = rng.range(0, NUM_WALLETS as u64 - 1) as usize;
                        let pi = rng.range(0, NUM_PAIRS as u64 - 1) as usize;
                        let score = rng.range(0, 100) as u32;
                        let wallet = wallets.get(wi as u32).unwrap();
                        wi_arr[idx] = wi;
                        pi_arr[idx] = pi;
                        sc_arr[idx] = score;
                        batch.push_back(ScoreSubmission {
                            wallet,
                            asset_pair: pairs[pi].clone(),
                            score,
                            benford_flag: false,
                            ml_flag: false,
                            timestamp: now.max(1),
                            confidence: 80,
                            model_version: 1,
                        });
                    }
                    match client.try_submit_scores_batch(&batch) {
                        Ok(result) => {
                            for idx in 0..result.results.len() {
                                let entry_res = result.results.get(idx).unwrap();
                                let wi = wi_arr[idx as usize];
                                let pi = pi_arr[idx as usize];
                                let score = sc_arr[idx as usize];
                                if entry_res.accepted {
                                    oracle.record_accept(wi, pi, score);
                                    last_submit[wi][pi] = now;
                                }
                            }
                        }
                        Err(Ok(Error::ContractPaused)) => {} // global pause
                        Err(_) => {}
                    }
                }
            }

            check_invariants(&env, &client, &oracle, &pairs, &wallets, &admin, seq, op);
        }
    }
}

// ── Additional focused chaos invariant tests ──────────────────────────────────

/// Paused pair never accepts a new score regardless of how many submissions
/// are attempted back-to-back.
#[test]
fn chaos_paused_pair_never_accepts_score() {
    let (env, client, _admin) = setup();
    let pair = Symbol::new(&env, "PA");
    let wallets = make_wallets(&env);
    let mut rng = Xorshift64(0xAAAA_BBBB_CCCC_DDDD);

    // Submit a baseline score for all wallets before pausing.
    for wi in 0..NUM_WALLETS as u32 {
        let wallet = wallets.get(wi).unwrap();
        client.submit_score(
            &Vec::new(&env), &wallet, &pair,
            &(wi * 10), &false, &false,
            &(START_TS + wi as u64), &80, &1, &None,
        );
    }

    // Pause the pair.
    client.set_pair_paused(&pair, &true);
    let baseline_ts = env.ledger().timestamp();

    // 200 submission attempts — all must fail.
    for _ in 0..200 {
        env.ledger().with_mut(|l| l.timestamp = l.timestamp.saturating_add(3_601));
        let wi = rng.range(0, NUM_WALLETS as u64 - 1) as u32;
        let wallet = wallets.get(wi).unwrap();
        let score = rng.range(0, 100) as u32;
        let res = client.try_submit_score(
            &Vec::new(&env), &wallet, &pair,
            &score, &false, &false,
            &(env.ledger().timestamp()), &80, &1, &None,
        );
        assert_eq!(res, Err(Ok(Error::PairPaused)));
        // Score must remain the pre-pause value.
        let stored = client.get_score(&wallet, &pair);
        assert_eq!(stored.score, wi * 10);
        assert!(stored.timestamp <= baseline_ts + wi as u64 + 1,
            "score timestamp advanced after pause for wi={wi}");
    }
}

/// After unpausing, submissions are accepted again and invariants hold.
#[test]
fn chaos_unpause_restores_submission() {
    let (env, client, _admin) = setup();
    let pair = Symbol::new(&env, "PA");
    let wallet = Address::generate(&env);

    // Submit, pause, attempt (fail), unpause, submit (succeed).
    client.submit_score(
        &Vec::new(&env), &wallet, &pair,
        &30, &false, &false, &START_TS, &80, &1, &None,
    );
    client.set_pair_paused(&pair, &true);
    env.ledger().with_mut(|l| l.timestamp = START_TS + 3_601);
    let res = client.try_submit_score(
        &Vec::new(&env), &wallet, &pair,
        &40, &false, &false, &(START_TS + 3_601), &80, &1, &None,
    );
    assert_eq!(res, Err(Ok(Error::PairPaused)));
    assert_eq!(client.get_score(&wallet, &pair).score, 30);

    client.set_pair_paused(&pair, &false);
    env.ledger().with_mut(|l| l.timestamp = START_TS + 7_202);
    client.submit_score(
        &Vec::new(&env), &wallet, &pair,
        &45, &false, &false, &(START_TS + 7_202), &80, &1, &None,
    );
    assert_eq!(client.get_score(&wallet, &pair).score, 45);
}
