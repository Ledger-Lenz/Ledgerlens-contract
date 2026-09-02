# Verkle Commitment Scheme for LedgerLens

## Overview

LedgerLens maintains an **incremental Verkle-tree-style commitment** over the full live contract state — every `(wallet, asset_pair, score)` triple. This commitment enables three capabilities without reading the entire state:

| Capability | Description |
|---|---|
| **Membership proof** | Prove that a specific wallet/pair has a specific score in the committed state |
| **Non-membership proof** | Prove that a wallet/pair has *no* score in the committed state |
| **Range proof** | Off-chain aggregation of membership proofs proves "all scores for pair P are below T" |

The commitment root is a single 48-byte value returned by `get_state_commitment()`. It is updated atomically on every accepted score write.

---

## Cryptographic Scheme

### Why "Verkle" Without BLS12-381

True Verkle trees use KZG polynomial commitments over **BLS12-381** with a trusted setup, enabling constant-size proofs for arbitrary subtrees. Soroban's on-chain execution environment provides only **SHA-256** and secp256k1 — BLS12-381 pairings are unavailable.

We implement a **hash-based polynomial commitment** that captures the structural properties of KZG:

- Each state entry maps to a unique `(evaluation point z, value v)` pair.
- A single accumulator commits to all `(z, v)` pairs at once.
- Opening proofs are O(1) in size and O(1) to verify.
- Proofs are non-interactive and verifiable from only the commitment root.

This is sometimes called a **VRF-commitment** or **algebraic hash commitment** in the literature. The term "Verkle" in this codebase refers to the structural design (vector commitment with efficient openings) rather than the specific KZG-over-BLS12-381 instantiation.

### BLS12-381 Field Reduction

All arithmetic is logically in the BLS12-381 scalar field:

```rust
r = 0x73eda753299d7d483339d80809a1d80553bda402fffe5bfeffffffff00000001
  ≈ 2^254.85

SHA-256 output is a 256-bit integer. We reduce it into the field by masking the top 3 bits of the most-significant byte:

```
z[31] &= 0x1F   // ensures result < 2^253 < r

This is a valid reduction because SHA-256 output is computationally indistinguishable from uniform in `[0, 2^256)`. Masking 3 bits produces a uniform element in `[0, 2^253)`, which is a strict subset of `[0, r)`.

---

## Commitment Construction

### Evaluation Point

Each `(wallet, asset_pair)` key maps to a unique **evaluation point** `z`:

```rust
preimage = 0x04 || wallet_strkey[56] || pair_ascii[9]
z        = SHA-256(preimage) with top-3 bits zeroed

Domain separator `0x04` prevents collisions with other hash uses. The wallet strkey is the 56-character Stellar base-32 encoding; the pair is zero-padded ASCII to 9 bytes.

### Value Element

The **value element** `v` encodes the current score and its timestamp:

```rust
preimage = 0x05 || score_le[4] || timestamp_le[8] || z[32]
v        = SHA-256(preimage) with top-3 bits zeroed

Including `z` in the preimage binds `v` to the specific key — a value from one key cannot be replayed as a value for another. Domain separator `0x05`.

### Leaf Hash

Each `(z, v)` pair produces a **leaf hash**:

```
leaf = SHA-256(0x02 || z || v)   // domain separator 0x02 = DOMAIN_LEAF

### Running Commitment

The global commitment is an **XOR-hash accumulator**:

```
new_C = SHA-256(0x06 || (C_prev XOR leaf_i))

**Properties**:

- **Order-independence**: XOR is commutative, so insertion order does not affect the commitment.
- **Incremental updates**: One SHA-256 per score write (`O(1)` regardless of state size).
- **Incremental removals**: XOR is self-inverse. To remove an entry, XOR its old leaf out.

When a score is *updated* (overwritten), the old leaf is XOR-ed out and the new leaf XOR-ed in, keeping the commitment consistent:

```
C' = SHA-256(0x06 || (SHA-256(0x06 || (C XOR old_leaf)) XOR new_leaf))

### Wire Format

The commitment is exposed as **48 bytes** to match the BLS12-381 G1 compressed point size expected by the specification:

```
output[0..16]  = b"LEDGERLENS_KZG_1"   // context prefix (version tag)
output[16..48] = 32-byte commitment hash

The prefix makes commitments version-locked: a proof from a different protocol version has an incompatible context prefix and cannot be used against a current commitment.

---

## Opening Proofs

### Membership Proof

A membership proof for `(wallet, asset_pair)` contains:

| Field | Size | Description |
|---|---|---|
| `type` | 1 byte | `0x01` = member |
| `z` | 32 bytes | Evaluation point for the key |
| `v` | 32 bytes | Value element (encodes score + timestamp) |
| `witness` | 32 bytes | KZG-analog witness hash |

**Witness construction**:

```
witness = SHA-256(0x03 || C || z || v)   // DOMAIN_WITNESS

This is analogous to the KZG quotient polynomial `Q(x) = (f(x) - v) / (x - z)`, but instantiated as a hash. The witness binds `(z, v)` to the specific commitment `C` — a proof generated against commitment `C` cannot be presented against a different commitment `C'`.

**Verification**:

1. Decode the 48-byte commitment to extract the inner 32-byte hash.
2. Recompute `z_expected` from `(wallet, pair)` using the deterministic evaluation-point derivation.
3. Check `z_expected == z_proof` (key binding).
4. Recompute `expected_witness = SHA-256(0x03 || C || z || v)`.
5. Check `witness == expected_witness`.

### Non-Membership Proof

A non-membership proof proves that a key has **no entry** in the committed state. The value element `v` is fixed to the **non-membership sentinel**:

```rust
NON_MEMBER_SENTINEL = 0x00...00   // all-zeros (32 bytes)

The sentinel is provably unreachable by the membership value derivation, because `derive_value_element` always includes a non-zero domain separator (`0x05`), making its SHA-256 output non-zero with overwhelming probability (2^−256 collision chance).

**Non-membership witness**:

```
witness = SHA-256(0x07 || C || z)   // DOMAIN_NONMEMBER — note: no v

A different domain separator (`0x07` vs `0x03`) ensures membership and non-membership witnesses are never confused, even when `v` coincidentally equals the sentinel.

**Verification**:

1. Check `v_proof == NON_MEMBER_SENTINEL`.
2. Check `score == 0` (caller signals non-membership intent).
3. Recompute `expected_witness = SHA-256(0x07 || C || z)`.
4. Check `witness == expected_witness`.

---

## Range Proofs

Range proofs ("all scores for pair P are below 80") are **off-chain aggregations** of membership proofs:

1. Collect the set of wallet addresses that have a score for pair `P` (from events or an indexer).
2. For each wallet `w`, call `get_membership_proof(w, P)` and `get_state_commitment()`.
3. Verify each proof against the commitment root using `verify_membership`.
4. Check that the score embedded in each proof's `v` field satisfies the bound.

Since `v = SHA-256(0x05 || score || timestamp || z)`, extracting the raw score requires off-chain evaluation — the on-chain `verify_membership` function confirms proof validity but not the numeric bound. The bound check is performed by the verifier on the scores they independently observe (from `get_score` calls or events).

> [!NOTE]
> A future upgrade can add a `verify_range_proof(commitment, pair, threshold, proofs[])` on-chain function that verifies the set of membership proofs in a single call, enabling fully on-chain range verification.

---

## API Reference

### `get_state_commitment() -> BytesN<48>`

Returns the current Verkle commitment over all live `(wallet, asset_pair, score)` tuples. Updated atomically on every accepted score write across all submission paths.

### `get_membership_proof(wallet, asset_pair) -> Bytes`

Returns a 97-byte opening proof. The proof type byte indicates membership (`0x01`) or non-membership (`0x02`). The proof is bound to the commitment at the time of the call.

### `verify_membership(commitment, wallet, asset_pair, score, proof) -> bool`

Verifies a proof against a known commitment root. Returns `true` iff:

- The proof is well-formed (97 bytes, valid type byte).
- The evaluation point in the proof matches `(wallet, asset_pair)`.
- For membership (`type = 0x01`): the witness is consistent with the commitment, `z`, and `v`.
- For non-membership (`type = 0x02`): `score == 0`, `v == NON_MEMBER_SENTINEL`, and the witness is consistent.

---

## Security Model

### Soundness

The scheme is **computationally sound** under the SHA-256 collision-resistance assumption:

- An adversary cannot produce a valid proof for a `(z, v)` pair that was not committed without finding a SHA-256 collision in the witness preimage.
- The commitment binds the entire state: changing any entry's `v` changes the XOR accumulator and thus the commitment, invalidating all previously issued proofs.
- Domain separators (`0x02`–`0x07`) ensure that no hash output from one context can be reused as a valid input in another context.

### Key Binding

The evaluation point `z` is deterministically derived from `(wallet, asset_pair)`:

```
z = SHA-256(0x04 || wallet_strkey || pair_ascii) with field reduction

A proof for key `K = (wallet_1, pair_1)` cannot be presented as a proof for key `K' = (wallet_2, pair_2)` because `verify_membership` independently recomputes `z_expected` and checks `z_expected == z_proof`.

### Value Binding

The value `v` encodes `(score, timestamp, z)`, binding it to both the key and the score value. An adversary cannot substitute a different score without invalidating `v`, and since `v` is part of the witness preimage, an invalid `v` produces an invalid witness.

### Timestamp Freshness

The `timestamp` field is embedded in `v`, which means different submissions of the same score at different times produce different commitments and different proofs. This prevents replay attacks where an old proof is presented as evidence of a current score.

### Limitations

| Limitation | Implication |
|---|---|
| **No zero-knowledge** | The proof reveals `v` which encodes the score. If the verifier knows `z` they can brute-force-recover the score (score is in `[0, 100]`). |
| **Order-independent commitment** | Two states with the same set of entries but inserted in different orders produce the same commitment. This is a feature, not a bug — but it means insertion order is not proven. |
| **No tree structure** | The XOR accumulator does not support subtree proofs (e.g., "all wallets for pair P"). Full range proofs require collecting individual proofs. |
| **Hash-based security** | Security relies on SHA-256, not BLS12-381 pairings. If SHA-256 is broken, the commitment scheme is broken. This is the same assumption as the existing Merkle-root attestation. |
| **On-chain trusted execution** | The commitment is computed by the contract itself — it is as trustworthy as the contract's execution. The commitment does not provide security against a malicious contract operator; it provides integrity guarantees to parties verifying proofs off-chain. |

### Comparison to Real KZG / BLS12-381

| Property | This implementation | Real KZG over BLS12-381 |
|---|---|---|
| Proof size | 97 bytes | 48 bytes (G1 point) |
| Verification cost | 2 SHA-256 calls | 2 pairing operations |
| Trusted setup | None required | SRS required |
| Security assumption | SHA-256 collision resistance | q-SDH over BLS12-381 |
| Subtree proofs | Not supported | Supported |
| Zero-knowledge | No | With additional masking |
| On-chain availability | Full (Soroban SHA-256) | Not yet (no BLS12-381 host function) |

When Soroban gains BLS12-381 host functions, this module can be upgraded to a true KZG scheme by replacing `derive_evaluation_point`, `compute_membership_witness`, and `update_commitment` with proper polynomial evaluation, quotient polynomial computation, and group operations — without changing the public API.

---

## Worked Example

### Setup

Suppose the contract has two live entries:

```
Entry A: wallet=GA..., pair=XLM_USDC, score=42, timestamp=1000
Entry B: wallet=GB..., pair=BTC_USDC, score=75, timestamp=2000

### Commitment Computation

```
z_A   = SHA-256(0x04 || "GA..."[56] || "XLM_USDC"[9]) with field reduction
v_A   = SHA-256(0x05 || 42_le32 || 1000_le64 || z_A) with field reduction
leaf_A = SHA-256(0x02 || z_A || v_A)

z_B   = SHA-256(0x04 || "GB..."[56] || "BTC_USDC"[9]) with field reduction
v_B   = SHA-256(0x05 || 75_le32 || 2000_le64 || z_B) with field reduction
leaf_B = SHA-256(0x02 || z_B || v_B)

C_0   = 0x00...00   (initial zero state)
C_1   = SHA-256(0x06 || (C_0 XOR leaf_A))
C_2   = SHA-256(0x06 || (C_1 XOR leaf_B))

commitment = b"LEDGERLENS_KZG_1" || C_2   // 48 bytes

### Membership Proof for Entry A

```
witness_A = SHA-256(0x03 || C_2 || z_A || v_A)

proof_A = [0x01, z_A[32], v_A[32], witness_A[32]]   // 97 bytes

### Verification of proof_A

1. Extract C_2 from commitment (bytes [16..48]).
2. Recompute z_A from "GA..." and "XLM_USDC".
3. Check z_A == proof_A[1..33]. ✓
4. Recompute SHA-256(0x03 || C_2 || z_A || v_A).
5. Check result == proof_A[65..97]. ✓ → proof valid.

### Non-Membership Proof for (GB..., XLM_USDC)

GB has no score for XLM_USDC (only BTC_USDC):

```
z_C   = SHA-256(0x04 || "GB..."[56] || "XLM_USDC"[9]) with field reduction
witness_C = SHA-256(0x07 || C_2 || z_C)   // DOMAIN_NONMEMBER, no v

proof_C = [0x02, z_C[32], 0x00...00[32], witness_C[32]]   // 97 bytes

Verification confirms `v == 0` and witness matches, proving absence.
