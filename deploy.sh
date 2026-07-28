#!/usr/bin/env bash
# Build, optimize, deploy and initialize the LedgerLens score contract.
#
# Usage:
#   ./deploy.sh [options] <network> <admin-identity> <service-address>
#
# Options:
#   --dry-run   Print the commands that would be executed without running them.
#   --help      Show this help message.
#
# Arguments:
#   network           soroban CLI network alias (e.g. testnet, futurenet)
#   admin-identity    soroban CLI identity used to deploy and initialize
#   service-address   Stellar public key authorised to call submit_score

set -euo pipefail

DRY_RUN=false
CANARY_KEYS=false
POSITIONAL=()

for arg in "$@"; do
  case "$arg" in
    --dry-run) DRY_RUN=true ;;
    --canary-keys) CANARY_KEYS=true ;;
    --help)
      sed -n '3,20p' "$0"
      exit 0
      ;;
    *) POSITIONAL+=("$arg") ;;
  esac
done
set -- "${POSITIONAL[@]+"${POSITIONAL[@]}"}"

NETWORK="${1:-testnet}"
ADMIN_IDENTITY="${2:-deployer}"
SERVICE_ADDRESS="${3:?ERROR: service-address argument is required}"

WASM_PATH="target/wasm32-unknown-unknown/release/ledgerlens_score.wasm"
OPTIMIZED_WASM_PATH="target/wasm32-unknown-unknown/release/ledgerlens_score.optimized.wasm"

# ── Helpers ───────────────────────────────────────────────────────────────────

run() {
  if [ "$DRY_RUN" = true ]; then
    echo "[dry-run] $*"
  else
    "$@"
  fi
}

log() { echo "==> $*"; }

# ── Validate inputs ───────────────────────────────────────────────────────────

case "$NETWORK" in
  testnet|futurenet|mainnet) ;;
  *)
    echo "WARNING: '$NETWORK' is not a recognised Stellar network alias." >&2
    echo "         Proceeding anyway — ensure the alias is configured in soroban config." >&2
    ;;
esac

if [ "$NETWORK" = "mainnet" ]; then
  echo ""
  echo "  ╔══════════════════════════════════════════════════════╗"
  echo "  ║  MAINNET DEPLOYMENT — this action cannot be undone  ║"
  echo "  ╚══════════════════════════════════════════════════════╝"
  echo ""
  read -rp "  Type 'deploy-mainnet' to confirm: " CONFIRM
  [ "$CONFIRM" = "deploy-mainnet" ] || { echo "Aborted."; exit 1; }
fi

# ── Build ─────────────────────────────────────────────────────────────────────

log "Building contract (wasm32-unknown-unknown, release)"
run cargo build --target wasm32-unknown-unknown --release -p ledgerlens-score

log "Optimizing wasm"
run soroban contract optimize --wasm "$WASM_PATH"

# ── Deploy ────────────────────────────────────────────────────────────────────

log "Deploying to $NETWORK"
if [ "$DRY_RUN" = true ]; then
  CONTRACT_ID="<CONTRACT_ID_PLACEHOLDER>"
  echo "[dry-run] soroban contract deploy --wasm $OPTIMIZED_WASM_PATH --source $ADMIN_IDENTITY --network $NETWORK"
else
  CONTRACT_ID=$(soroban contract deploy \
    --wasm "$OPTIMIZED_WASM_PATH" \
    --source "$ADMIN_IDENTITY" \
    --network "$NETWORK")
fi

log "Deployed contract: $CONTRACT_ID"

# ── Initialize ────────────────────────────────────────────────────────────────

ADMIN_ADDRESS=$(run soroban keys address "$ADMIN_IDENTITY" 2>/dev/null || echo "<ADMIN_ADDRESS>")

log "Initializing contract (admin=$ADMIN_ADDRESS, service=$SERVICE_ADDRESS)"
run soroban contract invoke \
  --id "$CONTRACT_ID" \
  --source "$ADMIN_IDENTITY" \
  --network "$NETWORK" \
  -- \
  initialize \
  --admin "$ADMIN_ADDRESS" \
  --service "$SERVICE_ADDRESS"

# ── Verify ────────────────────────────────────────────────────────────────────

log "Verifying deployment"
if [ "$DRY_RUN" = false ]; then
  STORED_ADMIN=$(soroban contract invoke \
    --id "$CONTRACT_ID" \
    --source "$ADMIN_IDENTITY" \
    --network "$NETWORK" \
    -- \
    get_admin 2>/dev/null || echo "VERIFICATION_FAILED")

  if [ "$STORED_ADMIN" = "VERIFICATION_FAILED" ]; then
    echo "ERROR: post-deployment verification failed — get_admin returned an error." >&2
    exit 1
  fi

  log "Admin verified on-chain: $STORED_ADMIN"

  CONTRACT_VERSION=$(soroban contract invoke \
    --id "$CONTRACT_ID" \
    --source "$ADMIN_IDENTITY" \
    --network "$NETWORK" \
    -- \
    get_version 2>/dev/null || echo "0")

  log "Contract version: $CONTRACT_VERSION"

  # ── Key-rotation canary (testnet only) ────────────────────────────────────
  if [ "$CANARY_KEYS" = true ] && [ "$NETWORK" = "testnet" ]; then
    log "Running key-rotation canary checks (#633)..."

    # Canary: add a service signer and verify
    log "  Testing service signer rotation..."
    ADD_RESULT=$(soroban contract invoke \
      --id "$CONTRACT_ID" \
      --source "$ADMIN_IDENTITY" \
      --network "$NETWORK" \
      -- \
      add_service_signer \
      --admin_signants "[\"$ADMIN_ADDRESS\"]" \
      --signer "$SERVICE_ADDRESS" 2>/dev/null || echo "FAILED")
    if echo "$ADD_RESULT" | grep -q "FAILED"; then
      echo "  ⚠ add_service_signer failed" >&2
    else
      log "  ✅ add_service_signer OK"
    fi

    # Canary: set threshold
    SET_RESULT=$(soroban contract invoke \
      --id "$CONTRACT_ID" \
      --source "$ADMIN_IDENTITY" \
      --network "$NETWORK" \
      -- \
      set_service_threshold \
      --admin_signants "[\"$ADMIN_ADDRESS\"]" \
      --threshold 1 2>/dev/null || echo "FAILED")
    if echo "$SET_RESULT" | grep -q "FAILED"; then
      echo "  ⚠ set_service_threshold failed" >&2
    else
      log "  ✅ set_service_threshold OK"
    fi

    # Canary: remove signer and verify threshold auto-adjust
    REMOVE_RESULT=$(soroban contract invoke \
      --id "$CONTRACT_ID" \
      --source "$ADMIN_IDENTITY" \
      --network "$NETWORK" \
      -- \
      remove_service_signer \
      --admin_signants "[\"$ADMIN_ADDRESS\"]" \
      --signer "$SERVICE_ADDRESS" 2>/dev/null || echo "FAILED")
    if echo "$REMOVE_RESULT" | grep -q "FAILED"; then
      echo "  ⚠ remove_service_signer failed" >&2
    else
      log "  ✅ remove_service_signer (threshold auto-adjust) OK"
    fi

    # Canary: admin signer operations
    log "  Testing admin signer rotation..."
    soroban contract invoke \
      --id "$CONTRACT_ID" \
      --source "$ADMIN_IDENTITY" \
      --network "$NETWORK" \
      -- \
      add_admin_signer \
      --admin_signants "[\"$ADMIN_ADDRESS\"]" \
      --signer "$SERVICE_ADDRESS" 2>/dev/null && log "  ✅ add_admin_signer OK" || echo "  ⚠ add_admin_signer failed" >&2

    soroban contract invoke \
      --id "$CONTRACT_ID" \
      --source "$ADMIN_IDENTITY" \
      --network "$NETWORK" \
      -- \
      remove_admin_signer \
      --admin_signants "[\"$ADMIN_ADDRESS\"]" \
      --signer "$SERVICE_ADDRESS" 2>/dev/null && log "  ✅ remove_admin_signer OK" || echo "  ⚠ remove_admin_signer failed" >&2

    log "Key-rotation canary checks complete."
  fi
fi

# ── Summary ───────────────────────────────────────────────────────────────────

echo ""
echo "  ── Deployment complete ──────────────────────────────────"
echo "  Network:    $NETWORK"
echo "  Contract:   $CONTRACT_ID"
echo "  Admin:      $ADMIN_ADDRESS"
echo "  Service:    $SERVICE_ADDRESS"
echo "  ─────────────────────────────────────────────────────────"
echo ""
echo "  Save CONTRACT_ID=$CONTRACT_ID in your environment and in"
echo "  the api repo's .env before routing submit_score calls."
echo ""
