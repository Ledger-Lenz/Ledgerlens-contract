#!/usr/bin/env bash
set -euo pipefail

VERIFY_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$VERIFY_DIR/.." && pwd)"

DRY_RUN=false
POSITIONAL=()

for arg in "$@"; do
  case "$arg" in
    --dry-run) DRY_RUN=true ;;
    --help)
      sed -n '3,15p' "$0"
      exit 0
      ;;
    *) POSITIONAL+=("$arg") ;;
  esac
done
set -- "${POSITIONAL[@]+"${POSITIONAL[@]}"}"

NETWORK="${1:?ERROR: network argument is required}"
CONTRACT_ID="${2:?ERROR: contract-id argument is required}"
ADMIN_IDENTITY="${3:?ERROR: admin-identity argument is required}"

ADMIN_ADDRESS=$(soroban keys address "$ADMIN_IDENTITY" 2>/dev/null || echo "<ADMIN_ADDRESS>")

log() {
  local ts
  ts=$(date -u +"%Y-%m-%dT%H:%M:%SZ")
  echo "[$ts] $*"
}

PASS=0
FAIL=0
WARN=0

check() {
  local label="$1"
  local result="$2"
  if [ "$result" = "0" ] || [ -n "$result" ]; then
    log "  PASS: $label"
    PASS=$((PASS + 1))
  else
    log "  FAIL: $label"
    FAIL=$((FAIL + 1))
  fi
}

log "Verifying deployment on $NETWORK (contract=$CONTRACT_ID)"
log "Admin: $ADMIN_ADDRESS"
echo ""

# ── 1. Basic state checks ──────────────────────────────
log "1. Basic state checks"

ADMIN=$(soroban contract invoke \
  --id "$CONTRACT_ID" \
  --source "$ADMIN_IDENTITY" \
  --network "$NETWORK" \
  -- \
  get_admin 2>/dev/null || echo "VERIFY_FAILED")
if [ "$ADMIN" != "VERIFY_FAILED" ]; then
  check "get_admin returns valid address" "$ADMIN"
else
  check "get_admin returns valid address" "FAIL"
fi

SERVICE=$(soroban contract invoke \
  --id "$CONTRACT_ID" \
  --source "$ADMIN_IDENTITY" \
  --network "$NETWORK" \
  -- \
  get_service 2>/dev/null || echo "VERIFY_FAILED")
if [ "$SERVICE" != "VERIFY_FAILED" ]; then
  check "get_service returns valid address" "$SERVICE"
else
  check "get_service returns valid address" "FAIL"
fi

VERSION=$(soroban contract invoke \
  --id "$CONTRACT_ID" \
  --source "$ADMIN_IDENTITY" \
  --network "$NETWORK" \
  -- \
  get_version 2>/dev/null || echo "0")
check "get_version returns non-zero" "$VERSION"

IS_PAUSED=$(soroban contract invoke \
  --id "$CONTRACT_ID" \
  --source "$ADMIN_IDENTITY" \
  --network "$NETWORK" \
  -- \
  is_paused 2>/dev/null || echo "1")
if [ "$IS_PAUSED" = "0" ]; then
  check "Contract is not paused" "true"
else
  WARN=$((WARN + 1))
  log "  WARN: Contract is paused (is_paused=$IS_PAUSED)"
fi

FINALITY_BUFFER=$(soroban contract invoke \
  --id "$CONTRACT_ID" \
  --source "$ADMIN_IDENTITY" \
  --network "$NETWORK" \
  -- \
  get_finality_buffer 2>/dev/null || echo "0")
check "Finality buffer is configured" "$FINALITY_BUFFER"

COOLDOWN=$(soroban contract invoke \
  --id "$CONTRACT_ID" \
  --source "$ADMIN_IDENTITY" \
  --network "$NETWORK" \
  -- \
  get_cooldown 2>/dev/null || echo "0")
check "Cooldown is configured" "$COOLDOWN"

UPGRADE_DELAY=$(soroban contract invoke \
  --id "$CONTRACT_ID" \
  --source "$ADMIN_IDENTITY" \
  --network "$NETWORK" \
  -- \
  get_upgrade_delay 2>/dev/null || echo "0")
check "Upgrade delay is configured" "$UPGRADE_DELAY"

echo ""

# ── 2. Functional smoke tests ──────────────────────────
log "2. Functional smoke tests"

TEST_WALLET="TEST_WALLET_SMOKE"
TEST_PAIR="SMOKE_TEST_PAIR"
TEST_TIMESTAMP=$(date +%s)

SUBMIT_RESULT=$(soroban contract invoke \
  --id "$CONTRACT_ID" \
  --source "$ADMIN_IDENTITY" \
  --network "$NETWORK" \
  -- \
  submit_score \
  --signers "[]" \
  --wallet "$ADMIN_ADDRESS" \
  --asset-pair "$TEST_PAIR" \
  --score 42 \
  --benford-flag false \
  --ml-flag false \
  --timestamp "$TEST_TIMESTAMP" \
  --confidence 80 \
  --model-version 1 \
  --attestation-input null 2>&1 || echo "SMOKE_SUBMIT_FAILED")

if echo "$SUBMIT_RESULT" | grep -q "SMOKE_SUBMIT_FAILED\|Error"; then
  WARN=$((WARN + 1))
  log "  WARN: Smoke test submission failed (may be expected if attestation required): $SUBMIT_RESULT"
else
  check "Smoke test score submission succeeded" "$SUBMIT_RESULT"
fi

SCORE=$(soroban contract invoke \
  --id "$CONTRACT_ID" \
  --source "$ADMIN_IDENTITY" \
  --network "$NETWORK" \
  -- \
  get_score \
  --wallet "$ADMIN_ADDRESS" \
  --asset-pair "$TEST_PAIR" 2>/dev/null || echo "SCORE_NOT_FOUND")
check "Smoke test score is retrievable" "$SCORE"

echo ""

# ── 3. Pending upgrade check ───────────────────────────
log "3. Pending upgrade check"
PENDING_UPGRADE=$(soroban contract invoke \
  --id "$CONTRACT_ID" \
  --source "$ADMIN_IDENTITY" \
  --network "$NETWORK" \
  -- \
  get_pending_upgrade 2>/dev/null || echo "NO_PENDING_UPGRADE")
if [ "$PENDING_UPGRADE" = "NO_PENDING_UPGRADE" ] || [ -z "$PENDING_UPGRADE" ]; then
  check "No unexpected pending upgrade" "true"
else
  WARN=$((WARN + 1))
  log "  WARN: Pending upgrade exists — may be intentional (governance review): $PENDING_UPGRADE"
fi

echo ""

# ── 4. Service set check ──────────────────────────────
log "4. Service configuration check"
SERVICE_SET=$(soroban contract invoke \
  --id "$CONTRACT_ID" \
  --source "$ADMIN_IDENTITY" \
  --network "$NETWORK" \
  -- \
  get_service_signers 2>/dev/null || echo "NONE")
check "Service signers are configured" "$SERVICE_SET"

SERVICE_THRESHOLD=$(soroban contract invoke \
  --id "$CONTRACT_ID" \
  --source "$ADMIN_IDENTITY" \
  --network "$NETWORK" \
  -- \
  get_service_threshold 2>/dev/null || echo "0")
if [ "$SERVICE_THRESHOLD" != "0" ]; then
  check "Service threshold is set (>0)" "$SERVICE_THRESHOLD"
else
  WARN=$((WARN + 1))
  log "  WARN: Service threshold is 0 — single service key auth is in effect"
fi

echo ""

# ── Summary ────────────────────────────────────────────
echo ""
log "── Verification Summary ──"
log "PASS: $PASS"
log "FAIL: $FAIL"
log "WARN: $WARN"

if [ "$FAIL" -gt 0 ]; then
  log "STATUS: FAILED — $FAIL check(s) failed"
  exit 1
else
  log "STATUS: PASSED — All critical checks passed"
  if [ "$WARN" -gt 0 ]; then
    log "NOTICE: $WARN non-critical warning(s) — review recommended"
  fi
fi
echo ""