# Security Policy

## Scope

This policy covers the **`ledgerlens-score` Soroban smart contract** and the surrounding deployment tooling in this repository.

Out-of-scope:
- The off-chain detection pipeline (`core`, `data` repos)
- The public API server (`api` repo)
- The web dashboard (`dashboard` repo)

## Supported Versions

| Contract version | Status  |
|-----------------|---------|
| 1.x (testnet)   | Active  |
| 0.x (pre-release)| Not supported |

## Reporting a Vulnerability

**Please do not open a public GitHub issue for security vulnerabilities.**

Report security issues by emailing **security@ledgerlens.io** with the subject line:

```
[SECURITY] <short description>
```

Include:

1. A clear description of the vulnerability and the affected contract function(s).
2. Steps to reproduce or a proof-of-concept (PoC) — even a pseudocode sketch helps.
3. The potential impact (e.g. unauthorized score submission, admin key extraction, fund loss if integrated with an AMM).
4. Your contact details if you would like to be credited.

## Response Timeline

| Milestone                     | Target            |
|------------------------------|-------------------|
| Acknowledgement              | Within 48 hours   |
| Triage and severity rating   | Within 7 days     |
| Fix or mitigation in testnet | Within 21 days    |
| Public disclosure            | After fix ships   |

We follow [Responsible Disclosure](https://en.wikipedia.org/wiki/Coordinated_vulnerability_disclosure). We will not take legal action against researchers who follow this policy.

## Contract Threat Model

| Attack vector                        | Mitigation                                                        |
|--------------------------------------|-------------------------------------------------------------------|
| Deployment initialization front-running | Both contract initializers require authorization from the nominated admin before writing any privileged state |
| Unauthorized score write             | `submit_score` requires `service.require_auth()`                  |
| Compromised service key              | `pause()` halts submissions; `set_service()` rotates the key      |
| Accidental admin key loss            | Two-step transfer: new admin must call `accept_admin()`           |
| Score poisoning via out-of-range data | `score` and `confidence` clamped to 0-100 on-chain               |
| Resource exhaustion via oversized symbols or cryptographic byte payloads | `asset_pair` symbols are capped at 9 bytes and malformed oversized commitment / dispute replay byte payloads reject before hashing, proof parsing, shard fan-out, or storage writes |
| DoS via unbounded storage            | History ring buffer capped at `HISTORY_MAX_DEPTH` (10) per pair  |
| Large batch denial of service        | Batch size capped at `MAX_BATCH_SIZE` (20) per invocation        |
| M-of-N `signers`/`admin_signers` Vec padding | Signer lists bounded by the current signer-set size before any per-signer storage read or `require_auth` call (`TooManySigners`) — see "Memory-Exhaustion & Nested Input Bounds" below |
| Compromised service floods a pair with submissions | Per-`(wallet, asset_pair)` cooldown (`RateLimitExceeded`); admin-bounded `[MIN_COOLDOWN_SECS, MAX_COOLDOWN_SECS]`, with `override_rate_limit` as an audited emergency escape hatch |
| Silent malicious contract upgrade    | Time-locked upgrade governance (see below): mandatory delay + on-chain proposal anyone can inspect, plus admin veto |
| Data-residency / GDPR erasure request | `clear_score_history` and `clear_score` (admin-only) permanently remove scoring data from persistent storage; `clr_hist` / `clr_scr` events provide an on-chain audit trail of every erasure |

The complete caller, signer, and contract-as-caller review is recorded in
[`docs/cross-contract-authorization-audit.md`](docs/cross-contract-authorization-audit.md).
The corresponding lifecycle and adversarial coverage index is
[`docs/critical-state-transition-matrix.md`](docs/critical-state-transition-matrix.md).

## Upgrade Governance & Threat Model

Soroban contracts are immutable once deployed, but the admin can replace the
entire WASM via `env.deployer().update_current_contract_wasm(...)`. Left
ungoverned, a single admin key (or a compromised one) could swap in a backdoor
— disabling auth checks, redirecting score writes, or bricking integrations —
in **one transaction, with no warning**. To remove that single point of
instant failure, upgrades are gated behind an on-chain time-lock.

### The flow

1. **Propose** — the admin calls `propose_upgrade(new_wasm_hash)`. This stores
   an `UpgradeProposal` (committed hash, `proposed_at`, `executable_after`,
   `proposed_by`) and emits `upgrade_proposed`. It does **not** change the code.
2. **Monitoring window** — for at least `MIN_UPGRADE_DELAY_SECS` (48 hours;
   configurable up to 14 days) nothing can execute. Anyone — users, monitoring
   bots, integrating protocols — can call `get_pending_upgrade` to read the
   committed hash and `executable_after`, diff the proposed WASM, and alert the
   community.
3. **Execute or veto** — only after `executable_after` can the admin call
   `execute_upgrade`, which re-checks the clock at execution time (never a
   cached decision) before installing the WASM. At any point during the window
   the admin can `veto_upgrade` to cancel — the escape hatch if a proposal is
   malicious or the key was compromised. The veto emits `upgrade_vetoed` naming
   the caller, completing the audit trail.

### Threat model

| Concern | Mitigation |
|---------|------------|
| Admin pushes a backdoor instantly | No instant path exists — every upgrade waits out the full delay before `execute_upgrade` will run |
| Compromised **service** key triggers an upgrade | Service keys have no upgrade powers; only the current admin can propose/execute/veto |
| Caller manipulates the time-lock | Deadlines derive from `env.ledger().timestamp()`, which is deterministic and not caller-settable |
| Stale/early execution | `execute_upgrade` re-verifies `now >= executable_after` on every call |
| Admin shortens the window to rush an upgrade | `set_upgrade_delay` is bounded to `[MIN, MAX]`; it can never go below 48 h, and a lowered delay only applies to *future* proposals — an in-flight proposal keeps its original `executable_after` |
| No record of who acted | `UpgradeProposal.proposed_by` plus the `upgrade_*` events give a full on-chain audit trail |

**Safe vs. sensitive delay changes:** *raising* `MIN`-bounded delay is always
safe (it only lengthens scrutiny). *Lowering* the configured delay shortens the
community veto window and should only be done with broad community consensus.

### What monitors should watch

Subscribe to the `upgrade_proposed` event (or poll `get_pending_upgrade`). On a
new proposal, verify the committed `new_wasm_hash` against a reviewed,
reproducible build before `executable_after`. An unexpected proposal — or one
whose hash does not match a published, audited build — is the signal to raise
an alarm and, if warranted, push for a `veto_upgrade`.

## Memory-Exhaustion & Nested Input Bounds (#612)

### Assets and actors

- **Asset:** the contract's CPU-instruction and memory budget for a single
  invocation, shared by every caller in that ledger close. A single
  over-large call can burn a disproportionate share of it before failing.
- **Actors:** any address able to invoke a public entry point — including
  a **contract-as-caller** in a composability setup (e.g. an aggregator or
  gateway forwarding a batch on a user's behalf), which is no more trusted
  than a direct EOA caller for sizing purposes.
- **Trust assumption:** the `signers` / `admin_signers` / `submissions` /
  `proof` arguments are entirely attacker-controlled in shape and size,
  even when the *content* (a valid address, a valid signature) requires a
  real credential the attacker may not have.

### Nested shapes in scope

`submit_scores_batch_attested(signers, submissions, attestation)` has two
independent attacker-controlled dimensions nested inside one call:

1. **Outer:** `submissions: Vec<ScoreSubmissionWithProof>`, bounded by
   `MAX_BATCH_SIZE` (20).
2. **Inner:** each entry's `proof: Vec<BytesN<32>>`, bounded by
   `MAX_MERKLE_PROOF_DEPTH` (30) — checked inside `verify_merkle_proof`
   before the hash-walk loop runs.

Both bounds were already enforced and are exercised at their maximum
combined size by `test_max_batch_of_max_depth_proofs_no_panic_and_bounded_cost`
in `test_memory_exhaustion.rs`.

### Gap found and fixed

The same call's `signers: Vec<Address>` M-of-N list — plus the identical
pattern in the shared `require_service_signers_auth` (used by
`veto_parameter_change`) and `require_admin_auth` (used by every
admin-gated entry point) — had **no upper bound**. A caller could pass an
arbitrarily long `Vec<Address>`, and the M-of-N loop would perform a
storage read (`check_signer_expired`) and a `require_auth` host call for
every entry before the function could fail, regardless of whether any of
those addresses could actually authorize the call.

**Fix:** each of the three call sites now rejects with `TooManySigners`
when `signers.len() > <current signer-set size>`, before the loop runs.
A legitimate M-of-N call never needs more entries than the signer set
itself contains, so this is a pure bound, not a behavior change for any
correct caller.

### Fail-safe behavior

- The bound check runs immediately after the existing "not enough
  signers" (`threshold`) check and before any storage access or
  `require_auth`, so the failure path itself does zero attacker-scaled
  work.
- Rejection returns `Result::Err`, never panics — preserving the
  "public reads/writes must not panic" invariant for every caller,
  including a contract-as-caller.
- `TooManySigners` reuses the `ServiceSetFull` discriminant (see
  `errors.rs`) rather than adding a new one: the error enum is already at
  Soroban's 50-variant XDR hard limit, and `ServiceSetFull` is the
  existing "too many index entries" family (`CounterpartyLinkFull`,
  `DisputeIndexFull`, `EmbargoedWalletIndexFull` are aliased the same
  way). No ABI or storage change.

### Alternatives rejected

- **A fixed constant ceiling (e.g. `MAX_SERVICE_SIGNERS`) instead of the
  live set size:** rejected because it would still allow padding up to
  that constant with no benefit to a legitimate caller, and would need to
  be kept in sync with `MAX_SERVICE_SIGNERS`/`MAX_ADMIN_SIGNERS`
  independently. Bounding by the actual current set size is both tighter
  and self-maintaining.
- **A new `Error` discriminant:** rejected — the enum is at the 50-variant
  XDR limit; aliasing an existing discriminant matches the project's
  established convention.
- **Rewriting `require_admin_auth`/`require_service_signers_auth` into a
  single shared generic helper:** would touch many call sites for a
  cosmetic dedup and is out of scope for this issue; each is fixed in
  place with the identical one-line bound instead.

### What monitors/operators should watch

No new event is required — a rejected oversized call is indistinguishable
in on-chain effect from any other rejected auth call (nothing is written).
Operators running off-chain signer tooling should treat a `TooManySigners`
error the same as `UnauthorizedSigner`/`InsufficientSigners`: a signal to
check the pipeline building the `signers` argument, not a contract
incident.

### Rollback / recovery

This is a pure validation tightening with no storage or ABI change: it can
be rolled back by reverting the three bound checks in a follow-up upgrade
(through the existing time-locked upgrade governance above) with no
migration step, since no persisted data or discriminant values change.



There is currently no formal bug bounty program.  Outstanding security reports will be credited in the release notes and can be listed in your portfolio with our written consent.

## Disclosure Policy

When a vulnerability is confirmed and a fix is ready, we will:

1. Deploy the patched contract to testnet.
2. Notify downstream teams (`api`, `dashboard`) with the new `CONTRACT_ID`.
3. Publish a post-mortem in the GitHub Releases section.
4. Credit the reporter (unless they prefer to remain anonymous).
