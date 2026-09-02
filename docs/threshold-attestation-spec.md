# Threshold Signature Attestation Specification

Version: 1.0 — introduced in contract ABI version 4.

## 1. Background and motivation

The M-of-N service signing model (`add_service_signer` / `set_service_threshold`) requires each of the M authorised signers to call `require_auth` individually. In Soroban, every `require_auth` call implies a separate Stellar account authorization, so a 3-of-5 setup produces five authorization entries per `submit_score` invocation. This:

- Inflates the transaction size by O(N) signatures.
- Requires all M signers to be online and coordinate for every submission.
- Is observable — on-chain indexers can see exactly which signers participated.

Threshold ECDSA aggregation replaces all of that with a **single 65-byte `(r, s, v)` secp256k1 signature** over the same payload commitment, recoverable to a single *aggregate* public key that the admin pre-registers on-chain.

---

## 2. Cryptographic protocol (off-chain)

Any t-of-n ECDSA threshold scheme that produces a standard secp256k1 `(r, s)` pair may be used. The two most common choices are:

### 2a. FROST (Flexible Round-Optimised Schnorr Threshold)

FROST produces Schnorr signatures, not ECDSA. It is **not** compatible with Stellar's `secp256k1_recover` host function, which only processes ECDSA. Use option 2b instead.

### 2b. GG18 / GG20 threshold ECDSA

GG18 (Gennaro–Goldfeder 2018) and its improved variant GG20 produce standard `(r, s)` ECDSA pairs verifiable with any secp256k1 implementation. The aggregate public key is derived from the key-generation ceremony and is a normal secp256k1 point — indistinguishable from a single-party key to the verifier.

**Recommended library:** [multi-party-ecdsa](https://github.com/ZenGo-X/multi-party-ecdsa) (Rust, MIT).

### Off-chain signing workflow

```
1. Each participant i holds a key share k_i produced during the DKG ceremony.
2. The t coordinators run the GG20 signing protocol on the payload commitment C:
     C = SHA-256(wallet_bytes || pair_bytes || score_le || flags || ts_le
                 || confidence_le || model_version_le || contract_addr || network_id)
   (same preimage layout as ScoreAttestation — see docs/attestation-spec.md)
3. The protocol produces (r, s). Append the recovery id v ∈ {0, 1} by
   trying secp256k1_recover(C, r, s, 0) and checking whether the result
   matches the aggregate public key; if not, v = 1.
4. Concatenate threshold_sig = r (32 bytes) || s (32 bytes) || v (1 byte).
5. Call submit_score(..., threshold_attestation = Some(ThresholdAttestation {
       commitment: C,
       threshold_sig,
       participating_signers: [addr_i1, addr_i2, …, addr_it],
   })).

### Key registration

After the DKG ceremony, the group's aggregate public key (an uncompressed 65-byte or compressed 33-byte SEC-1 secp256k1 point) is registered on-chain by the admin:

```bash
soroban contract invoke ... -- set_aggregate_service_pubkey \
  --admin_signers '[]' \
  --pubkey '<hex-encoded 33 or 65 byte SEC-1 pubkey>'
```yaml

The key can be rotated at any time by calling `set_aggregate_service_pubkey` again with the new key (requires admin authorization). There is no way to unset it once registered — consistent with the `set_service_pubkey` invariant.

---

## 3. On-chain verification

When `submit_score` receives a `threshold_attestation: Some(ta)`:

1. **Aggregate pubkey check** — fails immediately with `AggregatePubkeyNotSet` if no key has been registered.
2. **Signer membership** — all addresses in `ta.participating_signers` must be in the registered service set (if any). Fails with `ThresholdSignerNotInSet` otherwise.
3. **Threshold count** — `participating_signers.len()` must meet the configured `ServiceThreshold`. Fails with `InsufficientThresholdSigners` otherwise.
4. **Commitment recomputation** — the contract recomputes `C` from the call's actual arguments and compares it with `ta.commitment`. Fails with `InvalidThresholdSignature` on mismatch.
5. **Signature recovery** — `secp256k1_recover(C, r || s, v)` is called. The recovered point is compared against the registered aggregate pubkey (compressed or uncompressed). Fails with `InvalidThresholdSignature` on mismatch.

No `require_auth` call is ever made in the threshold path — the signature is the sole authorization proof.

---

## 4. Interaction with the ordinary attestation path

- If `threshold_attestation` is `Some`, the threshold path runs and the `attestation` parameter is ignored entirely.
- If `threshold_attestation` is `None`, the existing paths run unchanged:
  - If a service set is configured → M-of-N `require_auth` path.
  - Otherwise → legacy single-service `require_auth` path.
  - If `set_service_pubkey` has been called → ordinary `ScoreAttestation` is required.

This means upgrading to threshold signatures is a drop-in change: existing callers can pass `threshold_attestation: None` indefinitely, and the new path is only activated when both the aggregate pubkey has been registered and the caller passes a `ThresholdAttestation`.

---

## 5. Storage

| Key | Type | Description |
|-----|------|-------------|
| `AggregatePubKey` | `Bytes` (33 or 65) | Aggregate secp256k1 public key for the threshold group. Instance storage. |

---

## 6. Events

| Event topic | Data | When |
|-------------|------|------|
| `("agg_pk",)` | `pubkey: Bytes` | `set_aggregate_service_pubkey` succeeds. |

---

## 7. New error codes

| Code | Name | Context |
|------|------|---------|
| 52 | `AggregatePubkeyNotSet` | Threshold path attempted but no aggregate pubkey registered. |
| 53 | `InvalidThresholdSignature` | Commitment mismatch, bad recovery id, or recovered key ≠ aggregate key. |
| 54 | `ThresholdSignerNotInSet` | A `participating_signers` entry is not in the service set. |
| 55 | `InsufficientThresholdSigners` | `participating_signers.len()` < `ServiceThreshold`. |

---

## 8. Security notes

- **Key rotation:** rotate the aggregate key after any suspected share compromise by calling `set_aggregate_service_pubkey` with the new post-rotation key.
- **Replay protection:** the commitment preimage includes the contract address and network ID, so a valid threshold signature cannot be replayed on a different contract or network.
- **No `participating_signers` forgery:** the on-chain check validates membership but does NOT call `require_auth`. An attacker who can forge the threshold signature would need to break secp256k1 ECDSA, which is equivalent to breaking Bitcoin's signature scheme.
- **1-of-1 degenerate case:** when `ServiceThreshold = 1` and a single-signer "aggregate" key is registered, the threshold path degenerates to a standard single-key ECDSA attestation — mathematically sound but semantically equivalent to `set_service_pubkey`.
