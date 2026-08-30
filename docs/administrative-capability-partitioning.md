# Administrative Capability Partitioning

Issue: #695 — Add administrative capability partitioning by operation risk.

## The problem this closes

Before this change, every privileged endpoint in `ledgerlens-score` —
pausing the contract, changing score-gating parameters, proposing a WASM
upgrade, and managing the admin signer set itself — was gated by exactly
the same check: `require_admin_auth`, backed by one shared admin
key/M-of-N set. Whoever could satisfy that one quorum could do all of it.
A single compromised admin signer set (or a colluding quorum) could pause
the contract, rewrite score thresholds, push a WASM upgrade, *and* rotate
the admin set itself, with no way to require a different, disjoint set of
approvers for the more dangerous operations.

## The fix

`Policy` (in `types.rs`) names five administrative capabilities,
partitioned by operation risk:

| Policy | Representative endpoints |
|---|---|
| `ScorePolicy` | `propose_parameter_change`, `execute_parameter_change` |
| `UpgradeGovernance` | `propose_upgrade`, `execute_upgrade`, `veto_upgrade` |
| `EmergencyPause` | `pause`, `unpause` |
| `DataDeletion` | `clear_score`, `clear_score_history` (pre-existing — see below) |
| `SignerAdmin` | `add_admin_signer`, `remove_admin_signer`, `set_admin_threshold` |

Each mapped endpoint still requires routine admin quorum via
`require_admin_auth` first (that check is unchanged). On top of that,
`Self::require_policy_auth(env, policy, admin_signers)` looks up an
optional, independently configured `PolicyApproval { enabled, approver }`
for that policy and, when `enabled`, additionally requires
`approver.require_auth()`. The approver must stay disjoint from the
routine admin key/set — checked by `set_policy_approval`, which rejects an
overlapping or missing approver (fail-closed: an enabled policy can never
end up silently equivalent to "no extra check").

Because each policy's approver is configured and checked independently,
authorization obtained under one policy does not carry over to another:
the wrong approver's address simply never satisfies `require_auth()` for
the endpoint it wasn't configured for. See
`test_cross_policy_approver_reuse_fails` in
`test_capability_partitioning.rs`, which configures two *real*, currently
enabled approvers for two different policies and proves the `SignerAdmin`
approver cannot authorize a `EmergencyPause`-gated call.

### `Policy::DataDeletion` is a pointer, not new code

`DataDeletion` reuses the pre-existing `DeletionApprovalPolicy` /
`require_deletion_auth` / `set_deletion_approval_policy` mechanism that
already gated `clear_score` and `clear_score_history` before this issue.
It is listed in the `Policy` enum purely so all five categories named in
#695 share one canonical, documented identifier. `set_policy_approval`
explicitly rejects `Policy::DataDeletion` with `Error::InvalidPolicy` so
there remains exactly one configuration entry point per policy — operators
configure deletion via `set_deletion_approval_policy`, and the other four
via `set_policy_approval`.

## Bounded resource use

`require_policy_auth` does one instance-storage read (`get_policy_approval`)
and, when enabled, exactly one `require_auth()` call — the same constant
amount of work regardless of which policy or how many times it's called.
No unbounded loop or caller-controlled iteration is introduced.

## Compatibility summary

- **No new `Error` variant** — the `#[contracterror]` enum is already at
  the 50-variant XDR cap; `Error::InvalidPolicy` and `Error::InvalidThreshold`
  (both existing aliases/discriminants) are reused.
- **No change to existing storage keys** — `PolicyApprovalEnabled(Policy)`
  / `PolicyApprovalApprover(Policy)` are new `DataKeyD` variants; the
  pre-existing `DeletionPolicyEnabled` / `DeletionApprover` keys and their
  semantics are untouched.
- **New event `pol_appr`** (topics: `pol_appr`, version, `policy`; data:
  `(enabled, approver)`) — additive, emitted only by the new
  `set_policy_approval`. Existing events are unaffected.
- **New public ABI surface**: `set_policy_approval`, `get_policy_approval`,
  and the `Policy` / `PolicyApproval` types, all additive.
- **Behavior change (only when explicitly configured):** by default
  (`enabled = false` for every policy), `pause`, `unpause`,
  `propose_upgrade`, `execute_upgrade`, `veto_upgrade`,
  `propose_parameter_change`, `execute_parameter_change`,
  `add_admin_signer`, `remove_admin_signer`, and `set_admin_threshold`
  behave exactly as before — `require_policy_auth` falls through to the
  unchanged `require_admin_auth` result. Only after an operator opts a
  policy in via `set_policy_approval` does that policy's mapped endpoints
  require the extra approver signature.
