# SDK Conformance Fixtures

LedgerLens SDKs for Rust, TypeScript, and Python must agree on the observable
contract in `tests/composability/sdk_conformance_fixtures.json`.

## Contract

The fixtures exercise production-shaped consumers, not direct happy-path calls
into LedgerLens. Each consumer validates amount first, requires admin
authorization for oracle/configuration rotation, checks optional contract
version compatibility, rejects stale scores, then asks the LedgerLens gate.
Read-only decisions must not create persistent writes.

Transport failure is never reported as low risk. Integrators must configure one
of two explicit policies:

| Policy | Unavailable oracle behavior |
| --- | --- |
| `fail_closed` | Reject with `OracleUnavailable`. |
| `fail_open` | Allow only because configuration explicitly chose availability over risk freshness. |

Risk rejections remain distinct from operational failures:

| Fixture outcome | Meaning |
| --- | --- |
| `allow` | Score exists, is fresh, below threshold, and meets confidence/version requirements. |
| `reject_high_risk` | Score is missing, embargoed, equal/above threshold, or the gate returned false. |
| `reject_low_confidence` | Score is below threshold but below the consumer confidence floor. |
| `reject_stale` | Score age exceeds `max_staleness_secs` or LedgerLens reports it stale. |
| `oracle_unavailable` | Cross-contract call trapped, target is missing, or response cannot be decoded. |
| `unsupported_version` | Oracle version is lower than the configured required version. |

## Compatibility

`required_oracle_version = 0` represents old clients that do not enforce a
contract-version floor. New clients against old contracts must either run with
that compatibility setting or reject with `unsupported_version`; they must not
guess by treating a failed version probe as safe.

No LedgerLens core ABI, storage key, event, error discriminant, or
cryptographic transcript changes are introduced by these fixtures. The added
types and errors are mock-consumer local.

## Operations

Monitor per-client counts for `oracle_unavailable`, `unsupported_version`,
`reject_stale`, and `reject_low_confidence`. Recovery is configuration-only:
rotate `set_risk_oracle` to a healthy deployment, lower
`required_oracle_version` only for a documented compatibility rollback, or
increase `max_staleness_secs` only under an incident policy. For risk-policy
rollback, switch from `fail_open` back to the default `fail_closed` once the
oracle is healthy.

## PR Design Notes

Trust assumptions: consumers trust the configured LedgerLens contract ID and
the configured admin, not arbitrary callback contracts.

Authorization boundary: only the mock fixture admin can rotate the oracle or
change thresholds, freshness, version, and failure policy.

State transitions: initialize stores admin and default fail-closed config;
configuration calls atomically replace the relevant instance-storage fields;
swap/borrow paths perform reads only.

Failure modes: high risk, low confidence, stale data, unavailable oracle,
unsupported version, malformed response, and unauthorized configuration all map
to explicit outcomes.

Rejected alternatives: collapsing every failure to a boolean was rejected
because transport failure could masquerade as low risk; adding LedgerLens core
ABI/storage changes was rejected because the existing gate and score APIs
already expose the needed signals.

Invariant protected: no consumer action proceeds unless the configured policy
and current oracle evidence explicitly permit it.

Closes #679
