#!/usr/bin/env bash
set -euo pipefail

CANARY_DEPLOY_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$CANARY_DEPLOY_DIR/.." && pwd)"

DRY_RUN=false
POSITIONAL=()

for arg in "$@"; do
  case "$arg" in
    --dry-run) DRY_RUN=true ;;
    --help)
      sed -n '3,25p' "$0"
      exit 0
      ;;
    *) POSITIONAL+=("$arg") ;;
  esac
done
set -- "${POSITIONAL[@]+"${POSITIONAL[@]}"}"

NETWORK="${1:?ERROR: network argument is required (testnet|futurenet)}"
ADMIN_IDENTITY="${2:?ERROR: admin-identity argument is required}"
SERVICE_ADDRESS="${3:?ERROR: service-address argument is required}"

WASM_PATH="$PROJECT_ROOT/target/wasm32-unknown-unknown/release/ledgerlens_score.wasm"
OPTIMIZED_WASM_PATH="$PROJECT_ROOT/target/wasm32-unknown-unknown/release/ledgerlens_score.optimized.wasm"

CONTRACT_ID_FILE="$PROJECT_ROOT/.canary-$(echo "$NETWORK" | tr '[:lower:]' '[:upper:]').cid"
LOG_FILE="$PROJECT_ROOT/.canary-$(echo "$NETWORK" | tr '[:lower:]' '[:upper:]').log"

run() {
  if [ "$DRY_RUN" = true ]; then
    echo "[dry-run] $*"
  else
    "$@"
  fi
}

log() {
  local ts
  ts=$(date -u +"%Y-%m-%dT%H:%M:%SZ")
  echo "[$ts] $*" | tee -a "$LOG_FILE"
}

# ── Validate network ──────────────────────────────────────────────────
case "$NETWORK" in
  testnet|futurenet) ;;
  *)
    echo "ERROR: canary deployment only supported on testnet and futurenet, not '$NETWORK'." >&2
    exit 1
    ;;
esac

# ── Build ─────────────────────────────────────────────────────────────
log "Building contract for canary deployment (network=$NETWORK)"
run cargo build --target wasm32-unknown-unknown --release -p ledgerlens-score --locked

log "Optimizing WASM"
run soroban contract optimize --wasm "$WASM_PATH"

# ── Verify WASM ───────────────────────────────────────────────────────
log "Computing WASM hash for canary verification"
CANARY_WASM_HASH=$(sha256sum "$OPTIMIZED_WASM_PATH" | awk '{print $1}')
log "Canary WASM SHA-256: $CANARY_WASM_HASH"

# ── Deploy canary ─────────────────────────────────────────────────────
log "Deploying canary to $NETWORK"
if [ "$DRY_RUN" = true ]; then
  CANARY_CONTRACT_ID="<CANARY_CONTRACT_ID_PLACEHOLDER>"
  echo "[dry-run] soroban contract deploy --wasm $OPTIMIZED_WASM_PATH --source $ADMIN_IDENTITY --network $NETWORK"
else
  CANARY_CONTRACT_ID=$(soroban contract deploy \
    --wasm "$OPTIMIZED_WASM_PATH" \
    --source "$ADMIN_IDENTITY" \
    --network "$NETWORK")
fi

log "Canary contract deployed: $CANARY_CONTRACT_ID"
echo "$CANARY_CONTRACT_ID" > "$CONTRACT_ID_FILE"

# ── Initialize canary ─────────────────────────────────────────────────
ADMIN_ADDRESS=$(run soroban keys address "$ADMIN_IDENTITY" 2>/dev/null || echo "<ADMIN_ADDRESS>")

log "Initializing canary contract (admin=$ADMIN_ADDRESS, service=$SERVICE_ADDRESS)"
run soroban contract invoke \
  --id "$CANARY_CONTRACT_ID" \
  --source "$ADMIN_IDENTITY" \
  --network "$NETWORK" \
  -- \
  initialize \
  --admin "$ADMIN_ADDRESS" \
  --service "$SERVICE_ADDRESS"

# ── Canary verification ──────────────────────────────────────────────
log "Running canary verification suite"

# Verify basic state
CANARY_ADMIN=$(soroban contract invoke \
  --id "$CANARY_CONTRACT_ID" \
  --source "$ADMIN_IDENTITY" \
  --network "$NETWORK" \
  -- \
  get_admin 2>/dev/null || echo "VERIFICATION_FAILED")

if [ "$CANARY_ADMIN" = "VERIFICATION_FAILED" ]; then
  log "ERROR: Canary verification failed — get_admin returned an error."
  exit 1
fi
log "Canary admin verified: $CANARY_ADMIN"

CANARY_VERSION=$(soroban contract invoke \
  --id "$CANARY_CONTRACT_ID" \
  --source "$ADMIN_IDENTITY" \
  --network "$NETWORK" \
  -- \
  get_version 2>/dev/null || echo "0")
log "Canary contract version: $CANARY_VERSION"

# Submit a canary test score
log "Submitting canary test score"
CANARY_TEST_RESULT=$(soroban contract invoke \
  --id "$CANARY_CONTRACT_ID" \
  --source "$ADMIN_IDENTITY" \
  --network "$NETWORK" \
  -- \
  submit_score \
  --signers "[]" \
  --wallet "$ADMIN_ADDRESS" \
  --asset-pair CANARY_TEST \
  --score 50 \
  --benford-flag false \
  --ml-flag false \
  --timestamp "$(date +%s)" \
  --confidence 80 \
  --model-version 1 \
  --attestation-input null 2>&1 || echo "CANARY_SUBMIT_FAILED")

if echo "$CANARY_TEST_RESULT" | grep -q "CANARY_SUBMIT_FAILED\|Error"; then
  log "WARNING: Canary test score submission reported: $CANARY_TEST_RESULT"
  log "This may be expected if a service key or attestation is required."
else
  log "Canary test score submitted successfully"
fi

# Verify the canary score
log "Verifying canary score"
CANARY_SCORE=$(soroban contract invoke \
  --id "$CANARY_CONTRACT_ID" \
  --source "$ADMIN_IDENTITY" \
  --network "$NETWORK" \
  -- \
  get_score \
  --wallet "$ADMIN_ADDRESS" \
  --asset-pair CANARY_TEST 2>/dev/null || echo "SCORE_NOT_FOUND")

log "Canary test score result: $CANARY_SCORE"

# ── Canary summary ────────────────────────────────────────────────────
echo ""
log "── Canary deployment complete ──"
log "Network:    $NETWORK"
log "Canary ID:  $CANARY_CONTRACT_ID"
log "Admin:      $ADMIN_ADDRESS"
log "Service:    $SERVICE_ADDRESS"
log "WASM Hash:  $CANARY_WASM_HASH"
log "Log file:   $LOG_FILE"
log "────────────────────────────────"
echo ""
log "Canary deployment ID saved to: $CONTRACT_ID_FILE"
log "Review the canary results before proceeding to full production deployment."
log "To promote: ./deploy.sh $NETWORK $ADMIN_IDENTITY $SERVICE_ADDRESS"
echo ""