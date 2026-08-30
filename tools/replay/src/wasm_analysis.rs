/// WASM size attribution by module and feature.
/// Breaks down WebAssembly binary growth by contract module or feature area
/// to identify and review performance regressions during releases.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// WASM analysis result type
pub type WasmAnalysisResult<T> = Result<T, WasmAnalysisError>;

/// WASM analysis errors
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WasmAnalysisError {
    /// Invalid binary format
    InvalidBinary { reason: String },
    /// Module not found
    ModuleNotFound { name: String },
    /// Size calculation failed
    SizeCalculationFailed { reason: String },
}

impl std::fmt::Display for WasmAnalysisError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WasmAnalysisError::InvalidBinary { reason } => {
                write!(f, "Invalid WASM binary: {}", reason)
            }
            WasmAnalysisError::ModuleNotFound { name } => {
                write!(f, "Module not found: {}", name)
            }
            WasmAnalysisError::SizeCalculationFailed { reason } => {
                write!(f, "Size calculation failed: {}", reason)
            }
        }
    }
}

impl std::error::Error for WasmAnalysisError {}

/// Size breakdown for a single module or feature
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleSize {
    /// Module or feature name
    pub name: String,
    /// Size in bytes
    pub bytes: u64,
    /// Percentage of total
    pub percentage: f64,
    /// Size category
    pub category: SizeCategory,
}

/// Size category for classification
#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SizeCategory {
    /// Critical functionality
    Critical,
    /// Core business logic
    Core,
    /// Supporting features
    Feature,
    /// Testing and debugging
    Test,
    /// Other
    Other,
}

/// Complete WASM binary analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WasmBinaryAnalysis {
    /// Total binary size in bytes
    pub total_size: u64,
    /// Breakdown by module
    pub modules: Vec<ModuleSize>,
    /// Feature-based breakdown
    pub features: Vec<ModuleSize>,
    /// Code section size
    pub code_section_bytes: Option<u64>,
    /// Data section size
    pub data_section_bytes: Option<u64>,
    /// Custom sections size
    pub custom_sections_bytes: Option<u64>,
    /// Estimated compressibility (gzip compression ratio)
    pub estimated_compression_ratio: Option<f64>,
    /// Analysis timestamp
    pub timestamp: u64,
}

/// Regression detection result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SizeRegression {
    /// Module or feature that regressed
    pub name: String,
    /// Previous size in bytes
    pub previous_size: u64,
    /// Current size in bytes
    pub current_size: u64,
    /// Size increase in bytes
    pub increase_bytes: i64,
    /// Percentage increase
    pub increase_percent: f64,
    /// Severity level
    pub severity: RegressionSeverity,
}

/// Regression severity classification
#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum RegressionSeverity {
    /// Increase < 1% (negligible)
    Negligible,
    /// Increase 1-5% (minor)
    Minor,
    /// Increase 5-10% (moderate)
    Moderate,
    /// Increase > 10% (severe)
    Severe,
}

impl RegressionSeverity {
    /// Classify severity based on percentage increase
    pub fn from_percentage(percent: f64) -> Self {
        match percent {
            p if p < 1.0 => RegressionSeverity::Negligible,
            p if p < 5.0 => RegressionSeverity::Minor,
            p if p < 10.0 => RegressionSeverity::Moderate,
            _ => RegressionSeverity::Severe,
        }
    }
}

/// WASM binary comparison for regression detection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WasmBinaryComparison {
    /// Previous binary analysis
    pub previous: WasmBinaryAnalysis,
    /// Current binary analysis
    pub current: WasmBinaryAnalysis,
    /// Total size difference in bytes
    pub total_difference_bytes: i64,
    /// Total percentage change
    pub total_percentage_change: f64,
    /// Detected regressions
    pub regressions: Vec<SizeRegression>,
    /// Major changes requiring review
    pub requires_review: bool,
}

/// WASM size tracking record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WasmSizeRecord {
    /// Contract name
    pub contract_name: String,
    /// Release or version identifier
    pub version: String,
    /// Analysis data
    pub analysis: WasmBinaryAnalysis,
}

impl WasmBinaryAnalysis {
    /// Create a new WASM binary analysis
    pub fn new(total_size: u64) -> Self {
        WasmBinaryAnalysis {
            total_size,
            modules: Vec::new(),
            features: Vec::new(),
            code_section_bytes: None,
            data_section_bytes: None,
            custom_sections_bytes: None,
            estimated_compression_ratio: None,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
        }
    }

    /// Add module size information
    pub fn add_module(&mut self, name: &str, bytes: u64, category: SizeCategory) {
        let percentage = (bytes as f64 / self.total_size as f64) * 100.0;
        self.modules.push(ModuleSize { name: name.to_string(), bytes, percentage, category });
    }

    /// Add feature size information
    pub fn add_feature(&mut self, name: &str, bytes: u64, category: SizeCategory) {
        let percentage = (bytes as f64 / self.total_size as f64) * 100.0;
        self.features.push(ModuleSize { name: name.to_string(), bytes, percentage, category });
    }

    /// Get modules sorted by size (largest first)
    pub fn modules_by_size(&self) -> Vec<&ModuleSize> {
        let mut sorted = self.modules.iter().collect::<Vec<_>>();
        sorted.sort_by(|a, b| b.bytes.cmp(&a.bytes));
        sorted
    }

    /// Get features sorted by size (largest first)
    pub fn features_by_size(&self) -> Vec<&ModuleSize> {
        let mut sorted = self.features.iter().collect::<Vec<_>>();
        sorted.sort_by(|a, b| b.bytes.cmp(&a.bytes));
        sorted
    }
}

/// Compare two WASM binaries for regressions
pub fn compare_binaries(
    previous: WasmBinaryAnalysis,
    current: WasmBinaryAnalysis,
) -> WasmBinaryComparison {
    let total_difference_bytes = current.total_size as i64 - previous.total_size as i64;
    let total_percentage_change = if previous.total_size > 0 {
        (total_difference_bytes as f64 / previous.total_size as f64) * 100.0
    } else {
        0.0
    };

    let mut regressions = Vec::new();
    let mut module_map: HashMap<String, u64> = HashMap::new();

    // Build map of previous sizes
    for module in &previous.modules {
        module_map.insert(module.name.clone(), module.bytes);
    }

    // Check current modules for regressions
    for module in &current.modules {
        if let Some(prev_size) = module_map.get(&module.name) {
            if module.bytes > *prev_size {
                let increase = module.bytes as i64 - *prev_size as i64;
                let increase_percent = (increase as f64 / *prev_size as f64) * 100.0;
                let severity = RegressionSeverity::from_percentage(increase_percent);

                regressions.push(SizeRegression {
                    name: module.name.clone(),
                    previous_size: *prev_size,
                    current_size: module.bytes,
                    increase_bytes: increase,
                    increase_percent,
                    severity,
                });
            }
        }
    }

    // Sort regressions by severity and size
    regressions.sort_by(|a, b| {
        if a.severity != b.severity {
            b.severity.cmp(&a.severity)
        } else {
            b.increase_bytes.cmp(&a.increase_bytes)
        }
    });

    let requires_review =
        total_percentage_change > 2.0 || regressions.iter().any(|r| r.severity == RegressionSeverity::Severe);

    WasmBinaryComparison {
        previous,
        current,
        total_difference_bytes,
        total_percentage_change,
        regressions,
        requires_review,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_wasm_analysis() {
        let analysis = WasmBinaryAnalysis::new(1000000);
        assert_eq!(analysis.total_size, 1000000);
        assert!(analysis.modules.is_empty());
    }

    #[test]
    fn test_add_module() {
        let mut analysis = WasmBinaryAnalysis::new(1000000);
        analysis.add_module("score", 500000, SizeCategory::Core);
        assert_eq!(analysis.modules.len(), 1);
        assert_eq!(analysis.modules[0].bytes, 500000);
        assert_eq!(analysis.modules[0].percentage, 50.0);
    }

    #[test]
    fn test_add_feature() {
        let mut analysis = WasmBinaryAnalysis::new(1000000);
        analysis.add_feature("validation", 200000, SizeCategory::Feature);
        assert_eq!(analysis.features.len(), 1);
        assert_eq!(analysis.features[0].bytes, 200000);
    }

    #[test]
    fn test_modules_sorted_by_size() {
        let mut analysis = WasmBinaryAnalysis::new(1000000);
        analysis.add_module("small", 100000, SizeCategory::Test);
        analysis.add_module("large", 500000, SizeCategory::Core);
        analysis.add_module("medium", 300000, SizeCategory::Feature);

        let sorted = analysis.modules_by_size();
        assert_eq!(sorted[0].bytes, 500000);
        assert_eq!(sorted[1].bytes, 300000);
        assert_eq!(sorted[2].bytes, 100000);
    }

    #[test]
    fn test_size_regression_detection() {
        let mut prev = WasmBinaryAnalysis::new(1000000);
        prev.add_module("score", 500000, SizeCategory::Core);
        prev.add_module("validator", 300000, SizeCategory::Feature);

        let mut curr = WasmBinaryAnalysis::new(1100000);
        curr.add_module("score", 550000, SizeCategory::Core); // 10% increase
        curr.add_module("validator", 300000, SizeCategory::Feature);

        let comparison = compare_binaries(prev, curr);
        assert!(!comparison.regressions.is_empty());
        assert_eq!(comparison.regressions[0].name, "score");
        assert_eq!(comparison.regressions[0].increase_percent, 10.0);
    }

    #[test]
    fn test_regression_severity_classification() {
        assert_eq!(RegressionSeverity::from_percentage(0.5), RegressionSeverity::Negligible);
        assert_eq!(RegressionSeverity::from_percentage(3.0), RegressionSeverity::Minor);
        assert_eq!(RegressionSeverity::from_percentage(7.0), RegressionSeverity::Moderate);
        assert_eq!(RegressionSeverity::from_percentage(15.0), RegressionSeverity::Severe);
    }

    #[test]
    fn test_requires_review_on_high_regression() {
        let mut prev = WasmBinaryAnalysis::new(1000000);
        prev.add_module("score", 500000, SizeCategory::Core);

        let mut curr = WasmBinaryAnalysis::new(1500000);
        curr.add_module("score", 750000, SizeCategory::Core); // 50% increase

        let comparison = compare_binaries(prev, curr);
        assert!(comparison.requires_review);
        assert_eq!(comparison.regressions[0].severity, RegressionSeverity::Severe);
    }

    #[test]
    fn test_size_comparison_total_difference() {
        let prev = WasmBinaryAnalysis::new(1000000);
        let curr = WasmBinaryAnalysis::new(1100000);

        let comparison = compare_binaries(prev, curr);
        assert_eq!(comparison.total_difference_bytes, 100000);
        assert_eq!(comparison.total_percentage_change, 10.0);
    }

    #[test]
    fn test_wasm_size_record() {
        let analysis = WasmBinaryAnalysis::new(1000000);
        let record =
            WasmSizeRecord { contract_name: "ledgerlens-score".to_string(), version: "1.0.0".to_string(), analysis };
        assert_eq!(record.contract_name, "ledgerlens-score");
        assert_eq!(record.version, "1.0.0");
    }

    #[test]
    fn test_no_regression_for_size_decrease() {
        let mut prev = WasmBinaryAnalysis::new(1000000);
        prev.add_module("score", 500000, SizeCategory::Core);

        let mut curr = WasmBinaryAnalysis::new(900000);
        curr.add_module("score", 450000, SizeCategory::Core); // 10% decrease

        let comparison = compare_binaries(prev, curr);
        assert!(comparison.regressions.is_empty()); // Improvement, not regression
    }
}
