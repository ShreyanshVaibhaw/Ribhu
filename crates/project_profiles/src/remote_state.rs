//! Remote session state for Project Profiles — Phase 8E.
//!
//! Saves and restores SSH connection details, remote working directory,
//! terminal sessions, running processes, and cumulative session cost so
//! that reconnecting to a cloud instance picks up where you left off.

use serde::{Deserialize, Serialize};

/// Saved state for a remote SSH session within a project profile.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RemoteSessionState {
    pub host: String,
    pub user: String,
    pub port: u16,
    pub remote_cwd: String,
    pub terminal_sessions: Vec<RemoteTerminalState>,
    pub running_processes: Vec<RemoteProcess>,
    pub cumulative_cost: f64,
    pub total_session_seconds: u64,
}

/// State of a single remote terminal session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteTerminalState {
    pub cwd: String,
    pub last_command: Option<String>,
}

/// A running process detected on the remote machine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteProcess {
    pub name: String,
    pub command: String,
    pub pid: Option<u32>,
}

impl RemoteSessionState {
    pub fn new(host: impl Into<String>, user: impl Into<String>, port: u16) -> Self {
        Self {
            host: host.into(),
            user: user.into(),
            port,
            ..Default::default()
        }
    }

    pub fn ssh_url(&self) -> String {
        if self.port == 22 {
            format!("{}@{}", self.user, self.host)
        } else {
            format!("{}@{}:{}", self.user, self.host, self.port)
        }
    }

    pub fn cost_display(&self) -> String {
        format!("${:.2}", self.cumulative_cost)
    }

    pub fn uptime_display(&self) -> String {
        let hours = self.total_session_seconds / 3600;
        let minutes = (self.total_session_seconds % 3600) / 60;
        if hours > 0 {
            format!("{}h {}m", hours, minutes)
        } else {
            format!("{}m", minutes)
        }
    }

    pub fn add_terminal(&mut self, cwd: impl Into<String>, last_command: Option<String>) {
        self.terminal_sessions.push(RemoteTerminalState {
            cwd: cwd.into(),
            last_command,
        });
    }

    pub fn add_process(&mut self, name: impl Into<String>, command: impl Into<String>) {
        self.running_processes.push(RemoteProcess {
            name: name.into(),
            command: command.into(),
            pid: None,
        });
    }

    pub fn is_empty(&self) -> bool {
        self.terminal_sessions.is_empty() && self.running_processes.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ssh_url_default_port() {
        let state = RemoteSessionState::new("10.0.0.5", "ubuntu", 22);
        assert_eq!(state.ssh_url(), "ubuntu@10.0.0.5");
    }

    #[test]
    fn ssh_url_custom_port() {
        let state = RemoteSessionState::new("ml.company.com", "user", 2222);
        assert_eq!(state.ssh_url(), "user@ml.company.com:2222");
    }

    #[test]
    fn cost_and_uptime_display() {
        let mut state = RemoteSessionState::new("host", "user", 22);
        state.cumulative_cost = 2.82;
        state.total_session_seconds = 5640;
        assert_eq!(state.cost_display(), "$2.82");
        assert_eq!(state.uptime_display(), "1h 34m");
    }

    #[test]
    fn add_terminal_and_process() {
        let mut state = RemoteSessionState::new("host", "user", 22);
        state.add_terminal("/home/user/project", Some("python train.py".into()));
        state.add_process("python", "python train.py --epochs 100");
        assert_eq!(state.terminal_sessions.len(), 1);
        assert_eq!(state.running_processes.len(), 1);
        assert_eq!(state.running_processes[0].command, "python train.py --epochs 100");
    }
}
