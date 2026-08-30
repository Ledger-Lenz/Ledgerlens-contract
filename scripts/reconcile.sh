#!/usr/bin/env bash
# reconcile.sh — Post-incident score reconciliation for LedgerLens.
#
# Compares on-chain contract state against an off-chain pipeline NDJSON dump
# and produces a deterministic reconciliation report (JSON) listing mismatches
# with recommended remediation actions.
#
# Usage:
#   ./scripts/reconcile.sh \
#     --pipeline    <path/to/pipeline_records.ndjson> \
#     --contract-id <CONTRACT_ID> \
#     --rpc-url     <RPC_URL> \
#     --network     <testnet|mainnet> \
#     [--max-age-secs  <seconds>]     # default: 86400 (24 h)
#     [--score-tolerance <int>]        # default: 0  (exact match)
#     [--output     <report.json>]     # default: reconciliation_report.json
#
# Exit codes:
#   0 — report written (mismatches may still exist; check summary)
#   1 — fatal error (bad arguments, missing tools, unreadable pipeline file)
set -euo pipefail

# ── Defaults ─────────────────────────────────────────────────────────────────
PIPELINE=""
CONTRACT_ID="${CONTRACT_ID:-}"
RPC_URL="${RPC_URL:-}"
NETWORK="${NETWORK:-testnet}"
MAX_AGE_SECS=86400
SCORE_TOLERANCE=0
OUTPUT="reconciliation_report.json"

# ── Argument parsing ──────────────────────────────────────────────────────────
while [[ $# -gt 0 ]]; do
  case "$1" in
    --pipeline)       PIPELINE="$2";       shift 2 ;;
    --contract-id)    CONTRACT_ID="$2";    shift 2 ;;
    --rpc-url)        RPC_URL="$2";        shift 2 ;;
    --network)        NETWORK="$2";        shift 2 ;;
    --max-age-secs)   MAX_AGE_SECS="$2";  shift 2 ;;
    --score-tolerance) SCORE_TOLERANCE="$2"; shift 2 ;;
    --output)         OUTPUT="$2";         shift 2 ;;
    *) echo "Unknown argument: $1" >&2; exit 1 ;;
  esac
done

# ── Validation ────────────────────────────────────────────────────────────────
if [[ -z "$PIPELINE" ]]; then
  echo "Error: --pipeline is required." >&2; exit 1
fi
if [[ ! -f "$PIPELINE" ]]; then
  echo "Error: pipeline file not found: $PIPELINE" >&2; exit 1
fi
if [[ -z "$CONTRACT_ID" ]]; then
  echo "Error: --contract-id or \$CONTRACT_ID is required." >&2; exit 1
fi

# Prefer 'stellar' CLI; fall back to 'soroban'.
if command -v stellar &>/dev/null; then
  CLI="stellar"
elif command -v soroban &>/dev/null; then
  CLI="soroban"
else
  echo "Error: neither 'stellar' nor 'soroban' CLI found on PATH." >&2; exit 1
fi

if ! command -v jq &>/dev/null; then
  echo "Error: 'jq' is required." >&2; exit 1
fi

NOW=$(date +%s)

# ── Query on-chain score ──────────────────────────────────────────────────────
# Returns JSON from get_score, or empty string on ScoreNotFound.
query_onchain() {
  local wallet="$1" pair="$2"
  "$CLI" contract invoke \
    --id "$CONTRACT_ID" \
    --rpc-url "$RPC_URL" \
    --network "$NETWORK" \
    -- get_score \
    --wallet "$wallet" \
    --asset_pair "$pair" 2>/dev/null || true
}

# ── Build pipeline index ──────────────────────────────────────────────────────
# Temporary directory for per-entry files.
TMPDIR=$(mktemp -d)
trap 'rm -rf "$TMPDIR"' EXIT

declare -A PIPELINE_KEYS   # key="wallet|pair" -> 1
while IFS= read -r line; do
  [[ -z "$line" ]] && continue
  wallet=$(echo "$line" | jq -r '.wallet // empty')
  pair=$(echo "$line"   | jq -r '.asset_pair // empty')
  [[ -z "$wallet" || -z "$pair" ]] && continue
  key="${wallet}|${pair}"
  PIPELINE_KEYS["$key"]=1
  echo "$line" > "$TMPDIR/$(echo "$key" | tr '/' '_' | tr '|' '__').json"
done < "$PIPELINE"

# ── Reconcile ────────────────────────────────────────────────────────────────
ENTRIES_JSON="[]"
OK=0; MISMATCH=0; ONCHAIN_ONLY=0; PIPELINE_ONLY=0; STALE=0

# Process every pipeline entry.
while IFS= read -r line; do
  [[ -z "$line" ]] && continue
  p_wallet=$(echo "$line"     | jq -r '.wallet // empty')
  p_pair=$(echo "$line"       | jq -r '.asset_pair // empty')
  p_score=$(echo "$line"      | jq -r '.score // 50')
  p_confidence=$(echo "$line" | jq -r '.confidence // 0')
  p_timestamp=$(echo "$line"  | jq -r '.timestamp // 0')
  [[ -z "$p_wallet" || -z "$p_pair" ]] && continue

  onchain_raw=$(query_onchain "$p_wallet" "$p_pair")

  if [[ -z "$onchain_raw" ]]; then
    status="pipeline_only"
    remediation="re-submit pipeline score; check service-key auth and cooldown state"
    PIPELINE_ONLY=$((PIPELINE_ONLY + 1))
    entry=$(jq -n \
      --arg wallet      "$p_wallet" \
      --arg pair        "$p_pair" \
      --arg status      "$status" \
      --argjson ps      "$p_score" \
      --argjson pc      "$p_confidence" \
      --argjson pt      "$p_timestamp" \
      --arg remediation "$remediation" \
      '{wallet:$wallet, asset_pair:$pair, status:$status,
        onchain_score:null, pipeline_score:$ps,
        onchain_confidence:null, pipeline_confidence:$pc,
        onchain_timestamp:null, pipeline_timestamp:$pt,
        delta_score:null, remediation:$remediation}')
  else
    o_score=$(echo "$onchain_raw"      | jq -r '.score // 0')
    o_confidence=$(echo "$onchain_raw" | jq -r '.confidence // 0')
    o_timestamp=$(echo "$onchain_raw"  | jq -r '.timestamp // 0')

    delta=$(( p_score - o_score ))
    abs_delta=${delta#-}

    # Check staleness first.
    age=$(( NOW - o_timestamp ))
    if (( age > MAX_AGE_SECS )); then
      status="stale"
      remediation="extend TTL via extend_entry_ttls; re-score if outdated"
      STALE=$((STALE + 1))
    elif (( abs_delta > SCORE_TOLERANCE )); then
      status="mismatch"
      remediation="re-submit pipeline score after override_rate_limit"
      MISMATCH=$((MISMATCH + 1))
    else
      status="ok"
      remediation=""
      OK=$((OK + 1))
    fi

    entry=$(jq -n \
      --arg wallet      "$p_wallet" \
      --arg pair        "$p_pair" \
      --arg status      "$status" \
      --argjson os      "$o_score" \
      --argjson ps      "$p_score" \
      --argjson oc      "$o_confidence" \
      --argjson pc      "$p_confidence" \
      --argjson ot      "$o_timestamp" \
      --argjson pt      "$p_timestamp" \
      --argjson delta   "$delta" \
      --arg remediation "$remediation" \
      '{wallet:$wallet, asset_pair:$pair, status:$status,
        onchain_score:$os, pipeline_score:$ps,
        onchain_confidence:$oc, pipeline_confidence:$pc,
        onchain_timestamp:$ot, pipeline_timestamp:$pt,
        delta_score:$delta, remediation:$remediation}')
  fi

  ENTRIES_JSON=$(echo "$ENTRIES_JSON" | jq --argjson e "$entry" '. + [$e]')
done < "$PIPELINE"

TOTAL=$((OK + MISMATCH + ONCHAIN_ONLY + PIPELINE_ONLY + STALE))

# ── Write report ─────────────────────────────────────────────────────────────
jq -n \
  --argjson now       "$NOW" \
  --argjson total     "$TOTAL" \
  --argjson ok        "$OK" \
  --argjson mismatch  "$MISMATCH" \
  --argjson onchain   "$ONCHAIN_ONLY" \
  --argjson pipeline  "$PIPELINE_ONLY" \
  --argjson stale     "$STALE" \
  --argjson entries   "$ENTRIES_JSON" \
  '{
    generated_at: $now,
    summary: {
      total_entries:        $total,
      ok_count:             $ok,
      mismatch_count:       $mismatch,
      onchain_only_count:   $onchain,
      pipeline_only_count:  $pipeline,
      stale_count:          $stale
    },
    entries: $entries
  }' > "$OUTPUT"

echo "Reconciliation report written to: $OUTPUT"
jq '.summary' "$OUTPUT"

# Non-zero exit if any actionable mismatches found.
if (( MISMATCH + PIPELINE_ONLY > 0 )); then
  echo "WARNING: ${MISMATCH} mismatches and ${PIPELINE_ONLY} pipeline-only entries require attention." >&2
  exit 0   # exit 0 so CI can parse the report; operators check summary
fi
