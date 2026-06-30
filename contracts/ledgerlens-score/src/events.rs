use soroban_sdk::{contracttype, symbol_short, Address, Bytes, BytesN, Env, Symbol};

use crate::types::RiskScore;

pub fn pair_weight_updated(env: &Env, asset_pair: &Symbol, weight: u32) {
    env.events().publish((symbol_short!("pw_upd"), asset_pair.clone()), weight);
}

pub fn score_submitted(env: &Env, wallet: &Address, asset_pair: &Symbol, score: &RiskScore) {
    env.events().publish(
        (symbol_short!("score"), wallet.clone(), asset_pair.clone()),
        (score.score, score.benford_flag, score.ml_flag, score.confidence, score.timestamp),
    );
}

pub fn service_updated(env: &Env, new_service: &Address) {
    env.events().publish((symbol_short!("svc_upd"),), new_service.clone());
}

pub fn contract_paused(env: &Env, by: &Address) {
    env.events().publish((symbol_short!("paused"),), by.clone());
}

pub fn contract_unpaused(env: &Env, by: &Address) {
    env.events().publish((symbol_short!("unpaused"),), by.clone());
}

pub fn pair_paused(env: &Env, asset_pair: &Symbol, paused: bool) {
    env.events().publish((symbol_short!("pr_pause"), asset_pair.clone()), paused);
}

pub fn admin_transfer_initiated(env: &Env, from: &Address, to: &Address) {
    env.events().publish((symbol_short!("adm_init"),), (from.clone(), to.clone()));
}

pub fn admin_transfer_accepted(env: &Env, new_admin: &Address) {
    env.events().publish((symbol_short!("adm_done"),), new_admin.clone());
}

pub fn admin_transfer_cancelled(env: &Env, admin: &Address) {
    env.events().publish((symbol_short!("adm_canc"),), admin.clone());
}

pub fn watchlist_updated(env: &Env, wallet: &Address, flagged: bool) {
    env.events().publish((symbol_short!("watch"),), (wallet.clone(), flagged));
}

pub fn threshold_updated(env: &Env, old_threshold: u32, new_threshold: u32) {
    env.events().publish((symbol_short!("thresh"),), (old_threshold, new_threshold));
}

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

/// Emitted by `reset_breach_counter` once the consecutive-breach counter for
/// `(wallet, asset_pair)` has been zeroed by an admin. `by` records the admin
/// address that authorized the reset, giving operators an on-chain audit
/// trail for investigations that conclude before a clean score submission
/// would otherwise reset the counter naturally.
pub fn breach_counter_reset(env: &Env, wallet: &Address, asset_pair: &Symbol, by: &Address) {
    env.events()
        .publish((symbol_short!("brc_rst"), wallet.clone(), asset_pair.clone()), by.clone());
}

pub fn signer_added(env: &Env, signer: &Address) {
    env.events().publish((symbol_short!("sig_add"),), signer.clone());
}

pub fn signer_removed(env: &Env, signer: &Address) {
    env.events().publish((symbol_short!("sig_rem"),), signer.clone());
}

pub fn service_threshold_updated(env: &Env, threshold: u32) {
    env.events().publish((symbol_short!("sig_thr"),), threshold);
}

pub fn upgrade_proposed(env: &Env, new_wasm_hash: &BytesN<32>, executable_after: u64) {
    env.events().publish((symbol_short!("upg_prop"),), (new_wasm_hash.clone(), executable_after));
}

pub fn upgrade_executed(env: &Env, new_wasm_hash: &BytesN<32>) {
    env.events().publish((symbol_short!("upg_exec"),), new_wasm_hash.clone());
}

pub fn upgrade_vetoed(env: &Env, by: &Address) {
    env.events().publish((symbol_short!("upg_veto"),), by.clone());
}

pub fn parameter_change_proposed(
    env: &Env,
    proposal_id: u64,
    param_key: &Symbol,
    executable_after: u64,
) {
    env.events().publish(
        (symbol_short!("prm_prop"),),
        (proposal_id, param_key.clone(), executable_after),
    );
}

pub fn parameter_change_executed(env: &Env, proposal_id: u64, param_key: &Symbol) {
    env.events().publish((symbol_short!("prm_exec"),), (proposal_id, param_key.clone()));
}

pub fn parameter_change_vetoed(env: &Env, proposal_id: u64, by: &Address) {
    env.events().publish((symbol_short!("prm_veto"),), (proposal_id, by.clone()));
}

pub fn score_history_cleared(env: &Env, wallet: &Address, asset_pair: &Symbol) {
    env.events().publish((symbol_short!("clr_hist"), wallet.clone()), asset_pair.clone());
}

pub fn score_cleared(env: &Env, wallet: &Address, asset_pair: &Symbol) {
    env.events().publish((symbol_short!("clr_scr"), wallet.clone()), asset_pair.clone());
}

pub fn cooldown_updated(env: &Env, cooldown_secs: u64) {
    env.events().publish((symbol_short!("cd_upd"),), cooldown_secs);
}

pub fn pair_cooldown_updated(env: &Env, asset_pair: &Symbol, cooldown_secs: u64) {
    env.events().publish((symbol_short!("pcd_upd"), asset_pair.clone()), cooldown_secs);
}

pub fn rate_limit_overridden(env: &Env, by: &Address, wallet: &Address, asset_pair: &Symbol) {
    env.events()
        .publish((symbol_short!("rl_ovrd"), wallet.clone(), asset_pair.clone()), by.clone());
}

pub fn score_velocity_cap_set(env: &Env, enabled: bool, points_per_hour: u32) {
    env.events().publish((symbol_short!("vel_set"),), (enabled, points_per_hour));
}

pub fn velocity_cap_overridden(env: &Env, admin: &Address, wallet: &Address, asset_pair: &Symbol) {
    env.events()
        .publish((symbol_short!("vel_ovr"), wallet.clone(), asset_pair.clone()), admin.clone());
}

pub fn service_pubkey_updated(env: &Env, pubkey: &Bytes) {
    env.events().publish((symbol_short!("pk_upd"),), pubkey.clone());
}

pub fn aggregate_service_pubkey_updated(env: &Env, pubkey: &Bytes) {
    env.events().publish((symbol_short!("agg_pk"),), pubkey.clone());
}

/// Emitted when `rotate_service_pubkey` is called. `new_key` is the incoming
/// pubkey; `overlap_expiry` is the ledger timestamp after which the old key
/// stops being accepted. When `overlap_expiry == 0` the rotation was instant.
pub fn service_pubkey_rotation_started(env: &Env, new_key: &Bytes, overlap_expiry: u64) {
    env.events().publish((symbol_short!("pk_rot"),), (new_key.clone(), overlap_expiry));
}

// ── Merkle-root batch attestation ───────────────────────────────────────────

/// Emitted by `submit_scores_batch_attested` once the batch has been
/// processed. `accepted` and `rejected` mirror the counts the function
/// returns in its `BatchResult`; `merkle_root` is the root the secp256k1
/// signature was produced over, so an off-chain indexer can reconcile
/// on-chain outcomes against the originally-signed batch without
/// re-reading the per-entry proofs.
pub fn batch_attested(env: &Env, accepted: u32, rejected: u32, merkle_root: &BytesN<32>) {
    env.events().publish((symbol_short!("bat_ok"), merkle_root.clone()), (accepted, rejected));
}

// ── Multi-model consensus scoring ─────────────────────────────────────────────

pub fn consensus_score_submitted(
    env: &Env,
    wallet: &Address,
    asset_pair: &Symbol,
    median_score: u32,
    agreeing_model_count: u32,
    epsilon: u32,
) {
    env.events().publish(
        (symbol_short!("cons_scr"), wallet.clone(), asset_pair.clone()),
        (median_score, agreeing_model_count, epsilon),
    );
}

pub fn consensus_config_updated(env: &Env, k: u32, epsilon: u32) {
    env.events().publish((symbol_short!("cons_cfg"),), (k, epsilon));
}

pub fn model_version_proposed(env: &Env, version: u32, executable_after: u64) {
    env.events().publish((symbol_short!("mv_prop"),), (version, executable_after));
}

pub fn model_version_activated(env: &Env, version: u32) {
    env.events().publish((symbol_short!("mv_act"),), version);
}

pub fn model_version_deprecated(env: &Env, version: u32) {
    env.events().publish((symbol_short!("mv_depr"),), version);
}

/// Emitted when the admin updates the consensus configuration.

// (intentionally empty: kept for backward compatibility of the symbol)

// ── History depth ─────────────────────────────────────────────────────────────

pub fn history_depth_updated(env: &Env, depth: u32) {
    env.events().publish((symbol_short!("hd_upd"),), depth);
}

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

pub fn decay_rate_updated(env: &Env, numerator: u32, denominator: u32) {
    env.events().publish((symbol_short!("decay_upd"),), (numerator, denominator));
}

pub fn fee_token_set(env: &Env, token: &Address) {
    env.events().publish((symbol_short!("ft_set"),), token.clone());
}

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

pub fn withdrawal_locked(env: &Env, admin: &Address) {
    env.events().publish((symbol_short!("wdl_lck"),), admin.clone());
}

pub fn fee_recipient_set(env: &Env, recipient: &Address) {
    env.events().publish((symbol_short!("fr_set"),), recipient.clone());
}

pub fn delegate_set(env: &Env, sub_wallet: &Address, custodian: &Address) {
    env.events().publish((symbol_short!("dlg_set"),), (sub_wallet.clone(), custodian.clone()));
}

pub fn delegate_removed(env: &Env, sub_wallet: &Address) {
    env.events().publish((symbol_short!("dlg_rem"),), sub_wallet.clone());
}

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

pub fn score_floor_policy_updated(
    env: &Env,
    enabled: bool,
    high_water_mark: u32,
    floor_value: u32,
) {
    env.events().publish((symbol_short!("sf_upd"),), (enabled, high_water_mark, floor_value));
}

pub fn score_floor_overridden(env: &Env, by: &Address, wallet: &Address, asset_pair: &Symbol) {
    env.events()
        .publish((symbol_short!("sf_ovrd"), wallet.clone(), asset_pair.clone()), by.clone());
}

pub fn risk_band_entered(
    env: &Env,
    wallet: &Address,
    asset_pair: &Symbol,
    score: u32,
    threshold: u32,
) {
    env.events().publish(
        (symbol_short!("band_in"), wallet.clone()),
        (asset_pair.clone(), score, threshold),
    );
}

pub fn risk_band_cleared(
    env: &Env,
    wallet: &Address,
    asset_pair: &Symbol,
    score: u32,
    exit_threshold: u32,
) {
    env.events().publish(
        (symbol_short!("band_out"), wallet.clone()),
        (asset_pair.clone(), score, exit_threshold),
    );
}

pub fn hysteresis_margin_updated(env: &Env, old_margin: u32, new_margin: u32) {
    env.events().publish((symbol_short!("hys_upd"),), (old_margin, new_margin));
}

pub fn embargo_set(env: &Env, wallet: &Address, expiry: Option<u64>) {
    env.events().publish((symbol_short!("emb_set"), wallet.clone()), expiry);
}

pub fn embargo_lifted(env: &Env, wallet: &Address) {
    env.events().publish((symbol_short!("emb_lift"), wallet.clone()), ());
}

pub fn dispute_opened(
    env: &Env,
    challenger: &Address,
    asset_pair: &Symbol,
    bond: i128,
    deadline: u64,
) {
    env.events().publish(
        (symbol_short!("disp_open"), challenger.clone()),
        (asset_pair.clone(), bond, deadline),
    );
}

pub fn dispute_resolved(
    env: &Env,
    challenger: &Address,
    asset_pair: &Symbol,
    corrected_score: u32,
    bond_returned: i128,
) {
    env.events().publish(
        (symbol_short!("disp_res"), challenger.clone()),
        (asset_pair.clone(), corrected_score, bond_returned),
    );
}

pub fn dispute_timed_out(
    env: &Env,
    challenger: &Address,
    asset_pair: &Symbol,
    bond: i128,
    bonus: i128,
) {
    env.events()
        .publish((symbol_short!("disp_to"), challenger.clone()), (asset_pair.clone(), bond, bonus));
}

pub fn finality_buffer_updated(env: &Env, secs: u64) {
    env.events().publish((symbol_short!("fb_upd"),), secs);
}

pub fn score_pending(env: &Env, wallet: &Address, asset_pair: &Symbol, commit_after: u64) {
    env.events()
        .publish((symbol_short!("scr_pend"), wallet.clone(), asset_pair.clone()), commit_after);
}

pub fn score_committed(env: &Env, wallet: &Address, asset_pair: &Symbol) {
    env.events().publish((symbol_short!("scr_comm"), wallet.clone()), asset_pair.clone());
}

pub fn score_pending_cancelled(
    env: &Env,
    wallet: &Address,
    asset_pair: &Symbol,
    cancelled_by: &Address,
) {
    env.events().publish(
        (symbol_short!("scr_canc"), wallet.clone(), asset_pair.clone()),
        cancelled_by.clone(),
    );
}

/// Emitted when an admin vetoes a pending score inside the finality buffer
/// window.
pub fn score_vetoed(env: &Env, wallet: &Address, asset_pair: &Symbol, reason_hash: &BytesN<32>) {
    env.events().publish(
        (symbol_short!("scr_veto"), wallet.clone(), asset_pair.clone()),
        reason_hash.clone(),
    );
}

// ── Service heartbeat monitor ────────────────────────────────────────────

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ServiceSilenceAlertEvent {
    pub last_active_at: u64,
    pub silent_secs: u64,
    pub threshold_secs: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ServiceResumedEvent {
    pub last_active_at: u64,
    pub gap_secs: u64,
}

pub fn service_silence_alert(env: &Env, event: &ServiceSilenceAlertEvent) {
    env.events().publish((symbol_short!("svc_sil"),), event.clone());
}

pub fn service_resumed(env: &Env, event: &ServiceResumedEvent) {
    env.events().publish((symbol_short!("svc_res"),), event.clone());
}

pub fn heartbeat_threshold_updated(env: &Env, secs: u64) {
    env.events().publish((symbol_short!("hb_upd"),), secs);
}

pub fn pair_cooldown_updated(env: &Env, asset_pair: &Symbol, secs: u64) {
    env.events().publish((symbol_short!("pc_upd"), asset_pair.clone()), secs);
}

pub fn signer_expiring(env: &Env, signer: &Address) {
    env.events().publish((symbol_short!("sig_exp"),), signer.clone());
}

pub fn signer_expired(env: &Env, signer: &Address) {
    env.events().publish((symbol_short!("sig_expd"),), signer.clone());
}

pub fn signer_ttl_updated(env: &Env, ttl_secs: u64) {
    env.events().publish((symbol_short!("sg_ttl"),), ttl_secs);
}

pub fn signer_grace_period_updated(env: &Env, grace_secs: u64) {
    env.events().publish((symbol_short!("sg_grc"),), grace_secs);
}

pub fn model_version_registered(env: &Env, version: u32) {
    env.events().publish((symbol_short!("mv_reg"),), version);
}

pub fn entry_ttls_extended(env: &Env, renewed: u32, requested: u32) {
    env.events().publish((symbol_short!("ttl_ext"),), (renewed, requested));
}

pub fn dormancy_decay_applied(
    env: &Env,
    wallet: &Address,
    asset_pair: &Symbol,
    new_score: u32,
    periods: u32,
) {
    env.events().publish(
        (symbol_short!("drm_dec"), wallet.clone(), asset_pair.clone()),
        (new_score, periods),
    );
// ── #297: IQR outlier rejection ───────────────────────────────────────────────

pub fn consensus_signer_rejected(env: &Env, signer: &Address, deviation: u32) {
    env.events().publish((symbol_short!("iqr_rej"), signer.clone()), deviation);
}

// ── #298: Upgrade approval events ────────────────────────────────────────────

pub fn upgrade_approval_added(env: &Env, signer: &Address, count: u32, required: u32) {
    env.events().publish((symbol_short!("upg_appr"), signer.clone()), (count, required));
}

// ── #299: Governance chain events ─────────────────────────────────────────────

pub fn governance_action_appended(env: &Env, new_head: &soroban_sdk::BytesN<32>) {
    env.events().publish((symbol_short!("gov_app"),), new_head.clone());
}

// ── #302: Gate enforcement mode ───────────────────────────────────────────────

pub fn gate_enforcement_mode_set(env: &Env, strict: bool) {
    env.events().publish((symbol_short!("gate_enf"),), strict);
}

// ── #289: Score momentum ──────────────────────────────────────────────────────

/// Emitted by `get_score_momentum` when the computed momentum exceeds the
/// configured alert threshold. `momentum` is the signed rate of change
/// (score units / second, positive = rising risk).
pub fn momentum_threshold_crossed(
    env: &Env,
    wallet: &Address,
    asset_pair: &Symbol,
    momentum: i32,
    threshold: u32,
) {
    env.events().publish(
        (symbol_short!("mom_cross"), wallet.clone()),
        (asset_pair.clone(), momentum, threshold),
    );
}
