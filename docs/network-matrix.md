# Deployment network matrix

`deploy.sh` accepts only the deployment profiles listed here. Any other alias is
rejected before build/deploy begins.

| Profile | Soroban alias | Intended use | RPC behavior | Admin / service format | Delay bounds | Failure mode |
|---|---|---|---|---|---|---|
| Local | `local` or `standalone` | Fast developer smoke checks against a local RPC node. | Local RPC endpoint only; no public-network assumptions. | Stellar StrKey public addresses (`G...`), same as every other profile. | Uses the contract’s configured delay bounds; local does not loosen them. | Unsupported alias or misconfigured local RPC fails fast. |
| Testnet | `testnet` | Primary shared integration and pre-production validation. | Public testnet RPC. | Stellar StrKey public addresses (`G...`). | Same contract delay bounds as production. | Wrong network alias, wrong contract ID, or missing service key aborts the run. |
| Futurenet | `futurenet` | Forward-compatibility and “next network” staging. | Public futurenet RPC. | Stellar StrKey public addresses (`G...`). | Same contract delay bounds as production. | Wrong alias or incompatible RPC config aborts the run. |
| Mainnet | `mainnet` | Production deployment. | Public mainnet RPC. | Stellar StrKey public addresses (`G...`). | Same contract delay bounds as production; the script adds a manual confirmation gate before deploy. | Anything ambiguous or unconfirmed fails closed before deploy. |

Notes:

- The admin identity is a Soroban CLI local identity name passed to
  `deploy.sh`; the script resolves it to a public address during initialization.
- The service address must already be a valid Stellar public key.
- `deploy.sh` is intentionally strict: unsupported profiles exit with status 2.
- Mainnet deploys require the explicit `deploy-mainnet` confirmation token.

Relevant implementation and docs:

- [`deploy.sh`](../deploy.sh)
- [`docs/upgrade-guide.md`](upgrade-guide.md)
- [`docs/reproducible-builds.md`](reproducible-builds.md)
