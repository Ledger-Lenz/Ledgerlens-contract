use soroban_sdk::{symbol_short, Address, Bytes, BytesN, Env, Symbol};

use crate::types::RiskScore;

// ── Aggregate risk ────────────────────────────────────────────────────────────

/// Emitted when the admin sets a per-asset-pair weight via `set_pair_weight`.
pub fn pair_weight_updated(env: &Env, asset_pair: &Symbol, weight: u32) {
    env.events().publish((symbol_short!("pw_upd"), asset_pair.clone()), weight);
}

// ── Score events ─────────────────────────────────────────────────────────────

pub fn score_submitted(env: &Env, wallet: &Address, asset_pair: &Symbol, score: &RiskScore) {
    env.events().publish(
        (symbol_short!("score"), wallet.clone(), asset_pair.clone()),
        (score.score, score.benford_flag, score.ml_flag, score.confidence, score.timestamp),
    );
}

// ── Service rotation ──────────────────────────────────────────────────────────

pub fn service_updated(env: &Env, new_service: &Address) {
    env.events().publish((symbol_short!("svc_upd"),), new_service.clone());
}

// ── Pause circuit breaker ────────────────────────────────────────────────────

pub fn contract_paused(env: &Env, by: &Address) {
    env.events().publish((symbol_short!("paused"),), by.clone());
}

pub fn contract_unpaused(env: &Env, by: &Address) {
    env.events().publish((symbol_short!("unpaused"),), by.clone());
}

// ── Per-asset-pair circuit breaker ──────────────────────────────────────────

/// Emitted by `set_pair_paused` for both the pause and unpause direction —
/// a single event type distinguished by the `paused` field, rather than two
/// separate event names, so off-chain indexers can subscribe once.
pub fn pair_paused(env: &Env, asset_pair: &Symbol, paused: bool) {
    env.events().publish((symbol_short!("pr_pause"), asset_pair.clone()), paused);
}

// ── Two-step admin transfer ──────────────────────────────────────────────────

pub fn admin_transfer_initiated(env: &Env, from: &Address, to: &Address) {
    env.events().publish((symbol_short!("adm_init"),), (from.clone(), to.clone()));
}

pub fn admin_transfer_accepted(env: &Env, new_admin: &Address) {
    env.events().publish((symbol_short!("adm_done"),), new_admin.clone());
}

pub fn admin_transfer_cancelled(env: &Env, admin: &Address) {
    env.events().publish((symbol_short!("adm_canc"),), admin.clone());
}

// ── Watchlist ────────────────────────────────────────────────────────────────

pub fn watchlist_updated(env: &Env, wallet: &Address, flagged: bool) {
    env.events().publish((symbol_short!("watch"),), (wallet.clone(), flagged));
}

// ── Risk threshold ───────────────────────────────────────────────────────────

pub fn threshold_updated(env: &Env, old_threshold: u32, new_threshold: u32) {
    env.events().publish((symbol_short!("thresh"),), (old_threshold, new_threshold));
}

/// Emitted inside `submit_score` / `submit_scores_batch` when a
/// submitted score meets or exceeds the configured risk threshold.
/// Off-chain indexers should subscribe to this for real-time alerting.
pub fn threshold_breached(
    env: &Env,
    wallet: &Address,
    asset_pair: &Symbol,
    score: u32,
    threshold: u32,
) {
    env.events()
        .publish((symbol_short!("breach"), wallet.clone()), (asset_pair.clone(), score, threshold));
}

// ── Multi-sig service set ─────────────────────────────────────────────────────

/// Emitted when a new signer is added to the service set.
pub fn signer_added(env: &Env, signer: &Address) {
    env.events().publish((symbol_short!("sig_add"),), signer.clone());
}

/// Emitted when a signer is removed from the service set.
pub fn signer_removed(env: &Env, signer: &Address) {
    env.events().publish((symbol_short!("sig_rem"),), signer.clone());
}

/// Emitted when the service signing threshold is updated.
pub fn service_threshold_updated(env: &Env, threshold: u32) {
    env.events().publish((symbol_short!("sig_thr"),), threshold);
}

// ── Upgrade governance ────────────────────────────────────────────────────────

pub fn upgrade_proposed(env: &Env, new_wasm_hash: &BytesN<32>, executable_after: u64) {
    env.events().publish((symbol_short!("upg_prop"),), (new_wasm_hash.clone(), executable_after));
}

pub fn upgrade_executed(env: &Env, new_wasm_hash: &BytesN<32>) {
    env.events().publish((symbol_short!("upg_exec"),), new_wasm_hash.clone());
}

pub fn upgrade_vetoed(env: &Env, by: &Address) {
    env.events().publish((symbol_short!("upg_veto"),), by.clone());
}

// ── GDPR / data-erasure audit trail ──────────────────────────────────────────

/// Emitted by `clear_score_history` after the history ring buffer is removed.
pub fn score_history_cleared(env: &Env, wallet: &Address, asset_pair: &Symbol) {
    env.events().publish((symbol_short!("clr_hist"), wallet.clone()), asset_pair.clone());
}

/// Emitted by `clear_score` after the latest score entry is removed.
pub fn score_cleared(env: &Env, wallet: &Address, asset_pair: &Symbol) {
    env.events().publish((symbol_short!("clr_scr"), wallet.clone()), asset_pair.clone());
}

// ── Per-wallet/pair submission rate limiting ──────────────────────────────────

/// Emitted when the admin sets the global submission cooldown via
/// `set_cooldown`.
pub fn cooldown_updated(env: &Env, cooldown_secs: u64) {
    env.events().publish((symbol_short!("cd_upd"),), cooldown_secs);
}

/// Emitted by `override_rate_limit`. `by` is the admin that cleared the
/// cooldown for `(wallet, asset_pair)` — the emergency re-score path, not a
/// routine operation, so this is worth a dedicated audit-trail event.
pub fn rate_limit_overridden(env: &Env, by: &Address, wallet: &Address, asset_pair: &Symbol) {
    env.events()
        .publish((symbol_short!("rl_ovrd"), wallet.clone(), asset_pair.clone()), by.clone());
}

// ── Score attestation ──────────────────────────────────────────────────────

/// Emitted when the admin sets/rotates the off-chain attestation pubkey via
/// `set_service_pubkey`.
pub fn service_pubkey_updated(env: &Env, pubkey: &Bytes) {
    env.events().publish((symbol_short!("pk_upd"),), pubkey.clone());
}

// ── Merkle-root batch attestation ───────────────────────────────────────────

/// Emitted by `submit_scores_batch_attested` once the batch has been
/// processed. `accepted` and `rejected` mirror the counts the function
/// returns in its `BatchResult`; `merkle_root` is the root the secp256k1
/// signature was produced over, so an off-chain indexer can reconcile
/// on-chain outcomes against the originally-signed batch without
/// re-reading the per-entry proofs.
pub fn batch_attested(
    env: &Env,
    accepted: u32,
    rejected: u32,
    merkle_root: &BytesN<32>,
) {
    env.events().publish(
        (symbol_short!("bat_ok"), merkle_root.clone()),
        (accepted, rejected),
    );
}

// ── History depth ─────────────────────────────────────────────────────────────

/// Emitted when the admin changes the ring-buffer depth via
/// `set_history_max_depth`.
pub fn history_depth_updated(env: &Env, depth: u32) {
    env.events().publish((symbol_short!("hd_upd"),), depth);
}

// ── Score delta / trend ───────────────────────────────────────────────────────

/// Emitted after every successful score write.
///
/// `previous_score` is `0` on the first submission. `trend` is `+1` (rising),
/// `0` (flat / first submission), or `-1` (falling). `consecutive_trend` counts
/// how many consecutive submissions have had this trend direction; it is `0` on
/// the first submission and on flat submissions.
#[allow(clippy::too_many_arguments)]
pub fn score_delta(
    env: &Env,
    wallet: &Address,
    asset_pair: &Symbol,
    previous_score: u32,
    new_score: u32,
    delta_abs: u32,
    trend: i32,
    consecutive_trend: u32,
) {
    env.events().publish(
        (symbol_short!("scr_dlt"), wallet.clone(), asset_pair.clone()),
        (previous_score, new_score, delta_abs, trend, consecutive_trend),
    );
}

// ── Time-weighted exponential decay ────────────────────────────────────────

/// Emitted when the admin sets the exponential decay rate via `set_decay_rate`.
pub fn decay_rate_updated(env: &Env, numerator: u32, denominator: u32) {
    env.events().publish((symbol_short!("decay_upd"),), (numerator, denominator));
}

// ── Fee withdrawal ────────────────────────────────────────────────────────────

/// Emitted when the admin configures or rotates the fee token via
/// `set_fee_token`.
pub fn fee_token_set(env: &Env, token: &Address) {
    env.events().publish((symbol_short!("ft_set"),), token.clone());
}

/// Emitted on successful completion of `withdraw_fees`.
pub fn fee_withdrawn(
    env: &Env,
    admin: &Address,
    recipient: &Address,
    fee_token: &Address,
    amount: i128,
) {
    env.events().publish(
        (symbol_short!("fee_out"),),
        (admin.clone(), recipient.clone(), fee_token.clone(), amount),
    );
}

/// Emitted when `withdraw_fees` is rejected because the concurrency lock is
/// already held by an in-flight call.
pub fn withdrawal_locked(env: &Env, admin: &Address) {
    env.events().publish((symbol_short!("wdl_lck"),), admin.clone());
}

// ── Score embargo (regulatory hold) ──────────────────────────────────────────

pub fn embargo_set(env: &Env, wallet: &Address, expiry: &Option<u64>) {
    env.events().publish((symbol_short!("emb_set"),), (wallet.clone(), *expiry));
}

pub fn embargo_lifted(env: &Env, wallet: &Address, lifted_by: &Address) {
    env.events().publish((symbol_short!("emb_lift"),), (wallet.clone(), lifted_by.clone()));
}

// ── Wallet-score delegation ───────────────────────────────────────────────────

/// Emitted when `set_score_delegate` registers or updates a delegation.
pub fn delegate_set(env: &Env, sub_wallet: &Address, custodian: &Address) {
    env.events().publish((symbol_short!("dlg_set"),), (sub_wallet.clone(), custodian.clone()));
}

/// Emitted when `remove_score_delegate` removes a delegation.
pub fn delegate_removed(env: &Env, sub_wallet: &Address) {
    env.events().publish((symbol_short!("dlg_rem"),), sub_wallet.clone());
}

// ── Wallet Relationship Graph ───────────────────────────────────────────────

/// Emitted when a counterparty link is added between two wallets.
pub fn counterparty_link_added(
    env: &Env,
    wallet_a: &Address,
    wallet_b: &Address,
    asset_pair: &Symbol,
) {
    env.events().publish(
        (symbol_short!("cpl_add"), wallet_a.clone(), wallet_b.clone()),
        asset_pair.clone(),
    );
}

/// Emitted when a counterparty link is removed.
pub fn counterparty_link_removed(
    env: &Env,
    wallet_a: &Address,
    wallet_b: &Address,
    asset_pair: &Symbol,
) {
    env.events().publish(
        (symbol_short!("cpl_rem"), wallet_a.clone(), wallet_b.clone()),
        asset_pair.clone(),
    );
}

/// Emitted for each wallet affected by `propagate_contagion`.
pub fn contagion_propagated(
    env: &Env,
    anchor: &Address,
    asset_pair: &Symbol,
    affected_wallet: &Address,
    old_score: u32,
    new_score: u32,
) {
    env.events().publish(
        (symbol_short!("cntag"), anchor.clone(), asset_pair.clone()),
        (affected_wallet.clone(), old_score, new_score),
    );
}
