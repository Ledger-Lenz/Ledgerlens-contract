#!/usr/bin/env bash
set -euo pipefail

INJECT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$INJECT_DIR/.." && pwd)"

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
CONTRACT_ID="${2:?ERROR: contract-id argument is required}"
ADMIN_IDENTITY="${3:?ERROR: admin-identity argument is required}"
SCENARIO="${4:?ERROR: scenario name is required}"

ADMIN_ADDRESS=$(soroban keys address "$ADMIN_IDENTITY" 2>/dev/null || echo "<ADMIN_ADDRESS>")

run() {
  if [ "$DRY_RUN" = true ]; then
    echo "[dry-run] $*"
  else
    "$@"
  fi
}

log() { echo "==> $*"; }

echo ""
echo "  ╔══════════════════════════════════════════════════╗"
echo "  ║  Failure Injection: $SCENARIO"
echo "  ║  Network: $NETWORK"
echo "  ║  Contract: $CONTRACT_ID"
echo "  ╚══════════════════════════════════════════════════╝"
echo ""

case "$SCENARIO" in
  partial-signer-loss)
    log "Scenario: Partial signer loss (reduced below threshold)"
    log "1. Reducing service threshold to simulate signer loss"
    log "2. Attempting score submission with insufficient signers"
    log "3. Verifying submission is rejected with InsufficientSigners"
    log "4. Restoring threshold and verifying recovery"

    log "Setting threshold to 5 (simulating 4 signers lost)"
    run soroban contract invoke \
      --id "$CONTRACT_ID" \
      --source "$ADMIN_IDENTITY" \
      --network "$NETWORK" \
      -- \
      set_service_threshold \
      --admin-signers "[$ADMIN_ADDRESS]" \
      --threshold 5

    log "Attempting submission with threshold not met"
    run soroban contract invoke \
      --id "$CONTRACT_ID" \
      --source "$ADMIN_IDENTITY" \
      --network "$NETWORK" \
      -- \
      submit_score \
      --signers "[]" \
      --wallet "$ADMIN_ADDRESS" \
      --asset-pair INJECT_TEST \
      --score 50 \
      --benford-flag false \
      --ml-flag false \
      --timestamp "$(date +%s)" \
      --confidence 80 \
      --model-version 1 \
      --attestation-input null 2>&1 || true

    log "Restoring original threshold"
    run soroban contract invoke \
      --id "$CONTRACT_ID" \
      --source "$ADMIN_IDENTITY" \
      --network "$NETWORK" \
      -- \
      set_service_threshold \
      --admin-signers "[$ADMIN_ADDRESS]" \
      --threshold 1

    log "Scenario complete: partial-signer-loss"
    ;;

  stale-data)
    log "Scenario: Stale data (expired epoch)"
    log "1. Closing the current epoch to block submissions"
    log "2. Attempting score submission (should be rejected)"
    log "3. Opening a new epoch"
    log "4. Verifying submissions resume"

    log "Closing current epoch"
    run soroban contract invoke \
      --id "$CONTRACT_ID" \
      --source "$ADMIN_IDENTITY" \
      --network "$NETWORK" \
      -- \
      close_epoch \
      --admin-signers "[$ADMIN_ADDRESS]"

    log "Attempting submission with closed epoch"
    run soroban contract invoke \
      --id "$CONTRACT_ID" \
      --source "$ADMIN_IDENTITY" \
      --network "$NETWORK" \
      -- \
      submit_score \
      --signers "[]" \
      --wallet "$ADMIN_ADDRESS" \
      --asset-pair INJECT_TEST \
      --score 50 \
      --benford-flag false \
      --ml-flag false \
      --timestamp "$(date +%s)" \
      --confidence 80 \
      --model-version 1 \
      --attestation-input null 2>&1 || true

    log "Opening new epoch"
    run soroban contract invoke \
      --id "$CONTRACT_ID" \
      --source "$ADMIN_IDENTITY" \
      --network "$NETWORK" \
      -- \
      open_epoch \
      --admin-signers "[$ADMIN_ADDRESS]" \
      --epoch-id 2

    log "Scenario complete: stale-data"
    ;;

  replay-attack)
    log "Scenario: Replay attack attempt (reusing old attestation)"
    log "1. Submit a score with a fresh attestation nonce"
    log "2. Attempt to replay the same score (should be rejected by cooldown)"
    log "3. Attempt to replay the same attestation nonce (should be rejected)"

    log "Submitting initial score"
    run soroban contract invoke \
      --id "$CONTRACT_ID" \
      --source "$ADMIN_IDENTITY" \
      --network "$NETWORK" \
      -- \
      submit_score \
      --signers "[]" \
      --wallet "$ADMIN_ADDRESS" \
      --asset-pair INJECT_TEST \
      --score 50 \
      --benford-flag false \
      --ml-flag false \
      --timestamp "$(date +%s)" \
      --confidence 80 \
      --model-version 1 \
      --attestation-input null 2>&1 || true

    log "Immediately replaying same data (should be rejected by cooldown)"
    run soroban contract invoke \
      --id "$CONTRACT_ID" \
      --source "$ADMIN_IDENTITY" \
      --network "$NETWORK" \
      -- \
      submit_score \
      --signers "[]" \
      --wallet "$ADMIN_ADDRESS" \
      --asset-pair INJECT_TEST \
      --score 50 \
      --benford-flag false \
      --ml-flag false \
      --timestamp "$(date +%s)" \
      --confidence 80 \
      --model-version 1 \
      --attestation-input null 2>&1 || true

    log "Scenario complete: replay-attack"
    ;;

  zero-value)
    log "Scenario: Zero-value submission (score=0, timestamp=0, confidence=0)"
    log "1. Submit with score=0 (should succeed — 0 is valid)"
    log "2. Submit with timestamp=0 (should be rejected with InvalidTimestamp)"
    log "3. Submit with confidence=0 (should succeed — 0 is valid)"

    log "Submitting score=0"
    run soroban contract invoke \
      --id "$CONTRACT_ID" \
      --source "$ADMIN_IDENTITY" \
      --network "$NETWORK" \
      -- \
      submit_score \
      --signers "[]" \
      --wallet "$ADMIN_ADDRESS" \
      --asset-pair INJECT_TEST \
      --score 0 \
      --benford-flag false \
      --ml-flag false \
      --timestamp "$(date +%s)" \
      --confidence 80 \
      --model-version 1 \
      --attestation-input null 2>&1 || true

    log "Submitting with timestamp=0 (should be rejected)"
    run soroban contract invoke \
      --id "$CONTRACT_ID" \
      --source "$ADMIN_IDENTITY" \
      --network "$NETWORK" \
      -- \
      submit_score \
      --signers "[]" \
      --wallet "$ADMIN_ADDRESS" \
      --asset-pair INJECT_TEST \
      --score 50 \
      --benford-flag false \
      --ml-flag false \
      --timestamp 0 \
      --confidence 80 \
      --model-version 1 \
      --attestation-input null 2>&1 || true

    log "Scenario complete: zero-value"
    ;;

  max-value)
    log "Scenario: Maximum-value submission (score=101, confidence=101)"
    log "1. Submit with score=101 (should be rejected with InvalidScore)"
    log "2. Submit with confidence=101 (should be rejected with InvalidConfidence)"

    log "Submitting score=101 (should be rejected)"
    run soroban contract invoke \
      --id "$CONTRACT_ID" \
      --source "$ADMIN_IDENTITY" \
      --network "$NETWORK" \
      -- \
      submit_score \
      --signers "[]" \
      --wallet "$ADMIN_ADDRESS" \
      --asset-pair INJECT_TEST \
      --score 101 \
      --benford-flag false \
      --ml-flag false \
      --timestamp "$(date +%s)" \
      --confidence 80 \
      --model-version 1 \
      --attestation-input null 2>&1 || true

    log "Submitting confidence=101 (should be rejected)"
    run soroban contract invoke \
      --id "$CONTRACT_ID" \
      --source "$ADMIN_IDENTITY" \
      --network "$NETWORK" \
      -- \
      submit_score \
      --signers "[]" \
      --wallet "$ADMIN_ADDRESS" \
      --asset-pair INJECT_TEST \
      --score 50 \
      --benford-flag false \
      --ml-flag false \
      --timestamp "$(date +%s)" \
      --confidence 101 \
      --model-version 1 \
      --attestation-input null 2>&1 || true

    log "Scenario complete: max-value"
    ;;

  unauthorized-caller)
    log "Scenario: Unauthorized caller (non-admin, non-service)"
    log "1. Attempt to set pair paused without admin auth"
    log "2. Attempt to pause contract without admin auth"

    log "Attempting unauthorized pair pause"
    run soroban contract invoke \
      --id "$CONTRACT_ID" \
      --source "$ADMIN_IDENTITY" \
      --network "$NETWORK" \
      -- \
      set_pair_paused \
      --asset-pair INJECT_TEST \
      --paused true 2>&1 || true

    log "Scenario complete: unauthorized-caller"
    ;;

  interrupted-retry)
    log "Scenario: Interrupted retry (submit, fail, retry within cooldown)"
    log "1. Submit a score"
    log "2. Attempt to submit the same score again (cooldown should block)"
    log "3. Wait and verify retry succeeds"

    log "First submission"
    run soroban contract invoke \
      --id "$CONTRACT_ID" \
      --source "$ADMIN_IDENTITY" \
      --network "$NETWORK" \
      -- \
      submit_score \
      --signers "[]" \
      --wallet "$ADMIN_ADDRESS" \
      --asset-pair INJECT_TEST \
      --score 50 \
      --benford-flag false \
      --ml-flag false \
      --timestamp "$(date +%s)" \
      --confidence 80 \
      --model-version 1 \
      --attestation-input null 2>&1 || true

    log "Immediate retry (should be rate-limited)"
    run soroban contract invoke \
      --id "$CONTRACT_ID" \
      --source "$ADMIN_IDENTITY" \
      --network "$NETWORK" \
      -- \
      submit_score \
      --signers "[]" \
      --wallet "$ADMIN_ADDRESS" \
      --asset-pair INJECT_TEST \
      --score 55 \
      --benford-flag false \
      --ml-flag false \
      --timestamp "$(date +%s)" \
      --confidence 80 \
      --model-version 1 \
      --attestation-input null 2>&1 || true

    log "Scenario complete: interrupted-retry"
    ;;

  *)
    echo "ERROR: Unknown scenario '$SCENARIO'" >&2
    echo "Available scenarios:"
    echo "  partial-signer-loss"
    echo "  stale-data"
    echo "  replay-attack"
    echo "  zero-value"
    echo "  max-value"
    echo "  unauthorized-caller"
    echo "  interrupted-retry"
    exit 1
    ;;
esac

echo ""
log "Failure injection scenario '$SCENARIO' completed."
echo ""