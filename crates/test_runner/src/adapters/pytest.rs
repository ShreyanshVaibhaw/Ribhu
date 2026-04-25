//! Pytest test adapter for Python projects.
//!
//! Detection: pytest.ini, pyproject.toml [tool.pytest], or conftest.py present.
//! Discovery: `python -m pytest --collect-only -q`
//! Run: `python -m pytest --tb=short -q [filter]`

use crate::{TestAdapter, TestCase, TestResult, TestStatus, TestSuite};
use anyhow::{Context as _, Result};
use std::{
    collections::HashMap,
    path::Path,
    process::Command,
};

pub struct PytestAdapter;

impl TestAdapter for PytestAdapter {
    fn name(&self) -> &str {
        "Pytest"
    }

    fn detect(&self, project_root: &Path) -> bool {
        project_root.join("pytest.ini").exists()
            || project_root.join("conftest.py").exists()
            || has_pytest_in_pyproject(project_root)
    }

    fn discover(&self, project_root: &Path) -> Result<Vec<TestSuite>> {
        let output = Command::new("python")
            .args(["-m", "pytest", "--collect-only", "-q", "--no-header"])
            .current_dir(project_root)
            .output()
            .context("failed to run `python -m pytest --collect-only`")?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(parse_collected_tests(&stdout))
    }

    fn run(&self, project_root: &Path, filter: Option<&str>) -> Result<Vec<TestResult>> {
        let mut args = vec!["-m", "pytest", "--tb=short", "-q", "--no-header"];
        if let Some(f) = filter {
            args.push("-k");
            args.push(f);
        }

        let output = Command::new("python")
            .args(&args)
            .current_dir(project_root)
            .output()
            .context("failed to run `python -m pytest`")?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        Ok(parse_pytest_output(&stdout, &stderr))
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn has_pytest_in_pyproject(root: &Path) -> bool {
    let path = root.join("pyproject.toml");
    if !path.exists() {
        return false;
    }
    std::fs::read_to_string(&path)
        .map(|s| s.contains("[tool.pytest"))
        .unwrap_or(false)
}

/// Parse `--collect-only -q` output into TestSuites.
///
/// Example lines:
///   `tests/test_auth.py::TestAuthService::test_login`
///   `tests/test_auth.py::test_helper`
fn parse_collected_tests(output: &str) -> Vec<TestSuite> {
    let mut suites: HashMap<String, TestSuite> = HashMap::new();
    for line in output.lines() {
        let line = line.trim();
        if !line.contains("::") {
            continue;
        }
        // Split on first `::` to get file and the rest
        let mut parts = line.splitn(2, "::");
        let file = match parts.next() {
            Some(f) => f.to_string(),
            None => continue,
        };
        let rest = parts.next().unwrap_or("").to_string();
        // The rest may be Class::method or just method
        let (suite_name, test_name) = if rest.contains("::") {
            let mut p = rest.splitn(2, "::");
            (
                p.next().unwrap_or("").to_string(),
                p.next().unwrap_or("").to_string(),
            )
        } else {
            (file.clone(), rest.clone())
        };

        let entry = suites.entry(file.clone()).or_insert_with(|| TestSuite {
            name: suite_name.clone(),
            file: file.clone(),
            tests: vec![],
        });
        entry.tests.push(TestCase {
            name: test_name.clone(),
            full_name: format!("{}::{}", file, rest),
        });
    }
    suites.into_values().collect()
}

/// Parse pytest short text output into TestResults.
///
/// Passed: lines ending with "PASSED"
/// Failed: lines ending with "FAILED"
/// Skipped: lines ending with "SKIPPED"
fn parse_pytest_output(stdout: &str, _stderr: &str) -> Vec<TestResult> {
    let mut results: Vec<TestResult> = Vec::new();
    let mut current_failure_lines: Vec<String> = Vec::new();
    let mut in_failure: Option<String> = None;

    for line in stdout.lines() {
        if line.contains(" PASSED") || line.contains(" FAILED") || line.contains(" SKIPPED") {
            // Flush previous failure context
            if let Some(_name) = in_failure.take() {
                if let Some(last) = results.last_mut() {
                    if let crate::TestStatus::Failed = last.status {
                        last.error = Some(current_failure_lines.join("\n"));
                    }
                }
                current_failure_lines.clear();
            }

            let (full_name, status) = if let Some(rest) = line.strip_suffix(" PASSED") {
                (rest.trim(), TestStatus::Passed)
            } else if let Some(rest) = line.strip_suffix(" FAILED") {
                (rest.trim(), TestStatus::Failed)
            } else if let Some(rest) = line.strip_suffix(" SKIPPED") {
                (rest.trim(), TestStatus::Skipped)
            } else {
                continue;
            };

            let (file, suite, name) = parse_test_id(full_name);
            if matches!(status, TestStatus::Failed) {
                in_failure = Some(full_name.to_string());
            }
            results.push(TestResult {
                name,
                suite,
                status,
                duration_ms: 0, // pytest -q doesn't emit timing in basic output
                error: None,
                file: Some(file),
                line: None,
            });
        } else if in_failure.is_some() {
            current_failure_lines.push(line.to_string());
        }
    }

    // Flush last failure
    if let Some(error) = current_failure_lines
        .iter()
        .filter(|l| !l.is_empty())
        .cloned()
        .reduce(|a, b| format!("{a}\n{b}"))
    {
        if let Some(last) = results.last_mut() {
            if matches!(last.status, TestStatus::Failed) {
                last.error = Some(error);
            }
        }
    }

    results
}

fn parse_test_id(full_name: &str) -> (String, String, String) {
    let mut parts = full_name.splitn(2, "::");
    let file = parts.next().unwrap_or("").to_string();
    let rest = parts.next().unwrap_or("").to_string();
    if rest.contains("::") {
        let mut p = rest.splitn(2, "::");
        let suite = p.next().unwrap_or("").to_string();
        let name = p.next().unwrap_or("").to_string();
        (file, suite, name)
    } else {
        (file.clone(), file, rest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_via_conftest() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("conftest.py"), "").unwrap();
        assert!(PytestAdapter.detect(dir.path()));
    }

    #[test]
    fn detect_via_pytest_ini() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("pytest.ini"), "[pytest]").unwrap();
        assert!(PytestAdapter.detect(dir.path()));
    }

    #[test]
    fn no_detection_empty_dir() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!PytestAdapter.detect(dir.path()));
    }

    #[test]
    fn parse_collected() {
        let output = "tests/test_auth.py::TestAuth::test_login\ntests/test_auth.py::TestAuth::test_logout\n";
        let suites = parse_collected_tests(output);
        assert_eq!(suites.len(), 1);
        assert_eq!(suites[0].tests.len(), 2);
    }

    #[test]
    fn parse_output_passed_and_failed() {
        let stdout =
            "tests/test_auth.py::test_login PASSED\ntests/test_auth.py::test_logout FAILED\n";
        let results = parse_pytest_output(stdout, "");
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].status, TestStatus::Passed);
        assert_eq!(results[1].status, TestStatus::Failed);
    }
}
