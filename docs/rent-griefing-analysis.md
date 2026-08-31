# Rent Griefing via High-Cardinality Monitored Wallets

**Date:** 2026-07-26 · **Status:** Analysis complete — existing bound sufficient, no code change required beyond regression coverage

## Threat model

`submit_score` (`contracts/ledgerlens-score/src/lib.rs`) is gated by
authorized signers/service — it is not open to arbitrary public callers.
The realistic risk is therefore not an external attacker, but the
**off-chain scoring service (or a compromised/careless authorized signer)
submitting scores for a very large number of low-value wallet/asset-pair
combinations**: wallets with negligible balances or one-off pairs that
provide little risk-monitoring value but still occupy persistent storage
and consume rent indefinitely once written.

Each `(wallet, asset_pair)` combination creates:
- One `DataKey::Score` persistent entry (the score itself).
- One `DataKeyB::ScoreEntryLastTouchedLedger` persistent entry (rent-tracking
  touch marker, `storage.rs:174-182`).
- Optionally, a slot in the shared `ScoreEntryIndex` rent-management queue.

Persistent storage rent is proportional to entry count and TTL window, so
unbounded growth in distinct `(wallet, asset_pair)` combinations directly
increases the contract's ongoing rent burden and the cost of any full-index
sweep operation.

## Existing bound

`storage::reindex_entry_to_back` (`storage.rs:141-166`) already caps the
proactive rent-management index at `MAX_TRACKED_SCORE_ENTRIES` = 500
(`constants.rs:158`): once the index holds 500 distinct entries, new
distinct `(wallet, asset_pair)` combos are silently **not** added to the
index. This bounds `get_expiring_entries`'s sweep cost regardless of how
many distinct combinations have ever been submitted — see
`test_get_expiring_entries_short_circuits_full_index_scan` in
`test_ttl_rent_manager.rs`.

This is a resource-usage bound, not a submission bound: `set_score` still
accepts and persists writes for combinations beyond the 500-entry index.
Those entries still get their own TTL extended on write (self-renewing),
they just aren't visible to the admin's proactive-renewal sweep. In
practice this means low-value combinations beyond the cap are **not**
actively kept alive — if never resubmitted, they simply expire and archive
at their natural TTL, which already functions as passive cleanup.

## Proposed mitigation

No new enforcement code is required: the 500-entry index cap already
bounds the worst-case sweep cost, and TTL expiry already reclaims storage
for combinations that stop being resubmitted, at no extra rent cost to the
admin (Soroban does not charge for expired/archived entries). The
remaining risk is bounded persistent-storage growth from ever-increasing
distinct combinations that *are* still actively resubmitted (so they never
expire) — a genuine but low-value monitoring habit rather than an attack
enabled by a missing guard.

Two complementary strategies, ranked by cost/benefit, for a future issue if
the off-chain service's cardinality grows further:

1. **Prioritization (recommended, no ABI change):** have the off-chain
   service rank wallets by monitored value (balance, transaction volume)
   before submission and only resubmit the top `MAX_TRACKED_SCORE_ENTRIES`
   to keep the proactive index fully representative of what's actively
   monitored. Zero on-chain cost; purely an off-chain scheduling policy.
2. **Per-signer submission quota (higher cost, requires governance):** cap
   distinct new `(wallet, asset_pair)` combinations one signer can submit
   per epoch. Adds a new counter write per submission (extra rent per call)
   and a new timelocked governance parameter — justified only if a single
   compromised signer submitting thousands of low-value entries per epoch
   is judged a credible threat, which the signer-gating on `submit_score`
   currently makes unlikely.

## Compatibility impact

Documentation only for this issue. The regression test added alongside this
ADR (`test_high_cardinality_rent.rs`) exercises existing behavior; no
public ABI, event, error, or storage changes.
