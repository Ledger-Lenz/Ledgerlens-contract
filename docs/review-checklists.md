# Reviewer Checklists for High-Risk Changes

Quick, actionable gates for the five change categories most likely to introduce a real
vulnerability or a silent breaking change in this repo. Each checklist is meant to be worked
through against the actual diff in a few minutes, not read as background — for the reasoning
behind an item, follow its link rather than expanding the checklist itself.

Every category shares two blanket rules from [`docs/invariants.md`](invariants.md), repeated here
because they're violated most often by changes that don't look like they touch them:

- [ ] Does this change any `query_risk_gate*` behavior, even indirectly (a new early return, a
      changed storage read)? If so, every new/changed path still resolves uncertainty to `false`.
      See [`docs/invariants.md` § 1](invariants.md#1-fail-closed-gates).
- [ ] Does this change touch a function reachable from a read/gate context? If so, it contains no
      `unwrap`/`expect`/`panic!` reachable from external input.
      See [`docs/invariants.md` § 2](invariants.md#2-no-panic-reads).

---

## Governance changes

M-of-N signer sets, thresholds, parameter time-locks, model version lifecycle, dispute
resolution.

- [ ] Every new/changed threshold has an explicit `threshold <= set.len()` check — this is
      already asserted post-mutation by
      [`invariants::invariant_check`](../contracts/ledgerlens-score/src/invariants.rs), but a new
      governance path must call it (in test builds) or be covered by an equivalent test.
- [ ] Signer/admin set changes respect the relevant `MAX_*` cap (`MAX_SERVICE_SIGNERS`,
      `MAX_ADMIN_SIGNERS`) and return the matching `*SetFull` error rather than truncating
      silently.
- [ ] Time-locked proposals (parameter changes, upgrades) derive their deadline from
      `env.ledger().timestamp()`, never from a caller-supplied or cached value, and re-check the
      deadline at execution time — not just at proposal time.
- [ ] A veto/cancel path exists and is tested for every new proposal type that has a delay window.
      A time-lock with no way to abort a bad proposal is not governance, it's just a warning.
- [ ] Model version transitions are one-way where the code says they are (`Deprecated` has no
      re-activation path) — a new transition must not accidentally open a reverse edge.
- [ ] New dispute/escalation logic is bounded (`MAX_OPEN_DISPUTES`, `MAX_DISPUTES_PER_ACTOR`) and
      the bond/fee accounting nets to zero across every resolution branch (settled, timed out,
      vetoed) — check this with a test per branch, not just the happy path.

**Deep dive:** [`docs/governance.md`](governance.md),
[`SECURITY.md` § Contract Threat Model](../SECURITY.md#contract-threat-model).

---

## Cryptography changes

Signature verification (`verify_signature`, `verify_attestation`, `verify_threshold_attestation`),
Merkle proofs, ZK range proofs, Verkle commitments, commit-reveal.

- [ ] Every comparison of secret-dependent or attacker-influenced fixed-size data (pubkeys,
      commitments, hashes) uses `subtle::ConstantTimeEq` (`ct_eq`), not `==`. Non-constant-time
      comparison is a timing side-channel, not just a style nit — see
      [`docs/audit-constant-time-2026-07-20.md`](audit-constant-time-2026-07-20.md) for the
      existing pass/fail bar and which paths are already covered.
- [ ] Any new commit-reveal mechanism scopes its storage key by **every** identity field relevant
      to the commitment (signer/model, wallet, asset pair — whatever applies), deletes the key
      immediately on successful reveal, and uses `temporary()` storage with a bounded TTL. See
      [`docs/replay-protection-audit.md`](replay-protection-audit.md) for the standard this repo
      already meets and the exact scoping trace to model a new mechanism on.
- [ ] The commitment/signature hash construction is domain-separated (a fixed prefix or distinct
      byte layout per mechanism) so a valid signature/commitment for one purpose can't be replayed
      as valid for another.
- [ ] New Merkle/Verkle proof verification enforces a maximum depth (`MAX_MERKLE_PROOF_DEPTH`) and
      rejects malformed proofs (wrong length, wrong element count) before doing any
      cryptographic work, not after.
- [ ] `require_auth()` and attestation/signature verification are not conflated in review
      comments or in the code's error handling — they answer different questions (see
      [`docs/glossary.md` § Attestation](glossary.md#attestation)) and a change that only
      satisfies one is not a substitute for the other.
- [ ] New crypto code has a dedicated audit doc or an explicit note in the PR that one is owed —
      don't let novel cryptography merge covered only by unit tests that assert the happy path
      output, with no adversarial-input or timing analysis at all.

**Deep dive:** [`docs/audit-constant-time-2026-07-20.md`](audit-constant-time-2026-07-20.md),
[`docs/replay-protection-audit.md`](replay-protection-audit.md),
[`docs/zk-range-proof-audit.md`](zk-range-proof-audit.md), [`docs/verkle-commitment.md`](verkle-commitment.md).

---

## Storage changes

New `DataKey`/`DataKeyB`/`DataKeyC`/`DataKeyD` variants, TTL/rent handling, bounded collections.

- [ ] No existing `DataKey*` variant is renamed, removed, or reordered. Unlike `Error` (which has
      a CI-enforced append-only check, `tools/check_error_discriminants.sh`), storage key enums
      have **no alias mechanism and no CI guard** — renaming a variant silently changes what
      on-chain bytes are read/written for it, orphaning existing stored data with no error at all.
      Treat this as strictly append-only even though nothing currently enforces it.
- [ ] A new collection has an enforced `MAX_*` cap from the moment it's introduced, not added
      later once someone notices unbounded growth. See
      [`docs/invariants.md` § 3](invariants.md#3-bounded-storage) for the existing catalogue to
      extend.
- [ ] Write paths call `extend_ttl` (or the contract's lazy-extension helper) immediately after
      the write; read paths that are meant to be side-effect-free and callable cross-contract use
      the `peek_*` variants, not the TTL-extending getters. See
      [`docs/storage-layout.md`](storage-layout.md#ttl-extension-triggers-read-vs-write-paths).
- [ ] Persistent-tier data that's core scoring history uses `persistent()`; short-lived
      rate-limit/cooldown/commitment state uses `temporary()`. Putting long-lived state in
      `temporary()` risks silent, unrecoverable deletion; putting short-lived state in
      `persistent()` wastes rent.
- [ ] A bulk operation that iterates a bounded collection (embargo revocation, TTL sweep, dispute
      resolution) is capped per-call (`MAX_EXPIRING_ENTRIES_PER_CALL`-style) so it can't blow a
      single transaction's resource budget even when the collection is at its `MAX_*` ceiling.

**Deep dive:** [`docs/storage-layout.md`](storage-layout.md), [`docs/adr/storage-key-split.md`](adr/storage-key-split.md).

---

## Upgrade changes

Anything touching `propose_upgrade` / `execute_upgrade` / `veto_upgrade`, the WASM build, or
`CONTRACT_VERSION`.

- [ ] The mandatory delay is still enforced with no bypass — `execute_upgrade` re-checks
      `now >= executable_after` at execution time, not just at proposal time.
- [ ] `set_upgrade_delay` changes stay within `[MIN_UPGRADE_DELAY_SECS, MAX_UPGRADE_DELAY_SECS]`,
      and a change to the *configured* delay does not retroactively shorten an *in-flight*
      proposal's `executable_after`.
- [ ] `veto_upgrade` still works for every new proposal shape — an upgrade path added without a
      corresponding cancel path is a regression on the whole point of the time-lock.
- [ ] If this PR changes the WASM build inputs (dependencies, `rust-toolchain.toml`, build flags),
      the reproducible-build CI job (`repro-build-1`/`repro-build-2`/`repro-verify` in
      [`.github/workflows/ci.yml`](../.github/workflows/ci.yml)) still passes — a non-deterministic
      build breaks the community's ability to verify a proposed upgrade hash before it executes.
- [ ] Anything that changes `CONTRACT_VERSION` also updates
      [`docs/interface-versioning-policy.md`](interface-versioning-policy.md)'s version table and
      has a `CHANGELOG.md` `Unreleased` entry with a migration guide, per that policy's 30-day
      notice requirement — a version bump with no changelog entry is a policy violation, not a
      formality.

**Deep dive:** [`README.md` § Upgrade Governance](../README.md#upgrade-governance),
[`SECURITY.md` § Upgrade Governance & Threat Model](../SECURITY.md#upgrade-governance--threat-model),
[`docs/reproducible-builds.md`](reproducible-builds.md).

---

## Composability changes

`query_risk_gate*`, `supports_interface`, anything `ledgerlens-aggregator` or the mock
integrations (`mock-amm`, `mock-lending`) depend on.

- [ ] Every gate function's signature, return type, and fail-closed behavior is unchanged, *or*
      the PR explicitly documents the ABI break and the required
      [`docs/interface-versioning-policy.md`](interface-versioning-policy.md) 30-day notice — see
      the known `query_risk_gate_relative` exception in
      [`docs/invariants.md` § 1](invariants.md#1-fail-closed-gates) for what an *undocumented*
      version of this problem looks like.
- [ ] A new `supports_interface` capability symbol is added to **both** the in-code doc-comment
      table above `supports_interface` **and** [`docs/interface-spec.md`](interface-spec.md) in
      the same PR — not just the match arm. (Five existing symbols currently fail this rule; see
      [`docs/invariants.md` § 4c](invariants.md#4c-interface--capability-registry) — don't add
      a sixth.)
- [ ] If this PR changes a struct decoded directly by integrators (`RiskScore`,
      `AggregateRiskScore`, etc.), any new field is appended at the end, never inserted or
      reordered — see
      [`docs/interface-versioning-policy.md` § 3.3](interface-versioning-policy.md#33-what-constitutes-a-new-field-vs-a-changed-struct).
- [ ] `ledgerlens-aggregator` changes that read from shards still check
      `shard_supports_required_interface` before trusting a shard's response, and still apply the
      configured `ConflictPolicy` rather than special-casing one shard.
- [ ] The AMM/lending mock integrations (`contracts/mock-amm`, `contracts/mock-lending`) still
      compile and their tests still pass against the changed interface — they're the executable
      proof that the guard-clause pattern in the README actually works, not just example code.

**Deep dive:** [`README.md` § Composability](../README.md#composability),
[`docs/interface-spec.md`](interface-spec.md), [`docs/amm-integration-guide.md`](amm-integration-guide.md),
[`docs/aggregator-conflict-resolution.md`](aggregator-conflict-resolution.md).
