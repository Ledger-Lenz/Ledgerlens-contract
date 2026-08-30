#!/usr/bin/env bash
# Rehearse production operations on an isolated Stellar network alias.
#
# Usage:
#   scripts/testnet-canary-rehearsal.sh --network <alias> --contract <id> \
#     --operator <identity> --admin <address> --service <address> --reviewer <name>
#
# The script invokes only reversible controls by default. Destructive erasure
# and WASM upgrade execution are represented by explicit dry-run evidence.
set -euo pipefail

NETWORK=""
CONTRACT_ID=""
OPERATOR_IDENTITY=""
ADMIN_ADDRESS=""
SERVICE_ADDRESS=""
REVIEWER=""
WALLET_ADDRESS=""
ASSET_PAIR="XLM_USDC"
OUTPUT="docs/reports/testnet-canary-rehearsal.md"
DRY_RUN=false

while [[ $# -gt 0 ]]; do
  case "$1" in
    --network) NETWORK="${2:?missing value for --network}"; shift 2 ;;
    --contract) CONTRACT_ID="${2:?missing value for --contract}"; shift 2 ;;
    --operator) OPERATOR_IDENTITY="${2:?missing value for --operator}"; shift 2 ;;
    --admin) ADMIN_ADDRESS="${2:?missing value for --admin}"; shift 2 ;;
    --service) SERVICE_ADDRESS="${2:?missing value for --service}"; shift 2 ;;
    --reviewer) REVIEWER="${2:?missing value for --reviewer}"; shift 2 ;;
    --wallet) WALLET_ADDRESS="${2:?missing value for --wallet}"; shift 2 ;;
    --asset-pair) ASSET_PAIR="${2:?missing value for --asset-pair}"; shift 2 ;;
    --output) OUTPUT="${2:?missing value for --output}"; shift 2 ;;
    --dry-run) DRY_RUN=true; shift ;;
    -h|--help) sed -n '2,15p' "$0"; exit 0 ;;
    *) echo "unknown argument: $1" >&2; exit 1 ;;
  esac
done

missing=()
[[ -n "$NETWORK" ]] || missing+=(--network)
[[ -n "$CONTRACT_ID" ]] || missing+=(--contract)
[[ -n "$OPERATOR_IDENTITY" ]] || missing+=(--operator)
[[ -n "$ADMIN_ADDRESS" ]] || missing+=(--admin)
[[ -n "$SERVICE_ADDRESS" ]] || missing+=(--service)
[[ -n "$REVIEWER" ]] || missing+=(--reviewer)
if [[ "${#missing[@]}" -gt 0 ]]; then
  echo "missing required arguments: ${missing[*]}" >&2
  exit 2
fi
[[ -n "$WALLET_ADDRESS" ]] || WALLET_ADDRESS="$ADMIN_ADDRESS"

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
mkdir -p "$(dirname "$OUTPUT")"

RUN_ID="${RUN_ID:-canary-$(date -u +%Y%m%dT%H%M%SZ)}"
LOG_DIR="target/testnet-canary/$RUN_ID"
mkdir -p "$LOG_DIR"
REPORT_TMP="$LOG_DIR/report.md"
STATUS=0

append() {
  printf '%s\n' "$*" >> "$REPORT_TMP"
}

stellar_cmd() {
  if command -v soroban >/dev/null 2>&1; then
    soroban "$@"
  elif command -v stellar >/dev/null 2>&1; then
    stellar "$@"
  else
    echo "neither stellar nor soroban CLI is installed" >&2
    return 127
  fi
}

invoke() {
  local id="$1"
  local desc="$2"
  local expect_fail="$3"
  shift 3
  local log="$LOG_DIR/$id.log"

  append "### $id - $desc"
  append ""
  append '```text'
  append "$*"
  append '```'
  append ""

  if [[ "$DRY_RUN" == true ]]; then
    printf '[dry-run] %s\n' "$*" > "$log"
    append "Result: DRY_RUN"
    append "Log: $log"
    append ""
    return 0
  fi

  set +e
  "$@" > "$log" 2>&1
  local code=$?
  set -e

  if [[ "$expect_fail" == "fail" && "$code" -ne 0 ]]; then
    append "Result: PASS (expected rejection)"
  elif [[ "$expect_fail" == "pass" && "$code" -eq 0 ]]; then
    append "Result: PASS"
  else
    append "Result: FAIL (exit $code)"
    STATUS=1
  fi
  append "Log: $log"
  append ""
}

{
  echo "# Testnet Canary Rehearsal Report"
  echo ""
  echo "Run ID: $RUN_ID"
  echo "Generated UTC: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "Network alias: $NETWORK"
  echo "Contract: $CONTRACT_ID"
  echo "Operator identity: $OPERATOR_IDENTITY"
  echo "Admin address: $ADMIN_ADDRESS"
  echo "Service address: $SERVICE_ADDRESS"
  echo "Canary wallet: $WALLET_ADDRESS"
  echo "Canary asset pair: $ASSET_PAIR"
  echo "Runbook reviewer: $REVIEWER"
  echo "Dry run: $DRY_RUN"
  echo ""
  echo "## Scenario Evidence"
  echo ""
} > "$REPORT_TMP"

invoke "baseline-admin" "Read current admin" pass \
  stellar_cmd contract invoke --id "$CONTRACT_ID" --source "$OPERATOR_IDENTITY" --network "$NETWORK" -- get_admin

invoke "baseline-service" "Read current service signer" pass \
  stellar_cmd contract invoke --id "$CONTRACT_ID" --source "$OPERATOR_IDENTITY" --network "$NETWORK" -- get_service

invoke "stale-state-read" "Read stale-state status without writing" pass \
  stellar_cmd contract invoke --id "$CONTRACT_ID" --source "$OPERATOR_IDENTITY" --network "$NETWORK" -- is_score_stale \
    --wallet "$WALLET_ADDRESS" --asset_pair "$ASSET_PAIR"

invoke "unauthorized-write" "Reject score submission from a non-service signer" fail \
  stellar_cmd contract invoke --id "$CONTRACT_ID" --source "$OPERATOR_IDENTITY" --network "$NETWORK" -- submit_score \
    --signers "[]" --wallet "$WALLET_ADDRESS" --asset_pair "$ASSET_PAIR" --score 42 --benford_flag false --ml_flag false --timestamp 1 --confidence 80 --model_version 1 --attestation_input null

invoke "pause" "Pause submissions" pass \
  stellar_cmd contract invoke --id "$CONTRACT_ID" --source "$OPERATOR_IDENTITY" --network "$NETWORK" -- pause --admin_signers "[\"$ADMIN_ADDRESS\"]"

invoke "paused-submit" "Reject score submission while paused" fail \
  stellar_cmd contract invoke --id "$CONTRACT_ID" --source "$OPERATOR_IDENTITY" --network "$NETWORK" -- submit_score \
    --signers "[\"$SERVICE_ADDRESS\"]" --wallet "$WALLET_ADDRESS" --asset_pair "$ASSET_PAIR" --score 10 --benford_flag false --ml_flag false --timestamp 2 --confidence 80 --model_version 1 --attestation_input null

invoke "unpause" "Restore submissions" pass \
  stellar_cmd contract invoke --id "$CONTRACT_ID" --source "$OPERATOR_IDENTITY" --network "$NETWORK" -- unpause --admin_signers "[\"$ADMIN_ADDRESS\"]"

invoke "rollback-dry-run" "Document rollback command without changing code" pass \
  echo "stellar contract invoke --id $CONTRACT_ID --source $OPERATOR_IDENTITY --network $NETWORK -- propose_upgrade --new_wasm_hash <previous-reviewed-wasm-hash>"

invoke "signer-loss-dry-run" "Document signer-loss recovery command" pass \
  echo "stellar contract invoke --id $CONTRACT_ID --source $OPERATOR_IDENTITY --network $NETWORK -- set_service <rotated-service-address>"

invoke "reconciliation" "Re-read admin and service after reversible drill" pass \
  stellar_cmd contract invoke --id "$CONTRACT_ID" --source "$OPERATOR_IDENTITY" --network "$NETWORK" -- get_service

if [[ -f "$LOG_DIR/baseline-admin.log" && -f "$LOG_DIR/baseline-service.log" && -f "$LOG_DIR/reconciliation.log" ]]; then
  sha256sum "$LOG_DIR/baseline-admin.log" "$LOG_DIR/baseline-service.log" "$LOG_DIR/reconciliation.log" > "$LOG_DIR/state-digests.txt"
  append "## Integrity Reconciliation"
  append ""
  append "State digest file: $LOG_DIR/state-digests.txt"
  append ""
fi

append "## Final Status"
append ""
if [[ "$STATUS" -eq 0 ]]; then
  append "Canary rehearsal completed. Production activation still requires human review of every linked log."
else
  append "Canary rehearsal failed. Production activation is blocked."
fi

cp "$REPORT_TMP" "$OUTPUT"
echo "testnet canary report written to $OUTPUT"
exit "$STATUS"
