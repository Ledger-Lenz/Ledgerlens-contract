# Audit Replay Testing

This document describes how to verify that off-chain auditors can reconstruct critical state transitions using event streams without requiring direct storage access.

## Overview

Audit replay testing verifies a fundamental property: **All contract state transitions can be reconstructed from emitted events alone**. This property is essential for:

1. **Off-chain auditing**: Independent verification of contract behavior
2. **Disaster recovery**: Reconstructing state if on-chain storage is corrupted
3. **Regulatory compliance**: Maintaining a tamper-proof audit trail
4. **Interoperability**: Third-party systems can operate without read access to contract storage

## Testing Strategy

The audit replay test suite verifies that:

1. **Events are complete**: All state-changing operations emit events
2. **Events are stable**: Public API events maintain schema compatibility
3. **Events are causally linked**: Related events can be grouped by correlation ID
4. **State is reconstructible**: Off-chain auditors can replay events to reach the same state

## Test Categories

### 1. Event Completeness Tests

Verify that every state-changing operation emits at least one event.

```rust
#[test]
fn test_audit_replay_score_submission_emits_event() {
    // Verify that submit_score() emits score_submitted event
    // Verify that finality buffer events are emitted
    // Verify that score_committed or score_vetoed is emitted
}
```

**Coverage**:
- Score submission → `score_submitted`
- Score committed → `score_committed`
- Score vetoed → `score_vetoed`
- Admin transfer → `admin_transfer_initiated`
- Admin acceptance → `admin_transfer_accepted`
- Upgrade proposal → `upgrade_proposed`
- Upgrade approval → `upgrade_approval_added`
- Upgrade execution → `upgrade_executed`
- Upgrade veto → `upgrade_vetoed`
- Dispute opening → `dispute_opened`
- Dispute resolution → `dispute_resolved` or `dispute_timed_out`
- Escalation trigger → `escalation_triggered`
- Escalation resolution → `escalation_resolved`

### 2. Event Correlation Tests

Verify that related events can be linked using correlation IDs.

```rust
#[test]
fn test_audit_replay_correlation_id_links_workflow() {
    // Compute correlation ID for a workflow
    let correlation_id = EventCausality::score_submission_correlation_id(
        &wallet, &asset_pair, timestamp
    );
    
    // Verify all events in the workflow reference this ID
    for event in workflow_events {
        assert_eq!(extract_correlation_id(event), correlation_id);
    }
}
```

**Workflow Coverage**:
- Score submission workflow
- Admin transfer workflow
- Upgrade workflow
- Parameter change workflow
- Dispute workflow
- Escalation workflow
- Governance chain workflow

### 3. Event Reconstruction Tests

Verify that off-chain systems can reconstruct state from events.

```rust
#[test]
fn test_audit_replay_reconstruct_score_history() {
    // 1. Execute contract operations
    client.submit_score(&wallet, &asset_pair, &score);
    client.submit_score(&wallet, &asset_pair, &new_score);
    
    // 2. Collect events
    let events = env.events().all();
    
    // 3. Off-chain reconstruction
    let reconstructed_history = reconstruct_score_history(&events, &wallet, &asset_pair);
    
    // 4. Verify reconstruction matches on-chain state
    assert_eq!(
        reconstructed_history.latest_score,
        client.get_score(&wallet, &asset_pair).score
    );
}
```

### 4. Event Schema Stability Tests

Verify that public API events maintain schema compatibility.

```rust
#[test]
fn test_audit_replay_score_event_schema_immutable() {
    // Verify that score_submitted event has consistent field order
    // Verify that field types are consistent
    // Verify that old events can still be parsed
}
```

### 5. Boundary and Failure Case Tests

Test edge cases and error conditions.

```rust
#[test]
fn test_audit_replay_incomplete_workflow() {
    // Submit score but don't wait for finality buffer
    // Verify correlation ID still uniquely identifies the workflow
    // Verify incomplete workflow can be detected and handled
}

#[test]
fn test_audit_replay_concurrent_workflows_same_wallet() {
    // Multiple workflows for same wallet in different asset pairs
    // Verify correlation IDs disambiguate them
    // Verify each workflow is independently reconstructible
}

#[test]
fn test_audit_replay_replay_after_upgrade() {
    // Upgrade contract to new schema version
    // Verify old events can still be replayed
    // Verify migration was successful
}
```

## Implementation Patterns

### Pattern 1: Audit Trail Reconstruction

```python
def reconstruct_audit_trail(events, wallet):
    """Reconstruct all state changes for a wallet."""
    timeline = []
    
    for event in events:
        if event.type == "score_submitted" and event.wallet == wallet:
            # Compute correlation ID
            corr_id = sha256(wallet || asset_pair || timestamp)
            
            # Start new workflow
            workflow = {
                'type': 'score_submission',
                'correlation_id': corr_id,
                'events': [event],
                'status': 'submitted'
            }
            timeline.append(workflow)
        
        elif event.correlation_id in known_workflows:
            # Add to existing workflow
            workflow = find_workflow(event.correlation_id)
            workflow['events'].append(event)
            
            if event.type == "score_committed":
                workflow['status'] = 'finalized'
            elif event.type == "score_vetoed":
                workflow['status'] = 'vetoed'
    
    return timeline
```

### Pattern 2: Fraud Detection

```python
def detect_fraud(events):
    """Detect inconsistencies between events and claimed state."""
    issues = []
    
    for event in events:
        if event.type == "score_submitted":
            claimed_score = event.score
            
            # Look for later contradictions
            for later_event in events:
                if (later_event.type == "score_vetoed" and
                    later_event.correlation_id == event.correlation_id):
                    issues.append({
                        'type': 'veto_after_submit',
                        'details': f'Score {claimed_score} was vetoed'
                    })
    
    return issues
```

### Pattern 3: State Reconciliation

```python
def reconcile_state(events, on_chain_state):
    """Verify event history matches on-chain state."""
    reconstructed = replay_events(events)
    
    for key, on_chain_value in on_chain_state.items():
        reconstructed_value = reconstructed.get(key)
        
        if on_chain_value != reconstructed_value:
            raise AuditException(
                f"Mismatch for {key}: on-chain={on_chain_value}, "
                f"reconstructed={reconstructed_value}"
            )
    
    return True  # Audit passed
```

## Resource Tracking

Event replay must be bounded in resource usage:

| Operation | Complexity | Bounded By |
|-----------|-----------|-----------|
| Replay single workflow | O(n) | n = events in workflow (typically < 10) |
| Reconstruct all scores | O(m) | m = total score submissions (on-chain history size) |
| Verify causality | O(m log m) | Sorting events by timestamp |
| Detect fraud | O(m²) | Worst-case comparison of all events |

## Testing Examples

### Example 1: Score Submission Audit

```rust
#[test]
fn test_audit_replay_score_lifecycle() {
    let env = Env::default();
    env.mock_all_auths();
    
    let contract = register_contract(&env);
    contract.initialize(&admin, &service);
    
    // Submit score
    let wallet = Address::generate(&env);
    let score = RiskScore { score: 50, /* ... */ };
    contract.submit_score(&wallet, &asset_pair, &score);
    
    // Wait for finality buffer to expire
    env.ledger().set_timestamp(env.ledger().timestamp() + FINALITY_BUFFER);
    
    // Commit score
    contract.commit_score(&wallet, &asset_pair);
    
    // Collect events
    let events = env.events().all();
    
    // Verify event sequence
    let relevant_events: Vec<_> = events
        .iter()
        .filter(|(_, topics, _)| {
            let event_name = topics.get(0)?;
            matches!(event_name, ... /* score or scr_comm */)
        })
        .collect();
    
    // Should have score_submitted and score_committed
    assert!(relevant_events.len() >= 2);
}
```

### Example 2: Admin Transfer Audit

```rust
#[test]
fn test_audit_replay_admin_transfer() {
    let env = Env::default();
    env.mock_all_auths();
    
    let contract = register_contract(&env);
    contract.initialize(&admin, &service);
    
    let new_admin = Address::generate(&env);
    
    // Initiate transfer
    contract.initiate_admin_transfer(&new_admin);
    
    // Accept transfer
    contract.accept_admin_transfer();
    
    // Verify events form a complete workflow
    let events = env.events().all();
    
    let mut found_init = false;
    let mut found_accept = false;
    
    for (_, topics, _) in events.iter() {
        if let Some(event_name) = topics.get(0) {
            match event_name.to_string().as_str() {
                "adm_init" => found_init = true,
                "adm_done" => found_accept = true,
                _ => {}
            }
        }
    }
    
    assert!(found_init && found_accept);
}
```

## Compliance Verification

Audit replay tests verify compliance with:

1. **Ledger Lenz Risk Management**: Events provide complete audit trail
2. **Soroban Guarantees**: Events are tamper-proof and ordered
3. **Off-Chain Indexers**: Third-party systems can operate without read access
4. **Regulatory Requirements**: Fraud detection and state reconciliation

## See Also

- [Event Schema Stability](./EVENT_SCHEMA_STABILITY.md) - Stability of event formats
- [Event Causality](./EVENT_CAUSALITY.md) - Linking related events
- [Deployment Checklist](./DEPLOYMENT_CHECKLIST.md) - Production sign-off process
