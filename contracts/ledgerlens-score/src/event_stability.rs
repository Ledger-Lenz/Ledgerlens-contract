/// Event Schema Stability Levels
///
/// This module defines compatibility guarantees for each emitted event topic.
/// Events are classified into three categories with corresponding stability commitments.
///
/// # Overview
///
/// Public API events are part of the contract's public interface and must maintain
/// compatibility across upgrades. Operator diagnostic events provide observability
/// but may change with notice. Internal test-only events are private implementation
/// details and can change freely.
///
/// # Migration Strategy
///
/// When modifying a stable event:
/// 1. Change the EVENT_VERSION constant
/// 2. Document the breaking change in CHANGELOG.md
/// 3. Add migration notes in the PR and event documentation
/// 4. Update off-chain indexers to handle both versions during the transition window
///
/// Stability level for event topics
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventStability {
    /// Public API events: Critical for external integrations.
    /// Breaking changes require version bumps and migration docs.
    /// These are auditable and must be reconstructible from logs.
    PublicApi,

    /// Operator diagnostic events: Used for monitoring, debugging, and alerting.
    /// Changes require one-release notice but can be made without breaking compatibility.
    /// Off-chain systems can adapt gracefully to missing or new fields.
    OperatorDiagnostic,

    /// Internal test-only events: Private implementation details.
    /// Can change freely without notice; not part of the public contract interface.
    /// Off-chain systems should not depend on these for critical operations.
    InternalTestOnly,
}

/// Event stability registry mapping event topic names to their stability level
pub struct EventStabilityRegistry;

impl EventStabilityRegistry {
    /// Returns the stability level of an event topic.
    /// If not explicitly registered, defaults to InternalTestOnly for safety.
    pub fn stability(topic: &str) -> EventStability {
        match topic {
            // ─────────────────────────────────────────────────────────────────
            // PUBLIC API EVENTS: Critical for auditing and external integrations
            // ─────────────────────────────────────────────────────────────────

            // Core scoring events: used to reconstruct wallet risk scores and histories
            "score" => EventStability::PublicApi,
            "scr_dlt" => EventStability::PublicApi, // Score delta
            "scr_comm" => EventStability::PublicApi, // Score committed
            "scr_veto" => EventStability::PublicApi, // Score vetoed by admin

            // Watchlist and embargo: regulatory and risk gating
            "watch" => EventStability::PublicApi,
            "emb_set" => EventStability::PublicApi,
            "emb_lift" => EventStability::PublicApi,

            // Admin configuration: must be auditable for compliance
            "pw_upd" => EventStability::PublicApi, // Pair weight updated
            "pw_rst" => EventStability::PublicApi, // Pair weight reset
            "thresh" => EventStability::PublicApi,
            "breach" => EventStability::PublicApi,
            "brc_rst" => EventStability::PublicApi, // Breach counter reset

            // Governance and upgrades: critical for system integrity
            "adm_init" => EventStability::PublicApi,
            "adm_done" => EventStability::PublicApi,
            "adm_canc" => EventStability::PublicApi,
            "upg_prop" => EventStability::PublicApi,
            "upg_exec" => EventStability::PublicApi,
            "upg_veto" => EventStability::PublicApi,
            "upg_appr" => EventStability::PublicApi,

            // Consensus and model: affects score reliability
            "cons_scr" => EventStability::PublicApi,
            "mv_act" => EventStability::PublicApi,
            "mv_depr" => EventStability::PublicApi,

            // Batch attestation: proves integrity of batch submissions
            "bat_ok" => EventStability::PublicApi,

            // Dispute resolution: regulatory compliance
            "disp_open" => EventStability::PublicApi,
            "disp_res" => EventStability::PublicApi,
            "disp_to" => EventStability::PublicApi,

            // ─────────────────────────────────────────────────────────────────
            // OPERATOR DIAGNOSTIC EVENTS: Observability with change-with-notice
            // ─────────────────────────────────────────────────────────────────

            // Service configuration and monitoring
            "svc_upd" => EventStability::OperatorDiagnostic,
            "svc_sil" => EventStability::OperatorDiagnostic, // Service silence alert
            "svc_res" => EventStability::OperatorDiagnostic, // Service resumed

            // Pause/unpause events
            "paused" => EventStability::OperatorDiagnostic,
            "unpaused" => EventStability::OperatorDiagnostic,
            "pr_pause" => EventStability::OperatorDiagnostic,

            // Signer management
            "sig_add" => EventStability::OperatorDiagnostic,
            "sig_rem" => EventStability::OperatorDiagnostic,
            "sig_thr" => EventStability::OperatorDiagnostic,
            "sig_exp" => EventStability::OperatorDiagnostic,
            "sig_expd" => EventStability::OperatorDiagnostic,
            "sa_rst" => EventStability::OperatorDiagnostic, // Signer accuracy reset
            "sa_upd" => EventStability::OperatorDiagnostic, // Signer accuracy updated

            // Rate limiting and caps
            "rl_ovrd" => EventStability::OperatorDiagnostic, // Rate limit overridden
            "vel_set" => EventStability::OperatorDiagnostic, // Velocity cap set
            "vel_ovr" => EventStability::OperatorDiagnostic, // Velocity override

            // Configuration updates
            "cd_upd" => EventStability::OperatorDiagnostic, // Cooldown updated
            "pcd_upd" => EventStability::OperatorDiagnostic, // Pair cooldown updated
            "decay_upd" => EventStability::OperatorDiagnostic,
            "hd_upd" => EventStability::OperatorDiagnostic, // History depth updated
            "hys_upd" => EventStability::OperatorDiagnostic, // Hysteresis margin updated

            // Fee management
            "ft_set" => EventStability::OperatorDiagnostic, // Fee token set
            "fr_set" => EventStability::OperatorDiagnostic, // Fee recipient set
            "fee_out" => EventStability::OperatorDiagnostic, // Fee withdrawn
            "wdl_lck" => EventStability::OperatorDiagnostic, // Withdrawal locked

            // Oracle and staleness
            "orc_reg" => EventStability::OperatorDiagnostic, // Oracle registered
            "orc_rem" => EventStability::OperatorDiagnostic, // Oracle removed
            "orc_stale" => EventStability::OperatorDiagnostic, // Oracle stale fallback
            "orc_sthr" => EventStability::OperatorDiagnostic, // Oracle staleness threshold updated
            "sw_upd" => EventStability::OperatorDiagnostic,  // Staleness window updated

            // Consensus configuration
            "cons_cfg" => EventStability::OperatorDiagnostic,
            "tier_upd" => EventStability::OperatorDiagnostic,
            "ae_upd" => EventStability::OperatorDiagnostic, // Adaptive epsilon
            "arl_upd" => EventStability::OperatorDiagnostic, // Adaptive rate limit

            // Parameter governance
            "prm_prop" => EventStability::OperatorDiagnostic,
            "prm_exec" => EventStability::OperatorDiagnostic,
            "prm_veto" => EventStability::OperatorDiagnostic,
            "pc_prop" => EventStability::OperatorDiagnostic, // Param change proposed
            "param_change_proposed" => EventStability::OperatorDiagnostic,

            // Scoring analytics
            "mom_cross" => EventStability::OperatorDiagnostic, // Momentum threshold crossed
            "esc_trg" => EventStability::OperatorDiagnostic,   // Escalation triggered
            "esc_res" => EventStability::OperatorDiagnostic,   // Escalation resolved
            "esc_thr" => EventStability::OperatorDiagnostic,   // Escalation threshold updated
            "clb_upd" => EventStability::OperatorDiagnostic,   // Cluster boundaries updated
            "clr_hist" => EventStability::OperatorDiagnostic,  // Score history cleared
            "clr_scr" => EventStability::OperatorDiagnostic,   // Score cleared

            // Delegation and counterparty
            "dlg_set" => EventStability::OperatorDiagnostic,
            "dlg_rem" => EventStability::OperatorDiagnostic,
            "cpl_add" => EventStability::OperatorDiagnostic, // Counterparty link added
            "cpl_rem" => EventStability::OperatorDiagnostic, // Counterparty link removed
            "cntag" => EventStability::OperatorDiagnostic,   // Contagion propagated

            // Risk bands and policies
            "band_in" => EventStability::OperatorDiagnostic, // Risk band entered
            "band_out" => EventStability::OperatorDiagnostic, // Risk band cleared
            "sf_upd" => EventStability::OperatorDiagnostic,  // Score floor policy
            "sf_ovrd" => EventStability::OperatorDiagnostic, // Score floor overridden
            "at_upd" => EventStability::OperatorDiagnostic,  // Adaptive threshold updated

            // Key rotation
            "pk_upd" => EventStability::OperatorDiagnostic,
            "pk_rot" => EventStability::OperatorDiagnostic,
            "agg_pk" => EventStability::OperatorDiagnostic,

            // Epochs and gates
            "epo_open" => EventStability::OperatorDiagnostic,
            "epo_cls" => EventStability::OperatorDiagnostic,
            "gate_enf" => EventStability::OperatorDiagnostic,

            // Failover and protection
            "failover" => EventStability::OperatorDiagnostic,
            "fp_upd" => EventStability::OperatorDiagnostic, // Flash protection mode updated
            "jt_upd" => EventStability::OperatorDiagnostic, // Jump threshold updated
            "jump" => EventStability::OperatorDiagnostic,   // Score jump anomaly
            "fb_upd" => EventStability::OperatorDiagnostic, // Finality buffer updated
            "susp_gate" => EventStability::OperatorDiagnostic, // Suspicious same-ledger submission

            // Heartbeat and maintenance
            "hb_upd" => EventStability::OperatorDiagnostic,
            "ttl_ext" => EventStability::OperatorDiagnostic,
            "gov_app" => EventStability::OperatorDiagnostic,

            // Pending scores and commit workflow
            "scr_pend" => EventStability::OperatorDiagnostic, // Score pending
            "scr_canc" => EventStability::OperatorDiagnostic, // Score pending cancelled

            // Model lifecycle
            "mv_prop" => EventStability::OperatorDiagnostic,
            "mv_reg" => EventStability::OperatorDiagnostic,

            // Dormancy
            "drm_dec" => EventStability::OperatorDiagnostic,

            // Wallet clustering
            "wc_asgn" => EventStability::OperatorDiagnostic,

            // ─────────────────────────────────────────────────────────────────
            // INTERNAL TEST-ONLY EVENTS: Private implementation details
            // ─────────────────────────────────────────────────────────────────

            // Consensus rejection (internal scoring metric)
            "iqr_rej" => EventStability::InternalTestOnly,

            // Default: treat unknown events as internal for safety
            _ => EventStability::InternalTestOnly,
        }
    }

    /// Returns true if the given event is part of the public API
    pub fn is_public_api(topic: &str) -> bool {
        Self::stability(topic) == EventStability::PublicApi
    }

    /// Returns true if the given event is an operator diagnostic
    pub fn is_operator_diagnostic(topic: &str) -> bool {
        Self::stability(topic) == EventStability::OperatorDiagnostic
    }

    /// Returns true if the given event is internal-only
    pub fn is_internal_only(topic: &str) -> bool {
        Self::stability(topic) == EventStability::InternalTestOnly
    }

    /// Returns a list of all public API event topics
    pub fn public_api_events() -> &'static [&'static str] {
        &[
            "score",
            "scr_dlt",
            "scr_comm",
            "scr_veto",
            "watch",
            "emb_set",
            "emb_lift",
            "pw_upd",
            "pw_rst",
            "thresh",
            "breach",
            "brc_rst",
            "adm_init",
            "adm_done",
            "adm_canc",
            "upg_prop",
            "upg_exec",
            "upg_veto",
            "upg_appr",
            "cons_scr",
            "mv_act",
            "mv_depr",
            "bat_ok",
            "disp_open",
            "disp_res",
            "disp_to",
        ]
    }
}

#[cfg(test)]
mod test_event_stability {
    use super::*;

    #[test]
    fn test_public_api_events_are_registered() {
        for event in EventStabilityRegistry::public_api_events() {
            assert_eq!(
                EventStabilityRegistry::stability(event),
                EventStability::PublicApi,
                "Public API event {} not registered correctly",
                event
            );
        }
    }

    #[test]
    fn test_score_submitted_is_public_api() {
        assert_eq!(EventStabilityRegistry::stability("score"), EventStability::PublicApi);
        assert!(EventStabilityRegistry::is_public_api("score"));
    }

    #[test]
    fn test_watchlist_is_public_api() {
        assert_eq!(EventStabilityRegistry::stability("watch"), EventStability::PublicApi);
    }

    #[test]
    fn test_breach_threshold_is_public_api() {
        assert_eq!(EventStabilityRegistry::stability("breach"), EventStability::PublicApi);
    }

    #[test]
    fn test_service_is_operator_diagnostic() {
        assert_eq!(
            EventStabilityRegistry::stability("svc_upd"),
            EventStability::OperatorDiagnostic
        );
        assert!(EventStabilityRegistry::is_operator_diagnostic("svc_upd"));
    }

    #[test]
    fn test_unknown_event_is_internal() {
        assert_eq!(
            EventStabilityRegistry::stability("unknown_event"),
            EventStability::InternalTestOnly
        );
        assert!(EventStabilityRegistry::is_internal_only("unknown_event"));
    }

    #[test]
    fn test_upgrade_events_are_public_api() {
        for event in &["upg_prop", "upg_exec", "upg_veto"] {
            assert_eq!(EventStabilityRegistry::stability(event), EventStability::PublicApi);
        }
    }

    #[test]
    fn test_admin_transfer_events_are_public_api() {
        for event in &["adm_init", "adm_done", "adm_canc"] {
            assert_eq!(EventStabilityRegistry::stability(event), EventStability::PublicApi);
        }
    }

    #[test]
    fn test_dispute_events_are_public_api() {
        for event in &["disp_open", "disp_res", "disp_to"] {
            assert_eq!(EventStabilityRegistry::stability(event), EventStability::PublicApi);
        }
    }

    #[test]
    fn test_rate_limit_override_is_diagnostic() {
        assert_eq!(
            EventStabilityRegistry::stability("rl_ovrd"),
            EventStability::OperatorDiagnostic
        );
    }

    #[test]
    fn test_consensus_rejection_is_internal() {
        assert_eq!(EventStabilityRegistry::stability("iqr_rej"), EventStability::InternalTestOnly);
    }
}
