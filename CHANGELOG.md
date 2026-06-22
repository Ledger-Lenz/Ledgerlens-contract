# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to Semantic Versioning for ABI-breaking changes.

## [Unreleased]

### Added
- Wallet Relationship Graph: counterparty link management (`add_counterparty_link`, `remove_counterparty_link`)
- Score submission floor policy to prevent reputation laundering
- Hysteresis layer for smoother score transitions
- Multi-model consensus scoring with configurable threshold and epsilon
- Time-weighted exponential decay for score aging
- Cross-contract risk gate integration
- Wallet-score delegation with cycle detection
- Per-pair circuit breaker
- Fee withdrawal mechanism
- Admin M-of-N multi-sig support
- Score attestation with EdDSA verification
- Per-wallet/pair submission rate limiting
- Signer tier system

### Fixed
- Resolved duplicate error codes in `errors.rs` (codes 26, 27, 30 were assigned to multiple variants)
- Removed orphaned `GATE_CALLER_*` constants from `errors.rs`
- Fixed formatting issues: `SignerTierViolation` misaligned indentation and `InvalidSignerTier`/`ScoreNotFound` on same line
- Removed stray branch name `feat/confidence-gated-risk-gate` from source file

## [3.0.0] - Previous Release

### Added
- Core score submission and retrieval
- Service signer management
- Upgrade mechanism with time-lock
- Basic rate limiting

## Migration Notes

ABI-breaking changes require coordination with dependent repos (`api`, `core`, `dashboard`).
See [CONTRIBUTING.md](./CONTRIBUTING.md) for details.
