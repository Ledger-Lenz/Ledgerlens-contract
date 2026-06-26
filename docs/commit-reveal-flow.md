# Commit-Reveal Score Submission Flow

## Overview

LedgerLens employs a commit-reveal pattern to prevent MEV (maximum extractable value) attacks during multi-model consensus scoring. This pattern is used in the consensus submission flow where multiple independent models submit their risk assessments: during the commit phase, models publish commitments (cryptographic hashes) of their scores without revealing the actual values; only after all commitments are recorded on-chain does the reveal phase begin, where the actual scores are published and verified against their commitments. This two-phase structure prevents models from observing each other's submissions and adjusting their own to game the final consensus value.

## Happy Path — Normal Consensus Flow

```mermaid
sequenceDiagram
    participant Model as Model 1-N
    participant Contract as Ledger-Lenz Contract
    participant Chain as Soroban Ledger

    Note over Model,Chain: Phase 1: Commit
    loop For each model
        Model->>Contract: commit_consensus(model, wallet, pair, commitment_hash)
        Contract->>Chain: Store commitment for (model, wallet, pair)
        Contract->>Contract: emit: No event (internal state)
    end
    
    Note over Model,Chain: Wait: Finality window
    Chain-->>Chain: Time advances by ≥ finality_buffer
    
    Note over Model,Chain: Phase 2: Reveal
    Model->>Contract: reveal_consensus(signers, wallet, pair, submissions[], nonces[])
    Contract->>Contract: For each submission verify hash(score || nonce) == stored_commitment
    Contract->>Contract: Collect valid scores, compute median
    Contract->>Contract: Tally consensus: count scores within ±epsilon of median
    alt Consensus reached (≥k models agree)
        Contract->>Chain: Store consensus score (model_version = 0)
        Contract->>Contract: emit: consensus_score_submitted
    else Insufficient consensus
        Contract->>Contract: return Err(Error::InsufficientConsensus)
    end
```

## Finality Buffer (Pending Score Commit Window)

When `submit_score` is called with a `finality_buffer > 0`, the score is held in a pending state instead of taking effect immediately. This is a separate administrative flow from consensus commit-reveal, used to allow admins to review and cancel suspicious scores before they become visible to downstream protocols.

```mermaid
sequenceDiagram
    participant Service as Off-Chain Service
    participant Contract as Ledger-Lenz Contract
    participant Downstream as Downstream Protocols

    Service->>Contract: submit_score(wallet, pair, score, ..., attestation=None)
    Contract->>Contract: Score enters pending state (commit_after = now + finality_buffer)
    Contract->>Contract: emit: score_pending
    Service-->>Downstream: Score NOT yet visible (finality window active)
    
    alt Admin review passes
        Note over Contract: Finality window elapses
        Service->>Contract: commit_pending_score(wallet, pair)
        Contract->>Contract: Move pending → live storage
        Contract->>Contract: emit: score_committed
        Downstream->>Contract: get_score(wallet, pair) → returns committed score
    else Admin cancels before finality window elapses
        Service->>Contract: cancel_pending_score(admin_signers, wallet, pair)
        Contract->>Contract: Discard pending score
        Contract->>Contract: emit: score_pending_cancelled
        Downstream->>Contract: get_score(wallet, pair) → still old/absent
    end
```

## Multi-Model Consensus Commit-Reveal (MEV-Resistant)

For consensus scoring, the flow is:

1. **Commitment Phase**: Each model's `commit_consensus()` call sends `commit(score || nonce || model_id)` to the contract.
2. **Finality Window**: Some time must pass before reveal is allowed (determined by the network's confirmation time).
3. **Reveal Phase**: Call `reveal_consensus()` with the full list of submissions and nonces. The contract re-computes each commitment hash and verifies it matches the stored commitment for that model.
4. **Tally**: Once all commitments are verified, the contract computes the median score and checks if at least `k` models are within `±epsilon` of that median.

### Sequence Diagram: Multi-Model Consensus

```mermaid
sequenceDiagram
    participant M1 as Model 1
    participant M2 as Model 2
    participant M3 as Model 3
    participant Contract

    Note over M1,Contract: Round 1: Commit Phase
    M1->>Contract: commit_consensus(model1, wallet, pair, hash(42 || nonce1 || model1))
    M2->>Contract: commit_consensus(model2, wallet, pair, hash(41 || nonce2 || model2))
    M3->>Contract: commit_consensus(model3, wallet, pair, hash(100 || nonce3 || model3))
    
    Note over Contract: Commitments stored. MEV: no model can see others' scores yet.
    
    Note over M1,Contract: Wait: Finality window (e.g., 10 ledgers)
    
    Note over M1,Contract: Round 2: Reveal Phase (single call)
    M1->>Contract: reveal_consensus([M1, M2, M3], wallet, pair, [(score: 42, ...), (score: 41, ...), (score: 100, ...)], [nonce1, nonce2, nonce3], timestamp)
    
    Contract->>Contract: Verify each hash re-computes correctly
    Contract->>Contract: Median = median(42, 41, 100) = 42
    Contract->>Contract: Consensus set within ±epsilon: [42, 41] (2 models agree)
    
    alt k = 2, epsilon = 5: consensus reached
        Contract->>Contract: Store final_score = 42 (median of {42, 41})
        Contract->>Contract: emit consensus_score_submitted(wallet, pair, 42, 2 agreeing, 5)
    else Insufficient consensus (k > 2)
        Contract->>Contract: return Err(InsufficientConsensus)
    end
```

## Admin Cancel Path

An admin can unilaterally cancel a pending score (from the finality buffer) at any time before the finality window has elapsed. This is the administrative escape hatch for catching erroneous submissions.

```mermaid
sequenceDiagram
    participant Service as Service
    participant Admin as Admin
    participant Contract

    Service->>Contract: submit_score(wallet, pair, score, ...)
    Contract->>Contract: Score enters pending (commit_after = T+finality_buffer)
    
    Admin->>Contract: cancel_pending_score(admin_signers, wallet, pair)
    Contract->>Contract: Verify admin auth
    Contract->>Contract: Delete pending entry
    Contract->>Contract: emit score_pending_cancelled(wallet, pair, admin)
    
    Note over Admin,Contract: Score never becomes visible
```

## Consensus Commit-Reveal No-Op Cases

- **Commitment exists but reveal never called**: The commitment remains stored until its TTL expires (see `ESCALATION_BREACH_TTL_EXTEND_TO` for the default TTL refresh strategy). No active cleanup is required.
- **Wrong nonce in reveal**: The re-computed hash will not match the stored commitment, triggering `Error::CommitmentMismatch`.
- **Timestamp is 0 or out of staleness window**: Rejected with `Error::InvalidTimestamp`.
- **Submissions and nonces length mismatch**: Rejected with `Error::CommitmentMismatch` (signal for desynchronization).

## Function Reference

| Function | Phase | Auth Required | Description |
|----------|-------|---------------|-------------|
| `submit_score` | Finality Buffer | Service | Submits a regular score; held pending if finality_buffer > 0 |
| `commit_pending_score` | Finality Buffer | None (time-gated) | Moves pending score to live storage once finality window elapses |
| `cancel_pending_score` | Finality Buffer | Admin | Discards a pending score before the finality window elapses |
| `commit_consensus` | Consensus Phase 1 | Model | Submits a consensus score commitment (hashed) |
| `reveal_consensus` | Consensus Phase 2 | Service Signers | Reveals scores, verifies commitments, tallies consensus |

**Source:** [contracts/ledgerlens-score/src/lib.rs](../../contracts/ledgerlens-score/src/lib.rs)

## Security Notes

### Finality Buffer

- The finality buffer is an optional administrative review window; it does **not** affect the security of individual attestations (which use off-chain secp256k1 signatures).
- Once committed, a score's commit timestamp is immutable and part of the stored `RiskScore`.
- The admin may cancel a pending score at any time before finality, but once committed it cannot be reverted without submitting a new score.

### Consensus Commit-Reveal

- **Nonce Reuse**: Each nonce must be unique per model per (wallet, asset_pair) to prevent commitment collision attacks. The off-chain orchestrator must ensure nonces are drawn from a cryptographically random source and never reused.
- **Commitment Hash Format**: The hash must be computed as `sha256(score || nonce || model_id || wallet || asset_pair || contract_id)` to bind it to a specific proposal context. The exact byte-ordering is defined in `contracts/ledgerlens-score/src/verkle.rs`.
- **Finality Window**: Even within consensus, a finality window (determined by network confirmation time) must pass between the commit and reveal phases. This prevents models from observing each other's commitments on-chain and adjusting their reveals.
- **Median Stability**: The consensus implementation uses integer division for the median to ensure deterministic results across different ledger environments.

## Known Limitations and Future Work

- **No multi-stage consensus**: Consensus is currently single-round. A multi-stage Byzantine voting protocol is not yet implemented (see #XXX for future enhancement).
- **Fixed epsilon and k**: The consensus threshold parameters (`epsilon`, `k`) are currently global; per-pair tuning is not yet supported.
- **No partial consensus recovery**: If some models fail to reveal, the entire proposal fails. A majority-recovery mode is a candidate enhancement.
