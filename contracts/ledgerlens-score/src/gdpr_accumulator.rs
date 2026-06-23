//! RSA accumulator / deletion proof helpers.
//!
//! NOTE: This is an on-chain-friendly, *deterministic* implementation based
//! on modular exponentiation over a fixed modulus N.
//!
//! In the absence of a full big-integer stack in the current codebase, this
//! file provides a minimal implementation scaffold so the contract ABI and
//! storage wiring can be completed.
//!
//! The exact RSA accumulator math required by a production-grade
//! cryptographic accumulator (including witness generation for non-membership)
//! is non-trivial under Soroban WASM constraints; this scaffold is designed so
//! it can be replaced with a full verified implementation once compatible
//! big-integer support is added.

#![allow(dead_code)]

use soroban_sdk::{BytesN, Env, Symbol, Address};

use crate::{types::RiskScore, Error};

/// Fixed-size deletion proof returned by `get_deletion_proof`.
///
/// Format (current scaffold):
/// - proof[0..32]  : accumulator value A (truncated)
/// - proof[32..64] : last witness nonce / epoch (u128 truncated)
/// - proof[64..]    : reserved
pub type DeletionProofBytes = BytesN<256>;

/// Deterministically map an entry's commitment digest to a scalar exponent.
///
/// This scaffold uses the low 64-bits of the hash as an exponent.
pub fn exponent_from_entry_digest(_env: &Env, digest: &[u8; 32]) -> u64 {
    let mut v: u64 = 0;
    for i in 0..8 {
        v |= (digest[i] as u64) << (8 * i);
    }
    // Ensure non-zero.
    if v == 0 { 1 } else { v }
}

/// Update accumulator: A' = A^{e} mod N
pub fn accumulator_update(_env: &Env, a: &u64, e: u64, n: &u64) -> u64 {
    // Scaffold: simple modular exponentiation on u64.
    // Replace with big-int modular exponentiation for real RSA.
    mod_pow_u64(*a, e, *n)
}

/// Generate a (non-membership) deletion witness.
///
/// Scaffold: produces a proof that will only verify if the entry set matches
/// what was deleted in the same `clear_score_history` call.
pub fn generate_deletion_witness(
    env: &Env,
    wallet: &Address,
    asset_pair: &Symbol,
    deleted_entry_digests: &[[u8; 32]],
    accumulator_value: &u64,
) -> Result<DeletionProofBytes, Error> {
    // Deterministic placeholder proof.
    // We'll embed: (1) truncated accumulator, (2) a hash of wallet/pair and deleted digests.
    let mut out = [0u8; 256];
    out[0..8].copy_from_slice(&accumulator_value.to_le_bytes());

    let mut tag_preimage = [0u8; 64];
    let wallet_bytes = wallet.to_string().as_bytes();
    let pair_bytes = asset_pair.to_string().as_bytes();
    for i in 0..wallet_bytes.len().min(32) { tag_preimage[i] = wallet_bytes[i]; }
    for i in 0..pair_bytes.len().min(32) { tag_preimage[32 + i] = pair_bytes[i]; }

    let mut digest_seed: [u8; 32] = [0u8; 32];
    // Soroban doesn't expose SHA here in scaffold. Keep deterministic by XOR.
    for d in deleted_entry_digests.iter() {
        for i in 0..32 {
            digest_seed[i] ^= d[i];
        }
    }
    out[8..40].copy_from_slice(&digest_seed);

    // remaining bytes reserved.
    let _ = env; // suppress unused
    Ok(BytesN::<256>::from_array(env, &out))
}

/// Verify deletion proof for a given accumulator public state.
///
/// Scaffold: returns true only if proof prefix matches expected truncated
/// accumulator and expected digest hash.
pub fn verify_deletion_proof(
    env: &Env,
    _wallet: &Address,
    _asset_pair: &Symbol,
    proof: &DeletionProofBytes,
    accumulator_public: &u64,
    expected_entry_digests: &[[u8; 32]],
) -> bool {
    let mut expected = [0u8; 256];
    expected[0..8].copy_from_slice(&accumulator_public.to_le_bytes());

    let mut digest_seed: [u8; 32] = [0u8; 32];
    for d in expected_entry_digests.iter() {
        for i in 0..32 {
            digest_seed[i] ^= d[i];
        }
    }
    expected[8..40].copy_from_slice(&digest_seed);

    proof.to_array() == expected
}

fn mod_pow_u64(mut base: u64, mut exp: u64, modulus: u64) -> u64 {
    if modulus == 1 {
        return 0;
    }
    let mut result: u64 = 1;
    base %= modulus;
    while exp > 0 {
        if exp & 1 == 1 {
            result = result.saturating_mul(base) % modulus;
        }
        exp >>= 1;
        base = base.saturating_mul(base) % modulus;
    }
    result
}

