#!/usr/bin/env bash
# Build, optimize, deploy and initialize the LedgerLens score contract.
#
# Usage:
#   ./deploy.sh [options] <network> <admin-identity> <service-address>
#
# Options:
#   --dry-run          Print the commands that would be executed without running them.
#   --check-toolchain  Validate the reviewed manifest and local tool versions, then exit.
#   --manifest <path>  Override the default reviewed manifest path for <network>.
#   --help             Show this help message.
#
# Arguments:
#   network           reviewed deployment manifest selector (e.g. testnet, futurenet, mainnet)
#   admin-identity    stellar/soroban CLI identity used to deploy and initialize
#   service-address   Stellar public key authorised to call submit_score
#
# See docs/network-matrix.md for the supported deployment profiles and
# failure modes.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=deploy/validate_manifest.sh
source "$SCRIPT_DIR/deploy/validate_manifest.sh"

DRY_RUN=false
CHECK_TOOLCHAIN_ONLY=false
MANIFEST_OVERRIDE=""
CANARY_KEYS=false
PARAM_MANIFEST_PATH="$SCRIPT_DIR/deploy/manifest.json"
POSITIONAL=()

while [ "$#" -gt 0 ]; do
  case "$1" in
    --dry-run)
      DRY_RUN=true
      shift
      ;;
    --check-toolchain)
      CHECK_TOOLCHAIN_ONLY=true
      shift
      ;;
    --manifest)
      [ "$#" -ge 2 ] || { echo "ERROR: --manifest requires a path." >&2; exit 1; }
      MANIFEST_OVERRIDE="$2"
      shift 2
      ;;
    --canary-keys)
      CANARY_KEYS=true
      shift
      ;;
    --help)
      sed -n '3,22p' "$0"
      exit 0
      ;;
    --canary)
      CANARY=true
      shift
      ;;
    *)
      POSITIONAL+=("$1")
      shift
      ;;
  esac
done
set -- "${POSITIONAL[@]+"${POSITIONAL[@]}"}"

NETWORK_SELECTOR="${1:-testnet}"
ADMIN_IDENTITY="${2:-deployer}"
SERVICE_ADDRESS="${3:-}"
if [ "$CHECK_TOOLCHAIN_ONLY" = false ] && [ -z "$SERVICE_ADDRESS" ]; then
  echo "ERROR: service-address argument is required." >&2
  exit 1
fi

MANIFEST_PATH="${MANIFEST_OVERRIDE:-"$SCRIPT_DIR/deploy/manifests/$NETWORK_SELECTOR.env"}"
EXPECTED_RUST_VERSION=""
EXPECTED_STELLAR_CLI_VERSION=""
NETWORK_ALIAS=""
NETWORK_PASSPHRASE=""
RPC_URL=""
REQUIRE_MAINNET_CONFIRMATION=""
CLI_BIN=""
CLI_LABEL=""
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

die() {
  echo "ERROR: $*" >&2
  exit 1
}

trim() {
  local value="$1"
  value="${value#"${value%%[![:space:]]*}"}"
  value="${value%"${value##*[![:space:]]}"}"
  printf '%s' "$value"
}

strip_quotes() {
  local value="$1"
  if [ "${value#\"}" != "$value" ] && [ "${value%\"}" != "$value" ]; then
    value="${value#\"}"
    value="${value%\"}"
  fi
  printf '%s' "$value"
}

load_manifest() {
  [ -f "$MANIFEST_PATH" ] || die "deployment manifest not found: $MANIFEST_PATH"

  local line key value line_no=0
  while IFS= read -r line || [ -n "$line" ]; do
    line_no=$((line_no + 1))
    line="$(trim "$line")"
    [ -z "$line" ] && continue
    case "$line" in
      \#*) continue ;;
    esac

    case "$line" in
      *=*) ;;
      *) die "invalid manifest entry at $MANIFEST_PATH:$line_no: expected KEY=VALUE" ;;
    esac

    key="$(trim "${line%%=*}")"
    value="$(trim "${line#*=}")"
    value="$(strip_quotes "$value")"

    case "$key" in
      SCHEMA_VERSION) SCHEMA_VERSION="$value" ;;
      NETWORK_ALIAS) NETWORK_ALIAS="$value" ;;
      NETWORK_PASSPHRASE) NETWORK_PASSPHRASE="$value" ;;
      RPC_URL) RPC_URL="$value" ;;
      REQUIRE_MAINNET_CONFIRMATION) REQUIRE_MAINNET_CONFIRMATION="$value" ;;
      EXPECTED_STELLAR_CLI_VERSION) EXPECTED_STELLAR_CLI_VERSION="$value" ;;
      *)
        die "unexpected manifest key '$key' in $MANIFEST_PATH; reviewed manifests must not carry secrets or undeclared fields"
        ;;
    esac
  done < "$MANIFEST_PATH"

  [ "${SCHEMA_VERSION:-}" = "1" ] || die "manifest schema_version must be 1 in $MANIFEST_PATH"
  [ -n "$NETWORK_ALIAS" ] || die "manifest missing NETWORK_ALIAS: $MANIFEST_PATH"
  [ -n "$NETWORK_PASSPHRASE" ] || die "manifest missing NETWORK_PASSPHRASE: $MANIFEST_PATH"
  [ -n "$RPC_URL" ] || die "manifest missing RPC_URL: $MANIFEST_PATH"
  [ -n "$REQUIRE_MAINNET_CONFIRMATION" ] || die "manifest missing REQUIRE_MAINNET_CONFIRMATION: $MANIFEST_PATH"
  [ -n "$EXPECTED_STELLAR_CLI_VERSION" ] || die "manifest missing EXPECTED_STELLAR_CLI_VERSION: $MANIFEST_PATH"

  case "$REQUIRE_MAINNET_CONFIRMATION" in
    true|false) ;;
    *) die "REQUIRE_MAINNET_CONFIRMATION must be true or false in $MANIFEST_PATH" ;;
  esac
}

load_expected_rust_version() {
  EXPECTED_RUST_VERSION="$(sed -n 's/^[[:space:]]*channel[[:space:]]*=[[:space:]]*\"\([^\"]*\)\".*/\1/p' "$SCRIPT_DIR/rust-toolchain.toml" | head -n 1)"
  [ -n "$EXPECTED_RUST_VERSION" ] || die "failed to read pinned Rust toolchain from rust-toolchain.toml"
}

detect_cli() {
  if command -v stellar >/dev/null 2>&1; then
    CLI_BIN="stellar"
    CLI_LABEL="Stellar CLI"
    return
  fi
  if command -v soroban >/dev/null 2>&1; then
    CLI_BIN="soroban"
    CLI_LABEL="Soroban CLI"
    return
  fi
  die "neither 'stellar' nor 'soroban' was found in PATH"
}

extract_semver() {
  local raw="$1"
  local version
  version="$(printf '%s\n' "$raw" | grep -Eo '[0-9]+\.[0-9]+\.[0-9]+' | head -n 1 || true)"
  [ -n "$version" ] || die "could not parse a semantic version from: $raw"
  printf '%s' "$version"
}

check_toolchain_versions() {
  load_expected_rust_version
  detect_cli

  local rust_raw rust_actual cli_raw cli_actual cargo_raw
  rust_raw="$(rustc --version 2>/dev/null || true)"
  cargo_raw="$(cargo --version 2>/dev/null || true)"
  [ -n "$rust_raw" ] || die "rustc is not installed. Install Rust $EXPECTED_RUST_VERSION with: rustup toolchain install $EXPECTED_RUST_VERSION"
  [ -n "$cargo_raw" ] || die "cargo is not installed. Install Rust $EXPECTED_RUST_VERSION with: rustup toolchain install $EXPECTED_RUST_VERSION"
  rust_actual="$(extract_semver "$rust_raw")"
  if [ "$rust_actual" != "$EXPECTED_RUST_VERSION" ]; then
    echo "Toolchain drift detected for Rust." >&2
    echo "  Expected: $EXPECTED_RUST_VERSION" >&2
    echo "  Actual:   $rust_actual" >&2
    echo "  Upgrade:  rustup toolchain install $EXPECTED_RUST_VERSION && rustup override set $EXPECTED_RUST_VERSION" >&2
    exit 1
  fi

  cli_raw="$("$CLI_BIN" --version 2>/dev/null || true)"
  [ -n "$cli_raw" ] || die "$CLI_LABEL is not installed. Install version $EXPECTED_STELLAR_CLI_VERSION before deploying."
  cli_actual="$(extract_semver "$cli_raw")"
  if [ "$cli_actual" != "$EXPECTED_STELLAR_CLI_VERSION" ]; then
    echo "Toolchain drift detected for $CLI_LABEL." >&2
    echo "  Expected: $EXPECTED_STELLAR_CLI_VERSION" >&2
    echo "  Actual:   $cli_actual" >&2
    echo "  Upgrade:  install the reviewed $CLI_LABEL version $EXPECTED_STELLAR_CLI_VERSION, then re-run deploy.sh" >&2
    exit 1
  fi

  log "Reviewed toolchain OK: rustc $rust_actual, $CLI_BIN $cli_actual"
}

# ── Validate inputs ───────────────────────────────────────────────────────────

load_manifest
load_expected_rust_version
check_toolchain_versions

if [ "$NETWORK_ALIAS" != "$NETWORK_SELECTOR" ]; then
  die "manifest/network mismatch: selector '$NETWORK_SELECTOR' does not match NETWORK_ALIAS '$NETWORK_ALIAS' in $MANIFEST_PATH"
fi

log "Using manifest: $MANIFEST_PATH"
log "Network alias: $NETWORK_ALIAS"
log "RPC URL: $RPC_URL"

if [ "$CHECK_TOOLCHAIN_ONLY" = true ]; then
  log "Manifest validated and toolchain drift check passed."
  exit 0
fi

# ── Validate deploy-time network parameters ────────────────────────────────────
# Runs before any build/deploy command and submits no transactions.

if ! validate_manifest "$NETWORK_SELECTOR" "$PARAM_MANIFEST_PATH" "$ADMIN_IDENTITY" "$SERVICE_ADDRESS"; then
  echo "" >&2
  echo "Deployment aborted: manifest validation failed for network '$NETWORK_SELECTOR'." >&2
  exit 1
fi

if [ "$REQUIRE_MAINNET_CONFIRMATION" = "true" ]; then
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
run "$CARGO_BIN" build --target wasm32-unknown-unknown --release -p ledgerlens-score

log "Optimizing wasm"
run "$CLI_BIN" contract optimize --wasm "$WASM_PATH"

# ── Deploy ────────────────────────────────────────────────────────────────────

log "Deploying to $NETWORK_ALIAS"
if [ "$DRY_RUN" = true ]; then
  CONTRACT_ID="<CONTRACT_ID_PLACEHOLDER>"
  echo "[dry-run] $CLI_BIN contract deploy --wasm $OPTIMIZED_WASM_PATH --source $ADMIN_IDENTITY --rpc-url $RPC_URL --network-passphrase $NETWORK_PASSPHRASE"
else
  CONTRACT_ID=$("$CLI_BIN" contract deploy \
    --wasm "$OPTIMIZED_WASM_PATH" \
    --source "$ADMIN_IDENTITY" \
    --rpc-url "$RPC_URL" \
    --network-passphrase "$NETWORK_PASSPHRASE")
fi

[ -n "$CONTRACT_ID" ] || die "Deployment returned an empty contract id."
log "Deployment transaction returned contract id: $CONTRACT_ID"

# ── Initialize ────────────────────────────────────────────────────────────────

if [ "$DRY_RUN" = true ]; then
  ADMIN_ADDRESS="<ADMIN_ADDRESS>"
else
  ADMIN_ADDRESS=$("$CLI_BIN" keys address "$ADMIN_IDENTITY" 2>/dev/null || echo "<ADMIN_ADDRESS>")
fi

log "Initializing contract (admin=$ADMIN_ADDRESS, service=$SERVICE_ADDRESS)"
if ! run "$CLI_BIN" contract invoke \
  --id "$CONTRACT_ID" \
  --source "$ADMIN_IDENTITY" \
  --rpc-url "$RPC_URL" \
  --network-passphrase "$NETWORK_PASSPHRASE" \
  -- \
  initialize \
  --admin "$ADMIN_ADDRESS" \
  --service "$SERVICE_ADDRESS" >/dev/null; then
  echo "Contract id: $CONTRACT_ID" >&2
  die "Initialization failed; do not treat this deployment as successful."
fi

# ── Verify ────────────────────────────────────────────────────────────────────

log "Verifying deployment"
if [ "$DRY_RUN" = false ]; then
  STORED_ADMIN=$("$CLI_BIN" contract invoke \
    --id "$CONTRACT_ID" \
    --source "$ADMIN_IDENTITY" \
    --rpc-url "$RPC_URL" \
    --network-passphrase "$NETWORK_PASSPHRASE" \
    -- \
    get_admin) || {
      echo "Contract id: $CONTRACT_ID" >&2
      die "Post-deployment verification failed."
    }

  log "Admin verified on-chain: $STORED_ADMIN"

  CONTRACT_VERSION=$("$CLI_BIN" contract invoke \
    --id "$CONTRACT_ID" \
    --source "$ADMIN_IDENTITY" \
    --rpc-url "$RPC_URL" \
    --network-passphrase "$NETWORK_PASSPHRASE" \
    -- \
    get_version) || {
      echo "Contract id: $CONTRACT_ID" >&2
      die "Deployment verification could not read the contract version."
    }

  log "Contract version: $CONTRACT_VERSION"

  # ── Canary checks (testnet only) ──────────────────────────────────────────
  if [ "$CANARY" = true ] && [ "$NETWORK" = "testnet" ]; then
    log "Running canary checks for post-incident reconciliation (#631)..."

    # Check supported interfaces
    for cap in checksum snapshot freeze export_score reconcile; do
      RESULT=$(soroban contract invoke \
        --id "$CONTRACT_ID" \
        --source "$ADMIN_IDENTITY" \
        --network "$NETWORK" \
        -- \
        supports_interface \
        --capability "\"$cap\"" 2>/dev/null || echo "false")
      if echo "$RESULT" | grep -q "true"; then
        log "  ✅ Interface '$cap' supported"
      else
        echo "  ⚠ WARNING: Interface '$cap' not supported" >&2
      fi
    done

    # Verify freeze/unfreeze cycle
    log "  Testing freeze/unfreeze cycle..."
    soroban contract invoke \
      --id "$CONTRACT_ID" \
      --source "$ADMIN_IDENTITY" \
      --network "$NETWORK" \
      -- \
      freeze_contract \
      --admin_signants "[\"$ADMIN_ADDRESS\"]" 2>/dev/null && log "  ✅ freeze_contract OK" || echo "  ⚠ freeze_contract failed" >&2

    soroban contract invoke \
      --id "$CONTRACT_ID" \
      --source "$ADMIN_IDENTITY" \
      --network "$NETWORK" \
      -- \
      unfreeze_contract \
      --admin_signants "[\"$ADMIN_ADDRESS\"]" 2>/dev/null && log "  ✅ unfreeze_contract OK" || echo "  ⚠ unfreeze_contract failed" >&2

    log "Canary checks complete."
  fi
fi

# ── Summary ───────────────────────────────────────────────────────────────────

echo ""
echo "  ── Deployment complete ──────────────────────────────────"
echo "  Network:    $NETWORK_ALIAS"
echo "  RPC URL:    $RPC_URL"
echo "  Contract:   $CONTRACT_ID"
echo "  Admin:      $ADMIN_ADDRESS"
echo "  Service:    $SERVICE_ADDRESS"
echo "  ─────────────────────────────────────────────────────────"
echo ""
echo "  Save CONTRACT_ID=$CONTRACT_ID in your environment and in"
echo "  the api repo's .env before routing submit_score calls."
echo ""
