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

Total preimage length: 243 bytes.

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

## 5. Key format

`set_service_pubkey` accepts a SEC-1-encoded secp256k1 public key, either:

- 33 bytes, compressed (`0x02`/`0x03` prefix + x-coordinate), or
- 65 bytes, uncompressed (`0x04` prefix + x + y coordinates).

Any other length is rejected with `Error::InvalidPubkeyLength`.

## 6. Migration & Cross-Deployment Binding

As of `CONTRACT_VERSION` 4, attestations now include `contract_id` and `contract_version` fields.
These fields cryptographically bind the signature to one specific contract deployment and version,
preventing cross-deployment and cross-version replay attacks.

**Operators running existing service signers must update their signing code to include
`contract_id` and `contract_version` in the digest.** Existing signatures without these
fields will be rejected as `InvalidAttestation` after this upgrade.

The digest layout changed from 175 bytes to 243 bytes (see §3). Signers must recompute
all attestations using the updated preimage format.
