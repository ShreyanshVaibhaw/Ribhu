//! AI Guardrails Security Layer — the 4th Ribhu differentiator.
//!
//! Every AI action passes through the SecurityPipeline before execution.
//! The pipeline runs 7 checks in order and returns Allow / Block / Ask.
//!
//! Per Ribhu_Security_Layer.md:
//!   "Every AI coding tool asks you to trust the AI.
//!    Ribhu is the first one that lets you verify."

use crate::autonomy::SuggestedAction;
use regex::Regex;
use std::{
    path::{Path, PathBuf},
    sync::OnceLock,
    time::{SystemTime, UNIX_EPOCH},
};
use uuid::Uuid;

// ── Result Types ─────────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SecuritySeverity {
    /// Never override — system destruction, data exfiltration.
    Critical,
    /// Strong warning — destructive but recoverable.
    High,
    /// Worth confirming.
    Medium,
    /// Informational escalation.
    Low,
}

#[derive(Clone, Debug)]
pub enum SecurityResult {
    Allow,
    /// Hard block — show error to developer, do not execute.
    Block { reason: String, severity: SecuritySeverity },
    /// Escalate to Level 3 modal regardless of original autonomy level.
    Ask { reason: String, severity: SecuritySeverity },
}

impl SecurityResult {
    pub fn is_allow(&self) -> bool {
        matches!(self, Self::Allow)
    }

    pub fn reason(&self) -> Option<&str> {
        match self {
            Self::Allow => None,
            Self::Block { reason, .. } | Self::Ask { reason, .. } => Some(reason),
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Allow => "allowed",
            Self::Block { .. } => "blocked",
            Self::Ask { .. } => "asked",
        }
    }
}

// ── Security Check Trait ─────────────────────────────────────────────────────

pub trait SecurityCheck: Send + Sync {
    fn name(&self) -> &'static str;
    fn check(&self, action: &SuggestedAction) -> SecurityResult;
}

// ── Check 1: Dangerous Command Filter ────────────────────────────────────────

pub struct DangerousCommandFilter;

impl SecurityCheck for DangerousCommandFilter {
    fn name(&self) -> &'static str { "DangerousCommandFilter" }

    fn check(&self, action: &SuggestedAction) -> SecurityResult {
        let command = match action {
            SuggestedAction::RunCommand { command, .. } => command.to_lowercase(),
            _ => return SecurityResult::Allow,
        };

        // ── Hard block — no override ─────────────────────────────────────
        let hard_block = [
            "rm -rf /",
            "rm -rf ~",
            ":(){:|:&};:",  // fork bomb
            "mkfs",
            "> /dev/sda",
            "chmod -R 777 /",
        ];
        for pattern in &hard_block {
            if command.contains(pattern) {
                return SecurityResult::Block {
                    reason: format!(
                        "This command could destroy your system: '{}'",
                        pattern
                    ),
                    severity: SecuritySeverity::Critical,
                };
            }
        }

        // Special-case for dd targeting a device
        if command.contains("dd if=") && command.contains("of=/dev") {
            return SecurityResult::Block {
                reason: "Direct disk write detected (dd of=/dev/...)".into(),
                severity: SecuritySeverity::Critical,
            };
        }

        // ── Soft block — developer can override via Level 3 modal ────────
        let ask_patterns: &[(&str, &str)] = &[
            ("rm -rf", "Recursive force delete"),
            ("rm -r", "Recursive delete"),
            ("drop database", "Database drop"),
            ("drop table", "Table drop"),
            ("truncate table", "Table truncation"),
            ("git push --force", "Force push to remote"),
            ("git reset --hard", "Hard git reset"),
        ];
        for (pattern, label) in ask_patterns {
            if command.contains(pattern) {
                return SecurityResult::Ask {
                    reason: format!("Potentially destructive command ({}): {}", label, command),
                    severity: SecuritySeverity::High,
                };
            }
        }

        if command.starts_with("sudo ") {
            return SecurityResult::Ask {
                reason: format!("Command requires elevated privileges: {}", command),
                severity: SecuritySeverity::High,
            };
        }

        SecurityResult::Allow
    }
}

// ── Check 2: File System Boundary Checker ────────────────────────────────────

pub struct FileSystemBoundaryChecker {
    pub project_root: PathBuf,
}

impl SecurityCheck for FileSystemBoundaryChecker {
    fn name(&self) -> &'static str { "FileSystemBoundaryChecker" }

    fn check(&self, action: &SuggestedAction) -> SecurityResult {
        let file_path = match action {
            SuggestedAction::ApplyCodeEdit { file_path, .. } => file_path,
            SuggestedAction::OpenFile { path, .. } => path,
            _ => return SecurityResult::Allow,
        };

        let path = Path::new(file_path);

        // Check boundary (canonicalize if possible, fall back to starts_with)
        let in_project = path
            .canonicalize()
            .ok()
            .map(|p| p.starts_with(&self.project_root))
            .unwrap_or_else(|| path.starts_with(&self.project_root));

        if !in_project {
            return SecurityResult::Block {
                reason: format!(
                    "AI tried to modify '{}' which is outside the project directory '{}'.",
                    file_path,
                    self.project_root.display()
                ),
                severity: SecuritySeverity::High,
            };
        }

        // Sensitive file patterns
        let filename = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("");
        let sensitive = [".env", ".env.local", ".env.production", "id_rsa", "id_ed25519"];
        if sensitive.contains(&filename) || filename.ends_with(".pem") || filename.ends_with(".key") {
            return SecurityResult::Ask {
                reason: format!("AI wants to modify sensitive file: {}", file_path),
                severity: SecuritySeverity::High,
            };
        }

        SecurityResult::Allow
    }
}

// ── Check 3: Secret Leak Detector ────────────────────────────────────────────

pub struct SecretLeakDetector;

static SECRET_PATTERNS: OnceLock<Vec<(Regex, &'static str)>> = OnceLock::new();

fn secret_patterns() -> &'static Vec<(Regex, &'static str)> {
    SECRET_PATTERNS.get_or_init(|| {
        // Each entry: (pattern, label). Patterns use explicit escapes to avoid
        // Rust 2021 raw-string-followed-by-identifier parsing ambiguity.
        vec![
            (
                Regex::new(r"(?i)(api_?key|apikey)\s*[:=]\s*[a-zA-Z0-9]{20,}").unwrap(),
                "API key",
            ),
            (
                Regex::new(r"sk-[a-zA-Z0-9]{20,}").unwrap(),
                "OpenAI API key",
            ),
            (
                Regex::new(r"sk-ant-[a-zA-Z0-9]{20,}").unwrap(),
                "Anthropic API key",
            ),
            (
                Regex::new(r"ghp_[a-zA-Z0-9]{36}").unwrap(),
                "GitHub PAT",
            ),
            (
                Regex::new(r"-----BEGIN .{0,20}PRIVATE KEY-----").unwrap(),
                "Private key",
            ),
            (
                Regex::new(r"(?i)bearer\s+[a-zA-Z0-9_.~+/=-]{20,}").unwrap(),
                "Bearer token",
            ),
            (
                Regex::new(r"(?i)(aws_access_key_id|aws_secret)\s*[:=]\s*[A-Z0-9]{16,}").unwrap(),
                "AWS credential",
            ),
            (
                Regex::new(r"(?i)(password|passwd|pwd)\s*[:=]\s*\S{8,}").unwrap(),
                "Password",
            ),
        ]
    })
}

impl SecurityCheck for SecretLeakDetector {
    fn name(&self) -> &'static str { "SecretLeakDetector" }

    fn check(&self, action: &SuggestedAction) -> SecurityResult {
        let content = action.content_string();
        for (pattern, name) in secret_patterns() {
            if pattern.is_match(&content) {
                return SecurityResult::Block {
                    reason: format!(
                        "AI output contains what appears to be a {} — blocked to prevent leaking.",
                        name
                    ),
                    severity: SecuritySeverity::Critical,
                };
            }
        }
        SecurityResult::Allow
    }
}

// ── Check 4: Network Access Checker ──────────────────────────────────────────

pub struct NetworkAccessChecker;

impl SecurityCheck for NetworkAccessChecker {
    fn name(&self) -> &'static str { "NetworkAccessChecker" }

    fn check(&self, action: &SuggestedAction) -> SecurityResult {
        let command = match action {
            SuggestedAction::RunCommand { command, .. } => command,
            _ => return SecurityResult::Allow,
        };

        // Non-default package registries
        if (command.contains("pip install") || command.contains("npm install"))
            && command.contains("--index-url")
        {
            return SecurityResult::Ask {
                reason: "AI is installing packages from a non-default registry.".into(),
                severity: SecuritySeverity::High,
            };
        }

        // Unknown curl/wget targets
        if command.contains("curl ") || command.contains("wget ") {
            if let Some(url) = extract_url(command) {
                let known_hosts = [
                    "localhost", "127.0.0.1", "npmjs.org", "pypi.org",
                    "crates.io", "github.com", "raw.githubusercontent.com",
                ];
                let unknown = !known_hosts.iter().any(|h| url.contains(h));
                if unknown {
                    return SecurityResult::Ask {
                        reason: format!("AI wants to make a network request to: {}", url),
                        severity: SecuritySeverity::Medium,
                    };
                }
            }
        }

        SecurityResult::Allow
    }
}

fn extract_url(command: &str) -> Option<String> {
    // Naive URL extraction — find the first http(s):// token
    command
        .split_whitespace()
        .find(|t| t.starts_with("http://") || t.starts_with("https://"))
        .map(|s| s.to_string())
}

// ── Check 5: Package Install Validator ───────────────────────────────────────

pub struct PackageInstallValidator;

impl SecurityCheck for PackageInstallValidator {
    fn name(&self) -> &'static str { "PackageInstallValidator" }

    fn check(&self, action: &SuggestedAction) -> SecurityResult {
        let command = match action {
            SuggestedAction::RunCommand { command, .. } => command,
            _ => return SecurityResult::Allow,
        };

        let is_install = command.contains("npm install")
            || command.contains("npm i ")
            || command.contains("pip install")
            || command.contains("cargo add");

        if !is_install {
            return SecurityResult::Allow;
        }

        // Check for typosquatting-adjacent names (simplified heuristic)
        if let Some(reason) = check_typosquat(command) {
            return SecurityResult::Ask {
                reason,
                severity: SecuritySeverity::Medium,
            };
        }

        // Always ask before installing packages
        SecurityResult::Ask {
            reason: format!("AI wants to install packages: {}", command),
            severity: SecuritySeverity::Low,
        }
    }
}

fn check_typosquat(command: &str) -> Option<String> {
    // Known popular package names; warn if command contains visually similar names
    let suspicious_pairs = [
        ("lodash", "iodash"),
        ("express", "expres"),
        ("react", "recat"),
        ("requests", "requets"),
    ];
    let cmd_lower = command.to_lowercase();
    for (legit, fake) in &suspicious_pairs {
        if cmd_lower.contains(fake) && !cmd_lower.contains(legit) {
            return Some(format!(
                "Package '{}' looks similar to '{}'. Verify it's correct.",
                fake, legit
            ));
        }
    }
    None
}

// ── Check 6: Destructive Edit Detector ───────────────────────────────────────

pub struct DestructiveEditDetector {
    pub max_lines_deleted: usize,
}

impl Default for DestructiveEditDetector {
    fn default() -> Self {
        Self { max_lines_deleted: 50 }
    }
}

impl SecurityCheck for DestructiveEditDetector {
    fn name(&self) -> &'static str { "DestructiveEditDetector" }

    fn check(&self, action: &SuggestedAction) -> SecurityResult {
        let (old, new) = match action {
            SuggestedAction::ApplyCodeEdit { old_content, new_content, .. } => {
                (old_content, new_content)
            }
            _ => return SecurityResult::Allow,
        };

        let old_lines = old.lines().count();
        let new_lines = new.lines().count();
        let deleted = old_lines.saturating_sub(new_lines);

        if deleted > self.max_lines_deleted {
            return SecurityResult::Ask {
                reason: format!(
                    "AI wants to delete {} lines (threshold: {}). Review the diff?",
                    deleted, self.max_lines_deleted
                ),
                severity: SecuritySeverity::Medium,
            };
        }

        // >80% of a meaningful file replaced
        if old_lines > 10 && new_lines < old_lines / 5 {
            return SecurityResult::Ask {
                reason: "AI wants to remove most of this file's content.".into(),
                severity: SecuritySeverity::High,
            };
        }

        SecurityResult::Allow
    }
}

// ── Check 7: Custom Rules ─────────────────────────────────────────────────────

#[derive(Default, Clone, Debug)]
pub struct CustomRules {
    pub blocked_commands: Vec<String>,
    pub protected_file_patterns: Vec<String>,
    pub always_ask_before: Vec<String>,
}

impl SecurityCheck for CustomRules {
    fn name(&self) -> &'static str { "CustomRules" }

    fn check(&self, action: &SuggestedAction) -> SecurityResult {
        let content = action.content_string().to_lowercase();

        for blocked in &self.blocked_commands {
            if content.contains(&blocked.to_lowercase()) {
                return SecurityResult::Block {
                    reason: format!("Custom rule blocked this action: '{}'", blocked),
                    severity: SecuritySeverity::High,
                };
            }
        }

        for ask_pattern in &self.always_ask_before {
            if content.contains(&ask_pattern.to_lowercase()) {
                return SecurityResult::Ask {
                    reason: format!("Custom rule requires approval for: '{}'", ask_pattern),
                    severity: SecuritySeverity::Medium,
                };
            }
        }

        if let SuggestedAction::ApplyCodeEdit { file_path, .. } = action {
            for pattern in &self.protected_file_patterns {
                // Simple glob: support leading * wildcard
                let matches = if let Some(suffix) = pattern.strip_prefix('*') {
                    file_path.ends_with(suffix)
                } else {
                    file_path.contains(pattern.as_str())
                };
                if matches {
                    return SecurityResult::Ask {
                        reason: format!(
                            "Custom rule protects file matching '{}': {}",
                            pattern, file_path
                        ),
                        severity: SecuritySeverity::Medium,
                    };
                }
            }
        }

        SecurityResult::Allow
    }
}

// ── Security Pipeline ─────────────────────────────────────────────────────────

/// Runs all checks in order. Returns the first non-Allow result.
/// Always logs to the AuditTrail regardless of result.
pub struct SecurityPipeline {
    checks: Vec<Box<dyn SecurityCheck>>,
    pub audit: AuditTrail,
}

impl SecurityPipeline {
    pub fn new(project_root: PathBuf, custom_rules: CustomRules) -> Self {
        Self {
            checks: vec![
                Box::new(DangerousCommandFilter),
                Box::new(FileSystemBoundaryChecker { project_root }),
                Box::new(SecretLeakDetector),
                Box::new(NetworkAccessChecker),
                Box::new(PackageInstallValidator),
                Box::new(DestructiveEditDetector::default()),
                Box::new(custom_rules),
            ],
            audit: AuditTrail::new(),
        }
    }

    /// Run all checks. The first non-Allow result wins.
    /// Always appends to the audit trail.
    pub fn check(&mut self, action_type: &str, action: &SuggestedAction) -> SecurityResult {
        let mut triggered_checks = Vec::new();
        let mut final_result = SecurityResult::Allow;

        for check in &self.checks {
            let result = check.check(action);
            if !result.is_allow() {
                triggered_checks.push(check.name().to_string());
                final_result = result;
                break; // first non-Allow wins
            }
        }

        log::debug!(
            target: "ribhu::security",
            "[SECURITY] {} → {} (checks: {:?})",
            action_type,
            final_result.as_str(),
            triggered_checks
        );

        self.audit.append(AuditEntry {
            id: Uuid::new_v4().to_string(),
            timestamp_ms: now_ms(),
            action_type: action_type.to_string(),
            action_detail: action.content_string(),
            result: final_result.as_str().to_string(),
            checks_triggered: triggered_checks,
        });

        final_result
    }
}

// ── Audit Trail ───────────────────────────────────────────────────────────────

#[derive(Clone, Debug, serde::Serialize)]
pub struct AuditEntry {
    pub id: String,
    pub timestamp_ms: u64,
    pub action_type: String,
    pub action_detail: String,
    /// "allowed", "blocked", "asked"
    pub result: String,
    pub checks_triggered: Vec<String>,
}

/// Append-only log of every AI action (allowed, blocked, or asked).
/// Never delete entries — append-only is part of the spec.
pub struct AuditTrail {
    entries: Vec<AuditEntry>,
}

impl AuditTrail {
    pub fn new() -> Self {
        Self { entries: Vec::new() }
    }

    pub fn append(&mut self, entry: AuditEntry) {
        self.entries.push(entry);
    }

    pub fn all(&self) -> &[AuditEntry] {
        &self.entries
    }

    pub fn blocked(&self) -> impl Iterator<Item = &AuditEntry> {
        self.entries.iter().filter(|e| e.result == "blocked")
    }

    pub fn asked(&self) -> impl Iterator<Item = &AuditEntry> {
        self.entries.iter().filter(|e| e.result == "asked")
    }

    pub fn blocked_count(&self) -> usize {
        self.blocked().count()
    }

    pub fn approved_count(&self) -> usize {
        self.entries.iter().filter(|e| e.result == "allowed").count()
    }
}

impl Default for AuditTrail {
    fn default() -> Self {
        Self::new()
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn cmd(s: &str) -> SuggestedAction {
        SuggestedAction::RunCommand {
            command: s.to_string(),
            cwd: None,
        }
    }

    fn edit(path: &str, old: &str, new: &str) -> SuggestedAction {
        SuggestedAction::ApplyCodeEdit {
            file_path: path.to_string(),
            old_content: old.to_string(),
            new_content: new.to_string(),
        }
    }

    #[test]
    fn rm_rf_root_is_blocked() {
        let check = DangerousCommandFilter;
        let result = check.check(&cmd("rm -rf /"));
        assert!(matches!(result, SecurityResult::Block { severity: SecuritySeverity::Critical, .. }));
    }

    #[test]
    fn rm_rf_subdir_asks() {
        let check = DangerousCommandFilter;
        let result = check.check(&cmd("rm -rf ./old_builds"));
        assert!(matches!(result, SecurityResult::Ask { .. }));
    }

    #[test]
    fn sudo_asks() {
        let check = DangerousCommandFilter;
        let result = check.check(&cmd("sudo apt-get install build-essential"));
        assert!(matches!(result, SecurityResult::Ask { .. }));
    }

    #[test]
    fn npm_test_is_allowed() {
        let check = DangerousCommandFilter;
        let result = check.check(&cmd("npm run test"));
        assert!(matches!(result, SecurityResult::Allow));
    }

    #[test]
    fn edit_outside_project_is_blocked() {
        let check = FileSystemBoundaryChecker {
            project_root: PathBuf::from("/home/user/project"),
        };
        let result = check.check(&edit("/etc/passwd", "", "evil"));
        assert!(matches!(result, SecurityResult::Block { .. }));
    }

    #[test]
    fn env_file_asks() {
        let check = FileSystemBoundaryChecker {
            project_root: PathBuf::from("/project"),
        };
        // .env is within project but sensitive
        let result = check.check(&edit("/project/.env", "", "KEY=val"));
        assert!(matches!(result, SecurityResult::Ask { .. }));
    }

    #[test]
    fn anthropic_key_in_edit_is_blocked() {
        let check = SecretLeakDetector;
        let result = check.check(&edit(
            "config.ts",
            "",
            "const key = 'sk-ant-abcdefghijklmnopqrstuvwxyz123';",
        ));
        assert!(matches!(result, SecurityResult::Block { .. }));
    }

    #[test]
    fn normal_code_is_allowed() {
        let check = SecretLeakDetector;
        let result = check.check(&edit("main.rs", "", "fn main() { println!(\"hello\"); }"));
        assert!(matches!(result, SecurityResult::Allow));
    }

    #[test]
    fn deleting_60_lines_asks() {
        let check = DestructiveEditDetector { max_lines_deleted: 50 };
        let old: String = (0..100).map(|i| format!("line {}\n", i)).collect();
        let new: String = (0..30).map(|i| format!("line {}\n", i)).collect();
        let result = check.check(&edit("big.rs", &old, &new));
        assert!(matches!(result, SecurityResult::Ask { .. }));
    }

    #[test]
    fn pipeline_allow_safe_command() {
        let mut pipeline = SecurityPipeline::new(
            PathBuf::from("/project"),
            CustomRules::default(),
        );
        let result = pipeline.check("run_tests", &cmd("cargo test"));
        assert!(result.is_allow());
        assert_eq!(pipeline.audit.approved_count(), 1);
    }

    #[test]
    fn pipeline_block_dangerous_and_logs() {
        let mut pipeline = SecurityPipeline::new(
            PathBuf::from("/project"),
            CustomRules::default(),
        );
        let result = pipeline.check("system_cmd", &cmd("rm -rf /"));
        assert!(!result.is_allow());
        assert_eq!(pipeline.audit.blocked_count(), 1);
        let entry = &pipeline.audit.all()[0];
        assert!(entry.checks_triggered.contains(&"DangerousCommandFilter".to_string()));
    }

    #[test]
    fn custom_rules_block_user_patterns() {
        let rules = CustomRules {
            blocked_commands: vec!["kubectl delete".to_string()],
            protected_file_patterns: vec!["*.pem".to_string()],
            always_ask_before: vec!["docker rm".to_string()],
        };
        let check = CustomRules {
            blocked_commands: rules.blocked_commands.clone(),
            ..rules
        };
        let result = check.check(&cmd("kubectl delete pod my-pod"));
        assert!(matches!(result, SecurityResult::Block { .. }));
    }
}
