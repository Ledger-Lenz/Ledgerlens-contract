/// Replay determinism checks across host versions.
/// Identifies semantic divergences in behavior between different Soroban host versions.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Determinism check result
pub type DeterminismResult<T> = Result<T, DeterminismError>;

/// Determinism-related errors
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DeterminismError {
    /// State divergence detected
    StateDivergence {
        host_version_a: String,
        host_version_b: String,
        key: String,
        value_a: String,
        value_b: String,
    },
    /// Event divergence detected
    EventDivergence {
        host_version_a: String,
        host_version_b: String,
        index: usize,
        event_a: String,
        event_b: String,
    },
    /// Error code divergence
    ErrorCodeDivergence {
        host_version_a: String,
        host_version_b: String,
        error_a: u32,
        error_b: u32,
    },
    /// Execution divergence (output differs)
    ExecutionDivergence {
        host_version_a: String,
        host_version_b: String,
        reason: String,
    },
    /// Invalid comparison parameters
    InvalidComparison { reason: String },
}

impl std::fmt::Display for DeterminismError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DeterminismError::StateDivergence { host_version_a, host_version_b, key, value_a, value_b } => {
                write!(
                    f,
                    "State divergence: key '{}' differs between {} and {}. Value in {}: '{}', in {}: '{}'",
                    key, host_version_a, host_version_b, host_version_a, value_a, host_version_b, value_b
                )
            }
            DeterminismError::EventDivergence { host_version_a, host_version_b, index, event_a, event_b } => {
                write!(
                    f,
                    "Event divergence: event #{} differs between {} and {}. Event in {}: '{}', in {}: '{}'",
                    index, host_version_a, host_version_b, host_version_a, event_a, host_version_b, event_b
                )
            }
            DeterminismError::ErrorCodeDivergence { host_version_a, host_version_b, error_a, error_b } => {
                write!(
                    f,
                    "Error code divergence: {} produced error {}, {} produced error {}",
                    host_version_a, error_a, host_version_b, error_b
                )
            }
            DeterminismError::ExecutionDivergence { host_version_a, host_version_b, reason } => {
                write!(f, "Execution divergence between {} and {}: {}", host_version_a, host_version_b, reason)
            }
            DeterminismError::InvalidComparison { reason } => {
                write!(f, "Invalid comparison: {}", reason)
            }
        }
    }
}

impl std::error::Error for DeterminismError {}

/// Execution result for a given input and host version
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostVersionResult {
    /// Soroban host version
    pub host_version: String,
    /// Accepted entries count
    pub accepted_count: u32,
    /// Rejected entries count
    pub rejected_count: u32,
    /// State snapshot (key-value pairs)
    pub state_snapshot: HashMap<String, String>,
    /// Events emitted during execution
    pub events: Vec<String>,
    /// Error code if execution failed
    pub error_code: Option<u32>,
    /// Execution metadata
    pub metadata: ExecutionMetadata,
}

/// Execution metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionMetadata {
    /// Total gas consumed
    pub gas_consumed: Option<u64>,
    /// Execution time in milliseconds
    pub execution_time_ms: Option<u64>,
    /// Peak memory usage in bytes
    pub peak_memory_bytes: Option<u64>,
    /// Custom metadata
    #[serde(default)]
    pub custom: HashMap<String, serde_json::Value>,
}

/// Determinism comparison result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeterminismComparison {
    /// First host version tested
    pub host_version_a: String,
    /// Second host version tested
    pub host_version_b: String,
    /// Whether results are deterministic
    pub is_deterministic: bool,
    /// Any divergences found
    pub divergences: Vec<DeterminismError>,
    /// State differences
    pub state_differences: Vec<StateDifference>,
    /// Event differences
    pub event_differences: Vec<EventDifference>,
}

/// Single state difference
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateDifference {
    /// State key that differs
    pub key: String,
    /// Value in first host version
    pub value_a: Option<String>,
    /// Value in second host version
    pub value_b: Option<String>,
}

/// Single event difference
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventDifference {
    /// Event index
    pub index: usize,
    /// Event in first host version
    pub event_a: Option<String>,
    /// Event in second host version
    pub event_b: Option<String>,
}

/// Replay scenario for determinism testing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayScenario {
    /// Scenario name/identifier
    pub name: String,
    /// Description of what's being tested
    pub description: Option<String>,
    /// Input entries
    pub entries: Vec<serde_json::Value>,
    /// Expected deterministic behavior documentation
    pub expected_behavior: Option<String>,
}

/// Compare two execution results for determinism
pub fn compare_results(
    result_a: &HostVersionResult,
    result_b: &HostVersionResult,
) -> DeterminismComparison {
    let mut divergences = Vec::new();
    let mut state_differences = Vec::new();
    let mut event_differences = Vec::new();

    // Check acceptance/rejection counts
    if result_a.accepted_count != result_b.accepted_count
        || result_a.rejected_count != result_b.rejected_count
    {
        divergences.push(DeterminismError::ExecutionDivergence {
            host_version_a: result_a.host_version.clone(),
            host_version_b: result_b.host_version.clone(),
            reason: format!(
                "Acceptance counts differ: ({}, {}) vs ({}, {})",
                result_a.accepted_count,
                result_a.rejected_count,
                result_b.accepted_count,
                result_b.rejected_count
            ),
        });
    }

    // Check error codes
    if result_a.error_code != result_b.error_code {
        if let (Some(ea), Some(eb)) = (result_a.error_code, result_b.error_code) {
            divergences.push(DeterminismError::ErrorCodeDivergence {
                host_version_a: result_a.host_version.clone(),
                host_version_b: result_b.host_version.clone(),
                error_a: ea,
                error_b: eb,
            });
        } else {
            divergences.push(DeterminismError::ExecutionDivergence {
                host_version_a: result_a.host_version.clone(),
                host_version_b: result_b.host_version.clone(),
                reason: format!("One version failed, the other succeeded"),
            });
        }
    }

    // Compare state snapshots
    let all_keys: std::collections::HashSet<_> = result_a
        .state_snapshot
        .keys()
        .chain(result_b.state_snapshot.keys())
        .cloned()
        .collect();

    for key in all_keys {
        let value_a = result_a.state_snapshot.get(&key).cloned();
        let value_b = result_b.state_snapshot.get(&key).cloned();

        if value_a != value_b {
            state_differences.push(StateDifference { key: key.clone(), value_a, value_b });

            if let (Some(va), Some(vb)) = (value_a, value_b) {
                divergences.push(DeterminismError::StateDivergence {
                    host_version_a: result_a.host_version.clone(),
                    host_version_b: result_b.host_version.clone(),
                    key,
                    value_a: va,
                    value_b: vb,
                });
            }
        }
    }

    // Compare events
    let max_events = result_a.events.len().max(result_b.events.len());
    for i in 0..max_events {
        let event_a = result_a.events.get(i).cloned();
        let event_b = result_b.events.get(i).cloned();

        if event_a != event_b {
            event_differences.push(EventDifference { index: i, event_a: event_a.clone(), event_b: event_b.clone() });

            if let (Some(ea), Some(eb)) = (event_a, event_b) {
                divergences.push(DeterminismError::EventDivergence {
                    host_version_a: result_a.host_version.clone(),
                    host_version_b: result_b.host_version.clone(),
                    index: i,
                    event_a: ea,
                    event_b: eb,
                });
            }
        }
    }

    let is_deterministic = divergences.is_empty();

    DeterminismComparison {
        host_version_a: result_a.host_version.clone(),
        host_version_b: result_b.host_version.clone(),
        is_deterministic,
        divergences,
        state_differences,
        event_differences,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_result(
        host_version: &str,
        accepted: u32,
        rejected: u32,
        state: HashMap<String, String>,
    ) -> HostVersionResult {
        HostVersionResult {
            host_version: host_version.to_string(),
            accepted_count: accepted,
            rejected_count: rejected,
            state_snapshot: state,
            events: vec![],
            error_code: None,
            metadata: ExecutionMetadata {
                gas_consumed: Some(1000),
                execution_time_ms: Some(100),
                peak_memory_bytes: Some(1024),
                custom: HashMap::new(),
            },
        }
    }

    #[test]
    fn test_identical_results_are_deterministic() {
        let mut state = HashMap::new();
        state.insert("score_wallet_1_XLM_USDC".to_string(), "50".to_string());

        let result_a = create_test_result("21.0.0", 5, 0, state.clone());
        let result_b = create_test_result("21.0.0", 5, 0, state);

        let comparison = compare_results(&result_a, &result_b);
        assert!(comparison.is_deterministic);
        assert!(comparison.divergences.is_empty());
    }

    #[test]
    fn test_different_acceptance_counts_detected() {
        let state = HashMap::new();
        let result_a = create_test_result("21.0.0", 5, 0, state.clone());
        let result_b = create_test_result("21.1.0", 4, 1, state);

        let comparison = compare_results(&result_a, &result_b);
        assert!(!comparison.is_deterministic);
        assert!(!comparison.divergences.is_empty());
    }

    #[test]
    fn test_state_divergence_detected() {
        let mut state_a = HashMap::new();
        state_a.insert("key1".to_string(), "value_a".to_string());

        let mut state_b = HashMap::new();
        state_b.insert("key1".to_string(), "value_b".to_string());

        let result_a = create_test_result("21.0.0", 5, 0, state_a);
        let result_b = create_test_result("21.1.0", 5, 0, state_b);

        let comparison = compare_results(&result_a, &result_b);
        assert!(!comparison.is_deterministic);
        assert!(!comparison.state_differences.is_empty());
        assert_eq!(comparison.state_differences[0].key, "key1");
    }

    #[test]
    fn test_error_code_divergence() {
        let state = HashMap::new();
        let mut result_a = create_test_result("21.0.0", 5, 0, state.clone());
        result_a.error_code = Some(1);

        let mut result_b = create_test_result("21.1.0", 5, 0, state);
        result_b.error_code = Some(2);

        let comparison = compare_results(&result_a, &result_b);
        assert!(!comparison.is_deterministic);
        assert!(comparison.divergences.iter().any(|d| matches!(d, DeterminismError::ErrorCodeDivergence { .. })));
    }

    #[test]
    fn test_event_divergence_detected() {
        let state = HashMap::new();
        let mut result_a = create_test_result("21.0.0", 5, 0, state.clone());
        result_a.events = vec!["event1".to_string()];

        let mut result_b = create_test_result("21.1.0", 5, 0, state);
        result_b.events = vec!["event2".to_string()];

        let comparison = compare_results(&result_a, &result_b);
        assert!(!comparison.is_deterministic);
        assert!(!comparison.event_differences.is_empty());
    }

    #[test]
    fn test_determinism_error_display() {
        let err = DeterminismError::StateDivergence {
            host_version_a: "21.0.0".to_string(),
            host_version_b: "21.1.0".to_string(),
            key: "test_key".to_string(),
            value_a: "value_a".to_string(),
            value_b: "value_b".to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("State divergence"));
        assert!(msg.contains("test_key"));
    }
}
