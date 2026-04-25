//! Autonomy system — classifies AI actions into 3 levels and routes them
//! to the appropriate notification UI.
//!
//! Level 1 (Auto)    — silent execution, logged to activity feed.
//! Level 2 (Suggest) — toast notification, user accepts or dismisses.
//! Level 3 (Ask)     — modal dialog with diff preview, user must decide.

use context_bus::{ContextEvent, ContextEventEnvelope};
use std::fmt;
use uuid::Uuid;

// ── Types ────────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AutonomyLevel {
    /// Execute silently and log to activity feed.
    Auto,
    /// Show a toast; user can accept or dismiss.
    Suggest,
    /// Show a modal with diff preview; user must explicitly approve.
    Ask,
}

impl fmt::Display for AutonomyLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Auto => write!(f, "Auto"),
            Self::Suggest => write!(f, "Suggest"),
            Self::Ask => write!(f, "Ask"),
        }
    }
}

/// A concrete action the AI wants to take, with enough metadata to render
/// the right notification and execute on approval.
#[derive(Clone, Debug)]
pub struct AiAction {
    /// Unique ID for tracking outcome in PreferenceTracker.
    pub id: String,
    /// How to present this action to the user.
    pub level: AutonomyLevel,
    /// Short key for preference learning (e.g., "fix_test", "fix_error").
    pub action_type: String,
    /// Human-readable description shown in the notification.
    pub description: String,
    /// What the AI will actually do when the user approves.
    pub suggested_action: Option<SuggestedAction>,
}

impl AiAction {
    pub fn suggest(action_type: &str, description: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            level: AutonomyLevel::Suggest,
            action_type: action_type.to_string(),
            description: description.into(),
            suggested_action: None,
        }
    }

    pub fn ask(action_type: &str, description: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            level: AutonomyLevel::Ask,
            action_type: action_type.to_string(),
            description: description.into(),
            suggested_action: None,
        }
    }

    pub fn auto(action_type: &str, description: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            level: AutonomyLevel::Auto,
            action_type: action_type.to_string(),
            description: description.into(),
            suggested_action: None,
        }
    }
}

/// What the AI will actually execute on approval.
#[derive(Clone, Debug)]
pub enum SuggestedAction {
    /// Apply a code edit to a file in the editor.
    ApplyCodeEdit {
        file_path: String,
        /// Original content (for DestructiveEditDetector and diff display).
        old_content: String,
        /// Replacement content.
        new_content: String,
    },
    /// Run a terminal command.
    RunCommand {
        command: String,
        cwd: Option<String>,
    },
    /// Open a file, optionally at a specific line.
    OpenFile {
        path: String,
        line: Option<u32>,
    },
    /// Send a message to the AI Agent Panel on behalf of the user.
    SendToAgent {
        message: String,
    },
}

impl SuggestedAction {
    /// Returns a plain-text summary for security checks and audit logging.
    pub fn content_string(&self) -> String {
        match self {
            Self::ApplyCodeEdit { file_path, new_content, .. } => {
                format!("Edit {}: {}", file_path, &new_content[..new_content.len().min(200)])
            }
            Self::RunCommand { command, .. } => command.clone(),
            Self::OpenFile { path, line } => match line {
                Some(l) => format!("Open {}:{}", path, l),
                None => format!("Open {}", path),
            },
            Self::SendToAgent { message } => message.clone(),
        }
    }
}

// ── How the user responded ───────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ActionOutcome {
    Accepted,
    Dismissed,
    /// User accepted but modified the suggested content before applying.
    Modified,
}

// ── Action Classifier ────────────────────────────────────────────────────────

/// Classifies Context Bus events into `AiAction`s.
///
/// Returns `None` when no AI action is warranted for the event.
/// The returned action's `level` is the _default_ before preference overrides.
pub struct ActionClassifier;

impl ActionClassifier {
    pub fn classify(envelope: &ContextEventEnvelope) -> Option<AiAction> {
        match &envelope.event {
            // ── Level 2: Suggest — toast notification ──────────────────────

            ContextEvent::TestFailed { test_name, error, file_path, line, .. } => {
                let loc = match (file_path, line) {
                    (Some(f), Some(l)) => format!(" ({}:{})", f, l),
                    (Some(f), None) => format!(" ({})", f),
                    _ => String::new(),
                };
                Some(AiAction::suggest(
                    "fix_test",
                    format!(
                        "Test '{}' failed{}: {}. Want me to look at it?",
                        test_name,
                        loc,
                        &error[..error.len().min(80)]
                    ),
                ))
            }

            ContextEvent::ErrorDetected { message, file_path: Some(path), line: Some(l), .. } => {
                Some(AiAction::suggest(
                    "fix_error",
                    format!(
                        "Error on {}:{} — {}. I can help fix this.",
                        path,
                        l,
                        &message[..message.len().min(80)]
                    ),
                ))
            }

            ContextEvent::ConflictFound { files } => {
                let count = files.len();
                Some(AiAction::suggest(
                    "resolve_conflict",
                    format!(
                        "Merge conflict in {} file{}. Want me to help resolve?",
                        count,
                        if count == 1 { "" } else { "s" }
                    ),
                ))
            }

            // ── Level 1: Auto — silent ─────────────────────────────────────

            ContextEvent::FileSaved {
                diagnostics_count: Some(0),
                ..
            } => {
                // Clean save — could auto-format, but no visible notification needed
                None
            }

            // Most events don't warrant a proactive action
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use context_bus::{
        ContextEventEnvelope, ContextEventMetadata, ContextSource, ErrorSeverity, Priority,
    };

    fn envelope(event: context_bus::ContextEvent) -> ContextEventEnvelope {
        ContextEventEnvelope {
            metadata: ContextEventMetadata {
                workspace_id: Some(1),
                source: ContextSource::TestRunner,
                timestamp_ms: 0,
                priority: Priority::High,
            },
            event,
        }
    }

    #[test]
    fn test_failed_produces_suggest_action() {
        let ev = envelope(context_bus::ContextEvent::TestFailed {
            test_name: "test_login".into(),
            suite: "auth".into(),
            error: "assertion failed: left != right".into(),
            file_path: Some("src/auth.rs".into()),
            line: Some(42),
        });
        let action = ActionClassifier::classify(&ev).unwrap();
        assert_eq!(action.level, AutonomyLevel::Suggest);
        assert_eq!(action.action_type, "fix_test");
        assert!(action.description.contains("test_login"));
    }

    #[test]
    fn error_detected_with_location_produces_suggest() {
        let ev = envelope(context_bus::ContextEvent::ErrorDetected {
            message: "cannot find value `x`".into(),
            file_path: Some("main.rs".into()),
            line: Some(5),
            column: None,
            severity: ErrorSeverity::Error,
            command: None,
            exit_code: None,
        });
        let action = ActionClassifier::classify(&ev).unwrap();
        assert_eq!(action.action_type, "fix_error");
    }

    #[test]
    fn branch_changed_produces_no_action() {
        let ev = envelope(context_bus::ContextEvent::BranchChanged {
            branch: "main".into(),
        });
        assert!(ActionClassifier::classify(&ev).is_none());
    }
}
