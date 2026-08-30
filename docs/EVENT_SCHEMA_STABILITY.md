# Event Schema Stability Levels

This document defines the compatibility guarantees for each event topic emitted by the LedgerLens Scoring Contract.

## Overview

Events are classified into three stability categories, each with different compatibility commitments:

### Public API Events

**Commitment**: Breaking changes require a version bump and explicit migration documentation.

**Use Cases**:
- Audit trail reconstruction
- Regulatory compliance logging
- Third-party integrations
- Off-chain indexing and querying

**Compatibility Rules**:
1. Field order must never change
2. Field types must never change  
3. Field removal requires major version bump
4. New fields can only be appended to the data payload
5. Changes to field meaning require version bump and migration docs

**Examples**: `score`, `watch`, `breach`, `scr_veto`, `dispute_*`, `admin_*`, `upgrade_*`

### Operator Diagnostic Events

**Commitment**: Changes may occur with one-release notice; off-chain systems should degrade gracefully.

**Use Cases**:
- Monitoring and alerting
- Debugging operational issues  
- Service health checks
- Non-critical observability

**Compatibility Rules**:
1. New fields can be added without notice
2. Fields may be deprecated with one-release notice
3. Field removal requires one-release notice
4. Changes to field meaning require one-release notice

**Examples**: `svc_upd`, `paused`, `sig_add`, `rl_ovrd`, `orc_stale`

### Internal Test-Only Events

**Commitment**: No compatibility guarantees; these are private implementation details.

**Use Cases**:
- Debugging during testing
- Internal performance analysis
- Development-time observability

**Compatibility Rules**:
1. May change freely without notice
2. May be removed without notice
3. Should never be relied upon by off-chain systems

**Examples**: `iqr_rej` (internal outlier metrics)

## Event Lifecycle

When modifying a stable event:

```
1. Identify the event stability level
   ↓
2. If Public API:
   - Increment EVENT_VERSION
   - Add migration documentation
   - Update this file with breaking changes
   - Coordinate with off-chain systems
   ↓
3. If Operator Diagnostic:
   - Add one-release notice in CHANGELOG
   - Update this file with changes
   - Notify operations team
   ↓
4. If Internal Test-Only:
   - Change freely; no coordination needed
```

## Public API Events (Audit Trail)

These events form the backbone of the contract's auditable history. Off-chain systems must be able to consume them reliably across upgrades.

| Event Topic | Emitted By | Auditable Purpose | Stability |
|-------------|-----------|-----------------|-----------|
| `score` | `submit_score()` | Record wallet risk score submission | Public API v1 |
| `scr_dlt` | `get_effective_score()` | Track score changes and trends | Public API v1 |
| `scr_comm` | `commit_score()` | Mark score as finalized | Public API v1 |
| `scr_veto` | `veto_pending_score()` | Admin intervention in scoring | Public API v1 |
| `watch` | `set_watchlist()` | Regulatory watch lists | Public API v1 |
| `emb_set` | `set_embargo()` | Regulatory holds | Public API v1 |
| `emb_lift` | `lift_embargo()` | Release from hold | Public API v1 |
| `pw_upd` | `set_pair_weight()` | Risk weighting per asset pair | Public API v1 |
| `pw_rst` | `reset_pair_weights()` | Reset to default weights | Public API v1 |
| `thresh` | `set_threshold()` | Breach threshold changes | Public API v1 |
| `breach` | `check_threshold()` | Threshold breach events | Public API v1 |
| `brc_rst` | `reset_breach_counter()` | Admin breach counter reset | Public API v1 |
| `adm_init` | `initiate_admin_transfer()` | Admin key transfer initiated | Public API v1 |
| `adm_done` | `accept_admin_transfer()` | Admin key transfer completed | Public API v1 |
| `adm_canc` | `cancel_admin_transfer()` | Admin key transfer cancelled | Public API v1 |
| `upg_prop` | `propose_upgrade()` | Upgrade proposal | Public API v1 |
| `upg_exec` | `execute_upgrade()` | Upgrade execution | Public API v1 |
| `upg_veto` | `veto_upgrade()` | Upgrade rejection | Public API v1 |
| `upg_appr` | `add_upgrade_approval()` | Multi-sig upgrade approval | Public API v1 |
| `cons_scr` | `submit_consensus_score()` | Multi-model consensus result | Public API v1 |
| `mv_act` | `activate_model_version()` | Model version activation | Public API v1 |
| `mv_depr` | `deprecate_model_version()` | Model version deprecation | Public API v1 |
| `bat_ok` | `submit_scores_batch_attested()` | Batch attestation result | Public API v1 |
| `disp_open` | `open_dispute()` | Score dispute opened | Public API v1 |
| `disp_res` | `resolve_dispute()` | Score dispute resolved | Public API v1 |
| `disp_to` | `timeout_dispute()` | Dispute timeout | Public API v1 |

## Operator Diagnostic Events

These events support operational observability and may change with notice.

| Event Topic | Emitted By | Purpose | Stability |
|-------------|-----------|---------|-----------|
| `svc_upd` | `update_service()` | Service address changed | Diagnostic v1 |
| `svc_sil` | `check_service_heartbeat()` | Service silence alert | Diagnostic v1 |
| `svc_res` | `check_service_heartbeat()` | Service resumed | Diagnostic v1 |
| `paused` | `pause_contract()` | Contract pause | Diagnostic v1 |
| `unpaused` | `unpause_contract()` | Contract unpause | Diagnostic v1 |
| `sig_add` | `add_signer()` | Signer added | Diagnostic v1 |
| `sig_rem` | `remove_signer()` | Signer removed | Diagnostic v1 |
| `rl_ovrd` | `override_rate_limit()` | Rate limit override | Diagnostic v1 |

## Compatibility Testing

All events are tested to ensure:

1. **Schema Stability**: `test_stable_event_topic_immutability` verifies that public API events maintain field order and types
2. **Version Tracking**: `test_all_events_carry_schema_version` ensures all events carry the correct schema version
3. **Registry Accuracy**: `test_event_stability_registry` validates that all event topics have correct stability classifications

## Migration Guide for Operators

### If you depend on Public API events:

1. **Monitor schema versions** in event topics
2. **Subscribe to upgrades** at https://github.com/Ledger-Lenz/Ledgerlens-contract/releases
3. **Implement version-aware parsers** that can handle multiple event schema versions during transition periods
4. **Test against backwards compatibility** before deploying off-chain changes

### If you depend on Operator Diagnostic events:

1. **Handle missing fields gracefully** (they may be removed)
2. **Ignore unknown fields** (new ones may be added)
3. **Update parsers frequently** to stay synchronized with contract changes
4. **Plan for one-release deprecation cycles**

### If you depend on Internal Test-Only events:

1. **Do not**. These are not stable and may be removed or changed without notice.
2. **Use public API events** instead for production systems
3. **Contact team** if you need production-grade observability for a feature currently in testing

## Resource Usage Considerations

Event emission has bounded resource costs:

- **Per-event cost**: O(1) in contract execution (topic + data sizes are bounded)
- **Per-submission cost**: O(n) where n = number of score updates in batch
- **Storage cost**: Events do not consume contract storage (they're part of ledger history)

Worst-case event payload size: ~500 bytes per score submission + ~200 bytes per governance action

## Examples

### Auditing a score change

```rust
// Event topics form an immutable audit trail:
// 1. score(wallet, asset_pair, ...) - original submission
// 2. scr_dlt(wallet, asset_pair, ...) - score updated
// 3. scr_veto(wallet, asset_pair, ...) - veto applied

// Off-chain auditor can reconstruct: what score was submitted, when it changed, and why
```

### Monitoring service health

```rust
// These events help monitor:
events:
  - svc_upd - Service address changed
  - svc_sil - Service went silent
  - svc_res - Service came back online

// Operators can set alerts on these without depending on stable schemas
```

## See Also

- [Event Causality Identifiers](./EVENT_CAUSALITY.md) - Correlation IDs linking multi-step workflows
- [Audit Replay Testing](./AUDIT_REPLAY.md) - Reconstructing state from events
- [Deployment Checklist](./DEPLOYMENT_CHECKLIST.md) - Production sign-off process
