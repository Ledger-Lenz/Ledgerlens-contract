# LedgerLens Contract Event Schema Reference

This document provides the complete event schema for all LedgerLens contract events. Use this for implementing event indexers, monitoring systems, and alert pipelines.

## Event Versioning

All events include a `EVENT_VERSION` (currently `1`) in their topic array to enable schema evolution without breaking off-chain systems. Any breaking changes (field reordering, type changes, field removal) will bump the version.

## Event Categories

### 1. Operational/Governance Events

#### `paused`
- **Topic**: `("paused", EVENT_VERSION)`
- **Data**: `Address` (admin who paused)
- **Use Case**: Alert when contract enters paused state
- **Example**: Admin paused contract during security incident

#### `unpaused`
- **Topic**: `("unpaused", EVENT_VERSION)`
- **Data**: `Address` (admin who unpaused)
- **Use Case**: Monitor contract resumption and duration of pause
- **Example**: Contract operational again after 2-hour maintenance

#### `pr_pause`
- **Topic**: `("pr_pause", EVENT_VERSION, asset_pair)`
- **Data**: `bool` (paused status)
- **Use Case**: Track which pairs are operational
- **Example**: Pair "BTC/USD" paused=true

### 2. Score Submission Events

#### `score`
- **Topic**: `("score", EVENT_VERSION, wallet, asset_pair)`
- **Data**: `(score, benford_flag, ml_flag, confidence, timestamp)`
- **Use Case**: Track all score submissions with anomaly flags
- **Example**: Score=42, confidence=85, no anomalies

#### `bat_ok`
- **Topic**: `("bat_ok", merkle_root)`
- **Data**: `(accepted: u32, rejected: u32)`
- **Use Case**: Monitor batch processing success rate
- **Example**: Batch accepted=98 entries, rejected=2

#### `bat_summ`
- **Topic**: `("bat_summ",)`
- **Data**: `(accepted, rejected_pause, rejected_data, rejected_model, rejected_ratelimit, rejected_attestation, rejected_gate)`
- **Use Case**: Aggregate rejection statistics per category
- **Example**: 95 accepted, 2 rejected_data, 1 rejected_ratelimit

#### `bat_rej_pa`
- **Topic**: `("bat_rej_pa",)`
- **Data**: `u32` (count)
- **Use Case**: Alert when contract pause causes rejections
- **Example**: 5 entries rejected due to pause

#### `bat_rej_dq`
- **Topic**: `("bat_rej_dq",)`
- **Data**: `(reason_code: u32, count: u32)`
- **Reason Codes**: 1=invalid_score, 2=invalid_confidence, 3=invalid_timestamp
- **Use Case**: Detect data quality issues from signers
- **Example**: 3 entries rejected for invalid_confidence

#### `bat_rej_mv`
- **Topic**: `("bat_rej_mv",)`
- **Data**: `(reason_code: u32, count: u32)`
- **Reason Codes**: 1=not_registered, 2=deprecated
- **Use Case**: Alert on model version synchronization issues
- **Example**: 2 entries rejected with deprecated model

#### `bat_rej_rl`
- **Topic**: `("bat_rej_rl",)`
- **Data**: `u32` (count)
- **Use Case**: Detect rate limit violations
- **Example**: 1 entry rejected due to rate limit

#### `bat_rej_at`
- **Topic**: `("bat_rej_at",)`
- **Data**: `u32` (count)
- **Use Case**: Alert on signature/attestation failures
- **Example**: 1 entry rejected with invalid attestation

#### `bat_rej_gt`
- **Topic**: `("bat_rej_gt",)`
- **Data**: `u32` (count)
- **Use Case**: Track gate enforcement rejections
- **Example**: 4 entries rejected by gate threshold

### 3. Risk Threshold & Configuration Events

#### `thresh`
- **Topic**: `("thresh", EVENT_VERSION)`
- **Data**: `(old_threshold: u32, new_threshold: u32)`
- **Use Case**: Audit threshold changes
- **Example**: Threshold changed from 75 to 65

#### `breach`
- **Topic**: `("breach", EVENT_VERSION, wallet)`
- **Data**: `(asset_pair, score, threshold)`
- **Use Case**: Monitor threshold breaches
- **Example**: Wallet score=92 exceeded threshold=75

#### `brc_rst`
- **Topic**: `("brc_rst", wallet, asset_pair)`
- **Data**: `Address` (admin who reset)
- **Use Case**: Audit breach counter resets
- **Example**: Admin reset breach counter for XLM/USD pair

#### `cd_upd`
- **Topic**: `("cd_upd", EVENT_VERSION)`
- **Data**: `u64` (cooldown in seconds)
- **Use Case**: Track cooldown period changes
- **Example**: Cooldown changed to 3600 seconds

#### `pcd_upd`
- **Topic**: `("pcd_upd", asset_pair)`
- **Data**: `u64` (pair-specific cooldown)
- **Use Case**: Monitor pair-specific configuration
- **Example**: XLM/USD cooldown=7200

### 4. Signer Management Events

#### `sig_add`
- **Topic**: `("sig_add", EVENT_VERSION)`
- **Data**: `Address` (new signer)
- **Use Case**: Track signer additions
- **Example**: New oracle signer registered

#### `sig_rem`
- **Topic**: `("sig_rem", EVENT_VERSION)`
- **Data**: `Address` (removed signer)
- **Use Case**: Monitor signer removals
- **Example**: Signer retired after rotation

#### `sig_exp`
- **Topic**: `("sig_exp",)`
- **Data**: `Address` (expiring signer)
- **Use Case**: Alert on imminent signer expiration
- **Example**: Signer key expires in 18 hours

#### `sig_expd`
- **Topic**: `("sig_expd",)`
- **Data**: `Address` (expired signer)
- **Use Case**: Alert when signer is no longer valid
- **Example**: Signer no longer accepted; submissions will fail

#### `sig_thr`
- **Topic**: `("sig_thr", EVENT_VERSION)`
- **Data**: `u32` (required number of signers)
- **Use Case**: Track quorum changes
- **Example**: Quorum requirement changed to 7

### 5. Upgrade Events

#### `upg_prop`
- **Topic**: `("upg_prop", EVENT_VERSION)`
- **Data**: `(new_wasm_hash: BytesN<32>, executable_after: u64)`
- **Use Case**: Log upgrade proposals with execution time
- **Example**: Upgrade v2.1.0 proposed; ready at timestamp 1700086400

#### `upg_exec`
- **Topic**: `("upg_exec", EVENT_VERSION)`
- **Data**: `BytesN<32>` (new WASM hash)
- **Use Case**: Confirm successful upgrade execution
- **Example**: Upgrade 0xabcd... executed

#### `upg_veto`
- **Topic**: `("upg_veto", EVENT_VERSION)`
- **Data**: `Address` (admin who vetoed)
- **Use Case**: Track rejected upgrades
- **Example**: Admin vetoed upgrade 2 days early

#### `upg_appr`
- **Topic**: `("upg_appr", signer)`
- **Data**: `(approval_count: u32, required_count: u32)`
- **Use Case**: Monitor multi-sig approval progress
- **Example**: 5 of 7 signatures collected

### 6. Parameter Governance Events

#### `prm_prop`
- **Topic**: `("prm_prop",)`
- **Data**: `(proposal_id: u64, param_key: Symbol, executable_after: u64)`
- **Use Case**: Log parameter change proposals
- **Example**: Proposal ID=42 for "cooldown"; executable in 24h

#### `prm_exec`
- **Topic**: `("prm_exec",)`
- **Data**: `(proposal_id: u64, param_key: Symbol)`
- **Use Case**: Confirm parameter changes applied
- **Example**: Proposal 42 "cooldown" executed

#### `prm_veto`
- **Topic**: `("prm_veto",)`
- **Data**: `(proposal_id: u64, admin: Address)`
- **Use Case**: Track rejected parameter proposals
- **Example**: Proposal 42 vetoed by admin

### 7. Oracle Events

#### `orc_reg`
- **Topic**: `("orc_reg", asset_pair)`
- **Data**: `Address` (oracle contract)
- **Use Case**: Track oracle registrations
- **Example**: Oracle for BTC/USD registered

#### `orc_rem`
- **Topic**: `("orc_rem",)`
- **Data**: `Symbol` (asset_pair)
- **Use Case**: Monitor oracle removals
- **Example**: XLM/USD oracle deregistered

#### `orc_stale`
- **Topic**: `("orc_stale", asset_pair)`
- **Data**: `(last_updated_ts: u64, threshold_secs: u64)`
- **Use Case**: Alert on stale oracle data
- **Example**: Oracle data 65 minutes old; threshold=60 minutes

#### `orc_sthr`
- **Topic**: `("orc_sthr",)`
- **Data**: `u64` (staleness threshold in seconds)
- **Use Case**: Track staleness threshold changes
- **Example**: Staleness threshold changed to 3600 seconds

### 8. Service Heartbeat Events

#### `svc_sil`
- **Topic**: `("svc_sil",)`
- **Data**: `ServiceSilenceAlertEvent { last_active_at, silent_secs, threshold_secs }`
- **Use Case**: Alert when service is not reporting
- **Example**: Service silent for 35 minutes; threshold=30 minutes

#### `svc_res`
- **Topic**: `("svc_res",)`
- **Data**: `ServiceResumedEvent { last_active_at, gap_secs }`
- **Use Case**: Track service recovery and gap duration
- **Example**: Service returned online after 18-minute gap

#### `hb_upd`
- **Topic**: `("hb_upd",)`
- **Data**: `u64` (heartbeat threshold in seconds)
- **Use Case**: Track heartbeat configuration changes
- **Example**: Heartbeat threshold changed to 1800 seconds

### 9. Admin & Authorization Events

#### `adm_init`
- **Topic**: `("adm_init", EVENT_VERSION)`
- **Data**: `(from: Address, to: Address)`
- **Use Case**: Log admin transfer initiations
- **Example**: Admin transfer initiated from 0xabc... to 0xdef...

#### `adm_done`
- **Topic**: `("adm_done", EVENT_VERSION)`
- **Data**: `Address` (new admin)
- **Use Case**: Confirm new admin is active
- **Example**: New admin 0xdef... accepted transfer

#### `adm_canc`
- **Topic**: `("adm_canc", EVENT_VERSION)`
- **Data**: `Address` (admin)
- **Use Case**: Track cancelled admin transfers
- **Example**: Admin transfer cancelled by current admin

### 10. Model Version Events

#### `mv_prop`
- **Topic**: `("mv_prop",)`
- **Data**: `(version: u32, executable_after: u64)`
- **Use Case**: Log model version proposals
- **Example**: Model v4 proposed; active after timestamp 1700086400

#### `mv_act`
- **Topic**: `("mv_act",)`
- **Data**: `u32` (active model version)
- **Use Case**: Confirm model version activation
- **Example**: Model v4 now active

#### `mv_depr`
- **Topic**: `("mv_depr",)`
- **Data**: `u32` (deprecated version)
- **Use Case**: Alert when model versions are retired
- **Example**: Model v2 deprecated; no longer accepted

#### `mv_reg`
- **Topic**: `("mv_reg",)`
- **Data**: `u32` (registered version)
- **Use Case**: Track new model registrations
- **Example**: Model v5 registered (available for proposal/activation)

### 11. Dispute Events

#### `disp_open`
- **Topic**: `("disp_open", challenger)`
- **Data**: `(asset_pair, bond: i128, deadline: u64)`
- **Use Case**: Alert when disputes are initiated
- **Example**: Dispute opened; resolution deadline in 7 days

#### `disp_res`
- **Topic**: `("disp_res", challenger)`
- **Data**: `(asset_pair, corrected_score: u32, bond_returned: i128)`
- **Use Case**: Confirm dispute resolution
- **Example**: Dispute resolved; score corrected to 65; bond returned

#### `disp_to`
- **Topic**: `("disp_to", challenger)`
- **Data**: `(asset_pair, bond: i128, bonus: i128)`
- **Use Case**: Track dispute timeout resolutions
- **Example**: Dispute timed out; bond + bonus forfeited

### 12. Data Integrity Events

#### `scr_dlt`
- **Topic**: `("scr_dlt", EVENT_VERSION, wallet, asset_pair)`
- **Data**: `(prev_score, new_score, delta_abs, trend, consecutive_trend)`
- **Use Case**: Monitor score changes for anomalies
- **Example**: Score jumped 25 points; trend=+1, consecutive=3

#### `jump`
- **Topic**: `("jump", wallet, asset_pair)`
- **Data**: `(prev_score, new_score, delta, model_version, timestamp)`
- **Use Case**: Alert on anomalous score jumps
- **Example**: Score jumped 35 points; model v3

#### `clr_hist`
- **Topic**: `("clr_hist", EVENT_VERSION, wallet)`
- **Data**: `Symbol` (asset_pair)
- **Use Case**: Audit score history deletions
- **Example**: History cleared for XLM/USD

#### `clr_scr`
- **Topic**: `("clr_scr", EVENT_VERSION, wallet)`
- **Data**: `Symbol` (asset_pair)
- **Use Case**: Audit score deletions
- **Example**: Score cleared for BTC/USDT

## Event Indexer Implementation Tips

### 1. Handle Optional Versions
Some events omit EVENT_VERSION (legacy events). Always check topic array length.

```python
def parse_topic(topic):
    if len(topic) >= 2 and isinstance(topic[1], int):
        version = topic[1]
    else:
        version = 1  # default
    return version
```

### 2. Aggregate by Category
Create tables indexed by event category for fast queries:

```sql
CREATE TABLE event_streams (
    event_type TEXT,         -- e.g., "bat_summ", "upg_exec"
    category TEXT,           -- "data_quality", "governance", etc.
    timestamp INT64,
    data JSON,
    PRIMARY KEY (category, timestamp)
);
```

### 3. Alert on Anomalies
Baseline normal event frequencies, then alert on deviations:

```python
# Normal: ~100-200 score submissions per 5-minute window
# Alert if < 50 or > 500
def check_submission_rate(window_5min):
    submissions = count_events("score", window_5min)
    if submissions < 50 or submissions > 500:
        alert("Anomalous submission rate: " + submissions)
```

### 4. Cross-Reference Events
Link related events for context:

```python
# When bat_summ shows high rejections, find associated events:
# - bat_rej_* (specific rejection types)
# - thresh (recent threshold changes?)
# - paused (contract paused?)
# - model_version_* (model update?)
```

## See Also

- [Operator Alerts Guide](./operator-alerts.md)
- [Event Emission Code](../contracts/ledgerlens-score/src/events.rs)
- [Event Tests](../contracts/ledgerlens-score/src/test_batch_error_events.rs)
