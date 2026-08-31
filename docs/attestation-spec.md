# Score Attestation — Commitment & Verification Spec

**Status:** Stable · **Contract:** `LedgerLensScoreContract` · introduced in
`CONTRACT_VERSION` 2.

`submit_score` accepts an optional `ScoreAttestation` that lets the off-chain
detection pipeline cryptographically vouch for the exact payload it computed,
independent of the Soroban `require_auth` check on the service account. This
closes the gap between "this transaction was sent by the authorised service
key" and "this specific score payload was produced by the off-chain pipeline,
unmodified" — relevant when the service key is held by infrastructure (a
relayer, a multisig signer, a batching service) that is trusted to submit
transactions but should not be able to silently alter the score payload
itself.

## 1. Opt-in enforcement model

Attestation is **off by default** and becomes mandatory once configured:

- Before `set_service_pubkey` has ever been called, `submit_score`'s
  `attestation` parameter is ignored entirely (it may be `None` or `Some`,
  either way it has no effect). Existing integrations are unaffected.
- After the admin calls `set_service_pubkey`, every subsequent `submit_score`
  call **must** carry a valid `ScoreAttestation` — a missing or invalid one
  is rejected with `Error::InvalidAttestation`.
- There is intentionally no `clear_service_pubkey`. Once enabled, attestation
  can only be rotated to a new key, never disabled, short of a contract
  upgrade — silently turning it back off would defeat the security property
  it provides.

`submit_scores_batch` does not support attestation; it remains the
plain `require_auth`-only path.

## 2. `ScoreAttestation`

```rust
pub struct ScoreAttestation {
    /// SHA-256 commitment over the canonical score payload (§3).
    pub commitment: BytesN<32>,
    /// 65-byte secp256k1 ECDSA signature over `commitment`: 32-byte `r`,
    /// 32-byte `s`, then a 1-byte recovery id which must be 0 or 1.
    pub signature: BytesN<65>,
    /// Instance-binding field folded into the commitment preimage (§3).
    /// Not independently checked against the contract's own address —
    /// see the note at the end of §6 for why that's still safe.
    pub contract_id: BytesN<32>,
    /// Must equal the contract's stored `CONTRACT_VERSION` or the
    /// attestation is rejected before the commitment is even recomputed.
    pub contract_version: u32,
    /// Per-signer sequence number, checked and incremented separately from
    /// commitment/signature verification — prevents replay of an
    /// otherwise-valid attestation for the *same* instance and payload.
    pub nonce: u64,
}
```

The `commitment` field is **never trusted as input** — `verify_attestation`
recomputes it independently from the call's actual arguments and rejects the
call if the two disagree. The field exists purely so a tampered payload
surfaces as `InvalidAttestation` via an explicit equality check, rather than
as a confusing signature-recovery failure against a digest the caller never
intended to sign.

## 3. Commitment preimage layout

`compute_commitment` builds a single byte buffer and hashes it with SHA-256.
Fields are concatenated in this exact order, with no length prefixes (every
field is either fixed-width or zero-padded to a fixed width):

| Field | Width | Encoding |
|---|---|---|
| `wallet` | 56 bytes | `wallet.to_string()` — the G... StrKey encoding, ASCII |
| `asset_pair` | 9 bytes | ASCII bytes of the `Symbol`, zero-padded on the right |
| `score` | 4 bytes | `u32`, little-endian |
| `benford_flag` | 1 byte | `0` or `1` |
| `ml_flag` | 1 byte | `0` or `1` |
| `timestamp` | 8 bytes | `u64`, little-endian |
| `confidence` | 4 bytes | `u32`, little-endian |
| `model_version` | 4 bytes | `u32`, little-endian |
| contract address | 56 bytes | `env.current_contract_address().to_string()` — StrKey encoding, ASCII |
| network id | 32 bytes | `env.ledger().network_id()` |
| `contract_id` | 32 bytes | contract's own address as raw 32 bytes |
| `contract_version` | 4 bytes | `u32`, little-endian |

Total preimage length: 211 bytes (56 + 9 + 4 + 1 + 1 + 8 + 4 + 4 + 56 + 32 +
32 + 4). This exact width, and the order and encoding of every field above, is
locked down by the golden-vector and domain-separation tests in
`test_attestation_domain_compat.rs` (issue #696): any field that is omitted,
resized, or reordered changes the pinned digest and fails the suite.

Rationale for the StrKey (`to_string()`) encoding of `wallet` and the
contract address: these are the only stable, deterministic byte
representations a Soroban contract can derive on-chain from the
guest-opaque `Address` type — there is no API to recover the raw 32-byte
account/contract ID directly from inside the contract.

`asset_pair` is restricted to at most 9 ASCII characters (the same bound
`symbol_short!` enforces elsewhere in this contract); `compute_commitment`
returns `Error::InvalidAttestation` for anything longer rather than silently
truncating.

Including the contract address and `network_id` in the preimage binds the
commitment to one specific deployment on one specific network, so a
signature produced for a testnet deployment (or a different contract
instance) cannot be replayed against another.

## 4. Verification

1. Recompute the commitment from the call's actual arguments (§3) and
   compare against `attestation.commitment` — any mismatch is
   `InvalidAttestation`.
2. Split `attestation.signature` into `r‖s` (first 64 bytes) and the
   recovery id (byte 64). Recovery id must be `0` or `1`; anything else is
   rejected.
3. Call `env.crypto().secp256k1_recover(&digest, &rs, recovery_id)`, which
   always yields the recovered public key in 65-byte uncompressed SEC-1
   form.
4. Compare the recovered key against the pubkey registered via
   `set_service_pubkey`:
   - If the registered key is 65 bytes (uncompressed), compare directly.
   - If the registered key is 33 bytes (compressed), compress the recovered
     key first — `0x02`/`0x03` parity prefix (even/odd y-coordinate) followed
     by the x-coordinate — and compare that. No elliptic-curve point
     arithmetic is needed since the recovered point's coordinates are already
     known.
5. Any mismatch at any step is `Error::InvalidAttestation`.

## 5. Key format and canonicalization

`set_service_pubkey` (and `rotate_service_pubkey`) enforce **SEC-1 canonical
encoding** on the supplied public key. The check is performed by
`storage::validate_pubkey_format` before the key is written to storage.

### 5.1 Accepted encodings

| Length | Prefix byte | SEC-1 meaning       | Accepted? |
|--------|-------------|---------------------|-----------|
| 33     | `0x02`      | Compressed, even y  | ✅ yes    |
| 33     | `0x03`      | Compressed, odd y   | ✅ yes    |
| 65     | `0x04`      | Uncompressed        | ✅ yes    |

### 5.2 Rejected encodings

Any input **not** matching the table above is rejected with
`Error::InvalidPubkeyLength`. This covers both wrong-length and wrong-prefix
cases — the error code is reused for prefix violations because the error enum
is at the XDR 50-variant limit and a prefix error has the same operational
meaning (the key is not usable).

Examples of rejected inputs:

| Length | Prefix byte | Reason for rejection                                   |
|--------|-------------|--------------------------------------------------------|
| 0      | —           | Empty; wrong length                                    |
| 1      | any         | Wrong length                                           |
| 32     | any         | Wrong length (one byte short of a compressed key)      |
| 34     | any         | Wrong length (one byte over a compressed key)          |
| 64     | any         | Wrong length (one byte short of an uncompressed key)   |
| 66     | any         | Wrong length (one byte over an uncompressed key)       |
| 33     | `0x00`      | Invalid prefix for compressed key                      |
| 33     | `0x01`      | Invalid prefix for compressed key                      |
| 33     | `0x04`      | `0x04` is only valid for 65-byte uncompressed keys     |
| 33     | `0x05`–`0xFF` | Invalid prefix for compressed key                   |
| 65     | `0x00`–`0x03` | Invalid prefix for uncompressed key                 |
| 65     | `0x05`–`0xFF` | Invalid prefix for uncompressed key                 |

### 5.3 What canonicalization does NOT check

- **Point-on-curve validity**: Soroban's host does not expose a secp256k1
  point-validation function at key-set time. A blob with a valid prefix but
  coordinates that do not lie on secp256k1 is accepted at storage time; it
  will simply never match any key recovered by `secp256k1_recover` during
  `verify_attestation`, making every subsequent attestation fail with
  `Error::InvalidAttestation`. Operators should set only genuine public keys.
- **Low-order or weak points**: same reasoning — rejected at signature-verify
  time by the host, not at key-set time.
- **All-zero or all-`0xFF` payloads**: a 33-byte `0x02 || 0x00…00` passes the
  prefix check. It is not a valid secp256k1 point, so no signature will ever
  verify against it.

### 5.4 Verification path (recap from §4)

`secp256k1_recover` always returns a 65-byte uncompressed point. Comparison
against the stored key depends on the stored format:

- **Stored as 65 bytes**: constant-time compare directly.
- **Stored as 33 bytes**: derive the compressed form from the recovered point
  (`0x02`/`0x03` parity prefix + x-coordinate), then constant-time compare.
  No additional elliptic-curve arithmetic is required — the recovered point's
  coordinates are already available.

The `pubkeys_match` helper in `storage.rs` encapsulates this dispatch and is
shared between the active-key and pending-key (overlap-window) comparison
paths.

## 6. Migration & Cross-Deployment Binding

As of `CONTRACT_VERSION` 4, attestations now include `contract_id` and `contract_version` fields.
These fields cryptographically bind the signature to one specific contract deployment and version,
preventing cross-deployment and cross-version replay attacks.

**Operators running existing service signers must update their signing code to include
`contract_id` and `contract_version` in the digest.** Existing signatures without these
fields will be rejected as `InvalidAttestation` after this upgrade.

The digest layout changed from 175 bytes to 211 bytes (see §3). Signers must recompute
all attestations using the updated preimage format.

### Domain-separation review (issue #401)

Confirmed: the signed payload already binds each attestation to one specific
contract instance and network, closing the cross-shard/cross-network replay
vector described in #401. Concretely:

- `compute_commitment` (§3) hashes `env.current_contract_address().to_string()`
  and `env.ledger().network_id()` **read directly from the executing
  contract**, not from any attacker- or signer-supplied field. This is the
  binding that actually matters: it means the recomputed digest for contract
  B can never equal a commitment signed for contract A's address, regardless
  of what the attestation's own `contract_id` field claims.
- The `contract_id` / `contract_version` fields on `ScoreAttestation` are
  additional preimage inputs and a version gate (`contract_version` is
  checked against `CONTRACT_VERSION` before the commitment is even
  recomputed), but `contract_id` itself is *not* separately compared against
  `env.current_contract_address()`. That's safe rather than a gap: it's
  redundant with the self-derived binding above, since any mismatch there
  already makes the recomputed digest fail to match `attestation.commitment`.
- `test_attestation.rs::test_attestation_signed_for_one_instance_rejected_on_another_instance`
  deploys two real contract instances sharing one service pubkey (the
  multi-shard scenario #401 describes), signs a valid attestation against
  instance A, and confirms the identical attestation is rejected with
  `InvalidAttestation` when replayed against instance B.

No ABI change or attestation-version bump was needed — the binding predates
this review; the gap was that it wasn't documented or covered by a
cross-instance test, both of which this section and the test above now
provide.

## 7. Key-rotation overlap window (issue #697)

Both attestation key slots — the single service pubkey (`set_service_pubkey`
/ `ScoreAttestation`) and the aggregate threshold pubkey
(`set_aggregate_service_pubkey` / `ThresholdAttestation`) — support a
**bounded overlap window** during rotation, so in-flight submissions signed
with the outgoing key are not orphaned by a rotation that happens mid-flight,
while still bounding how long the outgoing key remains usable.

### Rotation record

`rotate_service_pubkey(admin_signers, new_key, overlap_secs)` and
`rotate_aggregate_service_pubkey(admin_signers, new_key, overlap_secs)` each
record a **pending key** paired with an **expiry bound**:

- Activation is implicit and immediate: the new key is accepted (as the
  *pending* key) from the moment the rotation call executes.
- `expiry = env.ledger().timestamp() + overlap_secs` at the time of the call
  — the upper bound of the window. `get_pending_service_pubkey()` /
  `get_pending_aggregate_pubkey()` return `(pending_key, expiry)` so
  operators and monitoring tooling can read both bounds of the window
  on-chain.
- `overlap_secs == 0` skips the pending state entirely: the new key is
  promoted to active immediately and the old key stops verifying in the same
  call.

### Verification during the window

`verify_signature` (single-key) and `verify_threshold_attestation`
(aggregate) both:

1. First check whether a pending key exists and its `expiry` has already
   passed. If so, the pending key is **promoted to active and the pending
   slot is cleared** before verification proceeds — this happens on the very
   next call after expiry, not on a timer, so there is no ledger-close race
   where neither slot is authoritative.
2. Check the signature against the **active** key.
3. If that fails and a pending key is still recorded with `now <= expiry`,
   check the signature against the **pending** key too.

The net effect: during `[rotation call, expiry]`, both the old (active) and
new (pending) keys verify. After `expiry`, only the new key verifies — a
signature from the retired key is rejected with `Error::InvalidAttestation`
exactly as any other unrecognized key, closing the window rather than
leaving it open indefinitely. See `test_dual_key_pubkey.rs` (single-key) and
`test_aggregate_key_rotation.rs` (aggregate) for the deterministic tests
proving this, including the post-expiry rejection case.

### Compatibility

- **No ABI break**: `rotate_aggregate_service_pubkey` /
  `get_pending_aggregate_pubkey` are new, additive endpoints;
  `set_aggregate_service_pubkey` (instant, no-overlap rotation) is
  unchanged. The single-key `rotate_service_pubkey` /
  `get_pending_service_pubkey` pair already existed (issue #295) and is
  unchanged here.
- **New storage key**: `PendingAggregateServicePubKey` (instance storage),
  mirroring the pre-existing `PendingServicePubKey`.
- **New event** `agg_pkrt` (topics: `agg_pkrt`; data: `(new_key,
  overlap_expiry)`), mirroring the pre-existing `pk_rot`. Additive only.
- **Bounded work**: verification does at most one extra storage read and one
  extra signature comparison, regardless of how many rotations have
  occurred — there is exactly one pending-key slot per key type, not a
  growing history.
