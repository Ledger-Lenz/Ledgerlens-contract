# Release Process

This document describes the automated release workflow for the `ledgerlens-score` Soroban contract. The workflow ensures a verifiable chain of custody from source code to published release artifact.

---

## Overview

Pushing a tag matching `contract-v*` (e.g., `contract-v1.2.3`) triggers the **Contract Release** workflow (`.github/workflows/release.yml`). The workflow:

1. **Builds** the contract with the pinned Rust toolchain (1.81.0)
2. **Optimizes** the WASM using Stellar CLI (v21.0.0)
3. **Signs** the optimized WASM with a detached SHA-256 manifest
4. **Re-verifies** build reproducibility via double-build comparison
5. **Publishes** a GitHub Release with the optimized WASM, its SHA-256, and release notes

The release is **fail-closed**: if the reproducibility check fails, no release is published.

---

## Triggering a Release

### Tag Format

```
contract-v<MAJOR>.<MINOR>.<PATCH>
```

Examples:
- `contract-v1.0.0`
- `contract-v2.3.1`
- `contract-v3.0.0-rc.1` (pre-release tags also match)

### How to Create a Release

```bash
# 1. Ensure you are on the correct commit (usually main)
git checkout main
git pull origin main

# 2. Verify CHANGELOG.md has an [Unreleased] section with the changes
#    The release workflow extracts notes from this section.

# 3. Create and push the tag
git tag contract-v1.2.3
git push origin contract-v1.2.3
```

### What Happens Next

1. GitHub Actions starts the **Contract Release** workflow
2. You can monitor progress at: `https://github.com/Ledger-Lenz/Ledgerlens-contract/actions`
3. On success, a GitHub Release appears at: `https://github.com/Ledger-Lenz/Ledgerlens-contract/releases`

---

## Workflow Stages

### Stage 1: Build, Optimize, and Sign (`build-and-sign` job)

| Step | Description |
|------|-------------|
| Checkout | Clones the repository at the tagged commit |
| Pin Rust toolchain | Installs Rust 1.81.0 + `wasm32-unknown-unknown` target (matches `rust-toolchain.toml` and `ci.yml`) |
| Install Stellar CLI | Downloads Stellar CLI v21.0.0 (matches `deploy/manifests/mainnet.env`) |
| Build raw WASM | `cargo build --target wasm32-unknown-unknown --release -p ledgerlens-score --locked` |
| Optimize WASM | `stellar contract optimize --wasm <raw> --output <optimized>` |
| Sign optimized WASM | Creates `ledgerlens_score.optimized.wasm.sha256` via `sha256sum` |
| Verify manifest | `sha256sum --check` — fails if hash mismatches |
| Upload artifacts | Optimized WASM + SHA-256 manifest (90-day retention) |

**Outputs:**
- `ledgerlens_score.optimized.wasm` — deployable artifact
- `ledgerlens_score.optimized.wasm.sha256` — integrity manifest

### Stage 2: Reproducibility Check (`reproducibility-check` job)

This stage **re-runs the exact double-build comparison from `ci.yml`** (jobs `repro-build-1`, `repro-build-2`, `repro-verify`). It does NOT trust CI's earlier run because:

- The tag may have been force-pushed after CI completed
- Chain-of-custody requires the release pipeline to independently verify
- A mismatch here means the source at this tag is non-deterministic

| Step | Description |
|------|-------------|
| Run 1 | Fresh runner, no cache, cold build → compute SHA-256 |
| Run 2 | Separate fresh runner, no cache, cold build → compute SHA-256 |
| Compare | Assert SHA-256 digests are identical |

**Failure mode:** If digests differ, the job exits non-zero. The `publish-release` job depends on this job via `needs: [reproducibility-check]`, so a failure **structurally prevents** any release from being published.

**Output:** Exports `VERIFIED_SHA256` (raw WASM hash) for inclusion in release notes.

### Stage 3: Publish Release (`publish-release` job)

Runs **only if** `reproducibility-check` succeeds (`needs:` + `if: success()`).

| Step | Description |
|------|-------------|
| Download artifacts | Retrieves optimized WASM + manifest from `build-and-sign` |
| Generate release notes | Extracts from `CHANGELOG.md` `[Unreleased]` section; falls back to `git log` |
| Create GitHub Release | Uses `gh release create` with: |
| | • Title: tag name (e.g., `contract-v1.2.3`) |
| | • Body: release notes + artifact table with both SHA-256 hashes |
| | • Attachments: optimized WASM, SHA-256 manifest file |
| SBOM hook | Commented placeholder for future SBOM attachment |

**Permissions:** `contents: write` (minimum required for release creation)

---

## Release Artifacts

Every release includes:

| File | Description |
|------|-------------|
| `ledgerlens_score.optimized.wasm` | Deployable WASM (output of `stellar contract optimize`) |
| `ledgerlens_score.optimized.wasm.sha256` | Detached SHA-256 manifest for the optimized artifact |

The release body also includes the **raw WASM SHA-256** (verified by the double-build check) so deployers can verify the source-to-binary correspondence.

---

## Verifying a Downloaded Artifact

### Verify Optimized WASM (Deployable Artifact)

```bash
# 1. Download from the release page
curl -LO https://github.com/Ledger-Lenz/Ledgerlens-contract/releases/download/contract-v1.2.3/ledgerlens_score.optimized.wasm
curl -LO https://github.com/Ledger-Lenz/Ledgerlens-contract/releases/download/contract-v1.2.3/ledgerlens_score.optimized.wasm.sha256

# 2. Verify SHA-256
sha256sum --check ledgerlens_score.optimized.wasm.sha256
# Should output: ledgerlens_score.optimized.wasm: OK
```

### Verify Raw WASM Matches Source (Reproducibility)

```bash
# 1. Check out the exact tag
git clone https://github.com/Ledger-Lenz/Ledgerlens-contract.git
cd Ledgerlens-contract
git checkout contract-v1.2.3

# 2. Build with pinned toolchain (rust-toolchain.toml handles this)
cargo build --target wasm32-unknown-unknown --release -p ledgerlens-score --locked

# 3. Compute hash and compare with release body
sha256sum target/wasm32-unknown-unknown/release/ledgerlens_score.wasm
# Compare with "Raw WASM SHA-256" in the release body
```

### Verify On-Chain Deployment Matches Release

After deploying the optimized WASM to a network:

```bash
# Get on-chain WASM hash
stellar contract info --id <CONTRACT_ID> --network <mainnet|testnet>
# Note the `wasm_hash` field

# Compare with the raw WASM SHA-256 from the release body
# (The on-chain hash matches the RAW artifact, not the optimized one,
# unless the deployment used the optimized artifact directly)
```

---

## What to Do If Reproducibility Check Fails

The `reproducibility-check` job will fail with output like:

```
ERROR: WASM output is NOT reproducible.
  Run 1: a1b2c3d4e5f6...
  Run 2: 9f8e7d6c5b4a...
```

**Do not force-push the tag or re-run the workflow.** A non-deterministic build means the artifact cannot be independently verified — this breaks the chain of custody.

### Debugging Steps

1. **Check for local non-determinism**: Run the local verification procedure from `docs/reproducible-builds.md` on your machine.

2. **Common causes** (from `docs/reproducible-builds.md`):
   - Wrong Rust version (must be 1.81.0 exactly)
   - `Cargo.lock` was regenerated (run `git diff Cargo.lock` — should be empty)
   - Post-build optimization applied inconsistently
   - Host-specific build metadata (confirm `debug = 0` in `Cargo.toml` `[profile.release]`)

3. **Fix the root cause** in the source code or build configuration.

4. **Create a new tag** (e.g., `contract-v1.2.4`) after the fix is merged to `main`.

5. **Delete the bad tag** locally and remotely:
   ```bash
   git tag -d contract-v1.2.3
   git push origin :refs/tags/contract-v1.2.3
   ```

---

## SBOM Hook

The workflow includes a **commented placeholder** for Software Bill of Materials (SBOM) generation:

```yaml
# - name: Generate SBOM (CycloneDX)
#   run: |
#     cargo install cargo-cyclonedx --locked
#     cargo cyclonedx --target wasm32-unknown-unknown --release -p ledgerlens-score \
#       --output-format json --output-file target/sbom.json
#
# - name: Upload SBOM to release
#   run: |
#     gh release upload "$TAG" target/sbom.json --clobber
```

**Activation:** When SBOM support is implemented (tracking issue: `#SBOM_ISSUE`), uncomment these steps. The SBOM will be attached to the GitHub Release alongside the WASM artifacts.

---

## Toolchain Pinning and Security

The workflow pins all critical toolchain versions to prevent drift attacks:

| Tool | Version | Where Pinned |
|------|---------|--------------|
| Rust | 1.81.0 | `rust-toolchain.toml`, `release.yml`, `ci.yml` |
| Stellar CLI | 21.0.0 | `deploy/manifests/mainnet.env`, `release.yml` |
| Cargo dependencies | Exact versions | `Cargo.lock` (committed) |

**Why this matters:** An attacker who compromises the build environment could substitute a newer Rust version that produces different (potentially malicious) bytecode. Pinning ensures the release workflow uses the exact same compiler that CI verified as reproducible.

---

## Permission Model

The workflow requests minimal permissions:

| Job | Permissions |
|-----|-------------|
| `build-and-sign` | `contents: read` |
| `reproducibility-check` | `contents: read` |
| `publish-release` | `contents: write` (for `gh release create`) |

**Repository setting required:** In **Settings → Actions → General → Workflow permissions**, ensure "Read and write permissions" is selected (or at minimum, the `GITHUB_TOKEN` must have `contents: write` for the `publish-release` job). The default token permissions in this repo must allow release creation.

---

## Related Documents

- `docs/reproducible-builds.md` — Local verification procedure and troubleshooting
- `docs/deployment-manifests.md` — Manifest schema and toolchain drift checks
- `docs/upgrade-guide.md` — Time-locked upgrade governance
- `SECURITY.md` — Threat model and responsible disclosure
- `CHANGELOG.md` — Keep a Changelog format (source for release notes)

---

## Quick Reference

| Action | Command |
|--------|---------|
| Create release | `git tag contract-vX.Y.Z && git push origin contract-vX.Y.Z` |
| Monitor workflow | `https://github.com/Ledger-Lenz/Ledgerlens-contract/actions` |
| View releases | `https://github.com/Ledger-Lenz/Ledgerlens-contract/releases` |
| Verify artifact | `sha256sum --check ledgerlens_score.optimized.wasm.sha256` |
| Local reproducibility check | See `docs/reproducible-builds.md` |
| Delete bad tag | `git tag -d contract-vX.Y.Z && git push origin :refs/tags/contract-vX.Y.Z` |