# Authorization Failure Telemetry

Issue: #694 — Create authorization failure telemetry without leaking signer strategy.

Every privileged call into `ledgerlens-score` — admin functions and
service-multisig submission paths alike — is gated by an M-of-N signer-set
check: the caller supplies a `Vec<Address>` of candidate signers, and the
contract verifies there are at least `threshold` of them, that each is a
member of the configured admin/service set, and (for entry points that
require it) that each really authorized the call via Soroban's
`require_auth`.

This document describes the failure-reporting contract for that check: what
operators see when a privileged call is denied, and why it deliberately says
less than it used to.

## The problem this closes

Before this change, a denied M-of-N check returned one of several distinct
`Error` codes depending on *why* it failed:

- `InsufficientAdminSigners` / `InsufficientSigners` — fewer than `threshold`
  addresses were supplied.
- `AdminSignerNotInSet` / `UnauthorizedSigner` — a supplied address was not a
  member of the configured set.

The set-membership check ran **before** any `require_auth()` call. That
ordering matters: it means the distinguishing error came back for a caller
who supplied *no valid signatures at all*. A caller could:

1. Pick a candidate address they suspect is an admin or service signer.
2. Call any admin- or service-gated function (`pause`, `veto_parameter_change`,
   `submit_score`, ...) with a `signers` vector containing just that one
   candidate address (or the candidate plus arbitrary padding to satisfy the
   length check).
3. Read the returned error. `AdminSignerNotInSet` / `UnauthorizedSigner`
   meant "not a member" — a free, unauthenticated answer. Anything else
   (including a `require_auth` panic, which only triggers once membership
   passes) meant "yes, a member."

Repeating this once per candidate is enough to fingerprint the entire
admin/service signer set at negligible cost, with no real signature
required. That's a targeting tool for an attacker deciding whose key to
phish or compromise — exactly the "signer strategy" this issue asks not to
leak.

## The fix

`Self::validate_signer_set` (in `lib.rs`) is now the single implementation
behind every signer-set check in the contract — `require_admin_auth`,
`require_service_signers_auth`, and the equivalent checks previously
duplicated inline in `submit_score` and `submit_scores_batch_attested`. It
has two properties that close the leak:

1. **One outcome for every denial reason.** Whether the count was too low,
   the count exceeded what the configured set could ever require, an
   address wasn't a member, or a member's signer entry had expired, the
   function returns the same `Error::Unauthorized` (discriminant 3, already
   part of the public error enum — no new variant was added, and none was
   possible: `#[contracterror]` enums are hard-capped at 50 variants by the
   XDR spec, and this contract is already at that cap). A caller can no
   longer distinguish "my candidate is a real signer" from "it isn't" by the
   error code alone.
2. **Membership is validated for the whole supplied set before any
   `require_auth()` call runs**, not interleaved per-address as before. This
   matters because `require_auth()` fails by trapping the host call, which
   is observable as a different failure shape than a returned `Result::Err`.
   By fully resolving membership first, a supplied address only ever reaches
   `require_auth()` once every other address in the same call also passed
   membership — so reaching that stage already requires knowing a full
   quorum of real signers, not just the one address being tested.

`InsufficientAdminSigners`, `AdminSignerNotInSet`, `InsufficientSigners`, and
`UnauthorizedSigner` remain defined in `errors.rs` for ABI/decoding
stability (existing tooling that maps a numeric code to a name still
resolves), and `AdminSignerNotInSet` is still genuinely reachable from
`remove_admin_signer`'s post-authorization "target address not in set"
check — that check runs only after the caller already passed full
`require_admin_auth`, so it carries no probing risk and was left unchanged.
`InsufficientAdminSigners`, `InsufficientSigners`, and `UnauthorizedSigner`
are otherwise unreachable at runtime going forward.

## What operators get instead

Immediately before returning `Error::Unauthorized` from a signer-set check,
the contract emits an `auth_den` event:

```
topics: (auth_den, gate)
data:   reason
```

- `gate` — a `Symbol`, either `admin` or `service`, identifying which
  signer set was being checked.
- `reason` — an `AuthDenialReason` (`#[contracttype]`, defined in
  `types.rs`), one of:
  - `InvalidSignerCount` — the supplied signer count was outside the valid
    range for the configured threshold (too few, or more than the
    configured set could ever contain). This only restates information
    already public via `get_admin_threshold()` / `get_service_threshold()`.
  - `SignerValidationFailed` — the supplied set didn't fully validate: at
    least one entry wasn't a member, had expired, or authorization failed.
    Deliberately does not say which entry, or how many.

This is enough for an operator dashboard to distinguish "someone's tooling
is misconfigured and under-supplying signers" from "someone attempted a
privileged call with signers that don't check out" — the two failure modes
that actually call for different operator responses — without ever
publishing which address(es) were involved.

## Bounded resource use

`validate_signer_set` rejects a caller-supplied `signers` vector longer than
the configured set's hard cap (`MAX_ADMIN_SIGNERS` = 5,
`MAX_SERVICE_SIGNERS` = 10) via the same length check used for "too few,"
before any per-entry membership check runs. A caller cannot force more than
a small constant amount of iteration by passing an oversized vector — see
`test_admin_gate_oversized_signer_vector_rejected_without_membership_scan`
and its service-gate counterpart in `test_auth_denial_telemetry.rs`.

## Compatibility summary

- **No new `Error` variant** — the enum was already at the 50-variant XDR
  cap; `Unauthorized` (existing, discriminant 3) is reused.
- **No storage layout change.**
- **New event `auth_den`** — additive; existing event consumers are
  unaffected.
- **New `AuthDenialReason` public type**, exported alongside the other
  `#[contracttype]`s from the crate root.
- **Behavior change (denial-path error codes only):** callers that pattern-matched
  on `InsufficientAdminSigners` / `AdminSignerNotInSet` /
  `InsufficientSigners` / `UnauthorizedSigner` from `pause`,
  `propose_upgrade`, `execute_upgrade`, `veto_upgrade`,
  `propose_parameter_change`, `execute_parameter_change`,
  `veto_parameter_change`, `submit_score`, `submit_consensus_score`,
  `reveal_consensus`, `submit_scores_batch_attested`, or any other
  admin-gated setter will now see `Unauthorized` instead. The set of inputs
  that succeed or fail is unchanged — only the returned code for a denial
  coarsens. No fail-closed guarantee, threshold, or membership check was
  weakened; `add_admin_signer` / `remove_admin_signer` /
  `set_admin_threshold` / `set_service_threshold` / `add_service_signer` /
  `remove_service_signer` still behave exactly as before.
