# Batch Attestation — Merkle-Root Verification Spec

**Status:** Stable · **Contract:** `LedgerLensScoreContract` · introduced in
`CONTRACT_VERSION` 3.

`submit_scores_batch_attested` is the cryptographic-payload-integrity
companion to `submit_scores_batch`. The legacy batch entry point only
enforces Soroban's native `service.require_auth()` check — the *transaction
was sent by the authorised service key* is verified, but *this specific
batch payload was produced by the off-chain detection pipeline* is not. A
compromised service key (or unauthorised relayer) can therefore submit
arbitrary scores in a batch.

The attested batch path closes that gap with a Merkle tree whose root is
signed once with the existing service secp256k1 key, and whose per-entry
inclusion proofs the contract walks on-chain.

## 1. Goals

* **One signature per batch** rather than one per entry. A 20-entry batch
  needs one `secp256k1_recover` call on-chain instead of 20 — a meaningful
  gas saving.
* **Forward-compatibility with the existing attestation key.** The same
  secp256k1 key registered via `set_service_pubkey` signs both
  single-score `ScoreAttestation`s and the new `BatchAttestation` root
  signatures, so no key rotation is needed to opt in.
* **Backward-compatible surface.** `submit_scores_batch` is unchanged;
  `submit_scores_batch_attested` is a *new* entry point that callers opt
  into. Both paths coexist indefinitely.

## 2. Public types

```rust
/// Merkle-root attestation for a complete batch: a single secp256k1
/// signature over the Merkle root of every leaf commitment in the batch.
pub struct BatchAttestation {
    pub merkle_root: BytesN<32>,
    /// 65-byte signature, identical byte layout to ScoreAttestation:
    /// 32-byte `r`, 32-byte `s`, 1-byte recovery id (must be `0` or `1`).
    pub signature: BytesN<65>,
}

/// A single batch entry that carries its own Merkle inclusion proof.
pub struct ScoreSubmissionWithProof {
    pub submission: ScoreSubmission,
    pub proof: Vec<BytesN<32>>,
    /// Bit i (LSB = 0) is 0 if the sibling at level i is to the right of
    /// the node being walked up, 1 if to the left.
    pub proof_flags: u32,
}
```

## 3. Domain-separation scheme (chosen over sorted-pair)

The contract uses the **RFC 9162 / RFC 6962 style explicit preimage prefix**
scheme to distinguish leaves from internal nodes, rather than the
alternative sorted-pair scheme:

* **Leaf**: `Le = SHA-256(0x00 || compute_commitment(submission))`
  — 33-byte preimage (1-byte prefix + 32-byte commitment).
* **Internal node**: `N = SHA-256(0x01 || left || right)`
  — 65-byte preimage (1-byte prefix + 32-byte left + 32-byte right).

The `0x00` leaf marker can never collide with the `0x01` internal-node
marker, so a leaf hash can never equal an internal-node hash regardless
of the input distribution. There is no need for an additional sort step
to randomize sibling ordering — sibling position is conveyed cleanly by
the explicit `proof_flags` field below.

### Why not sorted-pair?

* Sorted-pair is **incompatible** with this contract's `proof_flags`
  design. `proof_flags` records whether the sibling at each level sits
  to the **left** or right of the node being walked up; a sorted-pair
  scheme derives sibling ordering from the hash values themselves (the
  smaller hash always goes left), making the position-flag redundant
  *and* incorrect.
* Sorted-pair leaves a proof ambiguous: another tree that produces the
  same root via a different shape would still verify, because what
  matters to a sorted-pair verifier is just the multiset of nodes, not
  the shape. With explicit prefixes, the shape is part of the protocol.

### Why not double-hash leaves?

* `compute_commitment` already returns the SHA-256 of a 175-byte preimage.
  The Merkle leaf could plausibly be that digest *unmodified*, since
  leaves (32 bytes) and internal nodes (32 bytes) collide on output
  length and would need separate prefixes anyway. We hash once more
  with the `0x00` prefix instead of skipping the prefix; this keeps the
  shape of the protocol symmetrical (every level hashes exactly one
  prefixed preimage) and leaves a single obvious way to compute the
  preimage.

## 4. Off-chain tree construction

For a batch of `N` entries (the contract enforces `N ≤ MAX_BATCH_SIZE` =
20, but the algorithm works for any power-of-two leaf count up to
`2^30`):

1. For each entry `i`, compute `commit_i = compute_commitment(submission_i)`
   — the same 175-byte preimage `submit_score` already binds.
2. For each entry, hash it into a leaf:
   `Le_i = SHA-256(0x00 || commit_i)`.
3. Pad the leaf list to a power of two (`leaf_count` is rounded up to
   `2^ceil(log2(N))`) by duplicating the last leaf. This is the standard
   Merkle "tail duplication" shape — stronger than zero-padding against
   known-leaf attacks because no extra zero-derived subtrees are
   introduced. The contract does not need to know whether you padded by
   zero-fill or tail-duplication, because the proof's reconstruction is
   unambiguous.
4. Build the tree bottom-up. At each non-bottom level, for each pair
   `(L, R)` (left to right), compute `N = SHA-256(0x01 || L || R)`. Use
   the resulting nodes as the next level's input.
5. Stop when one 32-byte root remains.

The pipeline then signs the root with the off-chain secp256k1 signing key
registered via `set_service_pubkey`, producing a 65-byte signature with
the same byte layout as `ScoreAttestation`.

### Worked 4-leaf example

Suppose we have four entries whose underlying commitments are `C0, C1, C2, C3`.
The leaves are:

```
L0 = SHA-256(0x00 || C0)
L1 = SHA-256(0x00 || C1)
L2 = SHA-256(0x00 || C2)
L3 = SHA-256(0x00 || C3)
```

The internal nodes are:

```
N0 = SHA-256(0x01 || L0 || L1)
N1 = SHA-256(0x01 || L2 || L3)
R  = SHA-256(0x01 || N0 || N1)        ← merkle_root
```

The per-entry proofs are:

| Index | `proof`     | `proof_flags` |
|------:|-------------|--------------:|
|   0   | `[L1, N1]`  | `0`           |
|   1   | `[L0, N1]`  | `1`           |
|   2   | `[L3, N0]`  | `2`           |
|   3   | `[L2, N0]`  | `3`           |

Walk-through for index 2 (proof = `[L3, N0]`, flags = `2`):

```
level 0: bit 0 of flags (value 2 → binary 10) is 0 → sibling on right.
         current = SHA-256(0x01 || L2 || L3) = N1
level 1: bit 1 of flags (value 2 → binary 10) is 1 → sibling on left.
         current = SHA-256(0x01 || N0 || N1) = R   ✓ matches root
```

## 5. On-chain verification

`submit_scores_batch_attested` performs the following steps in order:

1. **Contract-state guards.** `NotInitialized`, `ContractPaused`, then
   `ServicePubkeyNotSet` (the new entry point has no "skip attestation"
   mode — if no pubkey is configured, the call fails fast before
   signature recovery).
2. **Service authorization.** Either M-of-N (`signers.len() >= threshold`
   and each signer is in `ServiceSet` and individually `require_auth`s)
   or legacy single-service `require_auth`, exactly mirroring
   `submit_score`.
3. **Batch-shape guards.** `EmptyBatch` (size 0) or `BatchTooLarge` (size
   > `MAX_BATCH_SIZE`).
4. **Root signature verification.** One `secp256k1_recover` over
   `SHA256(attestation.merkle_root)` — **not** over `merkle_root` directly
   — recovered public key compared against the registered service
   pubkey. The 65-byte signature format is byte-identical to
   `ScoreAttestation`, so this is the same code path as the per-score
   verifier, just with a different digest.

   **Why the SHA-256 wrap?** soroban-sdk 21.x's `env.crypto().secp256k1_recover`
   takes an opaque `Hash<32>`, and `Hash<N>` has no public constructor —
   it can only be built via a host crypto function (`sha256`, `keccak256`).
   To get an opaque handle from a 32-byte `merkle_root` passed in as
   `BytesN<32>`, the contract wraps once via `env.crypto().sha256`.
   The off-chain pipeline signs `SHA256(root)`, so both sides agree.
   The wrap is purely a compatibility shim — no security property
   changes. (A direct signing of `root` would be equivalent.) If a
   future soroban-sdk release adds a public `Hash::from_array`
   constructor, this wrap can be dropped.

   If the root signature fails verification, the **entire batch is
   rejected** with `Error::InvalidAttestation` — a bad root signature
   means no entry can be trusted to have come from the off-chain
   pipeline.
5. **Per-entry Merkle proof verification.** For each entry `i`:
   * `proof.len() > MAX_MERKLE_PROOF_DEPTH` (currently 30) → reject
     with `Error::InvalidAttestation`. The contract cannot afford an
     unbounded number of SHA-256 invocations.
   * Walking the full proof from the recomputed leaf (`compute_merkle_leaf`)
     to the supplied `merkle_root` yields the same root → entry may
     proceed. Any mismatch → that entry alone is rejected with
     `Error::InvalidAttestation`; the rest of the batch still
     processes.
6. **Existing per-entry validation. Mirrors `submit_scores_batch`**:
   `InvalidScore` (`score > 100`), `InvalidConfidence` (`confidence >
   100`), `InvalidTimestamp` (`timestamp == 0`), `RateLimitExceeded`
   (per-pair cooldown). Rejection code is recorded in the entry's
   `BatchEntryResult` — the batch is never aborted by a single bad
   entry.

The proof loop runs to completion on every entry regardless of whether
an intermediate hash diverges — so the gas cost is always bounded and
no timing-style side channel is exposed.

## 6. Reference off-chain pipeline (Python)

A minimal reference for the off-chain detection pipeline. Uses `pyca/cryptography`
for secp256k1.

```python
import hashlib
from dataclasses import dataclass
from typing import List, Tuple
from cryptography.hazmat.primitives.asymmetric import ec, utils
from cryptography.hazmat.primitives import hashes, serialization
from cryptography.hazmat.backends import default_backend

LEAF_PREFIX  = b"\x00"
INNER_PREFIX = b"\x01"

def commitment(submission: dict) -> bytes:
    """Reproduces the contract's `compute_commitment` layout."""
    preimage = b""
    # (wallet strkey, pair zero-padded to 9, score LE u32, bf, ml,
    #  ts LE u64, confidence LE u32, model_version LE u32,
    #  contract_address strkey, network_id — see docs/attestation-spec.md.)
    # ... omitted for brevity; see attestation-spec.md for the full layout.
    return hashlib.sha256(preimage).digest()

def merkle_leaf(commit: bytes) -> bytes:
    return hashlib.sha256(LEAF_PREFIX + commit).digest()

def merkle_internal(left: bytes, right: bytes) -> bytes:
    assert len(left) == len(right) == 32
    return hashlib.sha256(INNER_PREFIX + left + right).digest()

def build_tree(leaves: List[bytes]) -> bytes:
    # Caller is responsible for padding to a power of two.
    assert len(leaves) & (len(leaves) - 1) == 0
    level = leaves
    while len(level) > 1:
        level = [merkle_internal(level[i], level[i+1]) for i in range(0, len(level), 2)]
    return level[0]

def proof_for(leaves: List[bytes], index: int) -> Tuple[List[bytes], int]:
    assert len(leaves) & (len(leaves) - 1) == 0
    proof, flags, level, idx = [], 0, leaves, index
    while len(level) > 1:
        sibling_idx = idx ^ 1
        if idx & 1:
            flags |= 1 << len(proof)
        proof.append(level[sibling_idx])
        level = [merkle_internal(level[i], level[i+1]) for i in range(0, len(level), 2)]
        idx //= 2
    return proof, flags

def sign_root(root: bytes, private_key) -> bytes:
    # **Sign `SHA256(root)`, not `root` directly.** The on-chain verifier
    # wraps `merkle_root` through SHA-256 once before
    # `secp256k1_recover` because soroban-sdk 21.x's `Hash<32>` is
    # opaque and has no public constructor — only host crypto
    # functions can build it. We mirror that wrap here so both sides
    # agree on the digest bytes.
    verified_digest = hashlib.sha256(root).digest()
    sig = private_key.sign(verified_digest, ec.ECDSA(utils.Prehashed(hashes.SHA256())))
    r = sig[0:32]
    s = sig[32:64]
    # recovery_id is determined out-of-band (e.g. by trying both 0/1
    # during signing). See cryptography's recoverable ECDSA for
    # higher-level helpers.
    raise NotImplementedError("see the pipeline's signing utility")
```

## 7. Reference off-chain pipeline (TypeScript)

Same algorithm, native Web Crypto API.

```typescript
import { createHash } from "node:crypto";

const LEAF_PREFIX  = Buffer.from([0x00]);
const INNER_PREFIX = Buffer.from([0x01]);

function merkleLeaf(commit: Buffer): Buffer {
  return createHash("sha256").update(Buffer.concat([LEAF_PREFIX, commit])).digest();
}

function merkleInternal(left: Buffer, right: Buffer): Buffer {
  return createHash("sha256").update(Buffer.concat([INNER_PREFIX, left, right])).digest();
}

function buildTree(leaves: Buffer[]): Buffer {
  // leaves must be padded to a power of two
  if ((leaves.length & (leaves.length - 1)) !== 0) throw new Error("not power of two");
  let level = leaves;
  while (level.length > 1) {
    const next: Buffer[] = [];
    for (let i = 0; i < level.length; i += 2) next.push(merkleInternal(level[i], level[i + 1]));
    level = next;
  }
  return level[0];
}

function proofFor(leaves: Buffer[], index: number): { proof: Buffer[]; flags: number } {
  let proof: Buffer[] = [];
  let flags = 0;
  let level = leaves;
  let idx = index;
  while (level.length > 1) {
    const siblingIdx = idx ^ 1;
    if (idx & 1) flags |= 1 << proof.length;
    proof.push(level[siblingIdx]);
    const next: Buffer[] = [];
    for (let i = 0; i < level.length; i += 2) next.push(merkleInternal(level[i], level[i + 1]));
    [level, idx] = [next, idx >> 1];
  }
  return { proof, flags };
}

function signRoot(root: Buffer, privateKey: crypto.KeyObject): Buffer {
  // **Sign `SHA256(root)`, not `root` directly.** The on-chain verifier
  // wraps `merkle_root` through SHA-256 once before
  // `secp256k1_recover` because soroban-sdk 21.x's `Hash<32>` is opaque
  // and can only be built via host crypto functions. The pipeline
  // mirrors that wrap here so both sides agree on the digest bytes.
  const verifiedDigest = createHash("sha256").update(root).digest();
  // Higher-level recoverable-ECDSA helpers vary by Node version;
  // a production pipeline should use a library that exports
  // `(r || s || recovery_id)` as 65 bytes directly.
  throw new Error("see the pipeline's signing utility");
}
```
```

## 8. Edge cases and limits

| Edge case                                | Behaviour                                                                                                  |
|------------------------------------------|------------------------------------------------------------------------------------------------------------|
| 1-entry batch                            | `proof = []`, `proof_flags = 0`. `verify_merkle_proof` skips its loop and matches `leaf == root`.          |
| `proof.len() > MAX_MERKLE_PROOF_DEPTH` (`30`) | Reject with `Error::InvalidAttestation` regardless of whether the root would otherwise match.          |
| Tampered `merkle_root` in `BatchAttestation` | `secp256k1_recover` returns the wrong key → entire batch rejected with `Error::InvalidAttestation`.    |
| Tampered entry-level proof               | Just that entry gets `Error::InvalidAttestation` in `BatchEntryResult.rejection_code`; rest of batch proceeds. |
| Service pubkey never set                 | Reject with `Error::ServicePubkeyNotSet` before signature recovery (clearer than `InvalidAttestation`). |
| No matching `service_set`/`threshold` config | Legacy single-service `require_auth` path runs — same behaviour as `submit_score`.                      |

## 9. Acceptance criteria

Cross-references the issue ([#40](https://github.com/Ledger-Lenz/Ledgerlens-contract/issues/40)):

1. `submit_scores_batch_attested` is implemented with per-entry Merkle proof verification.
2. `verify_merkle_proof` and `sha256_pair` (here called `hash_internal_node`)
   are pure, testable functions, callable from sibling modules in the crate.
3. Domain separation (leaf prefix `0x00`, internal prefix `0x01`) is documented
   and implemented.
4. `supports_interface("batch_attested")` returns `true`.
5. All 12 mandatory tests in `test_batch_attestation.rs` pass.
6. `cargo clippy -- -D warnings` and `cargo fmt --check` pass on the change.
