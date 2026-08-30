# On-Chain Governance

LedgerLens uses a lightweight on-chain governance module for admin parameter
changes and contract WASM upgrades. Both flows follow the same high-level
pattern:

```
propose → time-lock → execute (or veto)
```

This document covers **parameter change governance** and the **governance action
registry**.  For WASM upgrade governance, see the
[Upgrade Governance](../README.md#upgrade-governance) section in the README.

## Governance Action Registry

Every privileged admin action is assigned a **stable `u8` discriminant** in
`contracts/ledgerlens-score/src/governance_actions.rs`.  This registry is the
single source of truth for action identifiers — no other file may introduce a
new raw byte literal for a governance action.

### Why a registry?

Before the registry, each audit-chain entry was stamped with an ad-hoc magic
byte (e.g. `0x01`, `0x02`) defined inline with only a terse comment.  There
was no central lookup, no uniqueness guarantee, and no stable name exposed to
events or documentation.  The registry fixes all three:

| Problem | Solution |
|---------|----------|
| Magic bytes scattered in `lib.rs` | All discriminants live in one module with doc-comments |
| No compile-time uniqueness check | Each constant has a distinct value; `is_known_action()` can detect gaps |
| Events carried no action type | `gov_action` event embeds `action_id` and `action_name` |
| Off-chain tools had to decode raw bytes | `all_actions()` provides a stable reverse-lookup slice |

### Stability rules

1. **Discriminants are frozen once assigned.**  Off-chain audit-chain replay
   tools reconstruct the Merkle root by replaying stored discriminants.  A
   reassignment silently corrupts every root that followed the change.
2. **New actions claim the next sequential value.**  Predictable ordering makes
   manual inspection and test fixtures easier.
3. **`0x00` is reserved** (uninitialized / zeroed payload sentinel).
4. **Name strings must be ≤ 9 ASCII characters** so they fit in a Soroban
   `symbol_short!()`.

### Registry table

| Discriminant | Constant | Name (`action_name()`) | Contract function |
|:------------:|----------|------------------------|-------------------|
| `0x00` | `GOV_ACTION_RESERVED` | *(reserved)* | — |
| `0x01` | `GOV_ACTION_SET_SERVICE` | `set_svc` | `set_service` |
| `0x02` | `GOV_ACTION_ADD_SERVICE_SIGNER` | `add_sig` | `add_service_signer` |
| `0x03` | `GOV_ACTION_SET_ADMIN_THRESHOLD` | `set_athr` | `set_admin_threshold` |
| `0x04` | `GOV_ACTION_PAUSE` | `pause` | `pause` |
| `0x05` | `GOV_ACTION_UNPAUSE` | `unpause` | `unpause` |
| `0x06` | `GOV_ACTION_PROPOSE_UPGRADE` | `upg_prop` | `propose_upgrade` |

### `gov_action` event

Every time a governance action is appended to the Merkle audit chain the
contract emits a `gov_action` event:

```
Topic:  ("gov_action", EVENT_VERSION)
Data:   (action_id: u32, action_name: Symbol, new_head: BytesN<32>)
```

`action_id` is the discriminant from the table above.  `action_name` is its
human-readable name string.  `new_head` is the updated Merkle root after the
action was folded in.  Off-chain indexers can filter by `action_id` to build a
typed timeline of all admin activity without decoding raw chain bytes.

### Adding a new action

1. Append a new `GOV_ACTION_*` constant in `governance_actions.rs` using the
   next available discriminant.
2. Add a matching `GOV_ACTION_NAME_*` string constant (≤ 9 chars).
3. Add an arm to `action_name()` and an entry to `all_actions()`.
4. Update this table.
5. Call `Self::append_governance_action(&env, GOV_ACTION_YOUR_ACTION, &data)`
   at the relevant call site in `lib.rs` — **never** call
   `append_governance_action_raw` directly for new actions.

## Motivation

Admin functions such as `set_cooldown`, `set_decay_rate`,
`set_score_velocity_cap`, and `set_history_max_depth` previously took effect
immediately when called by the admin multi-sig. A compromised admin key could
alter core contract parameters without giving the community or service signers
time to react.

Parameter change governance introduces a mandatory delay between proposal and
execution, with a service-signer veto window during the first half of that
delay.

## Flow

```
 Admin                          Contract                         Service signers
   │                               │                                    │
   │ propose_parameter_change      │                                    │
   ├──────────────────────────────►│  store ParameterProposal           │
   │                               │  emit prm_prop                     │
   │                               │                                    │
   │         [0 .. time_lock/2]    │  veto window open                  │
   │                               │◄───────────────────────────────────┤
   │                               │  veto_parameter_change (optional)  │
   │                               │                                    │
   │         [time_lock/2 .. lock] │  irrevocable until execute/expiry  │
   │                               │                                    │
   │ execute_parameter_change      │                                    │
   ├──────────────────────────────►│  apply param, mark Executed        │
   │                               │  emit prm_exec                     │
```

### 1. Propose

An admin calls `propose_parameter_change(admin_signers, param_key, new_value)`.

- Validates the parameter key and value (same bounds as the direct setter).
- Records a `ParameterProposal` with `time_lock_secs = get_upgrade_delay()`
  (minimum 48 hours / `MIN_UPGRADE_DELAY_SECS`).
- Returns a monotonic `proposal_id`.
- Emits `prm_prop`.
- At most **10** proposals may be pending at once.

### 2. Veto (service signers)

During the first half of the time-lock (`now <= proposed_at + time_lock_secs / 2`),
service signers may call `veto_parameter_change(service_signers, proposal_id)`.

- Uses the same M-of-N service signer authorization as score submission.
- Marks the proposal `Vetoed` and removes it from the pending index.
- Emits `prm_veto`.
- After the veto deadline the proposal is **irrevocable** until execution or
  expiry.

### 3. Execute (admin)

After the full time-lock elapses (`now >= proposed_at + time_lock_secs`), an
admin calls `execute_parameter_change(admin_signers, proposal_id)`.

- Re-checks the ledger timestamp at execution time (never cached).
- Applies the parameter change via the same storage paths as the direct setters.
- Marks the proposal `Executed` so it cannot be applied again.
- Emits `prm_exec`.

### 4. Expiry

If a proposal is neither executed nor vetoed within `time_lock_secs * 2`, it
expires and can no longer be executed. Attempting execution marks it `Expired`
and returns `ParameterProposalExpired`.

## Proposal-spam bounds and cleanup behavior

Current contract behavior as of July 25, 2026:

- Pending parameter proposals are capped at `MAX_PENDING_PARAMETER_PROPOSALS = 10`.
- A proposal leaves the pending index immediately when it is executed or vetoed.
- Expired proposals are pruned before new proposals are accepted, and also when
  `get_parameter_proposal` is queried.
- Proposal IDs remain monotonic after cleanup; pruning frees pending capacity,
  not IDs.

This means a compromised admin can create at most 10 concurrent pending
parameter proposals before the contract fails closed with
`TooManyPendingParameterProposals`. The operational load is therefore bounded
by the pending index plus the cost of reviewing at most 10 live proposals.

Recommended operator limits:

- Alert when pending proposals reach 8 of 10.
- Treat any proposal still pending near `time_lock_secs * 2` as cleanup debt
  and either execute, veto, or let the next governance read/propose prune it.
- Keep monitoring keyed to `prm_prop`, `prm_exec`, and `prm_veto` so off-chain
  responders can measure backlog without scanning full storage.

## Supported Parameters

| `param_key` symbol | Direct setter | `new_value` encoding |
|--------------------|---------------|----------------------|
| `cooldown` | `set_cooldown` | 8-byte big-endian `u64` (seconds) |
| `hist_dep` | `set_history_max_depth` | 4-byte big-endian `u32` |
| `decay_rt` | `set_decay_rate` | 8 bytes: numerator `u32` BE + denominator `u32` BE |
| `vel_cap` | `set_score_velocity_cap` | 1 byte enabled (`0`/`1`) + 4-byte `u32` points/hour |
| `upg_dlay` | `set_upgrade_delay` | 8-byte big-endian `u64` (seconds) |

## Read APIs

- `get_parameter_proposal(proposal_id)` — returns the full
  `ParameterProposalRecord` (proposal + status). Callable by anyone.
- `get_pending_param_prop_ids()` — returns IDs still marked pending.

## Security Properties

| Threat | Mitigation |
|--------|------------|
| Instant parameter change by compromised admin | No instant path — every change waits out the full time-lock |
| Service signers blocked from reacting | Veto window during first half of time-lock |
| Stale execution after community objection period | Veto deadline at `time_lock_secs / 2`; irrevocable after |
| Replay / double execution | Executed proposals marked in storage |
| Unbounded storage growth | Cap of 10 concurrent pending proposals; expiry at `2 × time_lock` |
| Time-lock too short | Minimum `MIN_UPGRADE_DELAY_SECS` (48 hours), shared with upgrade governance |

## Events

| Topic | When |
|-------|------|
| `prm_prop` | Proposal created `(proposal_id, param_key, executable_after)` |
| `prm_exec` | Parameter applied `(proposal_id, param_key)` |
| `prm_veto` | Proposal vetoed `(proposal_id, vetoer)` |
| `prm_clean` | Expired proposals cleaned up `(count, oldest_kept_timestamp)` |

## Proposal Cleanup and Lifecycle

Proposals expire at `proposed_at + time_lock_secs * 2` and can no longer be executed. To reclaim storage and
prevent unbounded growth:

1. Call `get_parameter_proposal(proposal_id)` to check status — once `Expired`, ready for cleanup.
2. Call `cleanup_expired_param_proposals(admin_signers)` to permanently remove expired proposals
   that have been expired for **at least 48 hours**.
3. Emits `prm_clean` event with count and oldest retained proposal timestamp.
4. Idempotent — safe to call repeatedly without side effects.

## Governance Simulation and Audit

Before proposing or executing a parameter change, preview its effects without applying it:

1. **Pre-proposal validation**: Call `simulate_parameter_change(param_key, new_value)` to preview the change
   before creating a proposal. Returns before/after values, affected subsystems, and execution window.
2. **Proposal audit**: Call `get_proposal_simulation(proposal_id)` to review the simulated impact of an
   existing proposal during the time-lock window. Deterministic output for reproducible audit trails.
3. **Simulation output includes**:
   - `param_key` — which parameter is changing
   - `current_value` — serialized current parameter value
   - `new_value` — proposed new value
   - `affected_capabilities` — list of subsystems affected (e.g., `["decay", "score"]` for decay rate)
   - `execution_window_start` — earliest execution timestamp
   - `execution_window_end` — expiry timestamp (execution stops being possible after this)

## Two-Person Control for Destructive Operations

Irreversible operations such as `bulk_reset_pair_weight` (clears all pair-weight assignments)
can be gated to require multi-admin approval:

1. Admin calls `set_destructive_multisig(admin_signers, true)` to **enable** the policy.
2. When policy is **enabled**:
   - `bulk_reset_pair_weight` rejects if supplied with only 1 admin signer.
   - Returns `InsufficientAdminSigners` error.
   - Requires at least **2** admin signers in the call.
3. When policy is **disabled** (default):
   - `bulk_reset_pair_weight` works as before — single admin sufficient.
4. Policy defaults to **disabled** for backward compatibility.
5. Admin can toggle on/off at any time with `set_destructive_multisig`.

## Emergency Pause Decision Trees

### Global Contract Pause (Circuit Breaker)

**Scenario**: Compromised service signer, malicious score submissions, or critical vulnerability.

```
├─ Call: pause(admin_signers)
│  │
│  ├─ Effect: All score submissions blocked immediately
│  ├─ Read behavior: get_score() still works (returns stale scores)
│  └─ Time to recover: Admin unpause or automatic unpause after TTL (~1 hour)
│
└─ Recovery:
   └─ When safe, call: unpause(admin_signers)
      └─ Effect: Submissions resume; no data loss
```

**When to use**: System-wide threat or malicious activity. Affects all asset pairs and wallets.

### Per-Pair Pause (Granular Circuit Breaker)

**Scenario**: Single asset pair experiencing anomalies, oracle failure, or market disruption.

```
├─ Call: set_pair_paused(admin_signers, asset_pair, true)
│  │
│  ├─ Effect: Submissions for ONLY this pair blocked
│  ├─ Read behavior: get_score(wallet, asset_pair) returns stale score; other pairs unaffected
│  └─ Time to recover: Immediate manual unpause
│
└─ Recovery:
   └─ When pair is stable, call: set_pair_paused(admin_signers, asset_pair, false)
      └─ Effect: Pair submissions resume
```

**When to use**: Isolated pair problem (e.g., oracle delay, price spike, model miscalibration).
Minimal blast radius; other pairs continue operating normally.

### Submission Freeze (Submit Path Only)

**Scenario**: Need to pause submissions while keeping reads active (e.g., during emergency upgrade).

```
├─ Call: set_submission_freeze(admin_signers, true)
│  │
│  ├─ Effect: submit_scores() and related write operations blocked
│  ├─ Read behavior: get_score(), query_risk_gate() work normally with stale data
│  └─ Time to recover: Immediate manual unfreeze or automatic after TTL
│
└─ Recovery:
   └─ When ready, call: set_submission_freeze(admin_signers, false)
      └─ Effect: Submissions resume
```

**When to use**: Maintenance, data migration, or temporary service disruption. Readers (consuming protocols)
stay unaffected; dApps querying LedgerLens can continue operating during the freeze.

### Decision Matrix

| Scenario | Action | Reversibility | Impact | TTL |
|----------|--------|---------------|---------|----|
| All submissions compromised | `pause(true)` | Manual unpause | Complete halt | ~1 hour |
| Single pair malfunction | `set_pair_paused(pair, true)` | Manual unpause | Pair-only halt | None (manual) |
| Need read-only mode | `set_submission_freeze(true)` | Manual unfreeze | Write-only halt | ~1 hour |
| Score data loss risk | Upgrade + redeploy | Redeploy | Full reset | Manual |

## Maintenance and Administration

### Regular Maintenance Tasks

1. **Weekly**: Review `get_pending_param_prop_ids()` for stalled proposals; veto or wait for expiry.
2. **Monthly**: Run `cleanup_expired_param_proposals(admin_signers)` to reclaim storage.
3. **Before upgrades**: Simulate parameter changes with `simulate_parameter_change` to preview impact.
4. **Incident response**: Check pause status with `is_paused()` and `is_pair_paused(pair)`.

### Audit Trail

- All governance actions emit events: `prm_prop`, `prm_exec`, `prm_veto`, `prm_clean`.
- Integrate with off-chain logging to build a tamper-evident proposal history.
- Use `simulated_at` timestamp in `get_proposal_simulation()` output to track audit window.

## Policy Bundles

`propose_policy_bundle` / `apply_policy_bundle` group the risk threshold and
submission cooldown into a single named change so operators review and roll
out both together, instead of as two independently-timelocked changes that
could land at different times (e.g. a lowered risk threshold taking effect
before its paired cooldown increase, temporarily over-tightening the gate).

This uses the same simple time-lock as `set_risk_threshold` /
`set_history_max_depth` (no veto window, single time-lock, `apply_after` gate
callable by anyone) rather than the richer `propose_parameter_change` flow
documented above — it is a separate, lighter-weight mechanism, not an
extension of the `Supported Parameters` table.

Both fields are validated before anything is stored: an invalid
`risk_threshold` (>100) or `cooldown_secs` (outside
`[MIN_COOLDOWN_SECS, MAX_COOLDOWN_SECS]`) rejects the whole proposal, so
there is no partial proposal. `apply_policy_bundle` writes both fields in the
same call, so no caller can observe one field updated while the other is
still pending.

| Topic | When |
|-------|------|
| `pbdl_prop` | Bundle proposed `(risk_threshold, cooldown_secs, apply_after)` |
| `pbdl_appl` | Bundle applied `(risk_threshold, cooldown_secs)` |

## Related

- WASM upgrade governance: `propose_upgrade` / `execute_upgrade` / `veto_upgrade`
- Upgrade delay configuration: `set_upgrade_delay` / `get_upgrade_delay`
- Threat model: [`SECURITY.md`](../SECURITY.md#upgrade-governance--threat-model)
- Canonical export: [`configuration-export.md`](./configuration-export.md)
- Safe defaults: [`configuration-safe-defaults.md`](./configuration-safe-defaults.md)
