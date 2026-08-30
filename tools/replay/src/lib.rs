use anyhow::{anyhow, bail, Result};
use ledgerlens_score::CONFIG_DRIFT_MANIFEST_FIELDS;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DriftDiffEntry {
    pub field: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observed: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DriftReport {
    pub status: String,
    pub diffs: Vec<DriftDiffEntry>,
}

fn known_fields() -> BTreeSet<&'static str> {
    CONFIG_DRIFT_MANIFEST_FIELDS.iter().copied().collect()
}

pub fn compare_config_manifests(approved: &Value, observed: &Value) -> Result<DriftReport> {
    let approved =
        approved.as_object().ok_or_else(|| anyhow!("approved manifest must be a JSON object"))?;
    let observed =
        observed.as_object().ok_or_else(|| anyhow!("observed manifest must be a JSON object"))?;

    let known = known_fields();
    let mut diffs = Vec::new();

    for field in approved.keys() {
        if !known.contains(field.as_str()) {
            diffs.push(DriftDiffEntry {
                field: field.clone(),
                status: "unknown_approved_field".into(),
                expected: approved.get(field).cloned(),
                observed: observed.get(field).cloned(),
            });
        }
    }

    for field in observed.keys() {
        if !known.contains(field.as_str()) {
            diffs.push(DriftDiffEntry {
                field: field.clone(),
                status: "unknown_observed_field".into(),
                expected: approved.get(field).cloned(),
                observed: observed.get(field).cloned(),
            });
        }
    }

    for field in CONFIG_DRIFT_MANIFEST_FIELDS {
        let expected = approved.get(*field);
        let actual = observed.get(*field);

        match (expected, actual) {
            (Some(left), Some(right)) if left != right => diffs.push(DriftDiffEntry {
                field: (*field).into(),
                status: "drift".into(),
                expected: Some(left.clone()),
                observed: Some(right.clone()),
            }),
            (Some(left), None) => diffs.push(DriftDiffEntry {
                field: (*field).into(),
                status: "missing_observed_field".into(),
                expected: Some(left.clone()),
                observed: None,
            }),
            (None, Some(right)) => diffs.push(DriftDiffEntry {
                field: (*field).into(),
                status: "unexpected_observed_field".into(),
                expected: None,
                observed: Some(right.clone()),
            }),
            _ => {}
        }
    }

    diffs.sort_by(|left, right| left.field.cmp(&right.field).then(left.status.cmp(&right.status)));

    Ok(DriftReport { status: if diffs.is_empty() { "ok".into() } else { "drifted".into() }, diffs })
}

pub fn parse_manifest_json(raw: &str) -> Result<Value> {
    let value: Value = serde_json::from_str(raw)?;
    if !value.is_object() {
        bail!("manifest must be a JSON object");
    }
    Ok(value)
}

pub fn recommended_manifest_template() -> Value {
    let mut map = Map::new();
    for field in CONFIG_DRIFT_MANIFEST_FIELDS {
        map.insert((*field).into(), Value::Null);
    }
    Value::Object(map)
}
