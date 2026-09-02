//! Spike proof-of-concept for cross-repository schema/type generation for the
//! canonical [`ledgerlens_score::RiskScore`] contract type.
//!
//! The generation source is the Soroban **contract-spec (XDR)** metadata that
//! the `#[contracttype]` derive already compiles into every contract:
//!
//! * natively via [`ledgerlens_score::RiskScore::spec_xdr()`] — the exact
//!   spec bytes generated from the Rust struct itself, and
//! * from a built/deployed contract WASM via [`soroban_spec::read::from_wasm`]
//!   — the same read path Soroban's SDK typed-client generators use.
//!
//! On top of the field shape (names, order, types) we layer the semantic score
//! domain `[0, 100]`, sourced from the contract's own
//! [`ledgerlens_score::constants`] range constants so the emitted schema
//! cannot drift from what `submit_score` actually enforces.

use anyhow::{bail, Context, Result};
use serde_json::{json, Map, Value};
use soroban_sdk::xdr::{Limits, ReadXdr, ScSpecEntry, ScSpecTypeDef, ScSpecUdtStructV0};

/// The canonical cross-repo name of the risk-score struct.
pub const RISK_SCORE_NAME: &str = "RiskScore";

/// Default directory the CLI emits generated artifacts into.
pub const DEFAULT_OUTPUT_DIR: &str = "schemas";

/// Filename of the generated JSON Schema artifact.
pub const SCHEMA_FILE: &str = "risk_score.schema.json";

/// Filename of the generated TypeScript artifact.
pub const TYPESCRIPT_FILE: &str = "risk_score.ts";

/// Filename of the generated Python (Pydantic) artifact.
pub const PYTHON_FILE: &str = "risk_score.py";

/// Decode the `RiskScore` UDT struct entry natively, straight from the
/// `#[contracttype]`-generated `spec_xdr()` on the Rust type itself — the same
/// bytes the contract's `contractspecv0` section is compiled from.
pub fn native_risk_score_struct() -> Result<ScSpecUdtStructV0> {
    let bytes = ledgerlens_score::RiskScore::spec_xdr();
    let entry = ScSpecEntry::from_xdr(bytes.as_slice(), Limits::none())
        .context("`RiskScore::spec_xdr()` must decode to an `ScSpecEntry`")?;
    into_risk_score_struct(entry)
}

/// Read the `RiskScore` UDT struct entry from a built/deployed contract WASM,
/// using the same `contractspecv0` read path the SDK typed-client generators
/// use.
pub fn wasm_risk_score_struct(wasm: &[u8]) -> Result<ScSpecUdtStructV0> {
    let entries = soroban_spec::read::from_wasm(wasm).context("contract WASM spec must parse")?;
    entries
        .into_iter()
        .find(|entry| {
            matches!(entry, ScSpecEntry::UdtStructV0(s) if xdr_str(&s.name) == RISK_SCORE_NAME)
        })
        .map(into_risk_score_struct)
        .unwrap_or_else(|| bail!("contract spec contains no `{RISK_SCORE_NAME}` UDT struct"))
}

fn into_risk_score_struct(entry: ScSpecEntry) -> Result<ScSpecUdtStructV0> {
    match entry {
        ScSpecEntry::UdtStructV0(struct_) if xdr_str(&struct_.name) == RISK_SCORE_NAME => {
            Ok(struct_)
        }
        other => bail!("expected `UdtStructV0` named `{RISK_SCORE_NAME}`, got {other:?}"),
    }
}

/// Returns the `[min, max]` domain for the bounded components of the risk
/// score, sourced from the contract's own range constants rather than a
/// hard-coded copy. All five bounded components (`score`, `confidence`, and
/// the three sub-scores) live on the same `[MIN_SCORE, MAX_SCORE]` domain that
/// `submit_score` enforces; only `score` has a dedicated named constant (see
/// the spike report for the follow-up to promote the others).
pub fn risk_domain_range(field: &str) -> Option<(u32, u32)> {
    match field {
        "score" | "confidence" | "benford_score" | "ml_score" | "network_score" => {
            Some((ledgerlens_score::constants::MIN_SCORE, ledgerlens_score::constants::MAX_SCORE))
        }
        _ => None,
    }
}

fn xdr_str(bytes: &[u8]) -> String {
    std::str::from_utf8(bytes).expect("contract spec strings must be UTF-8").to_owned()
}

/// JSON Schema fragment for a fixed-size byte string (base64 wire encoding).
fn bytes_schema(n: u32) -> Value {
    let mut schema = json!({ "type": "string", "contentEncoding": "base64" });
    if n > 0 {
        schema["minLength"] = json!(n);
        schema["maxLength"] = json!(n);
    }
    schema
}

/// Map an XDR `ScSpecTypeDef` to its JSON Schema (draft-07) fragment.
pub fn json_schema_type(type_: &ScSpecTypeDef) -> Value {
    match type_ {
        ScSpecTypeDef::Val => json!({}),
        ScSpecTypeDef::Bool => json!({ "type": "boolean" }),
        ScSpecTypeDef::Void => json!({}),
        ScSpecTypeDef::Error => json!({ "type": "integer" }),
        ScSpecTypeDef::U32
        | ScSpecTypeDef::U64
        | ScSpecTypeDef::U128
        | ScSpecTypeDef::U256
        | ScSpecTypeDef::Timepoint
        | ScSpecTypeDef::Duration => json!({ "type": "integer", "minimum": 0 }),
        ScSpecTypeDef::I32 | ScSpecTypeDef::I64 | ScSpecTypeDef::I128 | ScSpecTypeDef::I256 => {
            json!({ "type": "integer" })
        }
        ScSpecTypeDef::Bytes => bytes_schema(0),
        ScSpecTypeDef::BytesN(bytes) => bytes_schema(bytes.n),
        ScSpecTypeDef::String | ScSpecTypeDef::Symbol | ScSpecTypeDef::Address => {
            json!({ "type": "string" })
        }
        ScSpecTypeDef::Option(option) => {
            json!({ "anyOf": [json_schema_type(&option.value_type), json!({"type": "null"})] })
        }
        ScSpecTypeDef::Result(result) => json!({
            "anyOf": [
                json!({"type": "object", "properties": {"ok": json_schema_type(&result.ok_type)}, "required": ["ok"], "additionalProperties": false}),
                json!({"type": "object", "properties": {"err": json_schema_type(&result.error_type)}, "required": ["err"], "additionalProperties": false}),
            ]
        }),
        ScSpecTypeDef::Vec(vec) => {
            json!({ "type": "array", "items": json_schema_type(&vec.element_type) })
        }
        ScSpecTypeDef::Map(map) => json!({
            "type": "object",
            "additionalProperties": json_schema_type(&map.value_type)
        }),
        ScSpecTypeDef::Tuple(tuple) => json!({
            "type": "array",
            "items": tuple.value_types.iter().map(json_schema_type).collect::<Vec<_>>()
        }),
        ScSpecTypeDef::Udt(udt) => json!({ "$ref": format!("#/$defs/{}", xdr_str(&udt.name)) }),
    }
}

/// Attach the semantic score-domain range to a field's schema fragment.
fn with_risk_range(field: &str, mut schema: Value) -> Value {
    if let Some((min, max)) = risk_domain_range(field) {
        if let Some(object) = schema.as_object_mut() {
            object.insert("minimum".to_string(), json!(min));
            object.insert("maximum".to_string(), json!(max));
        }
    }
    schema
}

/// Convert a Soroban UDT struct spec entry into a JSON Schema (draft-07)
/// document preserving contract-spec (/XDR) field order — the alphabetical
/// ordering the `#[contracttype]` derive emits, which is what on-chain
/// decoders and SDK clients use.
pub fn struct_to_json_schema(struct_: &ScSpecUdtStructV0) -> Value {
    let mut properties = Map::new();
    let mut required = Vec::new();
    for field in struct_.fields.iter() {
        let name = xdr_str(&field.name);
        let schema = with_risk_range(&name, json_schema_type(&field.type_));
        properties.insert(name.clone(), schema);
        required.push(name);
    }
    let mut root = json!({
        "$schema": "http://json-schema.org/draft-07/schema#",
        "$id": "https://ledgerlens.dev/schemas/risk_score.schema.json",
        "title": xdr_str(&struct_.name),
        "$defs": {
            xdr_str(&struct_.name): {
                "type": "object",
                "additionalProperties": false,
                "properties": properties,
                "required": required,
            }
        }
    });
    let doc = xdr_str(&struct_.doc);
    if !doc.is_empty() {
        root["description"] = json!(doc);
    }
    root
}

/// Map an XDR `ScSpecTypeDef` to its TypeScript type.
fn ts_type(type_: &ScSpecTypeDef) -> String {
    match type_ {
        ScSpecTypeDef::Val => "unknown".to_string(),
        ScSpecTypeDef::Bool => "boolean".to_string(),
        ScSpecTypeDef::Void => "void".to_string(),
        ScSpecTypeDef::Error => "number".to_string(),
        ScSpecTypeDef::U32
        | ScSpecTypeDef::I32
        | ScSpecTypeDef::U64
        | ScSpecTypeDef::I64
        | ScSpecTypeDef::U128
        | ScSpecTypeDef::I128
        | ScSpecTypeDef::U256
        | ScSpecTypeDef::I256
        | ScSpecTypeDef::Timepoint
        | ScSpecTypeDef::Duration => "number".to_string(),
        ScSpecTypeDef::Bytes
        | ScSpecTypeDef::BytesN(_)
        | ScSpecTypeDef::String
        | ScSpecTypeDef::Symbol
        | ScSpecTypeDef::Address => "string".to_string(),
        ScSpecTypeDef::Option(option) => format!("{} | null", ts_type(&option.value_type)),
        ScSpecTypeDef::Result(result) => format!(
            "{{ ok: {} }} | {{ err: {} }}",
            ts_type(&result.ok_type),
            ts_type(&result.error_type)
        ),
        ScSpecTypeDef::Vec(vec) => format!("{}[]", ts_type(&vec.element_type)),
        ScSpecTypeDef::Map(map) => format!("Record<string, {}>", ts_type(&map.value_type)),
        ScSpecTypeDef::Tuple(tuple) => {
            let inner = tuple.value_types.iter().map(ts_type).collect::<Vec<_>>().join(", ");
            format!("[{inner}]")
        }
        ScSpecTypeDef::Udt(udt) => xdr_str(&udt.name),
    }
}

/// Convert a Soroban UDT struct spec entry into a TypeScript interface,
/// preserving contract-spec (/XDR) field order (alphabetical, as the
/// `#[contracttype]` derive emits).
pub fn struct_to_typescript(struct_: &ScSpecUdtStructV0) -> String {
    let mut lines = vec![
        "// Generated by tools/schema-gen from the Soroban contract-spec of".to_string(),
        "// contracts/ledgerlens-score/src/types.rs (`RiskScore`). Do not edit by hand."
            .to_string(),
        "// Fields are in contract-spec (XDR) order — alphabetical, matching the on-chain spec."
            .to_string(),
        format!("export interface {} {{", xdr_str(&struct_.name)),
    ];
    for field in struct_.fields.iter() {
        let name = xdr_str(&field.name);
        if let Some((min, max)) = risk_domain_range(&name) {
            lines.push(format!("  /** @minimum {min} @maximum {max} */"));
        }
        lines.push(format!("  {}: {};", name, ts_type(&field.type_)));
    }
    lines.push("}".to_string());
    lines.push(format!("// JSON-Schema twin: {SCHEMA_FILE}"));
    let mut text = lines.join("\n");
    text.push('\n');
    text
}

/// Map an XDR `ScSpecTypeDef` to its Pydantic type annotation, recording any
/// `typing` imports it needs in `needs`.
fn py_type(type_: &ScSpecTypeDef, needs: &mut Vec<&'static str>) -> String {
    match type_ {
        ScSpecTypeDef::Val => "Any".to_string(),
        ScSpecTypeDef::Bool => "bool".to_string(),
        ScSpecTypeDef::Void => "None".to_string(),
        ScSpecTypeDef::Error => "int".to_string(),
        ScSpecTypeDef::U32
        | ScSpecTypeDef::I32
        | ScSpecTypeDef::U64
        | ScSpecTypeDef::I64
        | ScSpecTypeDef::U128
        | ScSpecTypeDef::I128
        | ScSpecTypeDef::U256
        | ScSpecTypeDef::I256
        | ScSpecTypeDef::Timepoint
        | ScSpecTypeDef::Duration => "int".to_string(),
        ScSpecTypeDef::Bytes
        | ScSpecTypeDef::BytesN(_)
        | ScSpecTypeDef::String
        | ScSpecTypeDef::Symbol
        | ScSpecTypeDef::Address => "str".to_string(),
        ScSpecTypeDef::Option(option) => {
            needs.push("Optional");
            format!("Optional[{}]", py_type(&option.value_type, needs))
        }
        ScSpecTypeDef::Result(result) => {
            needs.push("Union");
            format!(
                "Union[{}, {}]",
                py_type(&result.ok_type, needs),
                py_type(&result.error_type, needs)
            )
        }
        ScSpecTypeDef::Vec(vec) => {
            needs.push("List");
            format!("List[{}]", py_type(&vec.element_type, needs))
        }
        ScSpecTypeDef::Map(_) => {
            needs.push("Dict");
            "Dict[str, Any]".to_string()
        }
        ScSpecTypeDef::Tuple(tuple) => {
            needs.push("Tuple");
            let inner =
                tuple.value_types.iter().map(|t| py_type(t, needs)).collect::<Vec<_>>().join(", ");
            format!("Tuple[{inner}]")
        }
        ScSpecTypeDef::Udt(udt) => xdr_str(&udt.name),
    }
}

fn typing_names() -> [&'static str; 6] {
    ["Any", "Dict", "List", "Optional", "Tuple", "Union"]
}

/// Convert a Soroban UDT struct spec entry into a Pydantic v2 model,
/// preserving contract-spec (/XDR) field order (alphabetical, as the
/// `#[contracttype]` derive emits).
pub fn struct_to_python(struct_: &ScSpecUdtStructV0) -> String {
    let mut needs = Vec::new();
    let mut field_lines = Vec::new();
    for field in struct_.fields.iter() {
        let name = xdr_str(&field.name);
        let annotation = py_type(&field.type_, &mut needs);
        let default = match &field.type_ {
            ScSpecTypeDef::Option(_) => " = None".to_string(),
            _ => risk_domain_range(&name)
                .map(|(min, max)| format!(" = Field(ge={min}, le={max})"))
                .unwrap_or_default(),
        };
        field_lines.push(format!("    {name}: {annotation}{default}"));
    }
    let used = typing_names()
        .iter()
        .filter(|name| needs.iter().any(|item| item == *name))
        .copied()
        .collect::<Vec<_>>();

    let mut lines = vec![
        "# Generated by tools/schema-gen from the Soroban contract-spec of".to_string(),
        "# contracts/ledgerlens-score/src/types.rs (`RiskScore`). Do not edit by hand.".to_string(),
        "# Fields are in contract-spec (XDR) order — alphabetical, matching the on-chain spec."
            .to_string(),
        "# Requires: pydantic>=2.0".to_string(),
        String::new(),
        "from pydantic import BaseModel, Field".to_string(),
    ];
    if !used.is_empty() {
        lines.push(format!("from typing import {}", used.join(", ")));
    }
    lines.push(String::new());
    lines.push(format!("# JSON-Schema twin: {SCHEMA_FILE}"));
    lines.push(format!("class {}(BaseModel):", xdr_str(&struct_.name)));
    lines.extend(field_lines);
    let mut text = lines.join("\n");
    text.push('\n');
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    fn native() -> ScSpecUdtStructV0 {
        native_risk_score_struct().expect("native RiskScore spec must decode")
    }

    #[test]
    fn native_spec_matches_the_current_struct_fields_in_contract_order() {
        let struct_ = native();
        let fields: Vec<String> = struct_.fields.iter().map(|f| xdr_str(&f.name)).collect();
        // The `#[contracttype]` derive emits the spec with fields sorted
        // alphabetically by name (derive_struct.rs sorted_by_key), so bindings
        // must follow XDR/contract-spec order — not Rust declaration order.
        assert_eq!(
            fields,
            [
                "benford_flag",
                "benford_score",
                "commitment",
                "confidence",
                "ml_flag",
                "ml_score",
                "model_version",
                "network_score",
                "score",
                "timestamp",
            ]
        );
        assert_eq!(fields.len(), 10);
    }

    #[test]
    fn score_range_is_driven_by_contract_constants_not_a_copy() {
        let struct_ = native();
        let schema = struct_to_json_schema(&struct_);
        let score = &schema["$defs"]["RiskScore"]["properties"]["score"];
        assert_eq!(score["type"], "integer");
        assert_eq!(
            score["minimum"].as_u64(),
            Some(u64::from(ledgerlens_score::constants::MIN_SCORE)),
            "`score.minimum` must equal the contract's MIN_SCORE constant"
        );
        assert_eq!(
            score["maximum"].as_u64(),
            Some(u64::from(ledgerlens_score::constants::MAX_SCORE)),
            "`score.maximum` must equal the contract's MAX_SCORE constant"
        );
        assert_eq!(score["minimum"].as_u64(), Some(0), "MIN_SCORE is currently 0");
        assert_eq!(score["maximum"].as_u64(), Some(100), "MAX_SCORE is currently 100");
    }

    #[test]
    fn all_bounded_components_carry_the_contract_range() {
        let struct_ = native();
        let schema = struct_to_json_schema(&struct_);
        for field in ["score", "confidence", "benford_score", "ml_score", "network_score"] {
            let prop = &schema["$defs"]["RiskScore"]["properties"][field];
            assert_eq!(prop["minimum"].as_u64(), Some(0), "{field} minimum");
            assert_eq!(prop["maximum"].as_u64(), Some(100), "{field} maximum");
        }
    }

    #[test]
    fn unsigned_scalars_are_not_capped_at_the_score_domain() {
        let struct_ = native();
        let schema = struct_to_json_schema(&struct_);
        let model_version = &schema["$defs"]["RiskScore"]["properties"]["model_version"];
        assert!(model_version["maximum"].is_null(), "model_version has no semantic max");
        let timestamp = &schema["$defs"]["RiskScore"]["properties"]["timestamp"];
        assert_eq!(timestamp["minimum"].as_u64(), Some(0), "u64 timestamp is unsigned");
        assert!(timestamp["maximum"].is_null(), "timestamp has no semantic max");
    }

    #[test]
    fn commitment_option_bytes_is_nullable_in_all_bindings() {
        let struct_ = native();
        let schema = struct_to_json_schema(&struct_);
        let commitment = &schema["$defs"]["RiskScore"]["properties"]["commitment"];
        assert_eq!(commitment["anyOf"][1]["type"], "null");
        assert_eq!(commitment["anyOf"][0]["type"], "string");
        assert_eq!(commitment["anyOf"][0]["contentEncoding"], "base64");

        let ts = struct_to_typescript(&struct_);
        assert!(ts.contains("commitment: string | null;"), "TS commitment is nullable");

        let py = struct_to_python(&struct_);
        assert!(py.contains("commitment: Optional[str] = None"), "Pydantic commitment is optional");
    }

    #[test]
    fn all_ten_fields_are_required_in_contract_order() {
        let struct_ = native();
        let schema = struct_to_json_schema(&struct_);
        let required =
            schema["$defs"]["RiskScore"]["required"].as_array().expect("required must be an array");
        assert_eq!(required.len(), 10);
        assert_eq!(required[0], "benford_flag");
        assert_eq!(required[9], "timestamp");
        let properties_len = schema["$defs"]["RiskScore"]["properties"]
            .as_object()
            .expect("properties must be an object")
            .len();
        assert_eq!(properties_len, 10);
    }

    #[test]
    fn pydantic_binding_includes_the_ranged_components() {
        let struct_ = native();
        let py = struct_to_python(&struct_);
        assert!(py.contains("score: int = Field(ge=0, le=100)"));
        assert!(py.contains("confidence: int = Field(ge=0, le=100)"));
        assert!(py.contains("benford_score: int = Field(ge=0, le=100)"));
        assert!(py.contains("ml_score: int = Field(ge=0, le=100)"));
        assert!(py.contains("network_score: int = Field(ge=0, le=100)"));
    }

    #[test]
    fn wasm_read_agrees_with_native_when_a_test_wasm_is_provided() {
        let Ok(path) = std::env::var("SCHEMA_GEN_TEST_WASM") else {
            return;
        };
        let bytes = std::fs::read(&path).expect("test wasm path must exist");
        let wasm = wasm_risk_score_struct(&bytes).expect("wasm spec must parse");
        let wasm_fields: Vec<String> = wasm.fields.iter().map(|f| xdr_str(&f.name)).collect();
        let native_fields: Vec<String> = native().fields.iter().map(|f| xdr_str(&f.name)).collect();
        assert_eq!(wasm_fields, native_fields);
    }

    #[test]
    fn provenance_notes_are_embedded_in_artifacts() {
        let struct_ = native();
        assert!(struct_to_typescript(&struct_).contains("Generated by tools/schema-gen"));
        assert!(struct_to_python(&struct_).contains("Generated by tools/schema-gen"));
        let schema = struct_to_json_schema(&struct_);
        assert_eq!(schema["title"], "RiskScore");
    }
}
