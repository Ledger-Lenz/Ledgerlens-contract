use std::fs;
use std::path::Path;

#[test]
fn test_docs_exist_and_are_non_empty() {
    let docs = [
        "docs/security/contributor-review-checklists.md",
        "docs/operations/runbook.md",
    ];

    for doc_path in &docs {
        let path = Path::new(doc_path);
        assert!(
            path.exists(),
            "Documentation file missing: {}",
            doc_path
        );
        let metadata = fs::metadata(path).expect("Failed to read doc metadata");
        assert!(
            metadata.len() > 0,
            "Documentation file is empty: {}",
            doc_path
        );
    }
}

// ── Drift checks: documentation content vs. contract source ──────────────────
//
// The tests above only verify that docs exist and have bytes. These checks go
// further: they parse a handful of version numbers that `docs/interface-spec.md`
// claims about the contract and verify each one matches the actual source. The
// interface spec is the canonical, versioned integration contract for third-party
// callers, so a spec that drifts from the source silently breaks integrators.

/// Extracts `N` from `pub const CONTRACT_VERSION: u32 = N;` in `constants.rs`.
fn extract_contract_version(constants_src: &str) -> String {
    let marker = "CONTRACT_VERSION: u32 = ";
    constants_src
        .lines()
        .find_map(|line| {
            let idx = line.find(marker)?;
            Some(
                line[idx + marker.len()..]
                    .trim()
                    .trim_end_matches(';')
                    .trim()
                    .to_string(),
            )
        })
        .expect("CONTRACT_VERSION not found in constants.rs")
}

/// Extracts `N` from `interface_version: N,` inside `get_interface_metadata` in
/// `lib.rs`.
fn extract_interface_version(lib_src: &str) -> String {
    let marker = "interface_version: ";
    lib_src
        .lines()
        .find_map(|line| {
            let idx = line.find(marker)?;
            Some(
                line[idx + marker.len()..]
                    .trim()
                    .trim_end_matches(',')
                    .trim()
                    .to_string(),
            )
        })
        .expect("interface_version not found in lib.rs")
}

/// Collects the value of every `` currently `N` `` claim in a doc. `interface-spec.md`
/// uses this exact phrasing for "the contract version as of the current build".
fn extract_current_version_claims(doc_src: &str) -> Vec<&str> {
    let marker = "currently `";
    doc_src
        .lines()
        .filter_map(|line| {
            let idx = line.find(marker)?;
            let rest = &line[idx + marker.len()..];
            rest.splitn(2, '`').next()?.trim()
        })
        .collect()
}

/// Extracts `N` from the `**Interface version:** N` header of a spec doc.
fn extract_documented_interface_version(doc_src: &str) -> &str {
    let marker = "Interface version:** ";
    doc_src
        .lines()
        .find_map(|line| {
            let idx = line.find(marker)?;
            line[idx + marker.len()..].split_whitespace().next()
        })
        .expect("'Interface version:** N' header not found in interface-spec.md")
}

#[test]
fn test_interface_spec_contract_version_matches_source() {
    let constants_src =
        fs::read_to_string("contracts/ledgerlens-score/src/constants.rs")
            .expect("failed to read constants.rs");
    let code_version = extract_contract_version(&constants_src);

    let spec_src =
        fs::read_to_string("docs/interface-spec.md").expect("failed to read interface-spec.md");

    let doc_claims = extract_current_version_claims(&spec_src);
    assert!(
        !doc_claims.is_empty(),
        "docs/interface-spec.md should state the current contract version (e.g. `currently `N``)"
    );

    for claim in doc_claims {
        assert_eq!(
            claim,
            code_version,
            "docs/interface-spec.md claims the contract version is `{claim}`, but \
             CONTRACT_VERSION in constants.rs is `{code_version}` — reconcile the doc"
        );
    }
}

#[test]
fn test_interface_spec_interface_version_matches_source() {
    let lib_src =
        fs::read_to_string("contracts/ledgerlens-score/src/lib.rs").expect("failed to read lib.rs");
    let code_version = extract_interface_version(&lib_src);

    let spec_src =
        fs::read_to_string("docs/interface-spec.md").expect("failed to read interface-spec.md");
    let doc_version = extract_documented_interface_version(&spec_src);

    assert_eq!(
        code_version,
        doc_version,
        "docs/interface-spec.md header claims interface version `{doc_version}`, but \
         get_interface_metadata publishes `{code_version}` — reconcile the doc"
    );
}