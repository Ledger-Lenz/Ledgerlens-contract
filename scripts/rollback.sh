#!/usr/bin/env bash
set -euo pipefail

ROLLBACK_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$ROLLBACK_DIR/.." && pwd)"

DRY_RUN=false
POSITIONAL=()

for arg in "$@"; do
  case "$arg" in
    --dry-run) DRY_RUN=true ;;
    --help)
      sed -n '3,20p' "$0"
      exit 0
      ;;
    *) POSITIONAL+=("$arg") ;;
  esac
done
set -- "${POSITIONAL[@]+"${POSITIONAL[@]}"}"

NETWORK="${1:?ERROR: network argument is required (testnet|futurenet|mainnet)}"
ADMIN_IDENTITY="${2:?ERROR: admin-identity argument is required}"
TARGET_CONTRACT_ID="${3:?ERROR: target-contract-id argument is required}"
PREVIOUS_WASM_PATH="${4:?ERROR: previous-wasm-path argument is required}"

run() {
  if [ "$DRY_RUN" = true ]; then
    echo "[dry-run] $*"
  else
    "$@"
  fi
}

log() { echo "==> $*"; }

# ── Safety check ──────────────────────────────────────────────
if [ "$NETWORK" = "mainnet" ]; then
  echo ""
  echo "  ╔══════════════════════════════════════════════════════╗"
  echo "  ║  MAINNET ROLLBACK — this action cannot be undone    ║"
  echo "  ╚══════════════════════════════════════════════════════╝"
  echo ""
  read -rp "  Type 'rollback-mainnet' to confirm: " CONFIRM
  [ "$CONFIRM" = "rollback-mainnet" ] || { echo "Aborted."; exit 1; }
fi

# ── Validate previous WASM ────────────────────────────────────
if [ ! -f "$PREVIOUS_WASM_PATH" ]; then
  echo "ERROR: Previous WASM not found at $PREVIOUS_WASM_PATH" >&2
  exit 1
fi

PREVIOUS_WASM_HASH=$(sha256sum "$PREVIOUS_WASM_PATH" | awk '{print $1}')
log "Previous WASM SHA-256: $PREVIOUS_WASM_HASH"

# ── Step 1: Propose the previous WASM ─────────────────────────
log "Step 1: Proposing rollback upgrade (previous WASM hash)"
if [ "$DRY_RUN" = true ]; then
  ROLLBACK_PROPOSAL_ID="<PROPOSAL_ID_PLACEHOLDER>"
  echo "[dry-run] soroban contract invoke --id $TARGET_CONTRACT_ID --source $ADMIN_IDENTITY --network $NETWORK -- propose_upgrade --admin-signers '[$ADMIN_IDENTITY]' --new-wasm-hash $PREVIOUS_WASM_HASH"
else
  PROPOSAL_OUTPUT=$(soroban contract invoke \
    --id "$TARGET_CONTRACT_ID" \
    --source "$ADMIN_IDENTITY" \
    --network "$NETWORK" \
    -- \
    propose_upgrade \
    --admin-signers "$(soroban keys address "$ADMIN_IDENTITY" 2>/dev/null || echo '<ADMIN>')" \
    --new-wasm-hash "$PREVIOUS_WASM_HASH" 2>&1)
  log "Proposal output: $PROPOSAL_OUTPUT"
fi

# ── Step 2: Monitor the delay ─────────────────────────────────
log "Step 2: Waiting for upgrade delay to elapse"
log "   Check pending upgrade status with:"
log "   soroban contract invoke --id $TARGET_CONTRACT_ID --source $ADMIN_IDENTITY --network $NETWORK -- get_pending_upgrade"
log "   Execute only after executable_after has passed."

# ── Step 3: Execute the rollback ──────────────────────────────
read -rp "  Execute the rollback upgrade now? (yes/no): " EXECUTE_CONFIRM
[ "$EXECUTE_CONFIRM" = "yes" ] || { log "Rollback aborted by operator."; exit 0; }

log "Step 3: Executing rollback upgrade"
if [ "$DRY_RUN" = true ]; then
  echo "[dry-run] soroban contract invoke --id $TARGET_CONTRACT_ID --source $ADMIN_IDENTITY --network $NETWORK -- execute_upgrade --admin-signers '[$ADMIN_IDENTITY]'"
else
  EXECUTE_OUTPUT=$(soroban contract invoke \
    --id "$TARGET_CONTRACT_ID" \
    --source "$ADMIN_IDENTITY" \
    --network "$NETWORK" \
    -- \
    execute_upgrade \
    --admin-signers "$(soroban keys address "$ADMIN_IDENTITY" 2>/dev/null || echo '<ADMIN>')" 2>&1)
  log "Execute output: $EXECUTE_OUTPUT"
fi

# ── Step 4: Verify rollback ───────────────────────────────────
log "Step 4: Verifying rollback"
if [ "$DRY_RUN" = false ]; then
  VERIFY_VERSION=$(soroban contract invoke \
    --id "$TARGET_CONTRACT_ID" \
    --source "$ADMIN_IDENTITY" \
    --network "$NETWORK" \
    -- \
    get_version 2>/dev/null || echo "VERIFY_FAILED")

  if [ "$VERIFY_VERSION" = "VERIFY_FAILED" ]; then
    log "ERROR: Rollback verification failed — get_version returned an error."
    exit 1
  fi

  log "Rollback contract version: $VERIFY_VERSION"

  VERIFY_ADMIN=$(soroban contract invoke \
    --id "$TARGET_CONTRACT_ID" \
    --source "$ADMIN_IDENTITY" \
    --network "$NETWORK" \
    -- \
    get_admin 2>/dev/null || echo "VERIFY_FAILED")

  if [ "$VERIFY_ADMIN" = "VERIFY_FAILED" ]; then
    log "ERROR: Rollback verification failed — get_admin returned an error."
    exit 1
  fi

  log "Rollback admin verified: $VERIFY_ADMIN"
fi

# ── Step 5: Replay verification ───────────────────────────────
log "Step 5: Replaying test data against rolled-back contract"
log "   Run: cargo test -p replay -- --nocapture"
log "   Run: cargo run -p replay --manifest-path tools/replay/Cargo.toml"

# ── Summary ────────────────────────────────────────────────────
echo ""
log "── Rollback complete ──"
log "Network:              $NETWORK"
log "Contract:             $TARGET_CONTRACT_ID"
log "Previous WASM hash:   $PREVIOUS_WASM_HASH"
log "────────────────────────────────────────"
echo ""
log "IMPORTANT: Re-run full verification suite against the rolled-back contract."
log "Notify all integrators that the contract has been rolled back."
echo ""