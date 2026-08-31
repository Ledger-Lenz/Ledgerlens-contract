# Runbook: Compromised Service Signer

Concrete containment, rotation, replay, audit, and recovery steps for a
suspected compromise of a **service signer** (one of the multisig keys
authorized under `set_service_threshold` / `add_service_signer` to submit
scores). For admin-key compromise, see the admin transfer flow in
[`docs/governance.md`](../governance.md) — this runbook covers the
service-signer set specifically.

## Out of scope

Does not cover the LedgerLens off-chain scoring model, GrantFox/campaign
labeling, or general key-management infrastructure — only the on-chain
containment/recovery sequence.

---

## 1. Detection — what a compromised signer looks like

| Signal | Where it surfaces | Contract read |
|---|---|---|
| Scores submitted outside expected cadence/rate | `RateLimitExceeded` errors, unexpected `submit_score` volume | — |
| Statistically anomalous scores from one signer | `iqr_rej` event (deviation flagged) | `events.rs:543` |
| Signer active outside its expected rotation window | Signer age near/at TTL | `get_signer_age(signer)`, `get_signer_rotation_ttl()` |
| Unexpected signer-set change | `sig_add` / `sig_rem` event you didn't initiate | `get_service_signers()` |

**Decision point 1:** If any of the above is confirmed and attributable to a
specific signer key, proceed to containment immediately. If attribution is
unclear, pause first (contain), then investigate.

---

## 2. Containment (stop the bleeding)

Pause the contract so no further scores can be submitted while you work:

```bash
soroban contract invoke \
  --network mainnet \
  --source-account YOUR_ADMIN_ACCOUNT \
  --id $CONTRACT_ID \
  -- pause \
  --admin_signers '["ADMIN_1","ADMIN_2"]'
```

Expected contract state: `is_paused() == true`. Confirm:

```bash
soroban contract invoke --network mainnet --id $CONTRACT_ID -- is_paused
```

**Decision point 2:** If only one asset pair is affected and you need the
rest of the system to keep operating, use `set_pair_paused(pair, true)`
instead of a full `pause`, and confirm with `is_pair_paused(pair)`.

---

## 3. Rotation — remove the compromised signer

```bash
soroban contract invoke \
  --network mainnet \
  --source-account YOUR_ADMIN_ACCOUNT \
  --id $CONTRACT_ID \
  -- remove_service_signer \
  --admin_signers '["ADMIN_1","ADMIN_2"]' \
  --signer $COMPROMISED_SIGNER
```

Expected contract state: `get_service_signers()` no longer includes
`$COMPROMISED_SIGNER`; `get_service_signer_count()` decremented by one.

If the service's shared attestation pubkey (not just a multisig signer) was
exposed, rotate it instead/also:

```bash
soroban contract invoke \
  --network mainnet --source-account YOUR_ADMIN_ACCOUNT --id $CONTRACT_ID \
  -- rotate_service_pubkey --new_pubkey $NEW_PUBKEY_HEX
```

Expected state: `get_pending_service_pubkey()` returns `(new_key,
overlap_expiry)` until the overlap window elapses, after which
`get_service_pubkey()` returns the new key.

Add a replacement signer once the new key material is confirmed clean:

```bash
soroban contract invoke \
  --network mainnet --source-account YOUR_ADMIN_ACCOUNT --id $CONTRACT_ID \
  -- add_service_signer --admin_signers '["ADMIN_1","ADMIN_2"]' --signer $NEW_SIGNER
```

**Decision point 3:** If the compromise could also have touched the admin
multisig (not just the service signer set), stop here and follow the admin
transfer flow (`transfer_admin` → `accept_admin`, guarded by
`get_admin_threshold()`) before resuming service.

---

## 4. Replay validation

Before unpausing, replay the affected window through the deterministic
harness in [`tools/replay`](../../tools/replay/README.md) to confirm no
scores accepted during the suspected compromise window violate score-range,
rate-limit, or determinism invariants:

```bash
cargo run -p ledgerlens-replay -- \
  --snapshot suspected_window.ndjson \
  --contract-id $CONTRACT_ID
```

Expected result: harness reports zero panics, all scores in `[0, 100]`, and
rate limits honored. Any violation is evidence to include in the audit trail
below and grounds to widen the affected-window rollback/dispute scope.

---

## 5. Audit trail

Capture, before resuming service:

- `get_admin_audit_root()` snapshot (before and after remediation).
- The full sequence of `sig_rem` / `sig_add` / `pk_rot` events emitted during
  remediation (topic + ledger sequence + tx hash).
- Replay harness output from step 4.
- Signer age/TTL state (`get_signer_age`, `get_active_signer_count`) at time
  of detection, to support post-incident root-cause analysis (was this a
  stale key past its rotation TTL, or an active key that was actually
  exfiltrated).

---

## 6. Recovery

1. Confirm `get_service_signer_count() >= get_service_threshold()` — enough
   healthy signers remain to meet quorum. If not, add signers first.
2. Unpause:

   ```bash
   soroban contract invoke \
     --network mainnet --source-account YOUR_ADMIN_ACCOUNT --id $CONTRACT_ID \
     -- unpause --admin_signers '["ADMIN_1","ADMIN_2"]'
   ```

3. Confirm `is_paused() == false` and monitor the freshness/availability SLIs
   in [`docs/slo-operational-targets.md`](../slo-operational-targets.md) for
   the following 24h before declaring the incident closed.
4. File the incident under the severity classification in
   [`docs/incident-severity-classification.md`](../incident-severity-classification.md).

---

## Compatibility note

This runbook only documents existing admin/service functions
(`pause`/`unpause`, `remove_service_signer`/`add_service_signer`,
`rotate_service_pubkey`, `transfer_admin`). No contract code, ABI, event, or
storage changes are introduced by this change.
