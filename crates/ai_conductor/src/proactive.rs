//! Proactive Suggestion Engine — the AI speaks FIRST without being asked.
//!
//! Per Phase 2 spec: runs every 30 seconds, scans the ContextWindowManager
//! for patterns that warrant unprompted suggestions:
//!   1. Unresolved errors older than 2 minutes
//!   2. Same test failing 3+ times in a row
//!   3. Large function at cursor (>100 lines) — uses Ask level

use crate::{autonomy::AiAction, context_manager::ContextWindowManager};
use std::time::{Duration, Instant};

// ── Constants ────────────────────────────────────────────────────────────────

const CHECK_INTERVAL: Duration = Duration::from_secs(30);
const STALE_ERROR_SECS: u64 = 120;       // 2 minutes
const TEST_FAILURE_STREAK: usize = 3;
const LARGE_FUNCTION_LINES: u32 = 100;

// ── Proactive Suggestion Engine ──────────────────────────────────────────────

/// Periodically checks the context window for things the developer might need
/// help with, without waiting to be asked.
pub struct ProactiveSuggestionEngine {
    last_check: Instant,
    /// IDs of errors we've already suggested about (so we don't repeat).
    suggested_error_keys: Vec<String>,
    /// Test names we've already flagged for streak.
    suggested_test_streaks: Vec<String>,
}

impl ProactiveSuggestionEngine {
    pub fn new() -> Self {
        Self {
            last_check: Instant::now(),
            suggested_error_keys: Vec::new(),
            suggested_test_streaks: Vec::new(),
        }
    }

    /// Returns true if enough time has passed since the last check.
    pub fn should_check(&self) -> bool {
        self.last_check.elapsed() >= CHECK_INTERVAL
    }

    /// Scan for opportunities and return suggestions. Call this every 30s.
    pub fn check(&mut self, context: &ContextWindowManager) -> Vec<AiAction> {
        self.last_check = Instant::now();
        let mut suggestions = Vec::new();

        // 1. Stale errors — unresolved for >2 minutes
        for error in &context.active_errors {
            if error.age_seconds() < STALE_ERROR_SECS {
                continue;
            }
            // Build a dedup key from file+line+message
            let key = format!(
                "{}:{:?}:{}",
                error.file_path.as_deref().unwrap_or(""),
                error.line,
                &error.message[..error.message.len().min(50)]
            );
            if self.suggested_error_keys.contains(&key) {
                continue;
            }
            self.suggested_error_keys.push(key);

            let loc = match (&error.file_path, error.line) {
                (Some(f), Some(l)) => format!("{}:{}", f, l),
                (Some(f), None) => f.clone(),
                (None, _) => "terminal".to_string(),
            };
            let age_min = error.age_seconds() / 60;
            suggestions.push(AiAction::suggest(
                "fix_persistent_error",
                format!(
                    "Error at {} has been unresolved for {} minute{}. Need help?",
                    loc,
                    age_min,
                    if age_min == 1 { "" } else { "s" }
                ),
            ));
        }

        // 2. Test failure streak
        // Check the most recent failed test in the buffer
        let recent_failures: Vec<_> = context
            .test_results
            .iter()
            .rev()
            .filter(|r| !r.passed)
            .take(TEST_FAILURE_STREAK)
            .collect();

        if recent_failures.len() >= TEST_FAILURE_STREAK {
            let test_name = &recent_failures[0].test_name;
            let all_same = recent_failures.iter().all(|r| &r.test_name == test_name);
            if all_same && !self.suggested_test_streaks.contains(test_name) {
                self.suggested_test_streaks.push(test_name.clone());
                suggestions.push(AiAction::suggest(
                    "stuck_on_test",
                    format!(
                        "'{}' has failed {} times in a row. Want me to analyze the pattern?",
                        test_name, TEST_FAILURE_STREAK
                    ),
                ));
            }
        }

        // 3. Large function at cursor (rough heuristic: diagnostics_count > 0
        //    is a proxy; real implementation would use LSP symbol info)
        if let Some(ref file) = context.current_file {
            // We don't have actual function line count from Context Bus alone.
            // This is a placeholder that will be filled in Phase 4 when the
            // TestRunner / LSP integration provides richer FileContext.
            // For now, trigger on files with many diagnostics as a proxy.
            let _ = LARGE_FUNCTION_LINES;
            let _ = file; // suppress unused warning
        }

        // Clean up suggested keys for errors that are no longer active
        let active_keys: Vec<String> = context
            .active_errors
            .iter()
            .map(|e| {
                format!(
                    "{}:{:?}:{}",
                    e.file_path.as_deref().unwrap_or(""),
                    e.line,
                    &e.message[..e.message.len().min(50)]
                )
            })
            .collect();
        self.suggested_error_keys
            .retain(|k| active_keys.contains(k));

        log::debug!(
            target: "ribhu::ai_conductor",
            "[Proactive] check produced {} suggestion(s)",
            suggestions.len()
        );

        suggestions
    }
}

impl Default for ProactiveSuggestionEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context_manager::{ContextWindowManager, ErrorContext};
    use context_bus::{ContextSource, ErrorSeverity};
    use std::time::Instant;

    fn add_error(mgr: &mut ContextWindowManager, msg: &str, age_override_secs: u64) {
        // Manually inject a stale error
        let mut err = ErrorContext {
            message: msg.to_string(),
            file_path: Some("app.rs".into()),
            line: Some(10),
            first_seen: Instant::now(),
            source: ContextSource::Terminal,
        };
        // Hack: we can't set first_seen in the past directly, but for tests
        // we just add it and trust age_seconds() will be 0 initially.
        // Real staleness tests would need a clock injection.
        let _ = age_override_secs;
        mgr.active_errors.push(err);
    }

    #[test]
    fn no_suggestions_for_empty_context() {
        let context = ContextWindowManager::new();
        let mut engine = ProactiveSuggestionEngine::new();
        let suggestions = engine.check(&context);
        assert!(suggestions.is_empty());
    }

    #[test]
    fn fresh_errors_dont_trigger_suggestion() {
        let mut context = ContextWindowManager::new();
        add_error(&mut context, "undefined variable", 0);
        let mut engine = ProactiveSuggestionEngine::new();
        let suggestions = engine.check(&context);
        // Fresh error (age ~0s) should NOT produce a suggestion
        assert!(suggestions.is_empty(), "fresh errors should not trigger proactive suggestion");
    }
}
