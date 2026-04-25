use std::path::Path;

/// Identifies which panel published an event.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ContextSource {
    Editor,
    Terminal,
    Git,
    AgentUi,
    TestRunner,
    ApiClient,
    Database,
    Browser,
    Debug,
    RemoteTerminal,
    RemoteEditor,
    RemoteMonitor,
}

impl ContextSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Editor => "editor",
            Self::Terminal => "terminal",
            Self::Git => "git",
            Self::AgentUi => "agent_ui",
            Self::TestRunner => "test_runner",
            Self::ApiClient => "api_client",
            Self::Database => "database",
            Self::Browser => "browser",
            Self::Debug => "debug",
            Self::RemoteTerminal => "remote_terminal",
            Self::RemoteEditor => "remote_editor",
            Self::RemoteMonitor => "remote_monitor",
        }
    }
}

/// Event priority level for the Context Bus.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum Priority {
    High,
    #[default]
    Normal,
    Low,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AiSurfaceKind {
    Diff,
    Terminal,
}

/// Severity level for detected errors.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ErrorSeverity {
    Error,
    Warning,
    Info,
}

/// All 19 typed events that flow through the Context Bus.
///
/// Panels publish these when significant things happen.
/// Other panels subscribe and react to relevant events.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ContextEvent {
    // ── Editor events ──────────────────────────────────────
    FileOpened {
        path: Option<String>,
        language: Option<String>,
        line_count: Option<u32>,
    },
    FileSaved {
        path: Option<String>,
        language: Option<String>,
        lines_changed: Option<u32>,
        diagnostics_count: Option<u32>,
    },
    FileCreated {
        path: String,
    },
    CursorMoved {
        path: Option<String>,
        row: u32,
        column: u32,
        symbol_name: Option<String>,
    },

    // ── Terminal events ────────────────────────────────────
    CmdExecuted {
        command: Option<String>,
        exit_code: Option<i32>,
        summary: String,
    },
    ErrorDetected {
        message: String,
        file_path: Option<String>,
        line: Option<u32>,
        column: Option<u32>,
        severity: ErrorSeverity,
        command: Option<String>,
        exit_code: Option<i32>,
    },
    OutputLogged {
        lines: Vec<String>,
    },

    // ── Test events ────────────────────────────────────────
    TestPassed {
        test_name: String,
        suite: String,
        duration_ms: u64,
    },
    TestFailed {
        test_name: String,
        suite: String,
        error: String,
        file_path: Option<String>,
        line: Option<u32>,
    },

    // ── Git events ─────────────────────────────────────────
    BranchChanged {
        branch: String,
    },
    CommitMade {
        sha: String,
        summary: Option<String>,
    },
    ConflictFound {
        files: Vec<String>,
    },

    // ── API / Database events (Phases 5-6) ─────────────────
    ApiRequest {
        method: String,
        url: String,
    },
    ApiResponse {
        status: u16,
        url: String,
        duration_ms: u64,
    },
    DbQuery {
        sql: String,
        row_count: Option<u32>,
        duration_ms: u64,
    },

    // ── AI events ──────────────────────────────────────────
    AiToolSurfaced {
        kind: AiSurfaceKind,
    },
    AiSuggestion {
        action: String,
        /// Confidence as a percentage (0–100).
        confidence_pct: u8,
        description: String,
    },

    // ── Workspace events ───────────────────────────────────
    ProjectSwitched {
        from_project: Option<String>,
        to_project: String,
    },
}

impl ContextEvent {
    pub fn summary(&self) -> String {
        match self {
            Self::FileOpened { path, .. } => format!("Opened {}", display_path(path.as_deref())),
            Self::FileSaved { path, .. } => format!("Saved {}", display_path(path.as_deref())),
            Self::FileCreated { path } => format!("Created {}", display_path(Some(path))),
            Self::CursorMoved { path, row, column, .. } => {
                format!(
                    "Cursor moved in {} to {}:{}",
                    display_path(path.as_deref()),
                    row + 1,
                    column + 1
                )
            }
            Self::CmdExecuted {
                command,
                exit_code,
                summary,
            } => match (command.as_deref(), exit_code) {
                (Some(command), Some(exit_code)) => {
                    format!("Command `{command}` finished with exit code {exit_code}: {summary}")
                }
                (Some(command), None) => format!("Command `{command}` finished: {summary}"),
                (None, Some(exit_code)) => {
                    format!("Terminal task finished with exit code {exit_code}: {summary}")
                }
                (None, None) => format!("Terminal task finished: {summary}"),
            },
            Self::ErrorDetected {
                message,
                command,
                exit_code,
                ..
            } => match (command.as_deref(), exit_code) {
                (Some(command), Some(exit_code)) => {
                    format!("Command `{command}` failed with exit code {exit_code}: {message}")
                }
                (Some(command), None) => format!("Command `{command}` failed: {message}"),
                (None, Some(exit_code)) => {
                    format!("Error detected (exit code {exit_code}): {message}")
                }
                (None, None) => format!("Error detected: {message}"),
            },
            Self::OutputLogged { lines } => {
                let count = lines.len();
                format!("Terminal output ({count} lines)")
            }
            Self::TestPassed {
                test_name,
                duration_ms,
                ..
            } => format!("Test passed: {test_name} ({duration_ms}ms)"),
            Self::TestFailed {
                test_name,
                error,
                ..
            } => format!("Test failed: {test_name}: {error}"),
            Self::BranchChanged { branch } => format!("Switched to branch {branch}"),
            Self::CommitMade { sha, summary } => match summary.as_deref() {
                Some(summary) => format!("Commit {} created: {summary}", short_sha(sha)),
                None => format!("Commit {} created", short_sha(sha)),
            },
            Self::ConflictFound { files } => {
                let count = files.len();
                format!("Merge conflict in {count} file(s)")
            }
            Self::ApiRequest { method, url } => format!("{method} {url}"),
            Self::ApiResponse {
                status,
                url,
                duration_ms,
            } => format!("{status} {url} ({duration_ms}ms)"),
            Self::DbQuery {
                sql, duration_ms, ..
            } => {
                let truncated = if sql.chars().count() > 60 {
                    let end: String = sql.chars().take(60).collect();
                    format!("{end}...")
                } else {
                    sql.clone()
                };
                format!("Query ({duration_ms}ms): {truncated}")
            }
            Self::AiToolSurfaced { kind } => match kind {
                AiSurfaceKind::Diff => "Agent surfaced a diff".to_string(),
                AiSurfaceKind::Terminal => "Agent surfaced a terminal".to_string(),
            },
            Self::AiSuggestion {
                action,
                description,
                ..
            } => format!("AI suggestion: {action} — {description}"),
            Self::ProjectSwitched { to_project, .. } => {
                format!("Switched to project {to_project}")
            }
        }
    }

    /// Returns the default priority for this event type.
    pub fn default_priority(&self) -> Priority {
        match self {
            Self::ErrorDetected { .. }
            | Self::TestFailed { .. }
            | Self::ConflictFound { .. }
            | Self::AiSuggestion { .. } => Priority::High,
            Self::CursorMoved { .. } | Self::OutputLogged { .. } => Priority::Low,
            _ => Priority::Normal,
        }
    }

    /// Returns true if this event should never be deduplicated.
    pub fn never_dedup(&self) -> bool {
        matches!(
            self,
            Self::ErrorDetected { .. }
                | Self::TestFailed { .. }
                | Self::ConflictFound { .. }
                | Self::AiSuggestion { .. }
        )
    }

    /// Returns a short type name for logging/stats.
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::FileOpened { .. } => "FileOpened",
            Self::FileSaved { .. } => "FileSaved",
            Self::FileCreated { .. } => "FileCreated",
            Self::CursorMoved { .. } => "CursorMoved",
            Self::CmdExecuted { .. } => "CmdExecuted",
            Self::ErrorDetected { .. } => "ErrorDetected",
            Self::OutputLogged { .. } => "OutputLogged",
            Self::TestPassed { .. } => "TestPassed",
            Self::TestFailed { .. } => "TestFailed",
            Self::BranchChanged { .. } => "BranchChanged",
            Self::CommitMade { .. } => "CommitMade",
            Self::ConflictFound { .. } => "ConflictFound",
            Self::ApiRequest { .. } => "ApiRequest",
            Self::ApiResponse { .. } => "ApiResponse",
            Self::DbQuery { .. } => "DbQuery",
            Self::AiToolSurfaced { .. } => "AiToolSurfaced",
            Self::AiSuggestion { .. } => "AiSuggestion",
            Self::ProjectSwitched { .. } => "ProjectSwitched",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ContextEventMetadata {
    pub workspace_id: Option<i64>,
    pub source: ContextSource,
    pub timestamp_ms: u64,
    pub priority: Priority,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContextEventEnvelope {
    pub metadata: ContextEventMetadata,
    pub event: ContextEvent,
}

impl ContextEventEnvelope {
    pub fn summary(&self) -> String {
        self.event.summary()
    }
}

fn display_path(path: Option<&str>) -> String {
    path.and_then(|path| {
        Path::new(path)
            .file_name()
            .and_then(|name| name.to_str())
            .map(ToOwned::to_owned)
    })
    .or_else(|| path.map(ToOwned::to_owned))
    .unwrap_or_else(|| "untitled buffer".to_string())
}

fn short_sha(sha: &str) -> &str {
    let end = sha
        .char_indices()
        .nth(7)
        .map(|(index, _)| index)
        .unwrap_or(sha.len());
    &sha[..end]
}
