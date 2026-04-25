//! Cargo test adapter for Rust projects.
//!
//! Detection: Cargo.toml present.
//! Discovery: `cargo test -- --list`
//! Run: `cargo test [filter] -- --format=json` (requires cargo-nextest or --format option)
//!       Falls back to parsing plain output.

use crate::{TestAdapter, TestCase, TestResult, TestStatus, TestSuite};
use anyhow::{Context as _, Result};
use std::{collections::HashMap, path::Path, process::Command};

pub struct CargoAdapter;

impl TestAdapter for CargoAdapter {
    fn name(&self) -> &str {
        "Cargo"
    }

    fn detect(&self, project_root: &Path) -> bool {
        project_root.join("Cargo.toml").exists()
    }

    fn discover(&self, project_root: &Path) -> Result<Vec<TestSuite>> {
        let output = Command::new("cargo")
            .args(["test", "--", "--list"])
            .current_dir(project_root)
            .output()
            .context("failed to run `cargo test -- --list`")?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(parse_cargo_list(&stdout))
    }

    fn run(&self, project_root: &Path, filter: Option<&str>) -> Result<Vec<TestResult>> {
        let mut args = vec!["test"];
        if let Some(f) = filter {
            args.push(f);
        }
        // Use --nocapture so we can see failure output
        args.extend(["--", "--nocapture"]);

        let output = Command::new("cargo")
            .args(&args)
            .current_dir(project_root)
            .output()
            .context("failed to run `cargo test`")?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        Ok(parse_cargo_output(&stdout, &stderr))
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Parse `cargo test -- --list` output.
/// Lines look like: `tests::module::test_name: test`
fn parse_cargo_list(output: &str) -> Vec<TestSuite> {
    let mut suites: HashMap<String, Vec<TestCase>> = HashMap::new();
    for line in output.lines() {
        let line = line.trim();
        let Some(name) = line.strip_suffix(": test") else {
            continue;
        };
        // Split on `::` to get module (suite) and test name
        let parts: Vec<&str> = name.rsplitn(2, "::").collect();
        let (test_name, suite) = if parts.len() == 2 {
            (parts[0].to_string(), parts[1].to_string())
        } else {
            (name.to_string(), "root".to_string())
        };
        let entry = suites.entry(suite.clone()).or_default();
        entry.push(TestCase {
            name: test_name.clone(),
            full_name: name.to_string(),
        });
    }
    suites
        .into_iter()
        .map(|(suite, tests)| TestSuite {
            name: suite.clone(),
            file: suite, // No file info from cargo --list
            tests,
        })
        .collect()
}

/// Parse `cargo test` plain output.
/// Lines: `test tests::auth::login ... ok`
///        `test tests::auth::logout ... FAILED`
fn parse_cargo_output(stdout: &str, _stderr: &str) -> Vec<TestResult> {
    let mut results = Vec::new();
    for line in stdout.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix("test ") else {
            continue;
        };
        let (name_part, status_str) = if let Some(idx) = rest.rfind(" ... ") {
            (&rest[..idx], &rest[idx + 5..])
        } else {
            continue;
        };
        let status = match status_str.trim() {
            "ok" => TestStatus::Passed,
            "FAILED" => TestStatus::Failed,
            "ignored" => TestStatus::Skipped,
            _ => continue,
        };
        let parts: Vec<&str> = name_part.rsplitn(2, "::").collect();
        let (test_name, suite) = if parts.len() == 2 {
            (parts[0].to_string(), parts[1].to_string())
        } else {
            (name_part.to_string(), "root".to_string())
        };
        results.push(TestResult {
            name: test_name,
            suite,
            status,
            duration_ms: 0,
            error: None,
            file: None,
            line: None,
        });
    }
    results
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_cargo_toml() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "[package]\nname=\"test\"").unwrap();
        assert!(CargoAdapter.detect(dir.path()));
    }

    #[test]
    fn parse_cargo_list_output() {
        let output = "tests::auth::login: test\ntests::auth::logout: test\n";
        let suites = parse_cargo_list(output);
        assert_eq!(suites.len(), 1);
        assert_eq!(suites[0].tests.len(), 2);
    }

    #[test]
    fn parse_cargo_test_output() {
        let stdout = "test tests::auth::login ... ok\ntest tests::auth::logout ... FAILED\n";
        let results = parse_cargo_output(stdout, "");
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].status, TestStatus::Passed);
        assert_eq!(results[1].status, TestStatus::Failed);
    }
}
