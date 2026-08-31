# Safe defaults and forbidden configurations

This document maps production-relevant parameters to the threat they control,
their default, their bounds, and the operational tradeoff.

The baseline assumption is fail-closed risk gating: when data is missing,
stale beyond the configured model, embargoed, or held in pending-finality
state, downstream protocols should assume the wallet is not safe.

## Core gate parameters

| Parameter | Threat controlled | Default | Bounds / rules | Safe production guidance | Forbidden or high-risk configuration |
|---|---|---:|---|---|---|
| `risk_threshold` | High-risk wallet admitted to swaps / borrows / LP actions | `75` | `0..=100` in practice | Keep in a policy-approved band and review with historical false-positive / false-negative data | Threshold so high that known risky scores routinely pass |
| `global_min_confidence` | Low-confidence “safe” score admitted as if trustworthy | `0` | `0..=100` | Set non-zero for any protocol using the score for capital access | `0` together with permissive downstream lending or LP admission |
| `staleness_window` | Long-old score treated as current | `604_800s` | must be `> 0` | Keep short enough that score freshness matches protocol risk horizon | Extremely long windows for fast-moving AMM / lending flows |
| `finality_buffer` | Unreviewed score becomes live immediately | `0s` | `0..=86_400s` | Use non-zero for higher-value or operator-reviewed deployments | Assuming pending scores are already safe to consume |
| `cooldown` | Score thrash / spam / rapid manipulation | `3_600s` | `60..=86_400s` | Keep long enough to bound adversarial resubmission churn | Tiny cooldown paired with low confidence floors |
| `adaptive_rate_limit` | Volatility spikes bypass flat cooldown assumptions | disabled | `variance_scale >= 0` | Enable for highly reactive pairs only if operators can explain the variance model | Large variance scale without operational monitoring |
| `burst_capacity` | Excessive short-term resubmission | implementation-defined bounded integer | non-zero positive integer | Keep minimal; size it to legitimate remediation bursts | Large burst capacity combined with short cooldown |
| `score_floor_policy` | Whitewashing known high-risk wallets with suddenly low scores | disabled with `(80,20)` defaults | `high_water_mark ∈ [50,100]`, `floor_value < high_water_mark` | Enable in production scoring systems that can face signer compromise or collusion | Disabled in environments where “score zeroing” would create material downstream loss |
| `hysteresis_margin` | Gate flapping near threshold | `0` | must stay `< risk_threshold` | Use small positive margin if downstream actions are sensitive to oscillation | Margin near threshold that delays exit from high-risk state too aggressively |
| `escalation_threshold` | Repeated high-risk breaches ignored as isolated events | `5` | `2..=20` | Lower for fast-response monitoring; higher only with good operator staffing | `1`-equivalent behavior via external automation that pages on every blip |

## Data-quality and model controls

| Parameter | Threat controlled | Default | Bounds / rules | Safe production guidance | Forbidden or high-risk configuration |
|---|---|---:|---|---|---|
| `consensus_config (k, epsilon)` | Weak model consensus or over-wide disagreement accepted | `(2, 5)` | validated on write | Keep `k` aligned to signer diversity and `epsilon` aligned to tolerated disagreement | `k` too small for signer count, or `epsilon` so large that consensus is nominal only |
| `adaptive_epsilon` | Static consensus slack under changing variance | disabled | bounded by configured min/max | Use only with explicit min/max policy and monitoring | Enabled with wide min/max and no alerting |
| `privacy_epsilon` | Private aggregate query leaks too much signal | `0` (disabled) | `0` or scaled positive integer | Treat `0` as exact query mode; if enabling privacy, document composition budget off-chain | Small epsilon assumed to solve composition by itself |
| `hll_precision` | Unique-wallet estimate too noisy or too expensive | `8` | `4..=16` | Keep default unless the deployment has measured need | Over-precision that wastes resources without operational value |
| `cluster_boundaries` | Inconsistent risk segmentation | implementation default | ordered list | Version-control and export them with policy review | Unreviewed boundary changes that alter downstream treatment silently |

## Resource-bounding and liveness controls

| Parameter | Threat controlled | Default | Bounds / rules | Safe production guidance | Forbidden or high-risk configuration |
|---|---|---:|---|---|---|
| `history_max_depth` | Unbounded history growth | `10` | `1..=50` | Keep near default unless analytics require more | Depth increases without storage-cost review |
| `upgrade_delay` | Instant governance capture | `172_800s` | `172_800..=1_209_600s` | Keep at least 48 hours; longer for externally governed deployments | Shrinking delay to the minimum during active incident response without public notice |
| `reveal_window` | Commit-reveal liveness failure or indefinite hanging | bounded contract setting | positive bounded integer | Size to realistic operator latency | Oversized windows that delay dispute resolution materially |
| `heartbeat_alert_threshold` | Silent service outage undetected | `3_600s` | positive bounded integer | Keep shorter than your incident SLO | Alert threshold longer than tolerated outage duration |
| `oracle_staleness_threshold` | Confidence adjusted by dead oracle feed | `3_600s` | positive bounded integer | Keep matched to external oracle cadence | Much larger than the oracle’s expected publish interval |
| `pair_volatility_window` | Variance estimate dominated by stale data | implementation default | positive bounded integer | Align with market regime you care about | Windows so long that old shocks dominate current behavior |
| `momentum_window` / `momentum_alert_threshold` | Missing fast directional shifts | implementation default | bounded integer settings | Calibrate together; alert threshold without window context is meaningless | Tiny threshold with noisy short window |

## Authorization and irreversible operations

| Parameter | Threat controlled | Default | Bounds / rules | Safe production guidance | Forbidden or high-risk configuration |
|---|---|---:|---|---|---|
| `admin_set` / `admin_threshold` | Single-key governance capture | legacy single-admin mode | threshold must not exceed set size | Use multisig in production | Threshold `0` in a deployment that claims multisig governance |
| `service_set` / `service_threshold` | Single-signer score submission compromise | legacy single-service mode | threshold must not exceed set size | Use diversified signers or threshold signatures | Single signer with no compensating operational control |
| `deletion_approval_policy` | Routine admins irreversibly delete scores or history | disabled | if enabled, approver must be non-admin and remain disjoint from admin set | Enable wherever deletion is a material privacy or legal action | Enabled with no approver, or approver overlapping the admin key/admin set |
| `gate_callers` / `gate_enforcement_mode` | Unauthorized contracts query or enforce gates incorrectly | open by default unless restricted | bounded allowlist | Restrict callers when the deployment model expects explicit integrators | Assuming allowlist enforcement exists while gate remains open |

## Operational notes

- Public ABI impact from this change set:
  - `set_deletion_approval_policy`
  - `get_deletion_approval_policy`
  - `export_configuration`
- Event impact:
  - adds `del_pol`
- Storage impact:
  - adds deletion-policy keys only; existing storage is unchanged
- Worst relevant bounded-resource case:
  - `export_configuration()` is bounded by a fixed list of global parameters and
    the capped pending-proposal index

