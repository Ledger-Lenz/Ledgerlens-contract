#!/usr/bin/env bash
# Fails if the head ref renames, removes, or renumbers any existing variant
# in contracts/ledgerlens-score/src/errors.rs's `Error` enum relative to the
# base ref. New discriminants and new `pub const` aliases (in `impl Error`)
# are always allowed — see CONTRIBUTING.md's "Keep error codes in errors.rs
# stable" rule and issue #436.
#
# Usage: check_error_discriminants.sh <base-ref> <head-ref>
set -euo pipefail

ERRORS_PATH="contracts/ledgerlens-score/src/errors.rs"
BASE_REF="${1:?usage: check_error_discriminants.sh <base-ref> <head-ref>}"
HEAD_REF="${2:?usage: check_error_discriminants.sh <base-ref> <head-ref>}"

# Reads errors.rs on stdin and prints "<discriminant> <variant name>" pairs,
# one per line, extracted from inside the `Error` enum body only — this is
# what keeps `impl Error { pub const Alias = ...; }` aliases out of scope.
extract_from_stream() {
    awk '/pub enum Error/{flag=1; next} flag && /^}/{flag=0} flag' \
        | grep -E '^[[:space:]]*[A-Za-z_][A-Za-z0-9_]*[[:space:]]*=[[:space:]]*[0-9]+' \
        | sed -E 's/^[[:space:]]*([A-Za-z_][A-Za-z0-9_]*)[[:space:]]*=[[:space:]]*([0-9]+).*/\2 \1/'
}

extract_discriminants() {
    # `|| true`: git show fails (128) when the file is absent at a ref, and
    # the grep inside extract_from_stream exits 1 when the enum has no
    # discriminants — both must fall through to the skip path below instead of
    # aborting the script via set -e/pipefail.
    git show "${1}:${ERRORS_PATH}" | extract_from_stream || true
}

base_pairs="$(extract_discriminants "$BASE_REF")"
head_pairs="$(extract_discriminants "$HEAD_REF")"

if [ -z "$base_pairs" ]; then
    echo "check_error_discriminants: found no discriminants in $ERRORS_PATH at $BASE_REF — skipping (file may not exist at that ref)"
    exit 0
fi

fail=0
while read -r num name; do
    [ -z "$num" ] && continue
    head_name="$(printf '%s\n' "$head_pairs" | awk -v n="$num" '$1 == n {print $2}')"
    if [ -z "$head_name" ]; then
        echo "::error file=$ERRORS_PATH::discriminant $num ($name) was removed or renumbered — Error enum discriminants must be append-only (see CONTRIBUTING.md)"
        fail=1
    elif [ "$head_name" != "$name" ]; then
        echo "::error file=$ERRORS_PATH::discriminant $num changed variant name from '$name' to '$head_name' — Error enum discriminants must be append-only (see CONTRIBUTING.md)"
        fail=1
    fi
done <<< "$base_pairs"

if [ "$fail" -ne 0 ]; then
    echo "Error discriminant stability check FAILED."
    exit 1
fi

added_count="$(comm -13 <(printf '%s\n' "$base_pairs" | sort -u) <(printf '%s\n' "$head_pairs" | sort -u) | grep -c . || true)"
echo "Error discriminant stability check passed (${added_count} new discriminant(s) added)."
