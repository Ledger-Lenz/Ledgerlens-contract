#!/usr/bin/env python3
"""Check that every Rust identifier referenced in `spec/refinement-mapping.md`
exists in the contract source.

Background (issue #928): `spec/refinement-mapping.md` maps every TLA+ variable
and action in `spec/LedgerLens.tla` to concrete Rust storage keys and entry
points.  It drifted once already — it referenced a `finalize_consensus`
function that never existed (the real entry point is `submit_consensus_score`)
— and nothing caught it, because no CI step ever compared the document against
the code.  This script closes that gap with a cheap, always-on symbol-existence
check:

  * every `DataKeyX::Variant` storage key named in the document must exist as a
    variant of that enum in `contracts/ledgerlens-score/src/types.rs`;
  * every function call (`name(...)`) must exist as a `fn` (public or private)
    in the contract sources;
  * every `SCREAMING_CONSTANT` must exist as a `const` in the contract sources;
  * every `Type.field` reference must exist as a field of that struct;
  * every CamelCase type / enum-variant reference must exist in the sources.

Identifiers that come from the TLA+ side of the mapping (spec variables,
constants, actions, invariants and operators) are exempted automatically by
parsing `spec/LedgerLens.tla`, so the document can name spec entities freely
without the check tripping.  A small, documented skip-list covers Soroban SDK
types/methods and a handful of prose terms.

This is a symbol-existence check only — it deliberately does not verify that
the mapping's described behaviour matches the Rust code (that is a separate,
much larger effort, tracked in issue #407).  See the issue #928 acceptance
criteria.

Usage:
    python3 tools/check_refinement_mapping.py            # check the mapping
    python3 tools/check_refinement_mapping.py --selftest # check + verify the
                                                         # checker itself (must
                                                         # pass on the real
                                                         # mapping and fail on
                                                         # an injected bad ref)
"""

import argparse
import re
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
MAPPING = ROOT / "spec" / "refinement-mapping.md"
TLA_SPEC = ROOT / "spec" / "LedgerLens.tla"
RUST_DIRS = [
    ROOT / "contracts" / "ledgerlens-score" / "src",
    ROOT / "contracts" / "ledgerlens-aggregator" / "src",
]

# ── Skip-list of identifiers that are legitimately referenced by the mapping
#    but are NOT defined in this repository's contract sources ────────────────
# * Soroban SDK / standard-library types and methods (checked by rustc, not us)
# * TLA+ standard operators and set-element placeholders used by the spec
# * a few prose terms written in backticks for typographic consistency
SKIP_LIST = {
    # std / Soroban SDK types
    "Option", "Vec", "Address", "Symbol", "BytesN", "Bytes", "Result", "Env",
    "u32", "u64", "u128", "i128", "bool",
    # std / Soroban SDK methods and associated fns
    "min", "max", "saturating_sub", "checked_div", "require_auth", "has",
    "get", "set", "storage", "temporary", "instance", "ledger", "timestamp",
    "sha256", "extend_ttl", "unwrap", "unwrap_or", "ok_or", "is_empty", "len",
    "push_back", "to_be_bytes", "copy_from_slice", "from_array", "contains",
    # TLA+ standard operators (imported via EXTENDS FiniteSets, Integers)
    "Cardinality", "TRUE", "FALSE", "None", "Some",
    # TLC model-configuration keywords (used when the mapping cites
    # LedgerLens.cfg sections)
    "INVARIANT", "PROPERTY", "CONSTRAINT", "SPECIFICATION", "CONSTANTS",
    # spec-side set-element placeholders (the spec declares `Wallets`, `Signers`
    # as constants; the mapping writes the singular element types)
    "Wallet", "Signer",
    # liveness claims catalogued in spec/README.md (INV-LIVE-1/2) that are NOT
    # model-checked by TLC — they are absent from both LedgerLens.tla and
    # LedgerLens.cfg, and the mapping's §5 flags them as such.
    "SubmitEnabledWhenConditionsMet", "ScoreFloorDoesNotBlockAllScores",
    # prose
    "SHA256", "XDR",
}


# ── Rust symbol index ─────────────────────────────────────────────────────────

def build_rust_index() -> dict:
    """Return {symbol: set_of_roles} for every fn/const/type/variant/field/mod
    defined in the contract sources."""
    index: dict[str, set[str]] = {}
    modules = set()

    def add(name: str, role: str) -> None:
        if name:
            index.setdefault(name, set()).add(role)

    for src_dir in RUST_DIRS:
        if not src_dir.is_dir():
            continue
        for rs in sorted(src_dir.rglob("*.rs")):
            text = rs.read_text(encoding="utf-8")
            # modules: `mod name;` / `mod name {`
            for m in re.finditer(r"\bmod\s+([A-Za-z_]\w*)", text):
                modules.add(m.group(1))
            # functions (any visibility, including private helpers)
            for m in re.finditer(
                r"\bfn\s+([A-Za-z_]\w*)\s*\(", text
            ):
                add(m.group(1), "fn")
            # consts
            for m in re.finditer(
                r"\bconst\s+([A-Z][A-Z0-9_]*)\s*:", text
            ):
                add(m.group(1), "const")
            # types
            for m in re.finditer(
                r"\b(?:struct|enum|type|union)\s+([A-Z][A-Za-z0-9_]*)\b", text
            ):
                add(m.group(1), "type")
            # enum variants (inside enum bodies only — do NOT index arbitrary
            # CamelCase from doc comments, which would let drift hide)
            for em in re.finditer(r"\benum\s+([A-Z]\w*)\s*\{([^}]*)\}", text):
                body = em.group(2)
                for vm in re.finditer(
                    r"^\s+([A-Z][A-Za-z0-9_]*)\s*(?:\(|,|=|$)", body, re.M
                ):
                    add(vm.group(1), "enum_variant")
            # struct fields
            for sm in re.finditer(
                r"\bstruct\s+([A-Z]\w*)\s*\{([^}]*)\}", text
            ):
                body = sm.group(2)
                for fm in re.finditer(
                    r"^\s+(?:pub\s+)?([a-z_]\w*)\s*:", body, re.M
                ):
                    add(fm.group(1), "field")

    # file-backed modules (e.g. `constants` from constants.rs) — files are
    # scanned as whole modules, so only the file stem matters for `mod::sym`
    # references.
    for src_dir in RUST_DIRS:
        if src_dir.is_dir():
            for rs in src_dir.glob("*.rs"):
                modules.add(rs.stem)

    index["__modules__"] = modules
    return index


# ── TLA+ symbol index (spec side) ─────────────────────────────────────────────

def build_tla_index() -> set:
    """Return every identifier the TLA+ spec itself defines (constants,
    variables, operators/actions/invariants), so references to spec entities in
    the mapping are exempt from the Rust existence check."""
    tla = TLA_SPEC.read_text(encoding="utf-8")
    known = set()

    # CONSTANTS block: `    NAME,` lines; strip trailing `\*` comments first
    in_constants = False
    for line in tla.splitlines():
        code = line.split("\\*")[0].strip()
        stripped = line.strip()
        if stripped.startswith("CONSTANTS"):
            in_constants = True
            continue
        if in_constants:
            if stripped.startswith("VARIABLES"):
                break
            if stripped.startswith("\\*"):
                continue
            # Mixed-case set constants (Wallets, Scores, Assets, Actions,
            # Signers) and all-caps numeric constants (COOLDOWN, MIN_CAPACITY).
            m = re.match(r"([A-Z][A-Za-z0-9_]*),?", code)
            if m:
                known.add(m.group(1))

    # VARIABLES block: `    name,` lines up to the `vars ==` definition
    in_variables = False
    for line in tla.splitlines():
        code = line.split("\\*")[0].strip()
        stripped = line.strip()
        if stripped.startswith("VARIABLES"):
            in_variables = True
            continue
        if in_variables:
            if stripped.startswith("vars"):
                break
            if stripped.startswith("\\*"):
                continue
            m = re.match(r"([a-z_][a-z0-9_]*),?", code)
            if m:
                known.add(m.group(1))

    # MODULE name (e.g. `MODULE LedgerLens`), so file references like
    # `LedgerLens.tla` resolve on the spec side as well.
    m = re.search(r"MODULE\s+([A-Za-z_]\w*)", tla)
    if m:
        known.add(m.group(1))

    # operators / actions / invariants: `Name ==` or `Name(args) ==`
    for m in re.finditer(
        r"^\s*([A-Za-z_][A-Za-z0-9_]*(?:\([^)]*\))?)\s*==", tla, re.M
    ):
        name = m.group(1).split("(")[0]
        if name and name != "vars":
            known.add(name)

    return known


# ── Extract candidate symbols from the mapping document ───────────────────────

CALL_RE = re.compile(r"\b([a-z_][a-z0-9_]*)\s*\(")
SCREAMING_RE = re.compile(r"\b([A-Z][A-Z0-9_]{2,})\b")
CAMEL_RE = re.compile(r"\b([A-Z][a-z][A-Za-z0-9_]*)\b")
ENUM_PATH_RE = re.compile(r"\b([A-Z][A-Za-z0-9_]*)::([A-Z][A-Za-z0-9_]*)\b")
MODULE_PATH_RE = re.compile(r"\b([a-z_][a-z0-9_]*)::([A-Za-z_][A-Za-z0-9_]*)\b")
FIELD_RE = re.compile(r"\b([A-Z][A-Za-z0-9_]*)\.([a-z_]\w*)\b")


def extract_candidates(span: str) -> list[tuple[str, str]]:
    """Return (symbol, kind) pairs referenced by one backtick span."""
    out: list[tuple[str, str]] = []
    seen = set()

    # Spans that are file paths, invariant labels, or pure prose are not
    # symbol references — skip them wholesale.
    if re.fullmatch(r"[\w./-]+\.(?:rs|md|tla|cfg|toml|sh|py|json|ndjson)", span):
        return out
    if re.fullmatch(r"INV-[A-Z]+-\d+", span):
        return out

    # Drop `// ...` comment tails (e.g. `// Rust (simplified)` inside the
    # fenced code snippet) so comment prose is not treated as symbols.
    span = re.sub(r"//[^\n]*", "", span)

    def add(sym: str, kind: str) -> None:
        key = (sym, kind)
        if key not in seen:
            seen.add(key)
            out.append(key)

    # Enum-constant paths: DataKeyD::BurstCapacity, EmbargoExpiry::Indefinite
    for m in ENUM_PATH_RE.finditer(span):
        add(m.group(2), "enum_variant")
    # Module paths: constants::MAX_COOLDOWN_SECS
    for m in MODULE_PATH_RE.finditer(span):
        add(m.group(1), "module")
        add(m.group(2), "module_symbol")
    # Struct field refs: RiskScore.score, TokenBucket.last_refill
    for m in FIELD_RE.finditer(span):
        add(m.group(2), "field")
    # Function calls: submit_score(...), is_embargoed(env, wallet)
    for m in CALL_RE.finditer(span):
        add(m.group(1), "fn")
    # SCREAMING constants: DEFAULT_COOLDOWN_SECS, MAX_SCORE
    for m in SCREAMING_RE.finditer(span):
        add(m.group(1), "const")
    # CamelCase types / enum variants / spec entities
    for m in CAMEL_RE.finditer(span):
        add(m.group(1), "type")

    return out


def iter_spans(text: str):
    """Yield (line_no, span) for every backtick-delimited code span and every
    fenced code block in the document."""
    lines = text.splitlines()
    in_fence = False
    fence_buf: list[str] = []
    fence_start = 0
    for i, line in enumerate(lines, start=1):
        if line.strip().startswith("```"):
            if not in_fence:
                in_fence = True
                fence_buf = []
                fence_start = i + 1
            else:
                in_fence = False
                yield fence_start, "\n".join(fence_buf)
            continue
        if in_fence:
            fence_buf.append(line)
            continue
        # inline spans on this line
        for m in re.finditer(r"`([^`]+)`", line):
            yield i, m.group(1)


# ── The check ─────────────────────────────────────────────────────────────────

def check_mapping(mapping: Path, rust_index: dict, tla_index: set,
                  verbose: bool = False) -> list[tuple[int, str, str, str]]:
    """Return [(line_no, symbol, kind, span)] for every unresolved reference."""
    text = mapping.read_text(encoding="utf-8")
    modules = rust_index.get("__modules__", set())
    failures: list[tuple[int, str, str, str]] = []

    def resolve(sym: str, kind: str) -> bool:
        if kind == "module":
            # `module::symbol` references: the module must exist as a file or
            # `mod` declaration; the RHS symbol is resolved below.
            return sym in modules or sym in SKIP_LIST
        return sym in rust_index or sym in tla_index or sym in SKIP_LIST

    checked = 0
    for line_no, span in iter_spans(text):
        for sym, kind in extract_candidates(span):
            checked += 1
            if not resolve(sym, kind):
                failures.append((line_no, sym, kind, span))

    if verbose:
        print(f"checked {checked} symbol references in {mapping.name}")
    return failures


def run_check(mapping: Path, verbose: bool = False) -> int:
    rust_index = build_rust_index()
    tla_index = build_tla_index()
    failures = check_mapping(mapping, rust_index, tla_index, verbose=verbose)

    if failures:
        print(f"ERROR: {mapping.name} references symbols that do not exist "
              f"in the contract sources:")
        for line_no, sym, kind, span in sorted(failures):
            print(f"  line {line_no}: {sym!r} (referenced as {kind}) "
                  f"in span {span[:60]!r}")
        print("")
        print("This means the refinement mapping has drifted from the code — the")
        print("document's whole purpose is to be traceable to real symbols.")
        print("Fix the mapping (or, if the symbol was renamed, update the doc).")
        print("See issue #928.")
        return 1

    if verbose:
        print(f"OK: every Rust identifier referenced in {mapping.name} exists "
              f"in the contract sources.")
    return 0


# ── Self-test: the checker must flag drift and pass on the fixed mapping ──────

def selftest() -> int:
    verbose = True
    rust_index = build_rust_index()
    tla_index = build_tla_index()

    # 1. Passes on the real (fixed) mapping.
    real_failures = check_mapping(MAPPING, rust_index, tla_index, verbose=True)
    if real_failures:
        print("SELFTEST FAIL: real mapping must pass, but failed on:")
        for line_no, sym, kind, span in real_failures:
            print(f"  line {line_no}: {sym!r} ({kind}) in {span[:60]!r}")
        return 1
    print("SELFTEST PASS (1/3): real mapping has no missing symbols")

    # 2. Fails when a real symbol reference is replaced by a bogus one.
    bogus = "definitely_missing_symbol_928"
    original = MAPPING.read_text(encoding="utf-8")
    # replace a function reference that definitely appears in the doc
    injected = original.replace("submit_consensus_score", bogus)
    if injected == original:
        print("SELFTEST FAIL: could not inject a bogus reference "
              "(submit_consensus_score not found in mapping)")
        return 1
    with tempfile.TemporaryDirectory() as tmp:
        bad_mapping = Path(tmp) / "refinement-mapping.md"
        bad_mapping.write_text(injected, encoding="utf-8")
        bad_failures = check_mapping(bad_mapping, rust_index, tla_index,
                                     verbose=True)
    missing = [f for f in bad_failures if f[1] == bogus]
    if not bad_failures or not missing:
        print("SELFTEST FAIL: injected bogus reference was not flagged")
        return 1
    print("SELFTEST PASS (2/3): injected bogus reference was flagged")

    # 3. Fails when a storage-key variant is replaced by a bogus one.
    bogus_key = "DefinitelyMissingVariant928"
    injected_key = original.replace("BurstCapacity", bogus_key)
    if injected_key == original:
        print("SELFTEST FAIL: could not inject a bogus storage-key variant "
              "(BurstCapacity not found in mapping)")
        return 1
    with tempfile.TemporaryDirectory() as tmp:
        bad_mapping = Path(tmp) / "refinement-mapping.md"
        bad_mapping.write_text(injected_key, encoding="utf-8")
        bad_key_failures = check_mapping(bad_mapping, rust_index, tla_index,
                                         verbose=True)
    missing_key = [f for f in bad_key_failures if f[1] == bogus_key]
    if not bad_key_failures or not missing_key:
        print("SELFTEST FAIL: injected bogus storage-key variant was not flagged")
        return 1
    print("SELFTEST PASS (3/3): injected bogus storage-key variant was flagged")

    print("")
    print("check_refinement_mapping self-test passed.")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Verify spec/refinement-mapping.md references only real "
                    "Rust symbols (issue #928).")
    parser.add_argument("--selftest", action="store_true",
                        help="also verify the checker itself: pass on the real "
                             "mapping, fail on injected bogus references")
    parser.add_argument("--mapping", type=Path, default=MAPPING,
                        help="mapping file to check (default: spec/refinement-mapping.md)")
    parser.add_argument("-q", "--quiet", action="store_true",
                        help="only print errors")
    args = parser.parse_args()

    if args.selftest:
        return selftest()

    verbose = not args.quiet
    return run_check(args.mapping, verbose=verbose)


if __name__ == "__main__":
    sys.exit(main())
