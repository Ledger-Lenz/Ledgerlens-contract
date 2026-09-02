# Security Audit: `zk_range_proof.rs`

**Scope:** `contracts/ledgerlens-score/src/zk_range_proof.rs` (hand-rolled Curve25519
field arithmetic, Ed25519 point arithmetic, and a Bulletproofs-style ZK range
proof), and its single call site, `verify_score_range_proof` in
`contracts/ledgerlens-score/src/lib.rs`.

**Method:** Manual/static code review (line-by-line reading of the arithmetic,
the prover, the verifier, and the on-chain integration) plus algebraic
derivation of the verifier's checks against the Bulletproofs paper (Bünz et
al., "Bulletproofs: Short Proofs for Confidential Transactions and More") and
RFC 8032 (Ed25519 point encoding).

**Not performed:** No dynamic testing. The local environment has no MSVC
linker (`link.exe`), so `cargo test` cannot build the `soroban-sdk` dependency
tree here; the existing test suite (`test_zk_range_proof.rs`) was read but not
executed. No fuzzing, no property-based testing (associativity/distributivity/
inverse laws), no formal verification, no constant-time analysis with tooling
(e.g. `dudect`, ct-verif). Everything below is a static-review finding, not a
proof; treat "confirmed" findings as high-confidence structural readings, not
as computationally verified.

---

## Summary of findings

| # | Severity | Area | Finding |
|---|----------|------|---------|
| 1 | **Critical** | Range-proof soundness / commitment binding | Bulletproof and Pedersen generators are small, known scalar multiples of the same base point — they are not independent |
| 2 | Medium | Range-proof robustness | Decompressed/deserialized curve points are not checked for prime-order subgroup membership (cofactor-8 small-subgroup risk) |
| 3 | Medium | Side-channel exposure | All field/scalar/point arithmetic is variable-time; secret-dependent branches exist in the prover's hot path |
| 4 | Low | Correctness / usability | `prove_range_proof` silently truncates `v` to its low 8 bits with no bounds check; no matching upper-bound check on `threshold` in `verify_score_range_proof` |
| 5 | Low | Fiat-Shamir design note | `wallet`/`asset_pair` are not bound into the proof transcript itself — binding is delegated entirely to the storage equality check |
| 6 | Informational | Robustness | `Fe::invert`/`Sc::invert` silently return `0` on a zero input (Fermat's-little-theorem identity), rather than signaling a degenerate case |

---

## 1. Critical: Bulletproof/Pedersen generators are not independent

**Location:** `get_generators()` (line 524) and `get_vector_generators()` (line 531).

```rust
pub fn get_generators() -> (Pt, Pt, Fe) {
    let d = ...;
    let g_pt = g();
    let h_pt = g_pt.mul(Sc::from_u64(8), d); // independent generator H = 8G
    (g_pt, h_pt, d)
}

pub fn get_vector_generators(d: Fe) -> ([Pt; 8], [Pt; 8]) {
    let mut gs = [Pt::identity(); 8];
    let mut hs = [Pt::identity(); 8];
    for i in 0..8 {
        gs[i] = g().mul(Sc::from_u64((16 + i) as u64), d);
        hs[i] = g().mul(Sc::from_u64((32 + i) as u64), d);
    }
    (gs, hs)
}
```yaml

Every generator used anywhere in this scheme — `H`, `G_0..G_7`, `H_0..H_7` —
is constructed as `k * G` for a small, hard-coded, **publicly known** integer
`k` (`H = 8G`; `G_i = (16+i)G`; `H_i = (32+i)G`), where `G` is the standard
Ed25519 base point. The comment on `h_pt` even calls this "independent,"
which reflects a misunderstanding of the requirement: an "independent"
generator must have an *unknown* discrete-log relationship to `G` (typically
constructed via a verifiable hash-to-curve / nothing-up-my-sleeve procedure),
not merely be a *different point*.

**Why this matters:**

- **Pedersen commitment binding** (`C = g^v h^r`) relies entirely on nobody
  knowing `log_g(h)`. Here `log_g(h) = 8` is public and trivial. Since 8 is
  invertible mod the group order `L`, the map `(v, r) -> v + 8r (mod L)` is
  an explicit, computable bijection: anyone can produce a *different* opening
  `(v', r')` of the *same* commitment point `C`. This directly contradicts
  the "perfectly binding" claim in `docs/zk-range-proofs.md`'s Security Model
  section ("The score reporter cannot open the commitment to any score other
  than the one submitted").
- **Bulletproofs range-proof soundness** requires the same independence for
  the full generator vector, not just `h`. Because *every* point in this
  scheme — `g`, `h`, all `gs[i]`, all `hs[i]`, and therefore every prover
  message (`A`, `S`, `T1`, `T2`, `L[i]`, `R[i]`, `Q`) — is provably a scalar
  multiple of the single point `G` with a known coefficient, the two
  verifier checks in `verify_range_proof` (the `lhs1 == rhs1` polynomial-
  commitment check and the `P_prime == expected` inner-product check)
  collapse from "genuine group-element equalities anchored by an unknown
  discrete-log relation" into equalities of *known linear combinations of a
  single scalar exponent*. That is exactly the degenerate case Bulletproofs'
  soundness proof excludes: with all generators mutually known multiples of
  one point, there is no discrete-log hardness gap left between "the prover
  actually supplied a valid bit-decomposition witness" and "the prover
  algebraically solved for values that satisfy the two final scalar
  equations." This is a foundational break, not an edge case.

**Impact:** Both the confidentiality/binding property of the score commitment
and the soundness of the "score < threshold" proof are undermined at the
generator-construction level, independent of anything else in the file being
correct. In the worst case this could allow a party who controls the
committed score (the LedgerLens service, or anyone who can see a commitment)
to produce a range proof that verifies successfully without the underlying
statement being true.

**Recommendation:** Replace the multiplicative small-integer generators with
a standard nothing-up-my-sleeve construction — e.g. derive each of
`h, g_0..g_7, h_0..h_7` as `hash_to_curve("LedgerLens-Bulletproof-<label>-<i>")`
using a documented, deterministic hash-to-curve method (Elligator2 for
Ed25519/Curve25519, as used by `curve25519-dalek`'s `RistrettoPoint::
hash_from_bytes`, or an equivalent try-and-increment construction), so that no
party — including the implementers — can know or compute a discrete-log
relation between any two generators. Given the issue's own framing (hand-
rolled crypto vs. an audited crate), the lowest-risk fix is likely to replace
this module's field/point/generator layer with `curve25519-dalek` +
`bulletproofs` directly rather than hand-maintaining a NUMS generator scheme.

**Disclosure note:** Per this repository's `SECURITY.md`, exploit-level
detail (e.g. a worked numeric forgery) is intentionally omitted from this
public document. The mechanism above is sufficient for a maintainer or
auditor to reproduce and confirm the break. Given the "Maybe Rewarded" /
active-campaign labels on issue #396, whoever triages this PR should also
loop in `security@ledgerlens.io` directly rather than relying solely on this
PR sitting in the public review queue, and should treat any *already-live*
deployment that uses this contract for real economic gating as at-risk until
fixed.

---

## 2. Medium: no prime-order subgroup check on deserialized points

**Location:** `decompress_pt_32` (line 373), `Bulletproof::from_bytes` (line 597).

`decompress_pt_32` correctly recovers a point on the curve (it solves for `x`
given `y` and checks both candidate square roots, in the standard RFC 8032
pattern), and `Bulletproof::from_bytes` checks each deserialized point with
`is_on_curve`. Neither path checks that the point lies in the prime-order
subgroup generated by `G` (Ed25519's curve has cofactor 8, so points of order
1, 2, 4, or 8 exist and satisfy the curve equation without being in the main
subgroup).

This is the classic Ed25519 "small-subgroup confinement" pitfall: a
`commitment` or proof component crafted to have low order can, in some
protocols, be used to bypass or manipulate verification logic that implicitly
assumes every point is a generator-multiple in the full-order subgroup. Given
finding #1 already breaks the scheme's soundness independent of this, the
practical severity here is compounded rather than independent, but this
should be fixed regardless as defense in depth once independent generators
are in place — e.g. by checking `point.mul(Sc::from_u64(8), d).is_identity()`
is false (order does not divide 8) or, more robustly, by using a
prime-order group construction (Ristretto) instead of raw Edwards points.

---

## 3. Medium: variable-time arithmetic on secret values

**Location:** throughout `Fe`, `Sc`, `Pt` (`mul`, `pow`, `invert`, `shr1`), and
the prover in `prove_range_proof` (line 791).

All scalar/field/point operations branch directly on operand bits
(`if e.0[0] & 1 == 1`, `while !e.is_zero()`, `if out.ge(&Self::P)`) and use a
plain (non-constant-time) square-and-multiply / double-and-add ladder. In
`prove_range_proof`, several of these operations run on secret data: the
blinding scalars (`alpha`, `beta`, `tau1`, `tau2`), the bit vectors (`a_L`,
`a_R`, derived from the secret value `v`), and the blinding factor `r`. A
timing side channel on any of these could, over repeated proof generations
observable by a co-located or network attacker, leak information about the
committed score.

Whether this is exploitable depends on the deployment threat model: if proof
generation happens inside a Soroban contract invocation (deterministic,
metered, no wall-clock timing observable to an external attacker) the risk is
low; if it happens in an off-chain service that a network attacker can time
(e.g. a query API that generates proofs on demand), the risk is real and
should be treated seriously, since the entire point of this scheme is to keep
the score confidential.

**Recommendation:** If proof generation ever happens in a context where an
attacker can observe timing (off-chain service, shared infrastructure), the
field/scalar arithmetic used in the *proving* path needs constant-time
implementations (constant-time conditional select instead of `if`/branches
keyed on secret bits). The *verifier* path operates only on public values
(the proof itself, the public commitment, the public threshold) and does not
need to be constant-time.

---

## 4. Low: silent truncation of out-of-range witnesses / no `threshold` upper bound

**Location:** `prove_range_proof` (line 797), `verify_score_range_proof` in
`lib.rs` (line 3639).

`prove_range_proof(env, v: u32, ...)` decomposes `v` into exactly 8 bits
(`(v >> i) & 1` for `i` in `0..8`), i.e. it only ever encodes `v mod 256`,
while the commitment `V = g^v h^r` (line 821) is built from the *full* `v`.
If a caller ever invokes this with `v >= 256` (i.e. `threshold - 1 - score >=
256`, meaning `threshold >= score + 257`), the bit vector no longer matches
the value actually committed in `V`, and the resulting proof will not
verify — this fails safe today (the caller just gets a proof that doesn't
verify, not a security bypass), but it fails with no clear error message,
and `verify_score_range_proof` has no explicit upper bound on `threshold` to
reject this case early with a clear error instead of a confusing
verification failure.

**Recommendation:** Add an explicit bounds check — either `debug_assert!(v <
256)` (or a proper `Result`-returning guard, since this can be reached with
attacker- or caller-supplied `threshold`) in `prove_range_proof`, and reject
`threshold > 256` up front in `verify_score_range_proof` with a distinct
error rather than a generic `false`.

---

## 5. Low: transcript does not directly bind `wallet` / `asset_pair` / `threshold`

**Location:** `hash_fs_y_z`, `hash_fs_x`, `hash_fs_w`, `hash_fs_challenge_ip`
(lines 714–755); call site `verify_score_range_proof` (line 3639).

The Fiat-Shamir transcript hashes `V` (i.e. the derived `C'`), `A`, `S`,
`T1`, `T2`, `tx`, `taux`, `mu`, and the inner-product round points — all
proof-internal or derived values. It never directly hashes `wallet`,
`asset_pair`, or `threshold`. This is not unsound as implemented, because
`threshold` is baked into `C'` before hashing (`C' = g^{threshold-1} *
C^{-1}`, computed by the verifier before calling `verify_range_proof`), and
`wallet`/`asset_pair` binding is enforced separately, by the exact-match
check `stored_commitment != commitment_bytes` against per-`(wallet,
asset_pair)` storage before the proof is even looked at.

This is a reasonable design (commit-then-check-then-verify), but it means
the soundness of "this proof is about *this* wallet/pair" depends entirely
on that storage equality check and not on the cryptography itself. If
`verify_range_proof` or `Bulletproof`/`decompress_pt_32` are ever reused
directly (e.g. exposed as a standalone verification entry point, or reused
in a different contract) without that surrounding storage check, the
wallet/pair binding would silently disappear. Worth a code comment at
minimum; ideally the transcript would hash `wallet` and `asset_pair`
directly so the binding is a cryptographic invariant rather than an
integration convention enforced by a single call site.

---

## 6. Informational: `invert()` on zero returns zero silently

**Location:** `Fe::invert` (line 184), `Sc::invert` (line 350).

Both use Fermat's little theorem (`a^(p-2) mod p`), which correctly yields
`0` when `a == 0` (since `0^(p-2) = 0`) rather than panicking. This is
algebraically "correct" in the sense of matching the mathematical identity,
but it means a degenerate zero input (e.g. `y == 0` in the inverse-power
step of `verify_range_proof`, astronomically unlikely from a SHA-256 output
but not provably impossible) is silently absorbed rather than surfaced. Not
independently exploitable given finding #1's severity, but worth a comment
noting the behavior is intentional, since a reader unfamiliar with the
Fermat identity could easily mistake this for a bug.

---

## What was checked

- Manual trace of `Fe`/`Sc` `add`/`sub`/`mul`/`pow`/`invert`/`shr1` against
  the standard "carry-propagate, multiply high limb by 19, conditional
  final subtract" reduction technique for the pseudo-Mersenne prime
  `2^255 - 19`, and the bit-serial reduction technique used for `Sc::mul`
  modulo the (non-pseudo-Mersenne) Ed25519 group order `L`. No structural
  defect found by inspection; **not** verified computationally (associativity,
  distributivity, `a * a.invert() == 1` for random `a`, etc. were not tested
  — see "Not performed" above).
- The Edwards point addition/doubling formulas (`Pt::add`) against the
  standard unified twisted-Edwards addition law; the curve constant `d =
  -121665/121666 mod p` matches the published Ed25519 value; the base point
  `g()` matches the standard published Ed25519 base point encoding.
- The full Bulletproofs range-proof protocol as implemented in
  `prove_range_proof`/`verify_range_proof` against the aggregated single-
  value range-proof construction in Bünz et al., specifically: the `l(X)`/
  `r(X)` polynomial construction, the `delta(y,z)` closed form, the two
  final verifier equations, and the 3-round inner-product argument's
  challenge-folding of `gs`/`hs`/`l`/`r`. Found the generator-independence
  break (finding #1) at the foundation of this construction.
- The Fiat-Shamir hash functions for domain separation (`b"y_z"`, `b"z"`,
  `b"x"`, `b"w"`, `b"ip"` prefixes) and transcript ordering/completeness
  relative to what each round's challenge is supposed to bind.
- The on-chain integration in `verify_score_range_proof` (commitment
  lookup/match, `C'` derivation, threshold handling).
- Point/scalar (de)serialization (`to_bytes`/`from_bytes`/`compress_pt`/
  `decompress_pt_32`) for structural correctness and on-curve validation.

## What was not checked / recommended follow-ups

- No dynamic testing was run (see "Not performed" above); the existing
  `test_zk_range_proof.rs` suite was read but not executed in this
  environment.
- No property-based/differential testing of `Fe`/`Sc` arithmetic against a
  reference implementation (e.g. `curve25519-dalek::FieldElement`/`Scalar`).
- No constant-time analysis with tooling.
- No review of how `SeededPrng` (line 759, a SHA-256 counter-mode stream
  cipher used as the *entire* randomness source for a real number of secret
  blinding values) is seeded outside this file — the quality of the entropy
  source feeding `SeededPrng::new(seed)` was out of scope for this file-level
  review but is directly load-bearing for both the hiding property of the
  commitment and the soundness of the proof (a predictable or reused seed
  would be independently catastrophic); recommend auditing the call site
  that supplies `seed`.

## Acceptance-criteria mapping

- "A documented audit trail exists covering \[correctness, soundness,
  completeness, side-channel\] properties" — satisfied by this document.
- "Any confirmed soundness or side-channel issues are fixed ... or the risk
  is explicitly accepted and documented" — **not fixed** in this PR
  (intentionally: fixing hand-rolled generator/curve code belongs in a
  separate, appropriately scoped, independently reviewed follow-up per the
  issue's own instructions, not bundled into the audit PR). Finding #1 is
  documented above as the risk to be tracked and fixed; it should **not**
  be treated as "accepted" — it is assessed as critical and in need of a
  prompt fix.
