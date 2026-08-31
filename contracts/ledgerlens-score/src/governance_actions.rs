//! Governance action registry — stable identifiers for every privileged operation.
#![cfg_attr(target_family = "wasm", allow(dead_code))]
//!
//! ## Why this module exists
//!
//! Before this registry, each privileged admin action was identified by an
//! ad-hoc magic byte literal (e.g. `0x01`, `0x02`) embedded directly in
//! `lib.rs` next to a terse inline comment.  Those literals had no canonical
//! home, no documentation, and no guarantee of uniqueness — a developer
//! adding a new action could accidentally reuse a discriminant without any
//! compile-time or runtime signal.
//!
//! This module assigns **every** privileged action:
//!
//! 1. A **stable `u8` discriminant** (`GOV_ACTION_*`) embedded in the 32-byte
//!    payload written to the Merkle audit chain via
//!    `append_governance_action_raw`.  Once deployed, a discriminant **must
//!    never be reassigned** to a different action — doing so would break
//!    off-chain audit-chain replay tools.
//! 2. A **human-readable name symbol** (`gov_action_name_*`) expressed as a
//!    `&str` constant.  Events, log messages, and docs reference the same
//!    name so operators can cross-reference an on-chain event topic with the
//!    table below without decoding raw bytes.
//! 3. **Doc-comments** on every constant that describe the associated contract
//!    function, the direction of the change, and any invariants that must hold.
//!
//! ## Stability contract
//!
//! | Rule | Rationale |
//! |------|-----------|
//! | `GOV_ACTION_*` discriminants are **frozen** once assigned | Off-chain audit tools rebuild the audit root by replaying stored discriminants; a reassignment silently corrupts every stored root that followed the change |
//! | New actions must claim the **next sequential** discriminant | Predictable ordering makes manual inspection and test fixtures easier |
//! | The `0x00` discriminant is **reserved** (uninitialized / unknown) | A zero-filled `[u8; 32]` payload would otherwise look like a valid action |
//! | Name strings must fit in a Soroban `Symbol` (≤ 9 ASCII chars) | Several callers convert the name directly to `symbol_short!()` |
//!
//! ## Registry
//!
//! | Discriminant | Name | Contract function |
//! |:------------:|------|-------------------|
//! | `0x00` | *(reserved)* | — |
//! | `0x01` | `set_svc` | `set_service` |
//! | `0x02` | `add_sig` | `add_service_signer` |
//! | `0x03` | `set_athr` | `set_admin_threshold` |
//! | `0x04` | `pause` | `pause` |
//! | `0x05` | `unpause` | `unpause` |
//! | `0x06` | `upg_prop` | `propose_upgrade` |

// ── Discriminants ─────────────────────────────────────────────────────────────
//
// Rule: once assigned, a discriminant is frozen.  The `0x00` value is
// reserved to distinguish an uninitialised / zeroed payload from a real
// action entry.

/// Reserved discriminant — must never be assigned to a real action.
///
/// A 32-byte payload filled with `0x00` is the genesis / empty-chain sentinel.
/// Treating it as a real action would silently corrupt audit roots.
pub const GOV_ACTION_RESERVED: u8 = 0x00;

/// `set_service` — replace the single authorised scoring service address.
///
/// Deprecated in favour of the M-of-N signer model (`add_service_signer` /
/// `set_service_threshold`), but kept for backwards compatibility.  Recorded
/// in the audit chain so a key-rotation event is always attributable.
pub const GOV_ACTION_SET_SERVICE: u8 = 0x01;

/// `add_service_signer` — add an address to the M-of-N service signer set.
///
/// The signer set size is bounded by `MAX_SERVICE_SIGNERS`.  Every addition
/// is irreversibly stamped into the audit chain so the signer roster can be
/// reconstructed offline.
pub const GOV_ACTION_ADD_SERVICE_SIGNER: u8 = 0x02;

/// `set_admin_threshold` — change the required quorum for admin M-of-N operations.
///
/// Must satisfy `1 ≤ threshold ≤ |admin_set|`.  Reducing the threshold lowers
/// the security bar for all subsequent admin actions until raised again.
pub const GOV_ACTION_SET_ADMIN_THRESHOLD: u8 = 0x03;

/// `pause` — activate the global circuit breaker, halting all score submissions.
///
/// While the contract is paused every `submit_score` and `submit_scores_batch`
/// call returns `ContractPaused`.  Read functions are unaffected.
pub const GOV_ACTION_PAUSE: u8 = 0x04;

/// `unpause` — deactivate the global circuit breaker, resuming normal operation.
pub const GOV_ACTION_UNPAUSE: u8 = 0x05;

/// `propose_upgrade` — commit to a new WASM hash and start the upgrade time-lock.
///
/// The 32-byte audit payload for this action is the new WASM hash itself (not
/// the discriminant byte), so the upgrade is unconditionally captured in the
/// chain regardless of whether it is eventually executed or vetoed.
pub const GOV_ACTION_PROPOSE_UPGRADE: u8 = 0x06;

// ── Human-readable name strings ───────────────────────────────────────────────
//
// Each name is ≤ 9 ASCII characters so it can be used directly with
// `symbol_short!()` in event topics and log messages.

/// Human-readable name for [`GOV_ACTION_SET_SERVICE`].
pub const GOV_ACTION_NAME_SET_SERVICE: &str = "set_svc";

/// Human-readable name for [`GOV_ACTION_ADD_SERVICE_SIGNER`].
pub const GOV_ACTION_NAME_ADD_SERVICE_SIGNER: &str = "add_sig";

/// Human-readable name for [`GOV_ACTION_SET_ADMIN_THRESHOLD`].
pub const GOV_ACTION_NAME_SET_ADMIN_THRESHOLD: &str = "set_athr";

/// Human-readable name for [`GOV_ACTION_PAUSE`].
pub const GOV_ACTION_NAME_PAUSE: &str = "pause";

/// Human-readable name for [`GOV_ACTION_UNPAUSE`].
pub const GOV_ACTION_NAME_UNPAUSE: &str = "unpause";

/// Human-readable name for [`GOV_ACTION_PROPOSE_UPGRADE`].
pub const GOV_ACTION_NAME_PROPOSE_UPGRADE: &str = "upg_prop";

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Returns the stable human-readable name string for `discriminant`, or
/// `"unknown"` for unrecognised values.
///
/// This is intentionally a simple lookup — callers that need a `Symbol` should
/// convert the returned `&str` via `Symbol::new(&env, name)`.
///
/// ```ignore
/// let name = governance_actions::action_name(GOV_ACTION_PAUSE);
/// assert_eq!(name, "pause");
/// ```
pub fn action_name(discriminant: u8) -> &'static str {
    match discriminant {
        GOV_ACTION_SET_SERVICE => GOV_ACTION_NAME_SET_SERVICE,
        GOV_ACTION_ADD_SERVICE_SIGNER => GOV_ACTION_NAME_ADD_SERVICE_SIGNER,
        GOV_ACTION_SET_ADMIN_THRESHOLD => GOV_ACTION_NAME_SET_ADMIN_THRESHOLD,
        GOV_ACTION_PAUSE => GOV_ACTION_NAME_PAUSE,
        GOV_ACTION_UNPAUSE => GOV_ACTION_NAME_UNPAUSE,
        GOV_ACTION_PROPOSE_UPGRADE => GOV_ACTION_NAME_PROPOSE_UPGRADE,
        _ => "unknown",
    }
}

/// Returns `true` when `discriminant` is a known, non-reserved action id.
///
/// Useful in tests and audit-chain replay tools to distinguish known actions
/// from zero-filled or future-version payloads.
pub fn is_known_action(discriminant: u8) -> bool {
    action_name(discriminant) != "unknown" && discriminant != GOV_ACTION_RESERVED
}

/// Returns an ordered slice of every defined `(discriminant, name)` pair.
///
/// The slice is ordered by discriminant value and can be used by off-chain
/// tools to build a reverse-lookup table without hard-coding the registry
/// a second time.
pub fn all_actions() -> &'static [(u8, &'static str)] {
    &[
        (GOV_ACTION_SET_SERVICE, GOV_ACTION_NAME_SET_SERVICE),
        (GOV_ACTION_ADD_SERVICE_SIGNER, GOV_ACTION_NAME_ADD_SERVICE_SIGNER),
        (GOV_ACTION_SET_ADMIN_THRESHOLD, GOV_ACTION_NAME_SET_ADMIN_THRESHOLD),
        (GOV_ACTION_PAUSE, GOV_ACTION_NAME_PAUSE),
        (GOV_ACTION_UNPAUSE, GOV_ACTION_NAME_UNPAUSE),
        (GOV_ACTION_PROPOSE_UPGRADE, GOV_ACTION_NAME_PROPOSE_UPGRADE),
    ]
}
