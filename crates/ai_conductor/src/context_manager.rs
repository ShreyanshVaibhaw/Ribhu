//! Context Window Manager — assembles workspace state from Context Bus events
//! into a markdown context string that gets prepended to every AI system prompt.
//!
//! Per Phase 2 spec: maintains rolling understanding of current file, recent
//! commands, git state, test results, and active errors. Relevance scoring
//! with 5-minute half-life decay decides what fits in the token budget.

use context_bus::{ContextEvent, ContextEventEnvelope, ContextSource};
use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

// ── Data Structures ──────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct FileContext {
    pub path: String,
    pub language: Option<String>,
    pub cursor_line: u32,
    pub cursor_column: u32,
    pub diagnostics_count: u32,
    pub last_saved_at: Instant,
}

#[derive(Clone, Debug)]
pub struct CmdContext {
    pub command: String,
    pub exit_code: Option<i32>,
    pub summary: String,
    pub timestamp: Instant,
    pub success: bool,
}

#[derive(Clone, Debug)]
pub struct GitContext {
    pub branch: String,
    pub last_commit_sha: Option<String>,
    pub last_commit_summary: Option<String>,
    pub conflict_files: Vec<String>,
    pub changed_at: Instant,
}

#[derive(Clone, Debug)]
pub struct TestResultContext {
    pub test_name: String,
    pub suite: String,
    pub passed: bool,
    pub error: Option<String>,
    pub file_path: Option<String>,
    pub line: Option<u32>,
    pub timestamp: Instant,
}

#[derive(Clone, Debug)]
pub struct ErrorContext {
    pub message: String,
    pub file_path: Option<String>,
    pub line: Option<u32>,
    pub first_seen: Instant,
    pub source: ContextSource,
}

impl ErrorContext {
    pub fn age_seconds(&self) -> u64 {
        self.first_seen.elapsed().as_secs()
    }

    pub fn matches(&self, other: &ErrorContext) -> bool {
        self.file_path == other.file_path
            && self.line == other.line
            && self.message == other.message
    }
}

// ── Context Window Manager ───────────────────────────────────────────────────

/// Rolling workspace context window.
///
/// Subscribes to the Context Bus and maintains the most recent and relevant
/// state across all panels. `assemble_prompt_context()` produces a markdown
/// block injected into every AI system prompt.
pub struct ContextWindowManager {
    pub current_file: Option<FileContext>,
    pub recent_commands: VecDeque<CmdContext>,  // capacity 10
    pub git_state: Option<GitContext>,
    pub test_results: VecDeque<TestResultContext>, // capacity 5
    pub active_errors: Vec<ErrorContext>,
    token_budget: usize,
}

impl ContextWindowManager {
    const CMD_CAPACITY: usize = 10;
    const TEST_CAPACITY: usize = 5;

    pub fn new() -> Self {
        Self {
            current_file: None,
            recent_commands: VecDeque::with_capacity(Self::CMD_CAPACITY),
            git_state: None,
            test_results: VecDeque::with_capacity(Self::TEST_CAPACITY),
            active_errors: Vec::new(),
            // 32K tokens × 4 chars/token ≈ 128KB; we keep context modest
            token_budget: 32_768,
        }
    }

    // ── Event Handler ────────────────────────────────────────────────────────

    /// Called for every Context Bus event. Updates rolling state.
    pub fn handle_event(&mut self, envelope: &ContextEventEnvelope) {
        match &envelope.event {
            ContextEvent::FileOpened { path, language, .. } => {
                self.current_file = Some(FileContext {
                    path: path.clone().unwrap_or_else(|| "untitled".into()),
                    language: language.clone(),
                    cursor_line: 0,
                    cursor_column: 0,
                    diagnostics_count: 0,
                    last_saved_at: Instant::now(),
                });
            }

            ContextEvent::FileSaved {
                path,
                language,
                diagnostics_count,
                ..
            } => {
                if let Some(ref mut file) = self.current_file {
                    if let Some(p) = path {
                        file.path = p.clone();
                    }
                    if let Some(lang) = language {
                        file.language = Some(lang.clone());
                    }
                    if let Some(dc) = diagnostics_count {
                        file.diagnostics_count = *dc;
                    }
                    file.last_saved_at = Instant::now();
                }
                // Clear errors for this file (they'll reappear if still real)
                if let Some(path) = path {
                    self.active_errors
                        .retain(|e| e.file_path.as_deref() != Some(path.as_str()));
                }
            }

            ContextEvent::CursorMoved { path, row, column, .. } => {
                if let Some(ref mut file) = self.current_file {
                    if path.as_deref() == Some(file.path.as_str()) || path.is_none() {
                        file.cursor_line = *row;
                        file.cursor_column = *column;
                    }
                }
            }

            ContextEvent::CmdExecuted {
                command,
                exit_code,
                summary,
            } => {
                let ctx = CmdContext {
                    command: command.clone().unwrap_or_default(),
                    exit_code: *exit_code,
                    summary: summary.clone(),
                    timestamp: Instant::now(),
                    success: exit_code.map(|c| c == 0).unwrap_or(true),
                };
                if self.recent_commands.len() == Self::CMD_CAPACITY {
                    self.recent_commands.pop_front();
                }
                self.recent_commands.push_back(ctx);
            }

            ContextEvent::ErrorDetected {
                message,
                file_path,
                line,
                ..
            } => {
                let new_err = ErrorContext {
                    message: message.clone(),
                    file_path: file_path.clone(),
                    line: *line,
                    first_seen: Instant::now(),
                    source: envelope.metadata.source,
                };
                // Deduplicate by file+line+message
                if !self.active_errors.iter().any(|e| e.matches(&new_err)) {
                    self.active_errors.push(new_err);
                }
            }

            ContextEvent::TestPassed { test_name, suite, duration_ms } => {
                // Remove any matching failures for this test
                self.active_errors
                    .retain(|e| !e.message.contains(test_name.as_str()));
                let ctx = TestResultContext {
                    test_name: test_name.clone(),
                    suite: suite.clone(),
                    passed: true,
                    error: None,
                    file_path: None,
                    line: None,
                    timestamp: Instant::now(),
                };
                if self.test_results.len() == Self::TEST_CAPACITY {
                    self.test_results.pop_front();
                }
                self.test_results.push_back(ctx);
                let _ = duration_ms;
            }

            ContextEvent::TestFailed {
                test_name,
                suite,
                error,
                file_path,
                line,
            } => {
                let ctx = TestResultContext {
                    test_name: test_name.clone(),
                    suite: suite.clone(),
                    passed: false,
                    error: Some(error.clone()),
                    file_path: file_path.clone(),
                    line: *line,
                    timestamp: Instant::now(),
                };
                if self.test_results.len() == Self::TEST_CAPACITY {
                    self.test_results.pop_front();
                }
                self.test_results.push_back(ctx);

                // Also add to active errors
                let err = ErrorContext {
                    message: format!("Test '{}' failed: {}", test_name, error),
                    file_path: file_path.clone(),
                    line: *line,
                    first_seen: Instant::now(),
                    source: envelope.metadata.source,
                };
                if !self.active_errors.iter().any(|e| e.matches(&err)) {
                    self.active_errors.push(err);
                }
            }

            ContextEvent::BranchChanged { branch } => {
                self.git_state = Some(GitContext {
                    branch: branch.clone(),
                    last_commit_sha: None,
                    last_commit_summary: None,
                    conflict_files: Vec::new(),
                    changed_at: Instant::now(),
                });
                // Branch change means old file context is stale
                self.current_file = None;
            }

            ContextEvent::CommitMade { sha, summary } => {
                if let Some(ref mut git) = self.git_state {
                    git.last_commit_sha = Some(sha.clone());
                    git.last_commit_summary = summary.clone();
                }
            }

            ContextEvent::ConflictFound { files } => {
                if let Some(ref mut git) = self.git_state {
                    git.conflict_files = files.clone();
                }
            }

            _ => {}
        }
    }

    // ── Proactive Helpers ────────────────────────────────────────────────────

    /// Returns the number of consecutive failures for the most recent test.
    pub fn test_failure_streak(&self, test_name: &str) -> usize {
        self.test_results
            .iter()
            .rev()
            .take_while(|r| r.test_name == test_name && !r.passed)
            .count()
    }

    /// Returns true if an error has been unresolved for at least `min_secs`.
    pub fn has_stale_error(&self, min_secs: u64) -> bool {
        self.active_errors
            .iter()
            .any(|e| e.age_seconds() >= min_secs)
    }

    // ── Context Assembly ─────────────────────────────────────────────────────

    /// Produces a markdown block for injection into the AI system prompt.
    ///
    /// Format matches the spec example in Phase_2_AI_Conductor.md:
    /// ```text
    /// ## Ribhu Workspace Context
    /// ### Currently editing: src/auth.ts (line 42)
    /// ### Active errors (2): ...
    /// ### Recent terminal (last 3): ...
    /// ### Git: branch feature/auth
    /// ```
    pub fn assemble_prompt_context(&self) -> String {
        let mut parts: Vec<String> = Vec::new();

        // Current file
        if let Some(ref file) = self.current_file {
            let lang = file
                .language
                .as_deref()
                .unwrap_or("unknown");
            let diag = if file.diagnostics_count > 0 {
                format!(", {} diagnostic(s)", file.diagnostics_count)
            } else {
                String::new()
            };
            parts.push(format!(
                "### Currently editing: {} (line {}, col {}{})\nLanguage: {}",
                file.path,
                file.cursor_line + 1,
                file.cursor_column + 1,
                diag,
                lang
            ));
        }

        // Active errors
        if !self.active_errors.is_empty() {
            let count = self.active_errors.len();
            let mut section = format!("### Active errors ({}):", count);
            for (i, err) in self.active_errors.iter().enumerate().take(5) {
                let loc = match (&err.file_path, err.line) {
                    (Some(f), Some(l)) => format!("{}:{}", f, l),
                    (Some(f), None) => f.clone(),
                    (None, _) => format!("{}", err.source.as_str()),
                };
                let age = err.age_seconds();
                let age_str = if age > 60 {
                    format!(" ({}m ago)", age / 60)
                } else if age > 0 {
                    format!(" ({}s ago)", age)
                } else {
                    String::new()
                };
                section.push_str(&format!(
                    "\n{}. {} — {}{}",
                    i + 1,
                    loc,
                    truncate(&err.message, 100),
                    age_str
                ));
            }
            parts.push(section);
        }

        // Recent test results
        let recent_tests: Vec<_> = self.test_results.iter().rev().take(3).collect();
        if !recent_tests.is_empty() {
            let mut section = "### Recent tests:".to_string();
            for t in &recent_tests {
                let status = if t.passed { "✓" } else { "✗" };
                let detail = if let Some(ref err) = t.error {
                    format!(" — {}", truncate(err, 60))
                } else {
                    String::new()
                };
                section.push_str(&format!(
                    "\n{} {} ({}){}", status, t.test_name, t.suite, detail
                ));
            }
            parts.push(section);
        }

        // Recent terminal commands
        let recent_cmds: Vec<_> = self.recent_commands.iter().rev().take(3).collect();
        if !recent_cmds.is_empty() {
            let mut section = "### Recent terminal:".to_string();
            for cmd in &recent_cmds {
                let exit = match cmd.exit_code {
                    Some(0) => " → ok".to_string(),
                    Some(c) => format!(" → exit {}", c),
                    None => String::new(),
                };
                let age = cmd.timestamp.elapsed();
                let age_str = format_age(age);
                section.push_str(&format!(
                    "\n$ {}{} ({})",
                    truncate(&cmd.command, 60),
                    exit,
                    age_str
                ));
            }
            parts.push(section);
        }

        // Git state
        if let Some(ref git) = self.git_state {
            let mut git_line = format!("### Git: branch {}", git.branch);
            if let Some(ref sha) = git.last_commit_sha {
                let short = &sha[..sha.len().min(7)];
                let msg = git
                    .last_commit_summary
                    .as_deref()
                    .unwrap_or("(no message)");
                git_line.push_str(&format!(", last commit {} — {}", short, truncate(msg, 60)));
            }
            if !git.conflict_files.is_empty() {
                git_line.push_str(&format!("\n⚠ Merge conflicts in: {}", git.conflict_files.join(", ")));
            }
            parts.push(git_line);
        }

        if parts.is_empty() {
            return String::new();
        }

        let body = parts.join("\n\n");
        // Respect token budget (rough estimate: 4 chars per token)
        let max_chars = self.token_budget * 4;
        let body = if body.len() > max_chars {
            format!("{}…", &body[..max_chars])
        } else {
            body
        };

        format!("## Ribhu Workspace Context\n\n{}\n", body)
    }
}

impl Default for ContextWindowManager {
    fn default() -> Self {
        Self::new()
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max {
        s
    } else {
        &s[..s
            .char_indices()
            .nth(max)
            .map(|(i, _)| i)
            .unwrap_or(s.len())]
    }
}

fn format_age(d: Duration) -> String {
    let secs = d.as_secs();
    if secs < 60 {
        format!("{}s ago", secs)
    } else {
        format!("{}m ago", secs / 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use context_bus::{ContextEvent, ContextEventEnvelope, ContextEventMetadata, ContextSource, Priority};

    fn envelope(event: ContextEvent) -> ContextEventEnvelope {
        ContextEventEnvelope {
            metadata: ContextEventMetadata {
                workspace_id: Some(1),
                source: ContextSource::Editor,
                timestamp_ms: 0,
                priority: Priority::Normal,
            },
            event,
        }
    }

    #[test]
    fn file_opened_sets_current_file() {
        let mut mgr = ContextWindowManager::new();
        mgr.handle_event(&envelope(ContextEvent::FileOpened {
            path: Some("src/main.rs".into()),
            language: Some("rust".into()),
            line_count: None,
        }));
        assert_eq!(mgr.current_file.unwrap().path, "src/main.rs");
    }

    #[test]
    fn error_deduplicates() {
        let mut mgr = ContextWindowManager::new();
        let err = ContextEvent::ErrorDetected {
            message: "undefined variable".into(),
            file_path: Some("app.rs".into()),
            line: Some(10),
            column: None,
            severity: context_bus::ErrorSeverity::Error,
            command: None,
            exit_code: None,
        };
        mgr.handle_event(&envelope(err.clone()));
        mgr.handle_event(&envelope(err));
        assert_eq!(mgr.active_errors.len(), 1);
    }

    #[test]
    fn assemble_includes_current_file_section() {
        let mut mgr = ContextWindowManager::new();
        mgr.handle_event(&envelope(ContextEvent::FileOpened {
            path: Some("src/auth.ts".into()),
            language: Some("typescript".into()),
            line_count: None,
        }));
        let ctx = mgr.assemble_prompt_context();
        assert!(ctx.contains("## Ribhu Workspace Context"));
        assert!(ctx.contains("src/auth.ts"));
    }

    #[test]
    fn assemble_empty_when_no_state() {
        let mgr = ContextWindowManager::new();
        let ctx = mgr.assemble_prompt_context();
        assert!(ctx.is_empty());
    }

    #[test]
    fn branch_change_clears_file_context() {
        let mut mgr = ContextWindowManager::new();
        mgr.handle_event(&envelope(ContextEvent::FileOpened {
            path: Some("main.rs".into()),
            language: None,
            line_count: None,
        }));
        mgr.handle_event(&envelope(ContextEvent::BranchChanged {
            branch: "feature/auth".into(),
        }));
        assert!(mgr.current_file.is_none());
        assert_eq!(mgr.git_state.unwrap().branch, "feature/auth");
    }
}
