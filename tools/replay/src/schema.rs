/// Canonical replay input schema with version negotiation.
/// Ensures replay files are self-describing and remain parseable as the contract evolves.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Current schema version
const CURRENT_SCHEMA_VERSION: u32 = 1;

/// Supported schema versions
const SUPPORTED_VERSIONS: &[u32] = &[1];

/// Schema version result type
pub type SchemaResult<T> = Result<T, SchemaError>;

/// Schema-related errors
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SchemaError {
    /// Unknown schema version
    UnsupportedVersion {
        version: u32,
        supported_versions: Vec<u32>,
    },
    /// Invalid schema structure
    InvalidSchema { reason: String },
    /// Missing required field
    MissingField { field: String },
    /// Type mismatch
    TypeMismatch { field: String, expected: String, got: String },
}

impl std::fmt::Display for SchemaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SchemaError::UnsupportedVersion { version, supported_versions } => {
                write!(
                    f,
                    "Unsupported schema version {}: supported versions are {}",
                    version,
                    supported_versions
                        .iter()
                        .map(|v| v.to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
            SchemaError::InvalidSchema { reason } => write!(f, "Invalid schema: {}", reason),
            SchemaError::MissingField { field } => write!(f, "Missing required field: {}", field),
            SchemaError::TypeMismatch { field, expected, got } => {
                write!(f, "Type mismatch in field '{}': expected {}, got {}", field, expected, got)
            }
        }
    }
}

impl std::error::Error for SchemaError {}

/// Replay file metadata with schema version
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayFileHeader {
    /// Mandatory schema version for version negotiation
    pub schema_version: u32,
    /// Optional metadata about the replay file
    #[serde(default)]
    pub metadata: Option<ReplayMetadata>,
}

/// Optional metadata describing a replay file
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayMetadata {
    /// Optional description of the replay data
    pub description: Option<String>,
    /// Optional timestamp when the replay was created
    pub created_at: Option<u64>,
    /// Optional Soroban host version this replay was created for
    pub host_version: Option<String>,
    /// Optional custom metadata
    #[serde(default)]
    pub custom: Option<HashMap<String, serde_json::Value>>,
}

/// Version 1 replay entry format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayEntryV1 {
    /// Wallet address
    pub wallet: String,
    /// Asset pair identifier
    pub asset_pair: String,
    /// Optional trade history for computing average price
    #[serde(default)]
    pub trades: Option<Vec<TradeRecord>>,
}

/// Trade record for price averaging
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeRecord {
    /// Trade price
    pub price: f64,
    #[serde(default)]
    pub quantity: Option<f64>,
    #[serde(default)]
    pub timestamp: Option<u64>,
}

/// Versioned replay entry wrapper
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum VersionedReplayEntry {
    V1(ReplayEntryV1),
}

/// Validate schema version is supported
pub fn validate_schema_version(version: u32) -> SchemaResult<()> {
    if SUPPORTED_VERSIONS.contains(&version) {
        Ok(())
    } else {
        Err(SchemaError::UnsupportedVersion {
            version,
            supported_versions: SUPPORTED_VERSIONS.to_vec(),
        })
    }
}

/// Get current schema version
pub fn current_version() -> u32 {
    CURRENT_SCHEMA_VERSION
}

/// Get all supported schema versions
pub fn supported_versions() -> Vec<u32> {
    SUPPORTED_VERSIONS.to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_current_version_is_supported() {
        assert!(SUPPORTED_VERSIONS.contains(&CURRENT_SCHEMA_VERSION));
    }

    #[test]
    fn test_validate_supported_version() {
        assert!(validate_schema_version(1).is_ok());
    }

    #[test]
    fn test_reject_unsupported_version() {
        let result = validate_schema_version(999);
        assert!(matches!(result, Err(SchemaError::UnsupportedVersion { .. })));
    }

    #[test]
    fn test_schema_error_display() {
        let err = SchemaError::UnsupportedVersion { version: 99, supported_versions: vec![1] };
        let msg = err.to_string();
        assert!(msg.contains("Unsupported schema version 99"));
        assert!(msg.contains("supported versions are 1"));
    }

    #[test]
    fn test_replay_file_header_serialization() {
        let header = ReplayFileHeader {
            schema_version: 1,
            metadata: Some(ReplayMetadata {
                description: Some("Test replay".to_string()),
                created_at: Some(1234567890),
                host_version: Some("21.0.0".to_string()),
                custom: None,
            }),
        };
        let json = serde_json::to_string(&header).unwrap();
        let parsed: ReplayFileHeader = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.schema_version, 1);
        assert_eq!(parsed.metadata.as_ref().unwrap().description.as_ref().unwrap(), "Test replay");
    }

    #[test]
    fn test_replay_entry_v1_serialization() {
        let entry = ReplayEntryV1 {
            wallet: "wallet_1".to_string(),
            asset_pair: "XLM_USDC".to_string(),
            trades: Some(vec![
                TradeRecord { price: 0.12, quantity: None, timestamp: None },
                TradeRecord { price: 0.13, quantity: None, timestamp: None },
            ]),
        };
        let json = serde_json::to_string(&entry).unwrap();
        let parsed: ReplayEntryV1 = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.wallet, "wallet_1");
        assert_eq!(parsed.asset_pair, "XLM_USDC");
        assert_eq!(parsed.trades.as_ref().unwrap().len(), 2);
    }

    #[test]
    fn test_empty_trades_handling() {
        let entry = ReplayEntryV1 {
            wallet: "wallet_empty".to_string(),
            asset_pair: "XLM_BTC".to_string(),
            trades: Some(vec![]),
        };
        assert_eq!(entry.trades.as_ref().unwrap().len(), 0);
    }

    #[test]
    fn test_null_trades_handling() {
        let entry = ReplayEntryV1 {
            wallet: "wallet_null".to_string(),
            asset_pair: "XLM_BTC".to_string(),
            trades: None,
        };
        assert!(entry.trades.is_none());
    }
}
