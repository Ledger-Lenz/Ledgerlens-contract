# Production Deployment Sign-Off Checklist

This document provides the concrete checklist for deployment and upgrade approvals. It enables release managers to complete sign-off without interpreting source code.

## Overview

Before deploying to production, release managers must verify:

1. **Code Quality & Safety**: Deterministic tests pass, no security issues
2. **Compatibility**: Public ABI, events, errors, and storage are stable
3. **Resource Usage**: Memory and CPU are bounded within limits
4. **Rollback Plan**: Procedures to recover if deployment fails
5. **Operator Readiness**: Infrastructure and monitoring are prepared

## Pre-Deployment Phase (Development)

### Code Review Checklist

- [ ] Pull request has explicit approval from at least 2 maintainers
- [ ] All CI checks pass (tests, clippy, coverage)
- [ ] Code review comments are resolved
- [ ] Security audit (if changes touch sensitive areas)
- [ ] Breaking changes are documented in CHANGELOG.md
- [ ] Test coverage for all new functions (>90%)

### Test Coverage Checklist

- [ ] Unit tests pass: `cargo test --lib`
- [ ] Integration tests pass: `cargo test --test '*'`
- [ ] Event stability tests pass: `test_all_events_carry_schema_version`
- [ ] Audit replay tests pass: `test_audit_replay_*`
- [ ] Deterministic test for new behavior: `test_<feature>_succeeds` AND `test_<feature>_fails_without_change`
- [ ] Boundary case tests exist (e.g., max scores, min thresholds)
- [ ] Adversarial failure mode tests (e.g., concurrent submissions)

**Verification**:
```bash
cargo test --all --release
cargo tarpaulin --out Html --output-dir coverage/
# Verify coverage badge shows >90%
```

### Compatibility Checklist

#### Public ABI Stability

- [ ] No breaking changes to exported functions (type signatures unchanged)
- [ ] No removal of public functions or methods
- [ ] New functions default to disabled (feature-gated) if not fully stable
- [ ] Error types preserve existing variants (can add new ones)
- [ ] Contract constants (fees, limits) are backward compatible

**Verification**:
```bash
# Compare exported signatures
cargo doc --no-deps
diff <(cargo doc old_rev) <(cargo doc new_rev)
```

#### Event Schema Stability

- [ ] All public API events carry `EVENT_VERSION` in topics
- [ ] Public API event fields are append-only (no reordering, no removal)
- [ ] Operator diagnostic events document non-breaking changes
- [ ] Event correlation IDs are deterministic and reproducible
- [ ] Migration guide provided for schema version bumps

**Verification**:
```bash
# Run event stability tests
cargo test event_stability --lib

# Verify correlation ID reproducibility
cargo test test_audit_replay_correlation_id_linking
```

#### Storage Compatibility

- [ ] Storage keys are unchanged (or migration path documented)
- [ ] Data types maintain binary compatibility
- [ ] No removal of stored state without migration guide
- [ ] Storage growth is bounded

**Verification**:
Provide a mapping of:
```markdown
| Storage Key | Data Type | Previous Type | Migration |
|------------|-----------|--------------|-----------|
| admin | Address | (new) | - |
| scores | Map<(Wallet,Pair), Score> | (same) | - |
```

#### Error Stability

- [ ] Existing error variants unchanged
- [ ] New error variants are additive
- [ ] Error messages are stable (off-chain systems may parse them)
- [ ] Error codes/numbers never change

**Verification**:
```bash
# List all error variants
grep -E "pub enum Error|^    [A-Z][a-zA-Z0-9]+," src/errors.rs
```

### Resource Usage Checklist

- [ ] Worst-case CPU time per function is documented
- [ ] Memory usage is bounded (no unbounded allocations)
- [ ] Storage growth per transaction is bounded
- [ ] Event emission is bounded (no loops emitting events)

**Verification**:
Complete the resource usage table for each changed function:

```markdown
| Function | Worst Case (ms) | Memory (KB) | Storage (B) | Notes |
|----------|-----------------|------------|-------------|-------|
| submit_score | 10 | 50 | 1024 | O(1) |
| get_score | 2 | 10 | 0 | Read-only |
```

Benchmarking command:
```bash
cargo +nightly bench --features bench 2>&1 | grep -E "submit_score|get_score"
```

## Pre-Deployment Phase (Release Management)

### Release Engineering Checklist

- [ ] Version number bumped in `Cargo.toml` and `package.json`
- [ ] `CHANGELOG.md` updated with changes and migration guide
- [ ] Release notes written for operators and integrators
- [ ] Deployment script (`deploy.sh`) updated if needed

**Version Numbering**:
- Semver format: `MAJOR.MINOR.PATCH`
- MAJOR: Breaking change to public ABI or storage
- MINOR: New features, public API extensions
- PATCH: Bug fixes, internal changes, operator diagnostics

### Build & Signing Checklist

- [ ] Build is deterministic: `cargo build --release` twice produces identical binary
- [ ] WASM hash is computed: `sha256sum target/wasm32-unknown-unknown/release/*.wasm`
- [ ] Build is reproducible from git tag: `git checkout v1.2.3 && cargo build --release`
- [ ] Release binary is signed with team GPG key
- [ ] Signatures are verified by 2 independent reviewers

**Verification**:
```bash
# Compute deployment hash for sign-off
DEPLOYMENT_HASH=$(sha256sum build/ledgerlens_score.wasm | cut -d' ' -f1)
echo "Deployment Hash: $DEPLOYMENT_HASH" > DEPLOYMENT.txt
gpg --sign --armor DEPLOYMENT.txt
```

Store in: `/deployments/v<VERSION>/DEPLOYMENT.txt.asc`

### Operator Preparation Checklist

- [ ] Operations team has read the release notes
- [ ] Monitoring and alerting is configured for new events (if any)
- [ ] Runbooks are updated (if operational procedures changed)
- [ ] Escalation procedures are documented
- [ ] Rollback procedure is tested and working

**Runbook Template**:
```markdown
## New Events to Monitor

| Event | Severity | Action |
|-------|----------|--------|
| scr_veto | INFO | Log for audit |
| escalation_triggered | WARNING | Alert ops team |

## Configuration Changes

- New parameter: `escalation_threshold` (default: 5 breaches)
- Deprecated parameter: None

## Upgrade Procedure

1. Verify current contract hash: ...
2. Submit upgrade proposal: ...
3. Wait for approvals: ...
4. Execute upgrade: ...
5. Verify new hash deployed: ...

## Rollback Procedure

1. Stop accepting new transactions: ...
2. Publish rollback upgrade: ...
3. Execute rollback: ...
4. Verify reverted: ...
5. Post-incident review: ...
```

## Deployment Phase (Network)

### Pre-Deployment Verification

- [ ] Contract initialization tested on testnet
- [ ] Upgrade path tested on testnet (if applicable)
- [ ] Contract interacts correctly with gates/bridges
- [ ] Event emission verified on testnet

**Testnet Verification Checklist**:
```bash
# Deploy to testnet
./deploy.sh --network testnet --wasm ledgerlens_score.wasm

# Run smoke tests
cargo test --test '*' -- --ignored --network testnet

# Verify WASM hash
soroban contract info --network testnet --id <CONTRACT_ID> | grep hash
```

### Deployment Safety Checks

- [ ] Network is healthy (RPC responding, not at capacity)
- [ ] Admin account has sufficient XLM for fees (~10 XLM)
- [ ] Upgrade account (if multi-sig) is ready
- [ ] Time window is during business hours for US/EU team
- [ ] At least 2 team members present during deployment

### Deployment Execution

- [ ] Backup current contract state (via event export)
- [ ] Record pre-deployment block height: ________________
- [ ] Submit upgrade proposal with new WASM hash: ________________
- [ ] Approvers sign within required timeframe
- [ ] Execute upgrade when quorum reached
- [ ] Record post-deployment block height: ________________
- [ ] Record actual deployment timestamp: ________________

**Deployment Log Template**:
```
=== DEPLOYMENT LOG ===
Version: v1.2.3
Date: 2024-01-15
Deployer: Alice (alice@example.com)
Approver 1: Bob (bob@example.com)
Approver 2: Charlie (charlie@example.com)

Pre-deployment:
  Block Height: 51234567
  Contract Hash: abc123...
  Network: public

Deployment:
  Upgrade Proposed: 2024-01-15 14:30:00 UTC
  Signatures Collected: 2024-01-15 14:35:00 UTC
  Executed: 2024-01-15 14:40:00 UTC
  New Contract Hash: def456...

Post-deployment:
  Block Height: 51234582
  Verification: PASSED ✓
```

## Post-Deployment Phase

### Smoke Tests Checklist

- [ ] Contract is callable: `get_score()` returns expected values
- [ ] Event emission works: New events appear in event stream
- [ ] Storage is consistent: No corrupted state detected
- [ ] No error spike in logs

**Smoke Test Script**:
```bash
#!/bin/bash
set -e

CONTRACT_ID="<CONTRACT_ID>"
NETWORK="public"

echo "Smoke Test 1: Query admin..."
soroban contract invoke --network $NETWORK --id $CONTRACT_ID get_admin

echo "Smoke Test 2: Submit score..."
soroban contract invoke --network $NETWORK --id $CONTRACT_ID submit_score \
  --wallet <WALLET> --pair "stellar:usdc" --score 50

echo "Smoke Test 3: Read score..."
soroban contract invoke --network $NETWORK --id $CONTRACT_ID get_score \
  --wallet <WALLET> --pair "stellar:usdc"

echo "✓ All smoke tests passed"
```

### Event Audit Checklist

- [ ] All expected events are emitted
- [ ] Event schema versions are correct
- [ ] Correlation IDs link related events correctly
- [ ] No duplicate or out-of-order events

**Event Audit Script**:
```bash
#!/bin/bash

# Export events from deployment block range
soroban events --network public --start-ledger <PRE_BLOCK> --end-ledger <POST_BLOCK> \
  --contract <CONTRACT_ID> > deployment_events.json

# Verify event structure
jq -r '.[] | .topics[0] | ascii_downcase' deployment_events.json | sort | uniq -c

# Check for error events
jq '.[] | select(.data | contains("Error"))' deployment_events.json

echo "Event audit complete. Review deployment_events.json"
```

### Operator Notification Checklist

- [ ] Post deployment summary to #operations Slack channel
- [ ] Include WASM hash, block height, and event links
- [ ] Notify monitoring team of any new metrics to track
- [ ] Update status page / incident tracker (if applicable)

**Notification Template**:
```
🚀 Deployment Successful

Version: v1.2.3
Network: Stellar Public
Deployed: 2024-01-15 14:40:00 UTC
Contract ID: CXXXXXX...
WASM Hash: abc123def456...
Block Range: 51234567 - 51234582

Changes:
- ✨ Added escalation_triggered event
- 🔧 Optimized score query performance
- 📝 Updated deployment documentation

Monitoring:
- New alerts configured for escalation_triggered
- Check dashboard: https://...

Rollback Plan: Available if needed
Contact: @release-team

All systems nominal ✓
```

## Post-Deployment Validation (24-48 hours)

### Extended Monitoring Checklist

- [ ] No error spike in contract logs (24 hours post-deployment)
- [ ] Event emission is continuous and healthy
- [ ] Score submissions processing normally
- [ ] No storage growth anomalies
- [ ] Off-chain indexers successfully consuming events

### Audit Trail Verification

- [ ] Complete event history exported: `ledgerlens_events_v1.2.3.json`
- [ ] Event schema verified against documentation
- [ ] Correlation IDs reconstructed for all workflows
- [ ] State reproducible from events alone
- [ ] Audit report generated and filed

**Audit Verification Script**:
```bash
#!/bin/bash

# Export complete event history since deployment
soroban events --network public --contract <CONTRACT_ID> \
  --start-ledger <DEPLOYMENT_BLOCK> \
  > audit_events.json

# Run audit replay tests
cargo test audit_replay --release -- --include-ignored

# Generate audit report
cat > AUDIT_REPORT.md << EOF
# Audit Report - v1.2.3

Generated: $(date)
Contract: <CONTRACT_ID>
Deployment Block: <BLOCK>

## Event Stream Verification
- Total events: $(jq length audit_events.json)
- Unique event types: $(jq -r '.[] | .topics[0]' audit_events.json | sort -u | wc -l)
- Schema version: 1

## Correlation Analysis
- Score workflows reconstructed: $(jq -r '.[] | select(.topics[0]=="score") | .data.wallet' audit_events.json | sort -u | wc -l)
- Admin transfers tracked: $(jq -r '.[] | select(.topics[0]=="adm_init")' audit_events.json | wc -l)

## State Audit
- ✓ No corruption detected
- ✓ All state transitions logged
- ✓ Replay test passed

Status: APPROVED ✓
EOF
```

### Issue Resolution Checklist

- [ ] Any blocking issues fixed in hot-patch release (if needed)
- [ ] Non-critical issues documented for next release
- [ ] Lessons learned documented
- [ ] Deployment runbook updated with new learnings

## Rollback Procedures

### When to Rollback

Rollback immediately if:
- [ ] Transactions failing (non-transient errors)
- [ ] Storage corruption detected
- [ ] Security vulnerability discovered
- [ ] Event emission broken

### Rollback Steps

1. **Stop new transactions** (if possible)
   ```bash
   # Update contract to reject new requests
   soroban contract invoke --id <CONTRACT_ID> set_pause_state --paused true
   ```

2. **Prepare rollback upgrade**
   ```bash
   git checkout v<PREVIOUS_VERSION>
   cargo build --release
   ROLLBACK_HASH=$(sha256sum build/ledgerlens_score.wasm | cut -d' ' -f1)
   ```

3. **Propose rollback**
   ```bash
   soroban contract invoke --id <CONTRACT_ID> propose_upgrade \
     --new_wasm_hash $ROLLBACK_HASH \
     --executable_after <TIMESTAMP + 1_HOUR>
   ```

4. **Collect approvals** (per multi-sig policy)

5. **Execute rollback**
   ```bash
   soroban contract invoke --id <CONTRACT_ID> execute_upgrade
   ```

6. **Verify rollback**
   ```bash
   soroban contract info --id <CONTRACT_ID> | grep hash
   # Should match v<PREVIOUS_VERSION> hash
   ```

7. **Incident review**
   - Document root cause
   - Create postmortem
   - Plan fixes for next release

## Sign-Off Template

**For Release Manager:**

I certify that:
- [ ] All checklist items are marked complete above
- [ ] I have verified the deployment independently
- [ ] I have tested the rollback procedure
- [ ] All team members have been notified

**Signed:**
```
Name: _____________________
Date: _____________________
Signature: _________________

WASM Hash: _________________
Block Height: ______________
Network: ___________________
```

**For Security Auditor:**

I certify that:
- [ ] Code changes reviewed for security issues
- [ ] No high/critical severity issues remaining
- [ ] Public ABI compatibility verified
- [ ] Resource usage is bounded

**Signed:**
```
Name: _____________________
Date: _____________________
Signature: _________________
```

**For Operations Manager:**

I certify that:
- [ ] Monitoring is configured
- [ ] Runbooks are updated
- [ ] Escalation procedures are ready
- [ ] Team is trained on changes

**Signed:**
```
Name: _____________________
Date: _____________________
Signature: _________________
```

## Deployment Records

All deployment records are stored in `/deployments/` directory:

```
deployments/
├── v1.2.3/
│   ├── DEPLOYMENT.txt.asc          # Signed deployment hash
│   ├── CHANGELOG_v1.2.3.md          # Release notes
│   ├── SMOKE_TESTS.log              # Test results
│   ├── DEPLOYMENT_LOG.txt           # Deployment execution log
│   ├── audit_events.json            # Complete event stream
│   ├── AUDIT_REPORT.md              # State audit verification
│   └── SIGN_OFF.txt                 # Approver signatures
```

## See Also

- [Event Schema Stability](./EVENT_SCHEMA_STABILITY.md) - Schema compatibility guarantees
- [Event Causality](./EVENT_CAUSALITY.md) - Linking related events
- [Audit Replay](./AUDIT_REPLAY.md) - Verifying state from events
- [CHANGELOG.md](../CHANGELOG.md) - Release history
