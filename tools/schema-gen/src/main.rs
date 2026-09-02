//! CLI for the schema-gen spike PoC.
//!
//! Regenerates the committed cross-repo `RiskScore` artifacts in contract-spec
//! (XDR) field order:
//!
//! ```text
//! cargo run -p schema-gen                # native spec source (default)
//! cargo run -p schema-gen -- --wasm target/wasm32-unknown-unknown/release/ledgerlens_score.wasm
//! cargo run -p schema-gen -- --check     # exit non-zero if committed artifacts are stale
//! ```

use anyhow::{bail, Context, Result};
use schema_gen::{
    native_risk_score_struct, struct_to_json_schema, struct_to_python, struct_to_typescript,
    wasm_risk_score_struct, DEFAULT_OUTPUT_DIR, PYTHON_FILE, RISK_SCORE_NAME, SCHEMA_FILE,
    TYPESCRIPT_FILE,
};
use std::path::{Path, PathBuf};

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();

    let mut wasm_path: Option<String> = None;
    let mut out_dir = DEFAULT_OUTPUT_DIR.to_string();
    let mut check = false;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--wasm" => {
                i += 1;
                wasm_path = Some(
                    args.get(i).context("`--wasm` requires a path to the contract WASM")?.clone(),
                );
            }
            "--out" => {
                i += 1;
                out_dir = args.get(i).context("`--out` requires a directory")?.clone();
            }
            "--check" => check = true,
            "--help" | "-h" => {
                print_usage();
                return Ok(());
            }
            unknown => bail!("unknown argument `{unknown}` (see --help)"),
        }
        i += 1;
    }

    let (source, struct_) = match &wasm_path {
        Some(path) => {
            let bytes =
                std::fs::read(path).with_context(|| format!("reading contract WASM {path}"))?;
            let struct_ = wasm_risk_score_struct(&bytes)?;
            (format!("contract WASM `{path}` (contractspecv0)"), struct_)
        }
        None => {
            let struct_ = native_risk_score_struct()?;
            (
                "contracts/ledgerlens-score/src/types.rs (`RiskScore::spec_xdr()`)".to_string(),
                struct_,
            )
        }
    };

    let schema = {
        let mut text = serde_json::to_string_pretty(&struct_to_json_schema(&struct_))?;
        text.push('\n');
        text
    };
    let typescript = struct_to_typescript(&struct_);
    let python = struct_to_python(&struct_);

    let dir = PathBuf::from(&out_dir);
    std::fs::create_dir_all(&dir)?;
    let changed = [
        write_if_changed(&dir.join(SCHEMA_FILE), &schema)?,
        write_if_changed(&dir.join(TYPESCRIPT_FILE), &typescript)?,
        write_if_changed(&dir.join(PYTHON_FILE), &python)?,
    ]
    .into_iter()
    .any(|changed| changed);

    println!("schema-gen: {RISK_SCORE_NAME} from {source}");
    println!(
        "schema-gen: {}",
        [SCHEMA_FILE, TYPESCRIPT_FILE, PYTHON_FILE]
            .iter()
            .map(|file| dir.join(file).display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    );

    if check && changed {
        bail!(
            "generated `{RISK_SCORE_NAME}` artifacts are stale; \
             run `cargo run -p schema-gen` and commit the changes"
        );
    }
    Ok(())
}

/// Write `contents` to `path` only when it differs; returns whether anything
/// was written. Used both for regeneration and for CI-style drift detection.
fn write_if_changed(path: &Path, contents: &str) -> Result<bool> {
    match std::fs::read_to_string(path) {
        Ok(existing) if existing == contents => Ok(false),
        Ok(_) => {
            std::fs::write(path, contents)?;
            Ok(true)
        }
        Err(_) => {
            std::fs::write(path, contents)?;
            Ok(true)
        }
    }
}

fn print_usage() {
    println!(
        "schema-gen — generate cross-repo RiskScore bindings from the Soroban contract-spec\n\n\
         USAGE:\n    schema-gen [OPTIONS]\n\n\
         OPTIONS:\n    --wasm <path>   Read the RiskScore spec from a built/deployed contract WASM\n\
         \x20                  (default: the native RiskScore::spec_xdr() from the source crate)\n\
         \x20  --out <dir>     Output directory (default: schemas/)\n\
         \x20  --check         Fail instead of rewriting whenever committed artifacts are stale\n\
         \x20  -h, --help      Print this help\n"
    );
}
