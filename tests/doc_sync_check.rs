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
