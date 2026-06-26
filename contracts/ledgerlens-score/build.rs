use std::fs;
use std::path::Path;

// CI guard: fails the build if an Error variant has no corresponding test in error_coverage.rs
fn main() {
    let src_dir = "src";

    // Count Error variants in errors.rs
    let errors_path = Path::new(src_dir).join("errors.rs");
    let errors_content = fs::read_to_string(&errors_path)
        .expect("Failed to read errors.rs");

    // Match lines like "    VariantName = <number>,"
    let variant_count = errors_content
        .lines()
        .filter(|line| {
            let trimmed = line.trim();
            // Lines that define enum variants have pattern like "Name = num," or "Name,"
            trimmed.ends_with(',') &&
            !trimmed.starts_with("//") &&
            !trimmed.starts_with('#') &&
            trimmed.contains('=')
        })
        .count();

    // Count test functions in error_coverage.rs
    let tests_path = Path::new(src_dir).join("error_coverage.rs");
    let tests_content = fs::read_to_string(&tests_path)
        .expect("Failed to read error_coverage.rs");

    let test_count = tests_content
        .lines()
        .filter(|line| line.trim().starts_with("fn test_error_"))
        .count();

    if variant_count != test_count {
        panic!(
            "Error variant count ({}) does not match error_coverage.rs test count ({}). \
            Every Error variant must have a corresponding test_error_* function.",
            variant_count, test_count
        );
    }

    println!("cargo:warning=Error coverage: {} variants, {} tests ✓", variant_count, test_count);
}
