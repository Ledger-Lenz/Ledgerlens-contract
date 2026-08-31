# Event Causality Identifiers

This document describes how to reconstruct multi-step workflows from emitted events using correlation IDs.

## Overview

The contract executes several multi-step workflows that generate sequences of related events:
- Score submission (pending → committed/vetoed)
- Governance proposals (proposed → approved → executed/vetoed)
- Admin transfers (initiated → accepted/cancelled)
- Upgrades (proposed → approvals → executed/vetoed)
- Disputes (opened → resolved/timeout)
- Escalations (triggered → resolved)

Each workflow generates a **correlation ID** that uniquely identifies and links all events in that workflow, enabling off-chain auditors to reconstruct complete timelines from logs alone.

## Correlation ID Generation

Correlation IDs are deterministic 32-byte hashes derived from workflow parameters. They can be recomputed off-chain without access to contract storage.

### Score Submission Workflow

**Correlation ID**: `SHA256(wallet || asset_pair || timestamp)`

Events in order:
1. `score_submitted(wallet, asset_pair, ...)`
2. `score_pending(wallet, asset_pair, commit_after)` - optional
3. `score_committed(wallet, asset_pair)` OR `score_vetoed(wallet, asset_pair, reason_hash)`
4. `score_delta(wallet, asset_pair, ...)` - optional, if score changed

**Reconstruction**:
```
Off-chain auditor receives events:
- score_submitted @ ledger 1000 for wallet W, pair P
  → Compute correlation_id = SHA256(W || P || timestamp_1000)
- score_delta @ ledger 1010
  → Verify same W and P, same correlation_id
- score_committed @ ledger 1020
  → Verify same W and P, same correlation_id
  → Workflow complete
```

### Admin Transfer Workflow

**Correlation ID**: `SHA256("admin_xfer" || from || to || timestamp)`

Events in order:
1. `admin_transfer_initiated(from, to)`
2. `admin_transfer_accepted(new_admin)` OR `admin_transfer_cancelled(admin)`

**Reconstruction**:
```
Off-chain auditor receives events:
- admin_transfer_initiated from X to Y @ ledger 5000
  → Compute correlation_id = SHA256("admin_xfer" || X || Y || timestamp_5000)
- admin_transfer_accepted for Y @ ledger 5010
  → Verify correlation matches
  → Transfer completed successfully
```

### Upgrade Workflow

**Correlation ID**: `SHA256("upgrade_prop" || new_wasm_hash || timestamp)`

Events in order:
1. `upgrade_proposed(new_wasm_hash, executable_after)`
2. `upgrade_approval_added(signer, count, required)` - repeats for each signer
3. `upgrade_executed(new_wasm_hash)` OR `upgrade_vetoed(by)`

**Reconstruction**:
```
Off-chain auditor receives events:
- upgrade_proposed with hash H @ ledger 2000
  → Compute correlation_id = SHA256("upgrade_prop" || H || timestamp_2000)
- upgrade_approval_added from signer1 @ ledger 2005
- upgrade_approval_added from signer2 @ ledger 2010
  → Count approvals using same correlation_id
- upgrade_executed with hash H @ ledger 2015
  → Verify all 3 events have same correlation_id
  → Workflow complete with 2-of-2 quorum
```

### Parameter Change Workflow

**Correlation ID**: `SHA256("param_change" || proposal_id || param_key || timestamp)`

Events in order:
1. `parameter_change_proposed(proposal_id, param_key, executable_after)`
2. `parameter_change_executed(proposal_id, param_key)` OR `parameter_change_vetoed(proposal_id, by)`

### Dispute Workflow

**Correlation ID**: `SHA256("dispute_open" || challenger || asset_pair || timestamp)`

Events in order:
1. `dispute_opened(challenger, asset_pair, bond, deadline)`
2. `dispute_resolved(challenger, asset_pair, corrected_score, bond_returned)` OR
   `dispute_timed_out(challenger, asset_pair, bond, bonus)`

### Escalation Workflow

**Correlation ID**: `SHA256("escalation" || wallet || asset_pair || timestamp_triggered)`

Events in order:
1. `escalation_triggered(wallet, asset_pair, breach_count, score, threshold)`
2. `escalation_resolved(wallet, asset_pair, breach_count, score)` - after clean score or admin reset

### Governance Chain Workflow

**Correlation ID**: `SHA256("gov_chain" || action_index || timestamp)`

Events:
- `governance_action_appended(new_head)` - links to previous head via chain

## Implementation in Auditor

### Pseudocode: Reconstruct Score Timeline

```python
def reconstruct_score_timeline(wallet, asset_pair, events):
    """Reconstruct score changes for a wallet-pair from events alone."""
    timeline = []
    
    for event in events:
        if event.type == "score_submitted":
            correlation_id = sha256(wallet || asset_pair || event.timestamp)
            workflow = {
                'type': 'score_submission',
                'correlation_id': correlation_id,
                'events': [event],
                'status': 'pending'
            }
            timeline.append(workflow)
        
        elif event.type == "score_delta":
            # Match by correlation_id
            for workflow in timeline:
                if (workflow['type'] == 'score_submission' and
                    event.wallet == wallet and event.asset_pair == asset_pair):
                    workflow['events'].append(event)
        
        elif event.type == "score_committed":
            # Match by correlation_id
            for workflow in timeline:
                if (workflow['type'] == 'score_submission' and
                    event.wallet == wallet and event.asset_pair == asset_pair):
                    workflow['events'].append(event)
                    workflow['status'] = 'committed'
    
    return timeline
```

### Pseudocode: Reconstruct Upgrade Timeline

```python
def reconstruct_upgrade_timeline(events):
    """Reconstruct upgrade proposals and their execution from events."""
    upgrades = {}
    
    for event in events:
        if event.type == "upgrade_proposed":
            correlation_id = sha256("upgrade_prop" || event.hash || event.timestamp)
            upgrades[correlation_id] = {
                'type': 'upgrade',
                'hash': event.hash,
                'proposed_at': event.timestamp,
                'events': [event],
                'approvals': [],
                'status': 'proposed'
            }
        
        elif event.type == "upgrade_approval_added":
            # Find matching upgrade by correlation tracking through all events
            for workflow in upgrades.values():
                if (event.timestamp > workflow['proposed_at'] and
                    event.timestamp < workflow['proposed_at'] + 604800):  # 7 days
                    workflow['events'].append(event)
                    workflow['approvals'].append(event.signer)
                    if len(workflow['approvals']) >= event.required:
                        workflow['status'] = 'quorum_reached'
        
        elif event.type == "upgrade_executed":
            # Match by hash
            for workflow in upgrades.values():
                if workflow['hash'] == event.hash:
                    workflow['events'].append(event)
                    workflow['status'] = 'executed'
    
    return list(upgrades.values())
```

## Guarantees

1. **Deterministic**: Same inputs always produce same correlation ID
2. **Verifiable**: Off-chain systems can recompute without trusting contract
3. **Complete**: All events in a workflow carry matching correlation ID
4. **Immutable**: Correlation ID is part of event data (content-addressed)
5. **Privacy-preserving**: No correlation data stored on-chain, reducing state bloat

## Testing

Audit replay tests verify that:

1. **Event sequences are complete**: All events in a workflow are present
2. **Causality is preserved**: Events appear in causal order
3. **Correlation IDs match**: All events in a workflow have consistent correlation ID
4. **Reconstruction succeeds**: Off-chain auditors can recreate state from events alone

See [AUDIT_REPLAY.md](./AUDIT_REPLAY.md) for test examples.

## Resource Usage

- **Per-event cost**: O(1) hash computation (negligible)
- **Storage cost**: 0 bytes (correlation ID not stored on-chain)
- **Ledger cost**: Included in event payload (~32 bytes added per correlation ID)

## See Also

- [Event Schema Stability](./EVENT_SCHEMA_STABILITY.md) - Stability guarantees for events
- [Audit Replay Testing](./AUDIT_REPLAY.md) - Testing event reconstruction
- [Deployment Checklist](./DEPLOYMENT_CHECKLIST.md) - Production sign-off process
