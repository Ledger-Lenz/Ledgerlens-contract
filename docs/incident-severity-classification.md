# Incident Severity Classification for Contract Events

Maps `ledgerlens-score` emitted events and failed operations to incident
severity levels and escalation paths, so on-call has a deterministic answer
to "how bad is this" instead of judgment calls under pressure.

> **Source of truth for event names:** [`contracts/ledgerlens-score/src/events.rs`](../contracts/ledgerlens-score/src/events.rs)
> **Source of truth for error names:** [`docs/errors.md`](errors.md)

## Severity levels

| Level | Meaning | Response time |
|---|---|---|
| **Sev-0** | Risk gate down or funds/upgrade path affected without authorization | Page immediately, all hands |
| **Sev-1** | Degraded trust in score integrity or signer set; contained but urgent | Page on-call, respond within 15m |
| **Sev-2** | Anomalous but bounded; needs investigation, not immediate paging | Ticket + Slack alert, respond within 1 business day |
| **Sev-3** | Informational / expected operational activity | Log only, no alert |

## Classification table

### Signer churn

| Event | Severity | Escalation |
|---|---|---|
| `sig_rem` (signer removed) unattributed to a change ticket | **Sev-1** | Page on-call — cross-check against runbook [`signer-compromise.md`](runbooks/signer-compromise.md) |
| `sig_add` (signer added) unattributed | **Sev-1** | Page on-call — could be attacker adding a colluding signer |
| `sig_exp` / `sig_expd` (signer expired) | **Sev-2** | Ticket to rotate; expected lifecycle event if within TTL policy |
| `sig_add` / `sig_rem` matching a tracked rotation change ticket | **Sev-3** | Log only |

### Pause / gate availability

| Event | Severity | Escalation |
|---|---|---|
| `paused` outside a scheduled maintenance window | **Sev-0** | Page immediately — risk gate is down |
| `paused` matching a scheduled maintenance ticket | **Sev-3** | Log only |
| `pr_pause` (single pair paused) unattributed | **Sev-1** | Page on-call |
| `unpaused` | **Sev-2** | Confirm SLOs in [`slo-operational-targets.md`](slo-operational-targets.md) hold post-unpause before closing |
| `gate_enf` (strict enforcement mode toggled) | **Sev-0** | Page immediately — this is an authorization-relevant control change |

### Upgrade / governance

| Event | Severity | Escalation |
|---|---|---|
| `upg_exec` (upgrade executed) unattributed to a tracked proposal | **Sev-0** | Page immediately, all hands — potential unauthorized code change |
| `upg_exec` matching a tracked, reviewed proposal | **Sev-2** | Verify new wasm hash matches release artifact; log |
| `upg_veto` | **Sev-2** | Ticket — confirm veto was intentional governance action |
| `upg_appr` (upgrade approval recorded) | **Sev-3** | Log only, tracked as part of normal governance flow |
| `mv_prop` / `mv_act` / `mv_depr` (model version lifecycle) | **Sev-3** | Log only unless activation is unattributed, then **Sev-1** |

### Rejection spikes

| Event | Severity | Escalation |
|---|---|---|
| `iqr_rej` (statistical-deviation rejection) rate spike from a single signer | **Sev-1** | Page on-call — possible compromised or misbehaving signer, see [`signer-compromise.md`](runbooks/signer-compromise.md) |
| `bat_ok` (batch accepted/rejected counts) with rejected ratio > 20% in a batch | **Sev-2** | Ticket — investigate submission pipeline |
| Isolated `iqr_rej` / per-entry batch rejections within normal noise bounds | **Sev-3** | Log only |

### Stale scores / freshness

| Event / condition | Severity | Escalation |
|---|---|---|
| `is_service_alive() == false` (heartbeat breach) | **Sev-1** | Page on-call — see freshness SLO in [`slo-operational-targets.md`](slo-operational-targets.md) |
| Score staleness exceeds `DEFAULT_STALENESS_WINDOW_SECS` for a widely-queried pair | **Sev-2** | Ticket + Slack alert |
| `hb_upd` (heartbeat threshold reconfigured) | **Sev-2** | Verify change was intentional and matches SLO doc |
| `sw_upd` (staleness window updated) | **Sev-2** | Verify change was intentional and matches SLO doc |

### Destructive / irreversible actions

| Event | Severity | Escalation |
|---|---|---|
| `wdl_lck` (withdrawal lock triggered) unattributed | **Sev-0** | Page immediately — funds-adjacent control |
| `upg_exec` (see Upgrade table — always at least Sev-2, Sev-0 if unattributed) | — | — |
| `emb_set` (wallet score embargoed) unattributed | **Sev-1** | Page on-call — verify against known enforcement action |
| `adm_done` (admin transfer completed) unattributed | **Sev-0** | Page immediately, all hands |
| `adm_init` / `adm_canc` (admin transfer initiated/cancelled) | **Sev-2** | Verify against a tracked governance ticket |

## Out of scope

No GrantFox/campaign-specific severity labeling. Does not replace the
LedgerLens risk model or off-chain scoring pipeline — this classifies
**contract-emitted signals**, not off-chain model outputs.
