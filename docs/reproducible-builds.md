# Reproducible Builds

LedgerLens commits to a **reproducible-build guarantee**: any third party who
checks out the same source commit and builds with the pinned toolchain must
obtain a byte-identical WASM artifact.  This lets you independently verify
that the contract deployed on-chain matches the published source — without
trusting the deployer.

---

## Why this matters

A Soroban contract's on-chain identity is its **WASM hash** (the SHA-256 of the
contract bytecode installed via `update_current_contract_wasm` or the initial
deploy).  If a build is not reproducible, the only way to trust that the
deployed hash corresponds to this repository's source is to trust whoever ran
the deploy script.

Reproducibility closes that gap: anyone can compute the expected hash
themselves and compare it against the on-chain value with a public RPC call.

---

## Pinned build inputs

All inputs that affect WASM output determinism are locked:

| Input | Where pinned | Value |
|-------|-------------|-------|
| Rust compiler version | `rust-toolchain.toml` | `1.81.0` |
| Cargo dependency tree | `Cargo.lock` (committed) | exact versions |
| Optimization level | `Cargo.toml` `[profile.release]` | `opt-level = "z"` |
| LTO | `Cargo.toml` `[profile.release]` | `lto = true` |
| Codegen units | `Cargo.toml` `[profile.release]` | `codegen-units = 1` |
| Symbol stripping | `Cargo.toml` `[profile.release]` | `strip = "symbols"` |
| Panic strategy | `Cargo.toml` `[profile.release]` | `panic = "abort"` |

> **Why Rust 1.81.0?**  `soroban-sdk 21.x` targets `wasm32-unknown-unknown`.
> Rust 1.82 changed that target to enable the `reference-types` and
> `multi-value` WebAssembly proposals, which the Soroban host environment does
> not support.  `1.81.0` is the latest stable release that produces a valid,
> deployable Soroban WASM artifact.

---

## CI verification

Every push and pull request runs three dedicated CI jobs
(see `.github/workflows/ci.yml`, jobs `repro-build-1`, `repro-build-2`,
`repro-verify`):

1. **`repro-build-1`** — fresh Ubuntu runner, no cargo cache, cold build.
2. **`repro-build-2`** — second fresh Ubuntu runner, no cargo cache, cold build.
3. **`repro-verify`** — downloads both artifacts and asserts their SHA-256
   digests are identical.  The job fails with a clear diff if they diverge.

This proves that two independent machines, starting from scratch, produce the
same binary from the same source commit.

---

## Dependency license policy & SBOM

LedgerLens commits to more than reproducible builds: it enforces an explicit,
machine-readable dependency license policy and ships a software bill of
materials (SBOM) for every release. Both run in CI and are traceable to the
same commit as the signed WASM.

### License & policy gate (cargo-deny)

The file `deny.toml` codifies the dependency gate enforced by the CI job
`dependency-license`:

| Policy | Enforcement |
|--------|-------------|
| **Allowed licenses** | Only the permissive/weakly-permissive SPDX licenses explicitly listed in `deny.toml [licenses].allow` may appear anywhere in the locked graph. Copyleft licenses (GPL, LGPL, strong MPL, …) are **absent from the list**, so introducing one transitively fails CI. |
| **Yanked crates** | `advisories.yanked = "deny"` — any dependency yanked from crates.io fails CI. |
| **Unmaintained crates** | `advisories.unmaintained = "all"` — any crate flagged unmaintained by RustSec fails CI. |
| **Wildcard path deps** | `bans.wildcards = "deny"` — a `path` dependency without an explicit `version` fails CI. |
| **Sources** | `sources.allow-registry` restricts to crates.io / pinned git, reinforcing the Cargo.lock source-pinning check. |

Known, pre-existing exceptions (a yanked `spin` and an unmaintained `paste`,
both pinned deep inside the immutable Soroban 21 toolchain, plus the
`RUSTSEC-2026-0009` `time` advisory already waived by the `audit` job) are
recorded in `deny.toml [advisories].ignore` with their rationale. **Any**
new yanked or unmaintained dependency that is not covered by those documented
exceptions fails CI — a PR introducing a disallowed license or a banned/yanked
dependency is rejected.

### SBOM generation

The `supply-chain` CI job — the same job that builds and SHA-256-signs the
release WASM — generates a [CycloneDX](https://cyclonedx.org) SBOM with
`cargo-cyclonedx`:

* **Format:** CycloneDX JSON (`bomFormat: CycloneDX`, `specVersion: 1.3`).
  Each component carries a `purl` (`pkg:cargo/...`) and an SPDX license
  expression.
* **Scope:** generated for the `wasm32-unknown-unknown` target, so the SBOM
  describes exactly the dependency graph embedded in the released
  `ledgerlens_score.wasm`.
* **Artifact:** uploaded to GitHub as the `ledgerlens-sbom-cdx` artifact
  (`target/sbom/*.cdx.json`), alongside the `wasm` and
  `ledgerlens-score-wasm-sha256-manifest` artifacts, in the same workflow run.
  Because they are produced in the same job from the same checkout, the SBOM,
  the binary, and its SHA-256 signature are traceable to a single commit.
* **Primary document:** `ledgerlens-score.cdx.json` (the shipped contract's
  embedded dependency graph).

> **Reproducibility note:** SBOM generation lives in the `supply-chain` job,
> **not** in the `repro-build-1` / `repro-build-2` double-build comparison.
> CycloneDX embeds a build timestamp, so including it in the byte-for-byte
> comparison would introduce non-determinism. The WASM reproducibility check
> remains a pure binary comparison.

#### Local regeneration & validation

```bash
# Install the pinned tool once
cargo +1.81.0 install cargo-cyclonedx --version 0.5.8 --locked

# Generate + structurally validate the SBOMs
tools/generate_sbom.sh
# Output: target/sbom/*.cdx.json (validated CycloneDX 1.3 JSON)

# Optional: validate any SBOM against the official CycloneDX 1.3 JSON schema
python3 -m jsonschema -i target/sbom/ledgerlens-score.cdx.json \
  <(curl -sL https://raw.githubusercontent.com/CycloneDX/specification/1.3/schema/bom-1.3.schema.json)
```

### Consuming the SBOM

Downstream integrators (AMM / lending protocols, and the `api` / `dashboard` /
`core` repos for their own compliance tooling) consume the
`ledgerlens-sbom-cdx` GitHub Actions artifact from each release. Use the `purl`
field of each component to map a crate to its canonical package, and the
`licenses` expression to drive their own license-policy checks. Because the
SBOM and the `ledgerlens-score.wasm.sha256` manifest are produced from the same
CI run, an integrator can bind a specific SBOM to the exact binary hash they
deploy, closing their transitive-supply-chain review loop.

---

## Local verification procedure

Use the steps below to independently reproduce the WASM artifact and compare it
against a deployed contract's on-chain hash.

### Prerequisites

```bash
# Install rustup (if not already installed)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# The rust-toolchain.toml at the repo root pins everything automatically.
# The first cargo command after checkout will download Rust 1.81.0 and the
# wasm32-unknown-unknown target.  You can also install it explicitly:
rustup toolchain install 1.81.0
rustup target add wasm32-unknown-unknown --toolchain 1.81.0
```yaml

### Step 1 — Check out the exact commit

```bash
git clone https://github.com/Ledger-Lenz/Ledgerlens-contract.git
cd Ledgerlens-contract

# Replace <COMMIT_SHA> with the Git commit that was deployed on-chain.
# For a tagged release, use the tag instead: git checkout contract-v1.2.3
git checkout <COMMIT_SHA>

### Step 2 — Build the contract

```bash
# --locked    enforces the committed Cargo.lock; no network resolution allowed.
# --release   uses the deterministic [profile.release] from Cargo.toml.
cargo build \
  --target wasm32-unknown-unknown \
  --release \
  -p ledgerlens-score \
  --locked
```yaml

### Step 3 — Compute the SHA-256 hash of the local artifact

```bash
sha256sum target/wasm32-unknown-unknown/release/ledgerlens_score.wasm

Example output:
```
a1b2c3d4e5f6...  target/wasm32-unknown-unknown/release/ledgerlens_score.wasm

### Step 4 — Retrieve the on-chain WASM hash

Use the Soroban / Stellar RPC to fetch the contract's installed WASM hash.  The
easiest way is via the Stellar CLI:

```bash
# Replace <CONTRACT_ID> with the deployed contract's address (C...)
# Replace <NETWORK>     with testnet or mainnet
stellar contract info \
  --id <CONTRACT_ID> \
  --network <NETWORK>
```yaml

The output includes a `wasm_hash` field — that is the SHA-256 hash of the
bytecode currently installed on-chain.

Alternatively, using `curl` against Horizon or the RPC directly:

```bash
# Soroban RPC getLedgerEntries — contract code entry
curl -s https://soroban-testnet.stellar.org \
  -H 'Content-Type: application/json' \
  -d '{
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getLedgerEntries",
    "params": {
      "keys": ["<CONTRACT_CODE_XDR_KEY>"]
    }
  }' | jq '.result.entries[0].xdr'

> **Note**: the on-chain `wasm_hash` is the hash of the *raw* WASM bytes before
> any Soroban optimization step.  If you apply `stellar contract optimize` (or
> the legacy `soroban contract optimize`) after the build step, hash the
> **unoptimized** artifact (`ledgerlens_score.wasm`) to match the on-chain
> hash, unless the deploy was done with the optimized artifact — in which case
> hash the optimized one.  The deployment logs or `deploy.sh` output will
> indicate which artifact was deployed.

### Step 5 — Compare

```bash
# Hash from Step 3 (local build)
LOCAL_HASH="a1b2c3d4e5f6..."

# Hash from Step 4 (on-chain)
ONCHAIN_HASH="a1b2c3d4e5f6..."

if [ "$LOCAL_HASH" = "$ONCHAIN_HASH" ]; then
  echo "✓ Verified: local build matches on-chain deployment."
else
  echo "✗ MISMATCH: hashes differ."
  echo "  Local:   $LOCAL_HASH"
  echo "  On-chain: $ONCHAIN_HASH"
fi
```yaml

If the hashes match, the deployed contract bytecode is confirmed to correspond
to the source at `<COMMIT_SHA>` in this repository.

---

## Troubleshooting

### Hashes differ

| Likely cause | Resolution |
|---|---|
| Wrong Rust version | Run `rustc --version` and confirm it prints `rustc 1.81.0`.  Delete `~/.rustup/toolchains/` and reinstall if needed. |
| Wrong source commit | Run `git log -1` and confirm the commit matches the one used for deployment. |
| Cargo.lock was regenerated | Run `git diff Cargo.lock`.  Should be empty.  If not, restore it with `git checkout Cargo.lock`. |
| Post-build optimization applied | Hash the optimized artifact if the deployment used `soroban contract optimize`. |
| Host-specific build metadata | Some Rust targets embed absolute paths in debug info.  Confirm `debug = 0` is set in `[profile.release]` (it is — see `Cargo.toml`). |

### Build fails with `error[E0463]: can't find crate for 'core'`

The `wasm32-unknown-unknown` target is not installed for the 1.81.0 toolchain.
Fix:

```bash
rustup target add wasm32-unknown-unknown --toolchain 1.81.0

### Build fails with feature errors on Rust ≥ 1.82

The `rust-toolchain.toml` in this repository pins to `1.81.0`, which `rustup`
respects automatically.  If you are seeing this error, your environment is
overriding the toolchain.  Unset `RUSTUP_TOOLCHAIN` and `RUSTC` environment
variables and try again.

---

## Deployed contract registry

Published WASM hashes for each network are recorded in deployment releases (GitHub Releases tab).
Cross-reference the release tag against the Git commit and the on-chain hash to
establish a full chain of custody from source → build → deployment.

---

## Related documents

- [`docs/upgrade-guide.md`](upgrade-guide.md) — time-locked upgrade governance procedure
- [`docs/attestation-spec.md`](attestation-spec.md) — score attestation (secp256k1 payload signing)
- [`SECURITY.md`](../SECURITY.md) — threat model and responsible-disclosure policy
