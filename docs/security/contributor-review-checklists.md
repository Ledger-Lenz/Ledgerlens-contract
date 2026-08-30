# LedgerLens Contributor Security Review Checklists

This document provides mandatory security review checklists organized by change category. Every Pull Request (PR) submitted to the `LedgerLens` smart contract suite must be audited against the applicable checklists before merging.

---

## 📋 Change Categories

### Category A: Entry Points & Public ABI
Applicable when modifying function signatures, parameters, visibility, or query interfaces.

- [ ] **Read-Only Invariant:** Are read-only entry points (e.g., `get_score`, `query_risk_gate`, `get_arch_owner`) strictly side-effect free with zero persistent state writes?
- [ ] **Input Bounds:** Are all string, symbol, vector, and map parameters explicitly bounded in length before execution?
- [ ] **Account vs Contract Address Safety:** Does the entry point correctly handle both Stellar accounts and Soroban contract addresses?
- [ ] **Infallible Gate Defaults:** Do gate queries fail-closed (`false`) on cross-contract call failures or unreachable dependency states?

---

### Category B: Storage & Data Layout
Applicable when introducing or modifying `DataKey` variants, storage layout, or state retention.

- [ ] **Enum Discriminant Integrity:** Are existing `DataKey` enum variants untouched to prevent state collisions?
- [ ] **Storage Type Selection:** Is instance vs. temporary vs. persistent storage selected correctly for the data's required lifecycle?
- [ ] **State Footprint Boundedness:** Is total state storage per user or global instance strictly bounded to prevent storage exhaustion?
- [ ] **TTL Management:** Are TTL (Time-To-Live) extension thresholds defined for persistent storage entries?

---

### Category C: Authorization & Privileged Roles
Applicable when modifying access controls, admin capabilities, or `require_auth()` boundaries.

- [ ] **Explicit Authorization:** Is `require_auth()` invoked on the exact `Address` executing the privileged operation?
- [ ] **Contract-as-Caller Support:** Can contract accounts invoke the function safely without authorization bypasses?
- [ ] **Role Ownership Transfers:** Do admin or architecture owner updates emit tracking events and enforce two-step or atomic transfers?
- [ ] **Least Privilege:** Does the function avoid requesting excessive caller authorizations?

---

### Category D: Resource Bounds & Computation
Applicable when modifying loops, collections, math logic, or cross-contract invocations.

- [ ] **Checked Arithmetic:** Are all numeric operations performed using checked math (`checked_add`, `checked_mul`, etc.) returning explicit errors on overflow/underflow?
- [ ] **Explicit Loop Bounds:** Are all iterations bounded by explicit compile-time constants (e.g., `MAX_SHARDS`, `MAX_MANDATORY_REVIEWERS`)?
- [ ] **Gas & Footprint Bounding:** Is worst-case CPU instruction count and memory footprint strictly bounded?
- [ ] **Deterministic Execution:** Is execution order independent of non-deterministic inputs?

---

### Category E: Events & Error Discriminants
Applicable when adding or updating `#[contracterror]` variants or contract events.

- [ ] **Discriminant Stability:** Are existing `#[contracterror]` numerical values untouched?
- [ ] **Event Schema Integrity:** Do emitted events conform to the standard topic and data schema without exceeding topic size limits?
- [ ] **Structured Error Codes:** Are error conditions mapped to explicit error variants instead of raw panics?

---

### Category F: Operations & Recovery
Applicable when modifying contract initialization, upgradeability, or operator workflows.

- [ ] **Zero-Value Fail-Safe Defaults:** Are uninitialized configurations safe by default?
- [ ] **Diagnostic Signals:** Does the contract state expose actionable health signals (e.g., `LastShardFailure`)?
- [ ] **Emergency Pause / Rollback:** Are operational procedures documented for isolating failing dependencies or shards?

---

## 🛡️ Adversarial Testing Checklist

Before opening a PR, ensure unit/integration tests cover the following edge cases:

- [ ] **Unauthorized Callers:** Attempting operations with unprivileged or spoofed callers.
- [ ] **Max-Plus-One Rejection:** Testing collections with `MAX + 1` elements to confirm explicit bound enforcement.
- [ ] **Zero / Null Parameters:** Invoking functions with zero amounts, empty vectors, or default addresses.
- [ ] **Duplicate Elements:** Submitting vectors with duplicate entries to confirm deduplication checks.
- [ ] **Unreachable Dependencies:** Simulating panics or failures in external cross-contract dependencies.
