//! Dev-server process detection and restart system — Phase 3.3
//!
//! Detects common dev-server commands from Context Bus CmdExecuted events,
//! saves them in the project profile, and restarts them on project switch.
//!
//! Detected patterns:
//!   npm run dev, npm start, yarn dev, pnpm dev
//!   cargo run, cargo watch
//!   python manage.py runserver, python -m flask run
//!   go run .

use crate::DevProcess;
use context_bus::ContextEvent;
use std::path::PathBuf;

// ── Detection ─────────────────────────────────────────────────────────────────

/// Dev-server command patterns (checked via contains, case-insensitive).
static DEV_SERVER_PATTERNS: &[&str] = &[
    "npm run dev",
    "npm run start",
    "npm start",
    "yarn dev",
    "yarn start",
    "pnpm dev",
    "pnpm start",
    "cargo run",
    "cargo watch",
    "python manage.py runserver",
    "python -m flask run",
    "flask run",
    "go run .",
    "go run main.go",
    "bun dev",
    "bun start",
    "deno task dev",
];

/// Return true if `command` looks like a dev-server invocation.
pub fn is_dev_server_command(command: &str) -> bool {
    let lower = command.to_lowercase();
    DEV_SERVER_PATTERNS.iter().any(|p| lower.contains(p))
}

/// Extract a `DevProcess` from a CmdExecuted context event if it matches a
/// dev-server pattern.
pub fn detect_from_event(event: &ContextEvent, tab_index: usize, cwd: PathBuf) -> Option<DevProcess> {
    let ContextEvent::CmdExecuted { command, .. } = event else {
        return None;
    };
    let command = command.as_deref()?;
    if !is_dev_server_command(command) {
        return None;
    }
    Some(DevProcess {
        command: command.to_string(),
        cwd,
        tab_index,
    })
}

// ── ProcessRestoreResult ──────────────────────────────────────────────────────

/// Outcome of a process restart attempt.
#[derive(Debug, Clone)]
pub enum ProcessRestoreResult {
    /// Command string that should be executed in a new terminal tab.
    NeedsTerminal { command: String, cwd: PathBuf },
}

/// Convert saved DevProcesses into restore instructions.
pub fn plan_restores(processes: &[DevProcess]) -> Vec<ProcessRestoreResult> {
    processes
        .iter()
        .map(|p| ProcessRestoreResult::NeedsTerminal {
            command: p.command.clone(),
            cwd: p.cwd.clone(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_npm_run_dev() {
        assert!(is_dev_server_command("npm run dev"));
        assert!(is_dev_server_command("NPM RUN DEV"));
    }

    #[test]
    fn detects_cargo_run() {
        assert!(is_dev_server_command("cargo run"));
        assert!(is_dev_server_command("cargo run --release"));
    }

    #[test]
    fn does_not_detect_safe_commands() {
        assert!(!is_dev_server_command("ls -la"));
        assert!(!is_dev_server_command("git status"));
        assert!(!is_dev_server_command("cargo build"));
    }

    #[test]
    fn detect_from_cmd_executed_event() {
        let event = ContextEvent::CmdExecuted {
            command: Some("npm run dev".into()),
            exit_code: None,
            summary: "npm run dev".into(),
        };
        let cwd = PathBuf::from("/home/user/project");
        let result = detect_from_event(&event, 0, cwd.clone());
        assert!(result.is_some());
        let p = result.unwrap();
        assert_eq!(p.command, "npm run dev");
        assert_eq!(p.cwd, cwd);
    }

    #[test]
    fn ignores_non_cmd_events() {
        let event = ContextEvent::FileSaved {
            path: Some("src/main.rs".into()),
            language: None,
            lines_changed: None,
            diagnostics_count: None,
        };
        assert!(detect_from_event(&event, 0, PathBuf::from(".")).is_none());
    }
}
