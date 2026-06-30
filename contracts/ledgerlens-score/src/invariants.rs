//! Formal pre/post-condition invariant checker for #292.
//!
//! `invariant_check` is compiled only in test/debug builds
//! (`#[cfg(any(test, feature = "testutils"))]`). Call it at the end of every
//! state-mutating public function in `lib.rs` to catch invariant violations at
//! the point of introduction rather than downstream.
//!
//! A violation panics with a descriptive message identifying the broken
//! invariant, which causes the surrounding test to fail with a clear diagnosis.

use soroban_sdk::Env;

use crate::storage;

/// Assert all contract-level invariants after a state mutation.
///
/// Checked invariants:
/// 1. Global min confidence is in [0, 100].
/// 2. Service threshold ≤ service signer set size (when set is non-empty).
/// 3. Admin threshold ≤ admin set size (when set is non-empty).
/// 4. Decay rate denominator is never zero.
/// 5. Gate query fee is non-negative.
/// 6. Accumulated fees are non-negative.
#[cfg(any(test, feature = "testutils"))]
pub fn invariant_check(env: &Env) {
    // 1. Global min confidence must be in [0, 100].
    let min_conf = storage::get_global_min_confidence(env);
    assert!(
        min_conf <= 100,
        "INVARIANT VIOLATED: global_min_confidence={min_conf} exceeds 100"
    );

    // 2. Service threshold ≤ service signer set size.
    let svc_set = storage::get_service_set(env);
    let svc_set_len = svc_set.len();
    if svc_set_len > 0 {
        let svc_threshold = storage::get_service_threshold(env);
        assert!(
            svc_threshold <= svc_set_len,
            "INVARIANT VIOLATED: service_threshold={svc_threshold} > signer_set_size={svc_set_len}"
        );
    }

    // 3. Admin threshold ≤ admin set size.
    let admin_set = storage::get_admin_set(env);
    let admin_set_len = admin_set.len();
    if admin_set_len > 0 {
        let admin_threshold = storage::get_admin_threshold(env);
        assert!(
            admin_threshold <= admin_set_len,
            "INVARIANT VIOLATED: admin_threshold={admin_threshold} > admin_set_size={admin_set_len}"
        );
    }

    // 4. Decay rate denominator must never be zero.
    let (_, denom) = storage::get_decay_rate(env);
    assert!(
        denom > 0,
        "INVARIANT VIOLATED: decay_rate_denominator=0 (division by zero risk)"
    );

    // 5. Gate query fee must be non-negative.
    let gate_fee = storage::get_gate_query_fee(env);
    assert!(
        gate_fee >= 0,
        "INVARIANT VIOLATED: gate_query_fee={gate_fee} is negative"
    );

    // 6. Accumulated fees must be non-negative.
    let accum = storage::get_accumulated_fees(env);
    assert!(
        accum >= 0,
        "INVARIANT VIOLATED: accumulated_fees={accum} is negative"
    );
}
