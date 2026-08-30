#!/usr/bin/env bash
# Deploy-time manifest validation for network-specific parameters.
#
# Rejects a deployment whose admin, service, upgrade-delay, cooldown,
# risk-threshold, or schema settings are invalid for the target network,
# before any build or deploy command runs. Meant to be sourced by deploy.sh.
#
# Bounds below mirror the on-chain constraints in
# contracts/ledgerlens-score/src/constants.rs — keep them in sync.
readonly MANIFEST_MIN_COOLDOWN_SECS=60
readonly MANIFEST_MAX_COOLDOWN_SECS=86400
readonly MANIFEST_MIN_UPGRADE_DELAY_SECS=172800
readonly MANIFEST_MAX_UPGRADE_DELAY_SECS=1209600
readonly MANIFEST_CONTRACT_SCHEMA_VERSION=4

# Validates a Stellar StrKey public key (e.g. a service address): 56
# characters, starting with 'G', using base32 alphabet [A-Z2-7].
manifest_is_valid_stellar_address() {
  [[ "$1" =~ ^G[A-Z2-7]{55}$ ]]
}

# validate_manifest <network> <manifest-path> <admin-identity> <service-address>
#
# Prints one actionable error per violation to stderr and returns 1 if any
# check fails. Returns 0 and prints nothing if the manifest is valid for
# the given network. Performs no network calls or transactions.
validate_manifest() {
  local network="$1" manifest_path="$2" admin_identity="$3" service_address="$4"
  local errors=0

  if ! command -v jq >/dev/null 2>&1; then
    echo "ERROR: manifest validation requires 'jq' but it is not installed." >&2
    return 1
  fi

  if [ ! -f "$manifest_path" ]; then
    echo "ERROR: deploy manifest not found at '$manifest_path'." >&2
    return 1
  fi

  if ! jq -e --arg net "$network" 'has($net)' "$manifest_path" >/dev/null 2>&1; then
    echo "ERROR: deploy manifest '$manifest_path' has no entry for network '$network'." >&2
    return 1
  fi

  local section
  section=$(jq -c --arg net "$network" '.[$net]' "$manifest_path")

  local delay cooldown threshold schema
  delay=$(jq -r '.upgrade_delay_secs' <<<"$section")
  cooldown=$(jq -r '.cooldown_secs' <<<"$section")
  threshold=$(jq -r '.risk_threshold' <<<"$section")
  schema=$(jq -r '.schema_version' <<<"$section")

  if ! [ "$delay" -ge "$MANIFEST_MIN_UPGRADE_DELAY_SECS" ] 2>/dev/null ||
    ! [ "$delay" -le "$MANIFEST_MAX_UPGRADE_DELAY_SECS" ] 2>/dev/null; then
    echo "ERROR: manifest[$network].upgrade_delay_secs=$delay is out of bounds" \
      "[$MANIFEST_MIN_UPGRADE_DELAY_SECS, $MANIFEST_MAX_UPGRADE_DELAY_SECS]." >&2
    errors=$((errors + 1))
  fi

  if ! [ "$cooldown" -ge "$MANIFEST_MIN_COOLDOWN_SECS" ] 2>/dev/null ||
    ! [ "$cooldown" -le "$MANIFEST_MAX_COOLDOWN_SECS" ] 2>/dev/null; then
    echo "ERROR: manifest[$network].cooldown_secs=$cooldown is out of bounds" \
      "[$MANIFEST_MIN_COOLDOWN_SECS, $MANIFEST_MAX_COOLDOWN_SECS]." >&2
    errors=$((errors + 1))
  fi

  if ! [ "$threshold" -ge 0 ] 2>/dev/null || ! [ "$threshold" -le 100 ] 2>/dev/null; then
    echo "ERROR: manifest[$network].risk_threshold=$threshold is out of bounds [0, 100]." >&2
    errors=$((errors + 1))
  fi

  if [ "$schema" != "$MANIFEST_CONTRACT_SCHEMA_VERSION" ]; then
    echo "ERROR: manifest[$network].schema_version=$schema does not match the" \
      "compiled contract schema version ($MANIFEST_CONTRACT_SCHEMA_VERSION)." \
      "Update the manifest or rebuild against the expected contract version." >&2
    errors=$((errors + 1))
  fi

  if [ -z "$admin_identity" ]; then
    echo "ERROR: admin identity is required." >&2
    errors=$((errors + 1))
  elif [ "$network" = "mainnet" ] && [ "$admin_identity" = "deployer" ]; then
    echo "ERROR: refusing to deploy to mainnet using the default 'deployer' identity." \
      "Pass an explicit admin identity for mainnet deployments." >&2
    errors=$((errors + 1))
  fi

  if ! manifest_is_valid_stellar_address "$service_address"; then
    echo "ERROR: service-address '$service_address' is not a valid Stellar public key" \
      "(expected 'G' followed by 55 base32 characters)." >&2
    errors=$((errors + 1))
  fi

  [ "$errors" -eq 0 ]
}
