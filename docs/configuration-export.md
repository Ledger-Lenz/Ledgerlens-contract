# Canonical configuration export

`export_configuration()` returns a deterministic machine-readable snapshot of
governance-controlled configuration.

Schema version: `1`

The export includes:

- `schema_version`
- `active_hash` — SHA-256 over ordered active key/value entries
- `pending_hash` — SHA-256 over ordered pending key/value entries
- `export_hash` — SHA-256 over `(schema_version, active_hash, pending_hash, omitted_secret_rationale)`
- `active_values`
- `pending_values`
- `omitted_secret_rationale`

## Determinism rules

- Entry order is fixed by contract code.
- Every `key` is a short symbolic identifier.
- Every `value` is a canonical binary encoding.
- Pending entries include `proposal_id`, `proposed_at`, and `executable_after`.

## Active keys in schema v1

The current export covers the global governance surface that is directly
material to risk gates, bounded-resource behavior, authorization, and operator
response:

| Key | Meaning | Encoding |
|---|---|---|
| `version` | Contract ABI version | big-endian `u32` |
| `admin` | Legacy admin address | address string bytes |
| `adm_set` | Admin signer set | count-prefixed address list |
| `adm_thr` | Admin multisig threshold | big-endian `u32` |
| `service` | Legacy service address | address string bytes |
| `svc_set` | Service signer set | count-prefixed address list |
| `svc_thr` | Service multisig threshold | big-endian `u32` |
| `risk_thr` | Global risk threshold | big-endian `u32` |
| `cooldown` | Global submission cooldown seconds | big-endian `u64` |
| `stale_w` | Staleness window seconds | big-endian `u64` |
| `upg_dly` | Upgrade / parameter timelock seconds | big-endian `u64` |
| `hist_dep` | History depth | big-endian `u32` |
| `fin_buf` | Finality buffer seconds | big-endian `u64` |
| `rvl_win` | Reveal window seconds | big-endian `u64` |
| `hb_alrt` | Heartbeat alert threshold seconds | big-endian `u64` |
| `min_conf` | Global confidence floor | big-endian `u32` |
| `priv_eps` | Privacy epsilon scaled by 100 | big-endian `u32` |
| `cons_cfg` | Consensus `(k, epsilon)` | `u32 || u32` |
| `adp_eps` | Adaptive epsilon config | `bool || min:u32 || max:u32 || scale:u32` |
| `adp_rate` | Adaptive rate-limit config | `bool || variance_scale:u32` |
| `burst` | Burst capacity | big-endian `u32` |
| `vel_cap` | Score velocity cap | `bool || points_per_hour:u32` |
| `scr_flr` | Score floor policy | `bool || high_water_mark:u32 || floor_value:u32` |
| `del_pol` | Deletion approval policy | `bool || option<address>` |
| `esc_thr` | Escalation threshold | big-endian `u32` |
| `hyst_mg` | Hysteresis margin | big-endian `u32` |
| `hll_prec` | HLL precision | big-endian `u32` |
| `flash` | Flash-protection mode | enum tag `u32` |
| `failovr` | Failover contract | `option<address>` |
| `gate_fee` | Gate query fee | big-endian `i128` |
| `gate_opn` | Gate enforcement mode flag | `bool` |
| `gate_acl` | Gate caller allowlist | count-prefixed address list |
| `ora_stl` | Oracle staleness threshold seconds | big-endian `u64` |
| `pair_vol` | Pair volatility window seconds | big-endian `u64` |
| `mom_win` | Momentum window seconds | big-endian `u64` |
| `mom_alt` | Momentum alert threshold | big-endian `u32` |
| `clstrs` | Cluster boundaries | count-prefixed `u32` list |
| `fin_dep` | Finality depth in ledgers | big-endian `u32` |
| `interp` | Interpolation method | enum tag `u32` |
| `ath_cfg` | Adaptive threshold config | `bool || pct:u32 || min:u32 || max:u32 || last:u32` |

## Pending keys in schema v1

Two pending sources are exported:

1. Legacy simple pending parameter changes (`PendingParamChange`)
2. Timelocked parameter governance proposals (`ParameterProposal`)

For each pending record:

- `key` identifies the parameter
- `value` contains the proposed canonical value bytes
- `proposal_id == 0` means legacy simple pending change
- `proposal_id > 0` means parameter-governance proposal ID

## Omitted-secret rationale

The export intentionally omits material that is not stored on-chain:

- off-chain private keys
- HSM seed material
- operator-only runbooks

The export also cannot reveal plaintext justifications for rate-limit override
operations because the contract stores only `justification_hash`, not the
plaintext itself.

## Compatibility impact

- Public ABI: adds `export_configuration()`
- Storage: no migration required for the export itself
- Events: none for export reads

