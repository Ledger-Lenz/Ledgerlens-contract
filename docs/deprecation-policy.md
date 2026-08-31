# Deprecation & Sunset Policy

**Tracks issue:** #763  
**Status:** Adopted · **Effective:** v3.0.0+  
**See also:** [`docs/interface-versioning-policy.md`](interface-versioning-policy.md)

This document defines how old functions, return shapes, error codes, and
capability symbols are supported, warned about, and eventually removed from the
`ILedgerLensScore` composability surface.

---

## 1. Deprecation Lifecycle

A symbol passes through four states before it is permanently gone:

```
  Active
    │
    │  deprecation announced in CHANGELOG (Unreleased)
    ▼
  Deprecated
    │
    │  minimum 2 full major interface versions pass
    │  (e.g. deprecated in v3 → removal not before v5)
    ▼
  Sunset-Announced
    │
    │  30-day notice period (same as breaking-change window)
    ▼
  Removed
```

### State definitions

| State | Observable behaviour | Consumer action |
|-------|----------------------|-----------------|
| **Active** | Callable, returns documented value | None required |
| **Deprecated** | Still callable, returns same value; `supports_interface` may return `true` for the old cap and `true` for the replacement cap | Migrate to replacement at next convenient release |
| **Sunset-Announced** | Still callable; CHANGELOG `Unreleased` entry with removal date; testnet updated with `supports_interface` returning `false` for old cap | Must migrate before removal date |
| **Removed** | Function removed from WASM; calling it traps the caller | Migration to replacement is mandatory |

---

## 2. Timelines

### Minimum deprecation window

| Change type | Minimum time from deprecation announcement to removal |
|-------------|-------------------------------------------------------|
| Any public function in `interface-spec.md §1` | 2 full major interface versions |
| `#[contracttype]` struct field | 2 full major interface versions |
| Error discriminant numeric value | Never removed; aliases may be added |
| Capability symbol in `supports_interface` | 2 full major interface versions |
| Internal helper (not in `interface-spec.md`) | 1 major interface version |

"2 full major interface versions" means: if something is deprecated at the
start of v3, it can be removed no earlier than v5. With the current roughly
annual cadence, this is typically at least 18–24 months of advance warning.

### Notice period before removal

Once the sunset date is set, a minimum 30-day notice period applies (identical
to the breaking-change policy in
[`interface-versioning-policy.md §4`](interface-versioning-policy.md)).
Testnet is updated first; mainnet removal follows after the 30-day window.

---

## 3. How Deprecation Is Signalled

### 3.1 CHANGELOG entry

Every deprecation is recorded in `CHANGELOG.md` under the `[Unreleased]`
section with:

```markdown
### Deprecated
- `function_name(param: Type) -> ReturnType` — use `new_function_name` instead.
  Will be removed in interface version N+2.
  Migration: replace `client.function_name(x)` with `client.new_function_name(x, &default_param)`.
```

### 3.2 `supports_interface` capability signal

During the **Deprecated** state, `supports_interface("old_cap")` continues to
return `true` (the function still works). During the **Sunset-Announced** and
beyond state:

- The testnet deployment is updated first; `supports_interface("old_cap")`
  returns `false` there before mainnet.
- On mainnet removal: `supports_interface("old_cap")` returns `false`.

New replacement capabilities receive their own capability symbol immediately on
introduction. Integrators should feature-detect the new symbol before
migrating:

```rust
if client.supports_interface(&Symbol::new(&env, "new_cap")) {
    // Use new path
} else {
    // Fall back to old path (still valid during deprecated window)
}
```

### 3.3 Rustdoc `#[deprecated]` annotation

Where a Rust-level wrapper function is deprecated, it is annotated:

```rust
/// Use `new_function_name` instead.
///
/// Deprecated since interface v3. Will be removed in interface v5.
#[deprecated(since = "3.0.0", note = "use `new_function_name` instead")]
pub fn old_function_name(...) { ... }
```

The `#![allow(deprecated)]` directive at the top of `lib.rs` suppresses the
warning inside the contract itself (the `contractimpl` macro calls spec
functions for all entries including deprecated ones). Integrators who enable
`#![deny(deprecated)]` in their own crates will get a compile-time warning
pointing at the migration note.

---

## 4. Compatibility Tests

Every deprecated function **must** have at least one test in
`src/test_deprecation_compat.rs` (see [§7 below](#7-test-file)) verifying that:

1. The function still compiles and is callable.
2. It returns the documented value (success path) or the documented error
   (failure path).
3. The test is tagged with a comment `// DEPRECATED_COMPAT: pinned for
   interface vN compatibility` so it is easy to locate when removing the
   function.

These tests are run on every CI pass. A failing compat test means a deprecated
symbol was broken before its removal window elapsed — that is a regression.

---

## 5. Migration Instructions Template

Each deprecation CHANGELOG entry must include a "Migration" subsection
following this template:

```markdown
**Migration from `old_function_name` to `new_function_name`:**

1. **Re-generate client bindings** after pointing at the new contract ID or
   after upgrading the WASM. The generated `LedgerLensScoreContractClient`
   will expose `new_function_name` automatically.

2. **Update call sites:**
   ```rust
   // Before
   let result = client.old_function_name(&wallet, &pair);

   // After
   let result = client.new_function_name(&wallet, &pair, &new_param);
   ```

3. **Update error handling** if the return type changed. Check
   `docs/interface-spec.md` for the new error variants.

4. **Validate on testnet** before deploying to mainnet. The testnet deployment
   is updated at least 30 days before the mainnet removal.
```

---

## 6. Sunset Checklist (for maintainers)

Use this checklist when progressing a symbol from **Deprecated** to **Removed**:

- [ ] Verify that at least 2 full major interface versions have elapsed since
      the deprecation announcement.
- [ ] Add a `[Unreleased]` CHANGELOG entry with the target removal date and a
      "Migration" section.
- [ ] Update testnet deployment; confirm `supports_interface("old_cap")` returns
      `false` on testnet.
- [ ] Announce removal in the project's communication channels with the 30-day
      countdown.
- [ ] After 30 days: remove the function from `lib.rs`, update
      `interface-spec.md`, increment `INTERFACE_VERSION`, delete the old
      compat test(s) from `test_deprecation_compat.rs`.
- [ ] Move the CHANGELOG entry from `[Unreleased]` to the dated release
      section.
- [ ] Update `docs/abi-compatibility-notes.md` with a new row in the breaking-
      changes table.

---

## 7. Test File

Compatibility tests live in
[`contracts/ledgerlens-score/src/test_deprecation_compat.rs`](../contracts/ledgerlens-score/src/test_deprecation_compat.rs).

The module header explains which interface version each test guards. Do **not**
delete tests from this file unless the sunset checklist above has been fully
completed for the corresponding symbol.

---

## 8. ABI Impact & Storage Compatibility

Deprecated functions do not change the storage layout — they read from or write
to the same keys as always. Removing a function in a future version does not
orphan existing storage entries; those entries remain valid and are readable by
the contract's non-deprecated paths.

If a deprecation *does* involve a storage key rename or removal, that is
classified as a **breaking change** and must follow the full process in
[`interface-versioning-policy.md §3`](interface-versioning-policy.md).

---

## 9. Resource Bounds

Deprecated functions share the same resource budget as their replacements.
There is no penalty for calling a deprecated function during the Deprecated or
Sunset-Announced window. The only cost is the eventual migration effort.

---

## 10. Cross-References

- Interface function signatures and capability table:
  [`docs/interface-spec.md`](interface-spec.md)
- Version numbering and breaking vs. non-breaking changes:
  [`docs/interface-versioning-policy.md`](interface-versioning-policy.md)
- ABI snapshot and CI enforcement:
  [`docs/abi-compatibility-notes.md`](abi-compatibility-notes.md)
- Migration guides for past breaking changes:
  [`CHANGELOG.md`](../CHANGELOG.md)
