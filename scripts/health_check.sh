#!/usr/bin/env bash
# Read-only health check for a deployed ledgerlens-score contract instance.
#
# Runs only view calls (get_admin, get_service, is_paused, get_version,
# is_service_alive, get_pending_upgrade) — no state-changing invocation is
# ever issued, so this is safe to run against production at any time.
#
# Usage:
#   ./scripts/health_check.sh <network> <contract-id> [source-identity]
#
# Exit codes:
#   0  all checks passed
#   1  one or more checks failed (contract likely unhealthy or unreachable)

set -euo pipefail

NETWORK="${1:?ERROR: network argument is required (e.g. testnet, mainnet)}"
CONTRACT_ID="${2:?ERROR: contract-id argument is required}"
SOURCE_IDENTITY="${3:-health-check}"

FAILURES=0

invoke() {
  soroban contract invoke \
    --id "$CONTRACT_ID" \
    --source "$SOURCE_IDENTITY" \
    --network "$NETWORK" \
    -- \
    "$@"
}

check() {
  local label="$1"
  shift
  local output
  if output=$("$@" 2>&1); then
    echo "PASS  $label: $output"
  else
    echo "FAIL  $label: $output"
    FAILURES=$((FAILURES + 1))
  fi
}

echo "== ledgerlens-score health check: $CONTRACT_ID on $NETWORK =="

check "admin address"    invoke get_admin
check "service address"  invoke get_service
check "pause state"      invoke is_paused
check "schema version"   invoke get_version
check "service liveness" invoke is_service_alive

# A pending upgrade proposal is optional state: its absence (Error::NoPendingUpgrade,
# discriminant #13) is healthy, not a failure — only a genuine RPC/contract error is.
if UPGRADE_OUTPUT=$(invoke get_pending_upgrade 2>&1); then
  echo "INFO  pending upgrade: $UPGRADE_OUTPUT"
else
  case "$UPGRADE_OUTPUT" in
    *NoPendingUpgrade*|*'#13'*)
      echo "PASS  pending upgrade: none"
      ;;
    *)
      echo "FAIL  pending upgrade: $UPGRADE_OUTPUT"
      FAILURES=$((FAILURES + 1))
      ;;
  esac
fi

echo "== $FAILURES check(s) failed =="
[ "$FAILURES" -eq 0 ]
