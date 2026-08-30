# Cross-contract authorization audit

Scope: every trust boundary involving a caller-supplied address, stored
administrator/service/signer, and contract-to-contract call in
`contracts/ledgerlens-score` and `contracts/ledgerlens-aggregator`.

Soroban does not expose a trustworthy ambient "caller" that should be compared
manually. LedgerLens therefore authenticates the address whose authority is
needed with `Address::require_auth()`. This works for accounts and contract
addresses: a calling contract must authorize the sub-invocation through
Soroban's authorization tree.

## Findings

| Assumption | Risk | Mitigation / status | Coverage |
|---|---|---|---|
| The first initializer invoker is entitled to nominate the score contract admin and service | A mempool observer could front-run initialization and permanently install attacker-controlled privileged addresses | **Fixed:** `ledgerlens-score::initialize` now requires the nominated admin's authorization before any write | `test_initialize_requires_nominated_admin_and_rolls_back` |
| The first initializer invoker is entitled to nominate the aggregator admin | Front-running grants control of shard registration and removal | **Fixed:** `ledgerlens-aggregator::initialize` now requires nominated-admin authorization before any write | aggregator `test_initialize_requires_nominated_admin_and_rolls_back` |
| Stored score-service authority may submit scores and heartbeat | Compromised service can poison data or keep liveness false-positive | `require_auth` is applied to the stored service in legacy mode; configured signer sets require membership, threshold, and each signer's auth. Pause/rotation remain admin mitigations | `test_multisig_service::*`, `test_signers::*`, `test_heartbeat::*`, composability unauthorized-submission snapshot |
| Caller-supplied signer vectors represent distinct authorized members | Duplicate or foreign addresses could inflate quorum | Membership and duplicate/count checks precede per-address `require_auth`; threshold vectors are bounded by configured sets | `test_admin_multisig::*`, `test_upgrade_multisig::*`, `test_multisig_service::*` |
| Current admin controls configuration, pause, deletion, upgrade, and shard registry | Service or arbitrary contract could mutate policy or destroy data | Score mutations use `require_admin_auth` (legacy stored-admin auth or M-of-N); direct legacy setters use stored `admin.require_auth`; aggregator add/remove uses stored admin auth | Admin, pause, deletion, upgrade, and aggregator tests indexed in the scenario matrix |
| Pending admin is the party accepting transfer | Current admin alone could redirect and immediately finalize control | Two-step transfer calls `pending.require_auth()` and clears pending state atomically | `test_admin_transfer::*` |
| Model/challenger addresses represent the actor funding or revealing an action | Spoofing could manipulate consensus/dispute state or another actor's balance | Model and challenger entry points call that supplied/stored actor's `require_auth`; commitments and windows are independently checked | `test_consensus::*`, `test_dispute::*`, `test_escrow::*` |
| Registered shard addresses implement the expected score interface | A malicious/drifted contract could return misleading values or trap fan-out | Registration probes required capabilities; shard calls are bounded. Risk gates return `false` on call failure and record the failed shard | aggregator unit tests and `tests/composability/tests/aggregator_*` |
| Read/query calls may be invoked by accounts or contracts without authorization | An attacker can read public risk data, but must never mutate privileged configuration through queries | Queries are intentionally public. Gate reads are conservative; zero data, embargo, and downstream failures fail closed. The documented flash-protection ledger marker is the only deliberate query-side state | query/gate/flash-protection tests |
| A contract address can satisfy `require_auth` merely because it is the immediate invoker | Confused-deputy escalation across nested calls | False: Soroban validates the authorization invocation tree. LedgerLens never treats an ambient invoker as authority and contains no `authorize_as_current_contract` bypass | Static audit of all `require_auth` and cross-contract client sites; initialization regression tests ensure the exact nominated address is required |

## Operator workflow

Deployment tooling must include authorization from the nominated admin in the
same transaction as each `initialize` call. Atomic deploy-and-initialize flows
already authorized by that address remain compatible. A relayer may submit the
transaction, but cannot substitute or authorize an admin on its own.

Service addresses do not authorize initialization because initialization grants
the broader admin role; requiring only the service would let a service key
choose its own governor. Once initialized, service rotation, pause, upgrades,
deletion, and aggregator membership continue to require current admin authority.

## Compatibility and bounded-resource statement

There are no ABI, event, error-enum, or storage-layout changes. The only
behavioral compatibility impact is rejection of previously accepted
unauthenticated initialization. Each initializer adds exactly one host
authorization check, executes once per contract instance, and adds no
iteration or storage growth. Cross-shard work remains capped at ten shards and
all existing batch/index/history bounds remain unchanged.
