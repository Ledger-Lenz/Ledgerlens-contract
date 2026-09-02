#!/usr/bin/env bash

# Generates a CycloneDX Software Bill of Materials (SBOM) for the LedgerLens
# workspace as compiled for the wasm32-unknown-unknown target — i.e. the
# dependency graph actually embedded in the released ledgerlens_score.wasm.
#
# Prerequisites:
#   cargo-cyclonedx (https://github.com/CycloneDX/cyclonedx-cargo)
#     cargo +1.81.0 install cargo-cyclonedx --version 0.5.8 --locked
#
# Outputs:
#   target/sbom/*.cdx.json                      — one CycloneDX 1.3 JSON SBOM
#                                                per workspace package
#   target/sbom/ledgerlens-score.cdx.json       — SBOM for the shipped contract
#                                                (its embedded dependency graph)
#
# This is the same script invoked by the `supply-chain` CI job so that the SBOM
# and the signed WASM are traceable to the same commit. See
# docs/reproducible-builds.md.

set -euo pipefail

# CycloneDX spec version emitted by cargo-cyclonedx 0.5.8 (the pinned version).
SPEC_VERSION="1.3"

if ! command -v cargo-cyclonedx >/dev/null 2>&1; then
  echo "ERROR: cargo-cyclonedx not found. Install it first:" >&2
  echo "  cargo +1.81.0 install cargo-cyclonedx --version 0.5.8 --locked" >&2
  exit 1
fi

echo "==> Generating CycloneDX SBOMs for wasm32-unknown-unknown target"
cargo cyclonedx --target wasm32-unknown-unknown --format json

echo "==> Collecting SBOMs into target/sbom"
rm -rf target/sbom
mkdir -p target/sbom
for f in $(find . -name '*.cdx.json' -not -path './target/*'); do
  cp "$f" "target/sbom/$(basename "$f")"
done

# The released contract's own dependency graph must be present.
if [ ! -f target/sbom/ledgerlens-score.cdx.json ]; then
  echo "ERROR: ledgerlens-score.cdx.json not generated" >&2
  exit 1
fi

echo "==> Validating SBOMs are well-formed CycloneDX JSON"
for f in target/sbom/*.cdx.json; do
  python3 - "$f" "$SPEC_VERSION" <<'EOF'
import json, sys

path, spec = sys.argv[1], sys.argv[2]
with open(path) as fh:
    doc = json.load(fh)

if doc.get("bomFormat") != "CycloneDX":
    sys.exit(f"ERROR {path}: bomFormat is not CycloneDX")
if doc.get("specVersion") != spec:
    sys.exit(f"ERROR {path}: specVersion is not {spec}")
if "components" not in doc or not isinstance(doc["components"], list):
    sys.exit(f"ERROR {path}: missing 'components' array")

for c in doc["components"]:
    for key in ("type", "name", "version", "purl"):
        if key not in c:
            sys.exit(f"ERROR {path}: component missing '{key}': {c.get('name')}")
    # Every component must carry a machine-readable license expression or id.
    lic = c.get("licenses")
    if not lic:
        sys.exit(f"ERROR {path}: component '{c.get('name')}' has no license")

print(f"OK {path}: {len(doc['components'])} components")
EOF
done

echo ""
echo "SUCCESS: SBOMs written to target/sbom/"
ls -1 target/sbom/*.cdx.json
