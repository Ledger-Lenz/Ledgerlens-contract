# Zero-Knowledge Range Proofs for Score Queries

LedgerLens implements a zero-knowledge (ZK) range proof scheme allowing third-party contracts to verify that a wallet's risk score is below a chosen threshold $T$ (i.e. $score < T$) without the LedgerLens score contract revealing the exact score value to the calling contract.

This is achieved using **Pedersen Commitments** and **Bulletproofs** verified on-chain via a SHA-256-based Fiat-Shamir heuristic.

---

## Cryptographic Design

### Pedersen Commitment

When the LedgerLens service submits a score $v \in [0, 100]$ to the contract, it can optionally submit a Pedersen commitment:
$$C = g^v \cdot h^r \pmod p$$
where:
*   $g$ and $h$ are independent generators on the Twisted Edwards Curve (Ed25519).
*   $v$ is the score.
*   $r$ is a randomly chosen blinding factor (scalar) in the scalar field of Ed25519 ($\mathbb{F}_p$ where $p = 2^{255}-19$).

This commitment is stored in the contract's persistent storage alongside the score.

### Proving $v < T$

To prove that the score $v$ satisfies the threshold $T$ (i.e. $v \le T - 1$), the prover shows that:
$$v' = T - 1 - v \ge 0$$
Since $v \in [0, 100]$ is already verified on-chain during score submission, $v' \ge 0$ is sufficient to guarantee $v < T$.

The verifier on-chain can compute a commitment $C'$ to $v'$ dynamically from the stored commitment $C$ and the threshold $T$:
$$C' = g^{T-1} \cdot C^{-1} = g^{T-1} \cdot (g^v h^r)^{-1} = g^{T-1-v} \cdot h^{-r}$$
The blinding factor for $C'$ is $r' = -r \pmod p$.

The prover then provides a Bulletproof range proof showing that the value in $C'$ (which is $v'$) lies in the range $[0, 2^8)$ (i.e. $0 \le v' \le 255$, which is satisfied since $0 \le v' \le 99$).

---

## Bulletproof Range Proof Protocol

The Bulletproof range proof demonstrates that $v' \in [0, 2^8)$ in zero-knowledge.

1.  **Bit Commitments**:
    The prover decomposes $v'$ into its binary representation $\vec{a}_L \in \{0, 1\}^8$.
    It computes $\vec{a}_R = \vec{a}_L - \vec{1}$.
    It commits to these vectors using:
    $$A = h^{\alpha} \cdot \vec{g}^{\vec{a}_L} \cdot \vec{h}^{\vec{a}_R}$$
    It commits to blinding vectors $\vec{s}_L, \vec{s}_R$:
    $$S = h^{\beta} \cdot \vec{g}^{\vec{s}_L} \cdot \vec{h}^{\vec{s}_R}$$
2.  **Fiat-Shamir Challenges**:
    The prover hashes the transcript to generate challenges $y, z$.
3.  **Polynomial Formulation**:
    The prover defines vector polynomials:
    $$\vec{l}(X) = (\vec{a}_L - z \cdot \vec{1}) + \vec{s}_L X$$
    $$\vec{r}(X) = \vec{y}^n \circ (\vec{a}_R + z \cdot \vec{1} + \vec{s}_R X) + z^2 \vec{2}^n$$
    The inner product $t(X) = \langle \vec{l}(X), \vec{r}(X) \rangle = t_0 + t_1 X + t_2 X^2$ is committed using:
    $$T_1 = g^{t_1} h^{\tau_1}$$
    $$T_2 = g^{t_2} h^{\tau_2}$$
    The verifier sends a challenge $x$.
4.  **Evaluations**:
    The prover evaluates:
    $$t_x = t(x)$$
    $$\tau_x = \tau_2 x^2 + \tau_1 x + z^2 r'$$
    $$\mu = \alpha + \beta x$$
5.  **Inner Product Argument**:
    A 3-round recursive inner product argument is run to prove that $t_x = \langle \vec{l}(x), \vec{r}(x) \rangle$ holds, compressing the vector opening down to $O(\log n)$ points.

---

## On-Chain Contract API

### `submit_score`
The score submission function accepts the commitment packed inside the `ScoreAttestationInput` struct to stay within Soroban's 10-parameter limit:

```rust
pub struct ScoreAttestationInput {
    pub attestation: MaybeScoreAttestation,
    pub threshold_attestation: MaybeThresholdAttestation,
    pub commitment: Option<Bytes>, // The Pedersen commitment C
}

pub fn submit_score(
    env: Env,
    signers: Vec<Address>,
    wallet: Address,
    asset_pair: Symbol,
    score: u32,
    benford_flag: bool,
    ml_flag: bool,
    timestamp: u64,
    confidence: u32,
    attestation_input: Option<ScoreAttestationInput>,
) -> Result<(), Error>;
```

### `verify_score_range_proof`
Allows third-party verifiers to check that a score is below a threshold:
```rust
pub fn verify_score_range_proof(
    env: Env,
    wallet: Address,
    asset_pair: Symbol,
    commitment: BytesN<32>, // The claimed commitment C
    proof: Bytes,           // The 800-byte Bulletproof
    threshold: u32,         // The threshold T
) -> bool;
```

The function returns `true` if:
1.  A score entry exists for the `(wallet, asset_pair)` pair.
2.  The stored commitment matches the provided `commitment`.
3.  The Bulletproof demonstrates that $T - 1 - v \in [0, 2^8)$ under $C' = g^{T-1} C^{-1}$.

---

## Security Model

*   **Binding**: The Pedersen commitment is perfectly binding. The score reporter cannot open the commitment to any score other than the one submitted.
*   **Hiding**: The commitment is perfectly hiding. A calling contract learns nothing about the exact score value from the commitment itself.
*   **Completeness**: A prover who knows the correct score and blinding factor can always generate a valid range proof that verifies.
*   **Soundness**: An attacker cannot generate a valid range proof for a score that is equal to or greater than the threshold without solving the discrete logarithm problem or finding SHA-256 collisions.
