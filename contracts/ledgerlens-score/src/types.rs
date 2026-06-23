use soroban_sdk::{contracttype, Address, Symbol, BytesN};


// Embargo expiry configuration stored per wallet in temporary storage.
//
// - `Indefinite` — embargo has no built-in expiry; only `lift_score_embargo`
//   removes it.
// - `Until(ts)` — embargo auto-expires when `ledger_timestamp > ts`; no
//   admin action needed once the timestamp is reached.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EmbargoExpiry {
    Indefinite,
    Until(u64),
}

// On-chain record of the latest LedgerLens risk assessment for a
// wallet / asset-pair combination. Written by `submit_score` and
// read by `get_score`.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RiskScore {
    // Overall risk score, 0-100. Higher = more suspicious.
    pub score: u32,
    // True if the Benford's Law engine flagged this entity.
    pub benford_flag: bool,
    // True if the ML ensemble classifier flagged this entity.
    pub ml_flag: bool,
    // Ledger timestamp when this score was computed off-chain.
    pub timestamp: u64,
    // Model confidence for this score, 0-100.
    pub confidence: u32,
    // Integer version of the detection-pipeline model that produced
    // this score.  Allows consumers to detect stale scores when the
    // pipeline is retrained.
    pub model_version: u32,
}

// A single entry in a batch score submission.  Mirrors the fields of
// `submit_score` so the service can write many scores in one call.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScoreSubmission {
    pub wallet: Address,
    pub asset_pair: Symbol,
    pub score: u32,
    pub benford_flag: bool,
    pub ml_flag: bool,
    pub timestamp: u64,
    pub confidence: u32,
    pub model_version: u32,
}

// Cross-asset aggregate risk view for a single wallet — a weighted
// average of every per-pair `RiskScore` the wallet currently has.
// Returned by `get_aggregate_score`; see that function's rustdoc for the
// exact formula and complexity bound.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AggregateRiskScore {
    // Weighted average of all contributing per-pair scores, 0-100.
    pub aggregate_score: u32,
    // Number of distinct asset pairs the wallet has a score for.
    pub pair_count: u32,
    // The highest individual per-pair score across all of the wallet's pairs.
    pub max_pair_score: u32,
    // The asset pair that produced `max_pair_score`.
    pub max_pair: Symbol,
    // Count of the wallet's pairs with `benford_flag == true`.
    pub benford_flag_count: u32,
    // Count of the wallet's pairs with `ml_flag == true`.
    pub ml_flag_count: u32,
    // Ledger timestamp of the most recently updated component score.
    pub last_updated: u64,
    // True when the aggregate was computed with a non-zero decay rate applied.
    // Allows callers to detect whether aging has affected the aggregate score.
    pub decay_lambda_applied: bool,
}

// A cryptographic attestation over a score payload, produced by the
// off-chain detection pipeline's secp256k1 signing key.
//
// See `docs/attestation-spec.md` for the exact commitment serialization
// this is checked against. Passed to `submit_score` only when the admin
// has configured a service public key via `set_service_pubkey` — see that
// function's rustdoc for the opt-in enforcement model.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScoreAttestation {
    // SHA-256 commitment over the canonical score payload. The contract
    // always recomputes this independently from the call's actual
    // arguments and rejects the call if it disagrees with this field — the
    // field exists so a mismatch surfaces as `InvalidAttestation` instead
    // of a confusing signature-recovery failure, not as a trusted input.
    pub commitment: BytesN<32>,
    // 65-byte secp256k1 ECDSA signature over `commitment`: 32-byte `r`,
    // 32-byte `s`, then a 1-byte recovery id which must be `0` or `1`.
    pub signature: BytesN<65>,
}

// A single model's contribution to an ensemble consensus submission.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModelSubmission {
    pub model_version: u32,
    pub score: u32,
    pub confidence: u32,
    pub benford_flag: bool,
    pub ml_flag: bool,
    pub attestation: ScoreAttestation,
}

// Result for a single entry in a batch score submission.
// Returned as part of `BatchResult` from `submit_scores_batch` so the
// caller knows exactly which entries succeeded and why any failed,
// without needing to re-query each (wallet, pair) individually.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BatchEntryResult {
    // Zero-based index of this entry in the submitted batch.
    pub index: u32,
    // True if the entry was written to storage.
    pub accepted: bool,
    // Set to the Error code if rejected; 0 if accepted.
    pub rejection_code: u32,
}

// Structured result from `submit_scores_batch` containing per-entry
// outcomes so the caller knows exactly which entries succeeded and why
// any failed, without needing to re-query each (wallet, pair) individually.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BatchResult {
    // Number of entries that were successfully written to storage.
    pub accepted_count: u32,
    // Number of entries that were rejected.
    pub rejected_count: u32,
    // Per-entry results in the same order as the submitted batch.
    pub results: soroban_sdk::Vec<BatchEntryResult>,
}

// Merkle-root attestation for an entire `submit_scores_batch_attested`
// call: a single secp256k1 signature over the Merkle root of every entry
// in the batch. See `docs/batch-attestation-spec.md` for the off-chain
// tree-construction algorithm, the on-chain verification path, and the
// rationale for choosing domain-separated prefix hashing (RFC 9162 style,
// `0x00` for leaves / `0x01` for internal nodes) over the alternative
// sorted-pair scheme.
//
// The signature format is intentionally byte-identical to that of
// [`ScoreAttestation`] — 65 bytes: 32-byte `r`, 32-byte `s`, 1-byte
// recovery id — so the same off-chain signing key can be reused for both
// per-score and per-batch paths, and so `verify_signature` can be a
// single shared helper.
//
// # Verified-digest convention
//
// `signature` is over `SHA256(merkle_root)`, **not** over `merkle_root`
// directly. This is a one-extra-hash convention forced by the soroban-sdk
// 21.x API: `env.crypto().secp256k1_recover` takes an opaque `Hash<32>`,
// and `Hash<32>` has no public constructor — it can only be built via a
// host crypto function call. Both sides wrap through SHA-256 once:
//
// * **Off-chain** (`api`/`core` pipeline): build `root`, then sign
//   `SHA256(root)`.
// * **On-chain** (`submit_scores_batch_attested`): wrap
//   `attestation.merkle_root` through `env.crypto().sha256` once before
//   calling `verify_signature`.
//
// The pipeline produces exactly the same merkle_root as the verifier
// recomputes from the entry commitments — the SHA-256 wrap is purely a
// soroban-sdk compatibility shim, not a security downgrade.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BatchAttestation {
    // `SHA-256(0x01 || SHA-256(0x00 || commit_i) || SHA-256(0x00 || commit_{i+1}))`
    // (recursively, until the 32-byte root). Bound to one specific
    // deployment on one specific network by including the contract
    // address and `network_id` inside every leaf's underlying commitment
    // (see [`ScoreAttestation`]'s preimage layout for context).
    //
    // The contract does **not** sign this value directly — see the struct
    // rustdoc's "Verified-digest convention" above for the SHA-256 wrap.
    pub merkle_root: BytesN<32>,
    // 65-byte secp256k1 ECDSA signature over `SHA256(merkle_root)`: 32-byte `r`,
    // 32-byte `s`, then a 1-byte recovery id which must be `0` or `1`.
    pub signature: BytesN<65>,
}

// A single entry in an attested batch score submission. Mirrors
// [`ScoreSubmission`] so the service can submit many scores in one call,
// and carries its own Merkle inclusion proof against the
// [`BatchAttestation`]'s `merkle_root`.
//
// # `proof_flags` bit layout
//
// `proof_flags` is a `u32` bit field that records, for every level of the
// Merkle tree from the leaf (`i == 0`) upward, whether the sibling at that
// level sits to the **left** (1) or right (0) of the current node being
// walked up. So an attestation for leaf index `5` in an 8-leaf tree will
// typically produce three flag bits; a single-entry batch produces
// `proof_flags == 0` and an empty `proof` ([`verify_merkle_proof`] handles
// this case explicitly).
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScoreSubmissionWithProof {
    pub submission: ScoreSubmission,
    // Sibling hashes from leaf to root, left-to-right (ordered to match
    // `proof_flags`'s LSB-first indexing). Length must be in
    // `[0, MAX_MERKLE_PROOF_DEPTH]` — anything longer is rejected with
    // `Error::InvalidAttestation`.
    pub proof: soroban_sdk::Vec<BytesN<32>>,
    // Bit-field encoding the left/right direction at each level. Bit `i`
    // (LSB = 0) is `0` if the sibling at level `i` is to the right, `1` if
    // to the left.
    pub proof_flags: u32,
}

// A pending, time-locked contract WASM upgrade.
//
// Created by `propose_upgrade` and cleared by `execute_upgrade` /
// `veto_upgrade`. While one exists, any observer can read it via
// `get_pending_upgrade` to inspect the committed WASM hash and the earliest
// time the upgrade can take effect — the basis of the community monitoring
// window described in the README's Upgrade Governance section.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UpgradeProposal {
    // Hash of the new contract WASM the admin has committed to installing.
    pub new_wasm_hash: BytesN<32>,
    // Ledger timestamp when the proposal was created.
    pub proposed_at: u64,
    // Earliest ledger timestamp at which `execute_upgrade` may run
    // (`proposed_at + upgrade_delay_secs`).
    pub executable_after: u64,
    // The admin address that created the proposal — recorded for the audit
    // trail so a veto can attribute the original proposer.
    pub proposed_by: Address,
}

// Per-(wallet, asset_pair) trend state persisted between submissions.
//
// `trend` encodes direction as a signed integer: `+1` = rising, `0` = flat,
// `-1` = falling. `consecutive` counts how many consecutive submissions have
// had the same non-zero direction; it is `0` on the first submission and
// resets to `1` on every direction change. Flat submissions (`delta == 0`)
// set both fields to `0`.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScoreTrend {
    // +1 = rising, 0 = flat / no history, -1 = falling.
    pub trend: i32,
    // Number of consecutive submissions in the current trend direction.
    // 0 on first submission or after a flat submission.
    pub consecutive: u32,
}

// Global configuration for the per-wallet score submission floor.
//
// Returned by `get_score_floor_policy` and configured by
// `set_score_floor_policy`. When `enabled`, any `(wallet, asset_pair)`
// whose historical peak score has reached `high_water_mark` can no longer
// receive a submission below `floor_value` — a second line of defence
// against a compromised or colluding signer laundering a known high-risk
// wallet's reputation by zeroing its score. Disabled by default.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScoreFloorPolicy {
    // Kill-switch: when `false`, no floor is enforced for any wallet.
    pub enabled: bool,
    // Historical peak score at or above which the floor begins to apply for
    // a given `(wallet, asset_pair)`. Bounded to `[50, 100]`.
    pub high_water_mark: u32,
    // Minimum score a high-risk wallet may be assigned while the floor
    // applies. Submissions below this are rejected with `BelowScoreFloor`.
    // Bounded to `[0, high_water_mark - 1]`.
    pub floor_value: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SnapshotRecord {
    pub root: BytesN<32>,
    pub leaf_count: u64,
    pub committed_at: u64,      // ledger timestamp
    pub committed_by: Address,  // who called commit_snapshot
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    Service,
    // Latest risk score for a (wallet, asset_pair) pair.
    Score(Address, Symbol),
    // Boolean flag — true when the contract is paused.
    Paused,
    // Per-asset-pair pause flag. True when `set_pair_paused(pair, true)` has
    // been called and not yet reversed.
    PairPaused(Symbol),
    // Ordered list of all currently paused asset pairs — an incrementally
    // maintained index so `get_paused_pairs` is O(1).
    PausedPairIndex,
    // Pending new admin address during a two-step admin transfer.
    PendingAdmin,
    // Per-wallet watchlist flag (true = high-priority monitoring).
    Watchlist(Address),
    // Ordered ring buffer of the last N risk scores for a wallet/pair.
    ScoreHistory(Address, Symbol),
    // Ordered, de-duplicated list of asset pairs a wallet has a score for.
    AssetPairs(Address),
    // Per-asset-pair weight used by the aggregate risk computation.
    // Defaults to 1 (simple average) when unset.
    PairWeight(Symbol),
    // Cached snapshot of the most recently computed aggregate risk score
    // for a wallet, refreshed as a side effect of `submit_score` /
    // `submit_scores_batch`. `get_aggregate_score` never reads this cache —
    // it always recomputes from the live per-pair scores — so this key
    // exists purely as a cheap snapshot for off-chain indexers.
    AggregateScore(Address),
    // The single in-flight time-locked upgrade proposal, if any.
    PendingUpgrade,
    // Admin-configured delay (seconds) between proposing and executing an
    // upgrade. Defaults to `DEFAULT_UPGRADE_DELAY_SECS` when unset.
    UpgradeDelay,
    // Ordered set of N addresses authorised to co-sign score submissions.
    ServiceSet,
    // The M-of-N threshold: minimum number of service-set members that must
    // sign a `submit_score` call for it to be accepted.
    ServiceThreshold,
    // Admin-configured staleness window (seconds). Scores older than this
    // are considered stale by `is_score_stale`. Defaults to
    // `DEFAULT_STALENESS_WINDOW_SECS` when unset.
    StalenessWindow,
    // Ledger timestamp of the most recent accepted submission for a
    // (wallet, asset_pair) pair, used to enforce the submission cooldown.
    LastSubmitTime(Address, Symbol),
    // Monotonically increasing count of total score submissions for a
    // (wallet, asset_pair) combination. Unlike `ScoreHistory` (which caps
    // at `HISTORY_MAX_DEPTH`), this counter is never truncated — it tracks
    // every submission since the first.
    ScoreCount(Address, Symbol),
    // Ordered set of M-of-N admin co-signers.
    AdminSet,
    // Minimum number of admin-set members that must sign an admin call.
    AdminThreshold,
    // Score delegation: maps a sub-wallet to its custodian wallet.
    ScoreDelegate(Address),
    // Per-(wallet, asset_pair) trend state: current trend direction (+1/0/-1)
    // and consecutive submission count in that direction. Updated by every
    // successful `submit_score` / `submit_scores_batch` write.
    TrendState(Address, Symbol),
    // ── Wallet Relationship Graph ──────────────────────────────────────────
    // List of counterparty addresses for a wallet on a specific asset pair.
    // Key: Counterparties(wallet, asset_pair) -> Vec<Address>
    Counterparties(Address, Symbol),
    // Per-(wallet, asset_pair) running maximum of every score ever accepted,
    // used to decide whether the submission floor applies. Updated on every
    // accepted `submit_score` / `submit_scores_batch` write.
    HistoricalMaxScore(Address, Symbol),
    // Per-(wallet, asset_pair) risk band state. `true` means the wallet is
    // currently inside the high-risk band for this pair. Stored in
    // temporary TTL-bounded storage so stale states expire automatically.
    RiskBandState(Address, Symbol),
    // Per-wallet score embargo. Stores an `EmbargoExpiry` describing whether
    // the embargo is indefinite or expires at a specific ledger timestamp.
    // Absent key means no embargo. Stored in temporary TTL-bounded storage.
    ScoreEmbargo(Address),
    // Per-(wallet, asset_pair) consecutive breach counter for auto-escalation.
    // Incremented on each high-risk submission; reset on low-risk.
    ConsecutiveBreachCount(Address, Symbol),
    // Signer tier bounds
    SignerTier(Address),
    // Model version stats keys
    ModelVersionStatsKey(u32),
}

#[contracttype]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct TierBounds {
    pub min_score: u32,
    pub max_score: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GateDataKey {
    GateCallers,
    GateOpen,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EffectiveRiskScore {
    pub raw_score: u32,
    pub effective_score: u32,
    pub decay_applied: bool,
    pub elapsed_secs: u64,
    pub timestamp: u64,
    pub confidence: u32,
    pub model_version: u32,
    pub benford_flag: bool,
    pub ml_flag: bool,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModelVersionStats {
    pub model_version: u32,
    pub submission_count: u64,
    pub score_sum: u64,
    pub score_max: u32,
    pub score_min: u32,
    pub first_seen: u64,
    pub last_seen: u64,
}

#[allow(dead_code)]
pub const MAX_GATE_CALLERS: u32 = 20;

