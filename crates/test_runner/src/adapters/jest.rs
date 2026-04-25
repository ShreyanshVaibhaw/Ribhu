//! Jest test adapter for JavaScript/TypeScript projects.
//!
//! Detection: package.json contains "jest" in dependencies/devDependencies.
//! Discovery: `npx jest --listTests`
//! Run: `npx jest --json --verbose [--testPathPattern=filter]`

use crate::{TestAdapter, TestResult, TestStatus, TestSuite};
use anyhow::{Context as _, Result, bail};
use serde::Deserialize;
use std::{
    path::{Path, PathBuf},
    process::Command,
};


pub struct JestAdapter;

impl TestAdapter for JestAdapter {
    fn name(&self) -> &str {
        "Jest"
    }

    fn detect(&self, project_root: &Path) -> bool {
        let pkg = project_root.join("package.json");
        if !pkg.exists() {
            return false;
        }
        let Ok(content) = std::fs::read_to_string(&pkg) else {
            return false;
        };
        let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) else {
            return false;
        };
        let has_jest = |section: &str| {
            json.get(section)
                .and_then(|d| d.as_object())
                .map(|m| m.contains_key("jest"))
                .unwrap_or(false)
        };
        has_jest("dependencies") || has_jest("devDependencies")
    }

    fn discover(&self, project_root: &Path) -> Result<Vec<TestSuite>> {
        let output = Command::new("npx")
            .args(["jest", "--listTests", "--no-coverage"])
            .current_dir(project_root)
            .output()
            .context("failed to run `npx jest --listTests`")?;

        if !output.status.success() {
            bail!(
                "jest --listTests failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let suites = stdout
            .lines()
            .filter(|l| !l.is_empty())
            .map(|file| {
                let name = PathBuf::from(file)
                    .file_stem()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| file.to_string());
                TestSuite {
                    name,
                    file: file.to_string(),
                    tests: vec![], // populated on run
                }
            })
            .collect();
        Ok(suites)
    }

    fn run(&self, project_root: &Path, filter: Option<&str>) -> Result<Vec<TestResult>> {
        let mut args = vec!["jest", "--json", "--no-coverage", "--forceExit"];
        if let Some(f) = filter {
            args.push("--testPathPattern");
            args.push(f);
        }

        let output = Command::new("npx")
            .args(&args)
            .current_dir(project_root)
            .output()
            .context("failed to run `npx jest --json`")?;

        // jest exits with non-zero when tests fail — that's expected
        let stdout = String::from_utf8_lossy(&output.stdout);
        parse_jest_json(&stdout)
    }
}

// ── Jest JSON output parser ───────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct JestOutput {
    test_results: Vec<JestTestFile>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct JestTestFile {
    test_file_path: String,
    test_results: Vec<JestTestResult>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct JestTestResult {
    ancestor_titles: Vec<String>,
    title: String,
    status: String,
    duration: Option<f64>,
    failure_messages: Vec<String>,
}

fn parse_jest_json(stdout: &str) -> Result<Vec<TestResult>> {
    // Jest may print non-JSON lines before the JSON object
    let json_start = stdout.find('{').unwrap_or(0);
    let json_str = &stdout[json_start..];
    let output: JestOutput =
        serde_json::from_str(json_str).context("failed to parse jest --json output")?;

    let mut results = Vec::new();
    for file in output.test_results {
        let file_path = file.test_file_path.clone();
        for test in file.test_results {
            let suite = test.ancestor_titles.join(" > ");
            let status = match test.status.as_str() {
                "passed" => TestStatus::Passed,
                "failed" => TestStatus::Failed,
                _ => TestStatus::Skipped,
            };
            let error = if test.failure_messages.is_empty() {
                None
            } else {
                Some(test.failure_messages.join("\n"))
            };
            results.push(TestResult {
                name: test.title.clone(),
                suite: suite.clone(),
                status,
                duration_ms: test.duration.unwrap_or(0.0) as u64,
                error,
                file: Some(file_path.clone()),
                line: None, // Jest JSON doesn't provide line numbers reliably
            });
        }
    }
    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_jest_in_package_json() {
        // Create a temp dir with a package.json that has jest
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"devDependencies":{"jest":"^29.0.0"}}"#,
        )
        .unwrap();
        assert!(JestAdapter.detect(dir.path()));
    }

    #[test]
    fn no_detection_without_jest_dep() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"devDependencies":{"typescript":"^5.0.0"}}"#,
        )
        .unwrap();
        assert!(!JestAdapter.detect(dir.path()));
    }

    #[test]
    fn parse_jest_json_output() {
        let json = r#"{
            "testResults": [{
                "testFilePath": "/project/src/auth.test.ts",
                "testResults": [
                    {
                        "ancestorTitles": ["AuthService"],
                        "title": "should login user",
                        "status": "passed",
                        "duration": 42.5,
                        "failureMessages": []
                    },
                    {
                        "ancestorTitles": ["AuthService"],
                        "title": "should reject wrong password",
                        "status": "failed",
                        "duration": 5.0,
                        "failureMessages": ["Expected: true\nReceived: false"]
                    }
                ]
            }]
        }"#;
        let results = parse_jest_json(json).unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].status, TestStatus::Passed);
        assert_eq!(results[1].status, TestStatus::Failed);
        assert!(results[1].error.is_some());
    }
}
