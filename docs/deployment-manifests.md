# Deployment manifests and toolchain drift checks

As of July 25, 2026, LedgerLens deploys through reviewed environment manifests
instead of relying on ad-hoc network flags and ambient CLI configuration.

## Current behavior before this change

Before the manifest layer:

- `deploy.sh` accepted a positional network alias and relied on local CLI
  network configuration.
- Rust was pinned in `rust-toolchain.toml`, but `deploy.sh` did not verify the
  active local `rustc` version before building.
- The deployment path did not compare the active Stellar/Soroban CLI version
  against a reviewed project expectation.

## Current behavior after this change

`deploy.sh` now performs three preflight checks before any build or deploy
action:

1. Loads a reviewed manifest from `deploy/manifests/<network>.env` unless
   `--manifest <path>` overrides it.
2. Validates the manifest schema and rejects any undeclared fields. Because the
   schema is an allowlist, secrets are excluded by construction.
3. Compares:
   - expected Rust version from `rust-toolchain.toml`
   - expected Stellar CLI version from the selected manifest
   against the locally installed toolchain, and fails with exact expected and
   actual versions plus upgrade instructions on mismatch.

## Manifest schema

Every reviewed manifest must define exactly these keys:

| Key | Meaning |
|---|---|
| `SCHEMA_VERSION` | Manifest schema version. Current value: `1`. |
| `NETWORK_ALIAS` | Human-selected environment key such as `testnet`, `futurenet`, or `mainnet`. |
| `NETWORK_PASSPHRASE` | Reviewed Stellar network passphrase used for deployment and invocation. |
| `RPC_URL` | Reviewed Soroban RPC endpoint for that environment. |
| `REQUIRE_MAINNET_CONFIRMATION` | `true` only for irreversible mainnet deployments. |
| `EXPECTED_STELLAR_CLI_VERSION` | Reviewed CLI version string that `deploy.sh` must match exactly. |

Any additional key causes validation failure.

## Usage

Dry-run a reviewed testnet deployment:

```bash
./deploy.sh --dry-run testnet deployer GXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX
```

Validate only the reviewed manifest and local tools:

```bash
./deploy.sh --check-toolchain testnet
```

Use an alternate reviewed manifest file:

```bash
./deploy.sh --manifest /path/to/reviewed-testnet.env --dry-run testnet deployer GXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX
```

## Failure mode examples

Rust drift:

```text
Toolchain drift detected for Rust.
  Expected: 1.81.0
  Actual:   1.82.0
```

CLI drift:

```text
Toolchain drift detected for Stellar CLI.
  Expected: 21.0.0
  Actual:   22.0.0
```

Unexpected manifest field:

```text
ERROR: unexpected manifest key 'SECRET_KEY' ...
```

## Resource and compatibility notes

- Public ABI: unchanged.
- Contract events/errors/storage: unchanged.
- Deployment-time resource use stays bounded to one manifest parse, one Rust
  version probe, one cargo version probe, and one CLI version probe before the
  build starts.
