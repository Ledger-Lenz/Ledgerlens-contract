#!/usr/bin/env bash
# Test for tools/check_refinement_mapping.py (issue #928).
#
# Verifies that:
#   1. The checker passes on the real (corrected) spec/refinement-mapping.md.
#   2. The checker FAILS loudly, naming the symbol, when the mapping references
#      a function that does not exist in the contract sources (the exact drift
#      class the issue describes: `finalize_consensus` never existed).
#   3. The checker FAILS when a storage-key enum variant drifts too.
set -euo pipefail

ROOT_DIR="$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)"
CHECKER="$ROOT_DIR/tools/check_refinement_mapping.py"
MAPPING="$ROOT_DIR/spec/refinement-mapping.md"
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

echo "==> 1/3: checker passes on the real mapping"
python3 "$CHECKER" --mapping "$MAPPING" --quiet
echo "    OK"

echo "==> 2/3: checker flags a nonexistent function reference"
sed 's/submit_consensus_score/nonexistent_function_928/g' \
    "$MAPPING" > "$TMP_DIR/bad-fn.md"
if python3 "$CHECKER" --mapping "$TMP_DIR/bad-fn.md" > "$TMP_DIR/out1.txt" 2>&1; then
    echo "FAIL: expected the checker to reject a nonexistent function reference" >&2
    exit 1
fi
grep -q "nonexistent_function_928" "$TMP_DIR/out1.txt" || {
    echo "FAIL: error output did not name the missing symbol" >&2
    cat "$TMP_DIR/out1.txt" >&2
    exit 1
}
echo "    OK (exit 1, names 'nonexistent_function_928')"

echo "==> 3/3: checker flags a nonexistent storage-key enum variant"
sed 's/BurstCapacity/NonexistentVariant928/g' \
    "$MAPPING" > "$TMP_DIR/bad-key.md"
if python3 "$CHECKER" --mapping "$TMP_DIR/bad-key.md" > "$TMP_DIR/out2.txt" 2>&1; then
    echo "FAIL: expected the checker to reject a nonexistent storage-key variant" >&2
    exit 1
fi
grep -q "NonexistentVariant928" "$TMP_DIR/out2.txt" || {
    echo "FAIL: error output did not name the missing storage-key variant" >&2
    cat "$TMP_DIR/out2.txt" >&2
    exit 1
}
echo "    OK (exit 1, names 'NonexistentVariant928')"

echo ""
echo "Refinement-mapping consistency check test passed."
