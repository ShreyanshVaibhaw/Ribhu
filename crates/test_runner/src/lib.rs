//! Ribhu Phase 4 — Test Runner core library.
//!
//! Defines the `TestAdapter` trait and shared data types used by both the panel
//! and the language-specific adapters (Jest, Pytest, Cargo).

pub mod adapters;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::Path;

// ── Core data types ───────────────────────────────────────────────────────────

/// A group of related tests in a single file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestSuite {
    pub name: String,
    pub file: String,
    pub tests: Vec<TestCase>,
}

/// A single test case within a suite.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestCase {
    pub name: String,
    /// Fully qualified name (suite::test or describe > it).
    pub full_name: String,
}

/// Result of running a single test.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestResult {
    pub name: String,
    pub suite: String,
    pub status: TestStatus,
    pub duration_ms: u64,
    pub error: Option<String>,
    /// Source file path (for gutter markers).
    pub file: Option<String>,
    /// Source line (for gutter markers and double-click-to-navigate).
    pub line: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TestStatus {
    Passed,
    Failed,
    Skipped,
}

impl TestStatus {
    pub fn is_failed(&self) -> bool {
        matches!(self, TestStatus::Failed)
    }
}

// ── TestAdapter trait ─────────────────────────────────────────────────────────

/// Language/framework-specific test runner adapter.
pub trait TestAdapter: Send + Sync {
    /// Human-readable name (e.g. "Jest", "Pytest", "Cargo").
    fn name(&self) -> &str;

    /// Return true if this adapter applies to the given project root.
    fn detect(&self, project_root: &Path) -> bool;

    /// Discover all test suites in the project.
    fn discover(&self, project_root: &Path) -> Result<Vec<TestSuite>>;

    /// Run tests, optionally filtered by a pattern string.
    fn run(&self, project_root: &Path, filter: Option<&str>) -> Result<Vec<TestResult>>;

    /// Run only the tests related to `file_path`.
    fn run_for_file(&self, project_root: &Path, file_path: &Path) -> Result<Vec<TestResult>> {
        let filter = file_path
            .file_stem()
            .and_then(|s| s.to_str())
            .map(|s| s.to_string());
        self.run(project_root, filter.as_deref())
    }
}

// ── Registry helper ───────────────────────────────────────────────────────────

/// Return all built-in adapters in detection-priority order.
pub fn all_adapters() -> Vec<Box<dyn TestAdapter>> {
    vec![
        Box::new(adapters::jest::JestAdapter),
        Box::new(adapters::pytest::PytestAdapter),
        Box::new(adapters::cargo::CargoAdapter),
    ]
}

/// Find the first adapter that detects the project at `root`.
pub fn detect_adapter(root: &Path) -> Option<Box<dyn TestAdapter>> {
    all_adapters().into_iter().find(|a| a.detect(root))
}
