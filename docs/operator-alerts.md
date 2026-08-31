# Operator Alert Rules for LedgerLens Contract Events

This document provides concrete alert thresholds and severity levels for production operators monitoring the LedgerLens smart contract. All rules are derived from contract events and enable proactive detection of anomalies.

## Alert Architecture

Alerts are organized by event category:
- **Operational/Governance**: Contract pause, admin changes, governance actions
- **Data Quality**: Score submission failures, validation issues
- **Configuration**: Parameter changes, model version updates
- **Security**: Signer rotation, authorization failures
- **Performance**: Rate limits, staleness, backlog

Each alert includes:
- **Event Topic**: The contract event that triggers the alert
- **Severity**: Critical, Warning, or Info
- **Threshold**: The condition that triggers the alert
- **Example**: Concrete scenario showing the alert in action
- **Response**: Recommended operator action

## Pause Events

### Alert: Contract Pause Detected
- **Event**: `paused`
- **Severity**: Critical
- **Threshold**: Any `paused` event emission
- **Example**: An admin calls `pause()` during suspected security incident
- **Response**: 
  - Verify pause reason with admin team immediately
  - Monitor batch submission failures (will reject with `ContractPaused` code)
  - Check for associated security events or anomalies

### Alert: Contract Unpaused
- **Event**: `unpaused`
- **Severity**: Warning
- **Threshold**: Any `unpaused` event emission after pause duration > 1 hour
- **Example**: Contract was paused for 4 hours; unpaused event detected
- **Response**:
  - Verify unpaused action with admin team
  - Monitor score submissions for any resumption backlog
  - Validate all system dependencies are ready

### Alert: Pair Paused
- **Event**: `pr_pause`
- **Severity**: Warning
- **Threshold**: `paused=true` for critical trading pairs (BTC/USD, ETH/USD, etc.)
- **Example**: XLM/USD pair paused; other pairs still active
- **Response**:
  - Identify the asset pair affected
  - Verify pause duration and expected resolution time
  - Notify downstream consumers (gate callers)

## Signer Churn

### Alert: Signer Added
- **Event**: `sig_add`
- **Severity**: Info
- **Threshold**: New signer registration
- **Example**: New oracle signer registered with public key rotation
- **Response**:
  - Log new signer for audit trail
  - Verify signer meets quorum/threshold requirements
  - Monitor first submissions from new signer

### Alert: Signer Removed
- **Event**: `sig_rem`
- **Severity**: Warning
- **Threshold**: Signer removal when active signers < required threshold + 1
- **Example**: Removing signer reduces active count to exactly the minimum
- **Response**:
  - Verify removal reason with admin
  - Check if replacement signer is being added
  - Alert if quorum would fall below minimum

### Alert: Signer Expiring Soon
- **Event**: `sig_exp`
- **Severity**: Warning
- **Threshold**: Signer TTL approaching expiry (< 24 hours remaining)
- **Example**: Signer key expires in 18 hours; no rotation in progress
- **Response**:
  - Immediately initiate signer rotation
  - Coordinate with signer infrastructure team
  - Monitor for grace period exhaustion

### Alert: Signer Expired
- **Event**: `sig_expd`
- **Severity**: Critical
- **Threshold**: Any `sig_expd` event (signer no longer accepted)
- **Example**: Signer key expired 2 hours ago; submissions now rejected
- **Response**:
  - Emergency: activate backup signer immediately
  - Investigate why rotation wasn't completed
  - Monitor rate of failed submissions due to invalid signer

## Rejection Spikes

### Alert: Batch Rejection Rate Spike
- **Event**: `bat_summ` (batch_processing_summary)
- **Severity**: Warning
- **Threshold**: `rejected_count > accepted_count * 0.2` (>20% rejection rate) in single batch
- **Example**: Batch of 10 entries: 8 accepted, 2 rejected
- **Response**:
  - Analyze rejection codes (`rejected_data`, `rejected_ratelimit`, etc.)
  - Check if external signers are sending malformed data
  - Verify rate limits haven't changed unexpectedly

### Alert: Contract Pause Rejections
- **Event**: `bat_rej_pa` (batch_rejected_contract_paused)
- **Severity**: Critical (if contract should not be paused)
- **Threshold**: Any `bat_rej_pa` events during expected operating hours
- **Example**: Batch submissions failing with pause code at 14:00 UTC
- **Response**:
  - Verify contract pause status immediately
  - Check if pause was intentional (security incident, maintenance)
  - Resume contract if pause was accidental

### Alert: Data Quality Rejections
- **Event**: `bat_rej_dq` (batch_rejected_data_quality)
- **Severity**: Warning
- **Threshold**: `count > 5` rejections per batch due to data quality
- **Example**: 6 entries rejected due to invalid_score (reason_code=1)
- **Response**:
  - Contact signer infrastructure team
  - Verify score calculation/validation pipeline
  - Check if model version changed unexpectedly

### Alert: Model Version Rejections
- **Event**: `bat_rej_mv` (batch_rejected_model_version)
- **Severity**: Warning
- **Threshold**: Any rejections due to `ModelVersionNotRegistered` or `ModelVersionDeprecated`
- **Example**: Batch entries using model v3, but only v4 is active
- **Response**:
  - Verify model version update was deployed to all signers
  - Check if model v3 deprecation was announced to signers
  - Coordinate model upgrade timeline if necessary

### Alert: Rate Limit Rejections
- **Event**: `bat_rej_rl` (batch_rejected_rate_limit)
- **Severity**: Warning
- **Threshold**: `count > 2` rate limit rejections in 5-minute window
- **Example**: Same wallet submitting 3 scores in 10 seconds
- **Response**:
  - Review rate limit configuration
  - Check if legitimate high-frequency signer
  - Consider adjusting limits or granting override

### Alert: Attestation Rejections
- **Event**: `bat_rej_at` (batch_rejected_attestation)
- **Severity**: Critical
- **Threshold**: Any `bat_rej_at` events (invalid attestation)
- **Example**: Batch with invalid merkle proof or bad signature
- **Response**:
  - Verify signer public key is current
  - Check if batch signing infrastructure has issues
  - Inspect batch processing pipeline

## Stale Submissions

### Alert: Oracle Staleness Detected
- **Event**: `orc_stale` (oracle_stale_fallback)
- **Severity**: Warning
- **Threshold**: Oracle unchanged for > staleness_threshold (e.g., 1 hour)
- **Example**: External oracle data hasn't updated in 75 minutes; confidence reduced
- **Response**:
  - Contact external oracle infrastructure team
  - Verify oracle data feed is active
  - Check network connectivity to oracle
  - Monitor gate callers for reduced confidence acceptance

### Alert: Service Silence Alert
- **Event**: `svc_sil` (service_silence_alert)
- **Severity**: Warning
- **Threshold**: Service heartbeat not detected for > threshold (e.g., 30 minutes)
- **Example**: Service last active at 14:20 UTC; alert triggered at 14:52 UTC
- **Response**:
  - Check if service is running
  - Verify network connectivity
  - Restart service if needed
  - Investigate why heartbeat was missed

### Alert: Service Resumed
- **Event**: `svc_res` (service_resumed)
- **Severity**: Info
- **Threshold**: Service recovered after silence event
- **Example**: Service returns online after 18-minute gap
- **Response**:
  - Log gap duration for metrics
  - Verify no critical events were missed during silence
  - Monitor for stability in following minutes

## Upgrade Windows

### Alert: Upgrade Proposed
- **Event**: `upg_prop` (upgrade_proposed)
- **Severity**: Info
- **Threshold**: New upgrade proposal with executable_after timestamp
- **Example**: Upgrade v2.1.0 proposed; executable at 2026-08-01 14:00:00 UTC
- **Response**:
  - Log upgrade details for audit trail
  - Calculate upgrade window (now + executable_after delay)
  - Notify team for change management
  - Monitor for competing proposals (only one pending at a time)

### Alert: Upgrade Executed
- **Event**: `upg_exec` (upgrade_executed)
- **Severity**: Info
- **Threshold**: Successful upgrade execution
- **Example**: Contract upgraded to WASM hash 0xabcd...
- **Response**:
  - Verify contract is functioning with canary checks
  - Monitor canary event emissions:
    - New scores submitted and readable
    - Gate enforcement still working
    - Pause/unpause operations functional
    - Governance parameters intact
  - Check for any anomalies in post-upgrade events

### Alert: Upgrade Vetoed
- **Event**: `upg_veto` (upgrade_vetoed)
- **Severity**: Warning
- **Threshold**: Admin veto of pending upgrade
- **Example**: Admin vetoes upgrade 2 days before execution
- **Response**:
  - Contact admin to understand veto reason
  - Assess if new/modified upgrade is needed
  - Monitor for replacement proposal
  - Verify no service degradation from veto

## Governance Actions

### Alert: Risk Threshold Changed
- **Event**: `thresh` (threshold_updated)
- **Severity**: Info (if within expected range), Warning (if extreme)
- **Threshold**: Threshold change > 20 points or outside [10, 95] range
- **Example**: Threshold changed from 75 to 55 (20-point reduction)
- **Response**:
  - Log change for audit trail
  - Verify change was intentional (admin action)
  - Monitor gate rejection rate for changes
  - Alert consumers of downstream threshold change

### Alert: Parameter Change Proposed
- **Event**: `prm_prop` (parameter_change_proposed)
- **Severity**: Info
- **Threshold**: Any parameter proposal with timelock
- **Example**: `cooldown` parameter change proposed; executable in 24 hours
- **Response**:
  - Log parameter change for audit trail
  - Identify which parameter is changing
  - Calculate veto window
  - Notify team for change management

### Alert: Parameter Change Executed
- **Event**: `prm_exec` (parameter_change_executed)
- **Severity**: Info
- **Threshold**: Successful parameter change application
- **Example**: `cooldown` parameter updated to new value
- **Response**:
  - Verify new parameter is in effect
  - Check for any unexpected behavior changes
  - Monitor downstream effects (e.g., submission rate if cooldown changed)

### Alert: Parameter Change Vetoed
- **Event**: `prm_veto` (parameter_change_vetoed)
- **Severity**: Warning
- **Threshold**: Veto of pending parameter change
- **Example**: Cooldown change vetoed before execution
- **Response**:
  - Contact admin to understand veto reason
  - Verify if modified change is planned
  - Assess impact of veto

## Configuration Examples

### Prometheus AlertRule Configuration

```yaml
groups:
- name: ledgerlens-operators
  rules:
  
  # Critical: Contract Paused
  - alert: LedgerLensPaused
    expr: contract_events{event="paused"} > 0
    for: 1m
    severity: critical
    annotations:
      summary: "LedgerLens contract is paused"
      action: "Verify pause reason with admin; check for rejections"
  
  # Warning: Batch Rejection Spike
  - alert: BatchRejectionSpike
    expr: |
      (batch_summary:rejected / (batch_summary:rejected + batch_summary:accepted) 
       > 0.2)
    for: 5m
    severity: warning
    annotations:
      summary: "Batch rejection rate > 20%"
      action: "Check rejection codes and signer data quality"
  
  # Critical: High Signer Churn
  - alert: SignerChurnHigh
    expr: |
      increase(signer_events:removed[1h]) > 2 AND
      signer_count < required_signers + 2
    for: 1m
    severity: critical
    annotations:
      summary: "Multiple signers removed; quorum at risk"
      action: "Verify signer additions and replacement timeline"
  
  # Warning: Oracle Staleness
  - alert: OracleStaleness
    expr: |
      time() - oracle_events:last_update > oracle_staleness_threshold
    for: 5m
    severity: warning
    annotations:
      summary: "Oracle data is stale"
      action: "Check oracle feed; restart if needed"
```

### Event Stream Monitoring

Monitor these events in real-time from your event indexer:

```bash
# Watch for pause events
event_stream watch --topic="paused|unpaused" --severity=critical

# Watch for rejection spikes
event_stream watch --topic="bat_rej_*" --window=1m --threshold=5

# Watch for signer expiration
event_stream watch --topic="sig_exp|sig_expd" --severity=warning

# Watch for upgrades
event_stream watch --topic="upg_*" --alert-on=all
```

## Best Practices

1. **Baseline First**: Run your system normally for 1-2 weeks to establish baselines for "normal" event frequencies
2. **Correlate Events**: Cross-reference batch rejection spikes with governance changes or signer rotations
3. **Timeline Windows**: Set alert windows based on your SLA (e.g., 5-minute windows for critical, 1-hour for info)
4. **Team Coordination**: Assign ownership of different alert categories to teams (security, operations, development)
5. **Test Runbooks**: Regularly test your response runbooks to ensure procedures are current
6. **Post-Mortems**: When alerts fire, document the root cause and update thresholds if needed

## Event Reference Table

| Event Code | Topic | Data | Severity | Category |
|-----------|-------|------|----------|----------|
| `paused` | Contract pause | admin_address | Critical | Operational |
| `unpaused` | Contract unpause | admin_address | Warning | Operational |
| `pr_pause` | Pair pause | (pair, paused_bool) | Warning | Operational |
| `sig_add` | Signer added | signer_address | Info | Security |
| `sig_rem` | Signer removed | signer_address | Warning | Security |
| `sig_exp` | Signer expiring | signer_address | Warning | Security |
| `sig_expd` | Signer expired | signer_address | Critical | Security |
| `bat_summ` | Batch summary | (accepted, rejected_*) | Warning | Data Quality |
| `bat_rej_pa` | Pause rejection | count | Critical | Data Quality |
| `bat_rej_dq` | Data quality rejection | (reason, count) | Warning | Data Quality |
| `bat_rej_mv` | Model version rejection | (reason, count) | Warning | Configuration |
| `bat_rej_rl` | Rate limit rejection | count | Warning | Performance |
| `bat_rej_at` | Attestation rejection | count | Critical | Security |
| `orc_stale` | Oracle stale | (last_update, threshold) | Warning | Data Quality |
| `svc_sil` | Service silent | (silent_secs, threshold) | Warning | Performance |
| `svc_res` | Service resumed | gap_secs | Info | Performance |
| `upg_prop` | Upgrade proposed | (hash, executable_after) | Info | Governance |
| `upg_exec` | Upgrade executed | wasm_hash | Info | Governance |
| `upg_veto` | Upgrade vetoed | admin_address | Warning | Governance |
| `thresh` | Threshold changed | (old, new) | Info | Governance |
| `prm_prop` | Param proposed | (key, executable_after) | Info | Governance |
| `prm_exec` | Param executed | param_key | Info | Governance |
| `prm_veto` | Param vetoed | (proposal_id, admin) | Warning | Governance |

## Support & Questions

For alert rule questions or to report false positives:
1. Check the [Operator FAQ](./operator-faq.md)
2. Review event schema in [contracts/ledgerlens-score/src/events.rs](../contracts/ledgerlens-score/src/events.rs)
3. File an issue with event logs and context
