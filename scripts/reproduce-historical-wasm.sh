#!/usr/bin/env bash
set -euo pipefail

source_commit="8336828159b7e7ff05d018200ce7f7a385bdade5"
expected_lock_sha="5b9a90e17efcb2886dc98b8de36f29bc1665338652f239cc769fcef4f6fc3d30"
expected_wasm_sha="ae9ec87205342f28d8072f7a1d62d7f35844ad0502ce597087666dd22e980e1b"

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd -- "$script_dir/.." && pwd)"
fixture_dir="$repo_root/tests/fixtures/historical"
fixture_lock="$fixture_dir/ledgerlens-score-v3-8336828.Cargo.lock"
expected_wasm="$fixture_dir/ledgerlens-score-v3-8336828.wasm"

for tool in cargo git shasum wasm-opt; do
    command -v "$tool" >/dev/null || {
        echo "required tool is missing: $tool" >&2
        exit 1
    }
done

if [[ "$(wasm-opt --version)" != "wasm-opt version 131" ]]; then
    echo "Binaryen 131 is required; found: $(wasm-opt --version)" >&2
    exit 1
fi
cargo +1.85.0 --version >/dev/null

actual_lock_sha="$(shasum -a 256 "$fixture_lock" | awk '{print $1}')"
if [[ "$actual_lock_sha" != "$expected_lock_sha" ]]; then
    echo "fixture lock hash mismatch: $actual_lock_sha" >&2
    exit 1
fi

fixture_worktree="$(mktemp -d "${TMPDIR:-/tmp}/ledgerlens-v3-repro.XXXXXX")"
rmdir "$fixture_worktree"
cleanup() {
    git -C "$repo_root" worktree remove --force "$fixture_worktree" >/dev/null 2>&1 || true
}
trap cleanup EXIT INT TERM

git -C "$repo_root" worktree add --detach "$fixture_worktree" "$source_commit"
cp "$fixture_lock" "$fixture_worktree/Cargo.lock"

(
    cd "$fixture_worktree"
    RUSTC_BOOTSTRAP=1 \
        RUSTFLAGS="-Zcrate-attr=feature(unsigned_is_multiple_of)" \
        cargo +1.85.0 build \
            --target wasm32-unknown-unknown \
            --release \
            -p ledgerlens-score \
            --locked
    wasm-opt target/wasm32-unknown-unknown/release/ledgerlens_score.wasm \
        -o ledgerlens-score-v3-8336828.wasm \
        -Oz \
        --disable-reference-types \
        --disable-multivalue \
        --disable-bulk-memory
)

reproduced_wasm="$fixture_worktree/ledgerlens-score-v3-8336828.wasm"
actual_wasm_sha="$(shasum -a 256 "$reproduced_wasm" | awk '{print $1}')"
if [[ "$actual_wasm_sha" != "$expected_wasm_sha" ]]; then
    echo "reproduced WASM hash mismatch: $actual_wasm_sha" >&2
    exit 1
fi
cmp "$expected_wasm" "$reproduced_wasm"

echo "historical WASM reproduced: $actual_wasm_sha"
