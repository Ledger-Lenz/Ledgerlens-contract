use soroban_sdk::{symbol_short, Address, Bytes, BytesN, Env, Symbol};

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

pub fn score_history_cleared(env: &Env, wallet: &Address, asset_pair: &Symbol) {
    env.events().publish((symbol_short!("clr_hist"), wallet.clone()), asset_pair.clone());
}

pub fn score_cleared(env: &Env, wallet: &Address, asset_pair: &Symbol) {
    env.events().publish((symbol_short!("clr_scr"), wallet.clone()), asset_pair.clone());
}

pub fn cooldown_updated(env: &Env, cooldown_secs: u64) {
    env.events().publish((symbol_short!("cd_upd"),), cooldown_secs);
}

pub fn rate_limit_overridden(env: &Env, by: &Address, wallet: &Address, asset_pair: &Symbol) {
    env.events()
        .publish((symbol_short!("rl_ovrd"), wallet.clone(), asset_pair.clone()), by.clone());
}

pub fn service_pubkey_updated(env: &Env, pubkey: &Bytes) {
    env.events().publish((symbol_short!("pk_upd"),), pubkey.clone());
}

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
    env.events().publish(
        (symbol_short!("emb_set"), wallet.clone()),
        expiry,
    );
}

pub fn embargo_lifted(env: &Env, wallet: &Address) {
    env.events().publish((symbol_short!("emb_lift"), wallet.clone()), ());
}

// ── Score jump anomaly ────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
pub fn score_jump_anomaly(
    env: &Env,
    wallet: &Address,
    asset_pair: &Symbol,
    previous_score: u32,
    new_score: u32,
    delta: i64,
    model_version: u32,
    ledger_timestamp: u64,
) {
    env.events().publish(
        (symbol_short!("jmp_ano"), wallet.clone(), asset_pair.clone()),
        (previous_score, new_score, delta, model_version, ledger_timestamp),
    );
}

// ── Escalation / consecutive breach ───────────────────────────────────────────

pub fn escalation_triggered(
    env: &Env,
    wallet: &Address,
    asset_pair: &Symbol,
    count: u32,
    current_score: u32,
    escalation_n: u32,
) {
    env.events().publish(
        (symbol_short!("escal"), wallet.clone(), asset_pair.clone()),
        (count, current_score, escalation_n),
    );
}

pub fn escalation_resolved(
    env: &Env,
    wallet: &Address,
    asset_pair: &Symbol,
    peak_count: u32,
    current_score: u32,
) {
    env.events().publish(
        (symbol_short!("esclr"), wallet.clone(), asset_pair.clone()),
        (peak_count, current_score),
    );
}

pub fn escalation_threshold_updated(env: &Env, old_threshold: u32, new_threshold: u32) {
    env.events().publish((symbol_short!("escupd"),), (old_threshold, new_threshold));
}

// ── Signer rotation ───────────────────────────────────────────────────────────

pub fn signer_expiring(env: &Env, signer: &Address) {
    env.events().publish((symbol_short!("sig_exp"),), signer.clone());
}

pub fn signer_expired(env: &Env, signer: &Address) {
    env.events().publish((symbol_short!("sig_expd"),), signer.clone());
}

pub fn signer_ttl_updated(env: &Env, ttl: u64) {
    env.events().publish((symbol_short!("sttl_upd"),), ttl);
}

pub fn signer_grace_period_updated(env: &Env, grace: u64) {
    env.events().publish((symbol_short!("sg_upd"),), grace);
}

// ── Score histogram ───────────────────────────────────────────────────────────

pub fn histogram_updated(env: &Env, total: u32) {
    env.events().publish((symbol_short!("hist_upd"),), total);
}
