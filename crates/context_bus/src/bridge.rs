//! Context Bus Bridge — receives events from a remote Ribhu agent over the
//! SSH transport and publishes them to the local Context Bus.
//!
//! On the remote side, a lightweight daemon collects terminal events, file
//! saves, and GPU status, serializes them as JSON, and sends them over Zed's
//! existing SSH stdout channel.  On the local side, this bridge module
//! deserializes those messages and feeds them into the Context Bus so the
//! AI Conductor can react to remote events identically to local ones.

use crate::events::{ContextEvent, ContextSource};
use serde::{Deserialize, Serialize};

/// Wire protocol message from the remote Ribhu agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeMessage {
    #[serde(rename = "type")]
    pub message_type: String,
    pub payload: BridgePayload,
}

/// The payload carried by a bridge message.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event_kind")]
pub enum BridgePayload {
    FileSaved {
        path: Option<String>,
        language: Option<String>,
    },
    CmdExecuted {
        command: Option<String>,
        exit_code: Option<i32>,
        summary: String,
    },
    ErrorDetected {
        message: String,
        file_path: Option<String>,
        line: Option<u32>,
    },
    GpuIdle,
    DiskWarning {
        usage_percent: f32,
    },
    RamCritical {
        usage_percent: f32,
    },
}

impl BridgeMessage {
    /// Converts a bridge message into a `(ContextSource, ContextEvent)` pair
    /// ready to be published to the local Context Bus.
    pub fn into_context_event(self) -> Option<(ContextSource, ContextEvent)> {
        match self.payload {
            BridgePayload::FileSaved { path, language } => Some((
                ContextSource::RemoteEditor,
                ContextEvent::FileSaved {
                    path,
                    language,
                    lines_changed: None,
                    diagnostics_count: None,
                },
            )),
            BridgePayload::CmdExecuted {
                command,
                exit_code,
                summary,
            } => Some((
                ContextSource::RemoteTerminal,
                ContextEvent::CmdExecuted {
                    command,
                    exit_code,
                    summary,
                },
            )),
            BridgePayload::ErrorDetected {
                message,
                file_path,
                line,
            } => Some((
                ContextSource::RemoteTerminal,
                ContextEvent::ErrorDetected {
                    message,
                    file_path,
                    line,
                    column: None,
                    severity: crate::events::ErrorSeverity::Error,
                    command: None,
                    exit_code: None,
                },
            )),
            BridgePayload::GpuIdle => Some((
                ContextSource::RemoteMonitor,
                ContextEvent::ErrorDetected {
                    message: "GPU utilization dropped to 0% on remote machine".into(),
                    file_path: None,
                    line: None,
                    column: None,
                    severity: crate::events::ErrorSeverity::Info,
                    command: None,
                    exit_code: None,
                },
            )),
            BridgePayload::DiskWarning { usage_percent } => Some((
                ContextSource::RemoteMonitor,
                ContextEvent::ErrorDetected {
                    message: format!(
                        "Remote disk usage at {:.0}% — consider freeing space",
                        usage_percent
                    ),
                    file_path: None,
                    line: None,
                    column: None,
                    severity: crate::events::ErrorSeverity::Warning,
                    command: None,
                    exit_code: None,
                },
            )),
            BridgePayload::RamCritical { usage_percent } => Some((
                ContextSource::RemoteMonitor,
                ContextEvent::ErrorDetected {
                    message: format!(
                        "Remote RAM at {:.0}% — OOM risk",
                        usage_percent
                    ),
                    file_path: None,
                    line: None,
                    column: None,
                    severity: crate::events::ErrorSeverity::Error,
                    command: None,
                    exit_code: None,
                },
            )),
        }
    }
}

/// Attempt to parse a JSON line from the remote agent into a bridge message.
pub fn parse_bridge_message(json: &str) -> Option<BridgeMessage> {
    serde_json::from_str(json).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_file_saved_message() {
        let json = r#"{"type":"context_event","payload":{"event_kind":"FileSaved","path":"src/main.py","language":"python"}}"#;
        let message = parse_bridge_message(json).unwrap();
        assert_eq!(message.message_type, "context_event");
        let (source, event) = message.into_context_event().unwrap();
        assert_eq!(source, ContextSource::RemoteEditor);
        assert!(matches!(event, ContextEvent::FileSaved { .. }));
    }

    #[test]
    fn parse_cmd_executed_message() {
        let json = r#"{"type":"context_event","payload":{"event_kind":"CmdExecuted","command":"python train.py","exit_code":0,"summary":"Training complete"}}"#;
        let message = parse_bridge_message(json).unwrap();
        let (source, _event) = message.into_context_event().unwrap();
        assert_eq!(source, ContextSource::RemoteTerminal);
    }

    #[test]
    fn parse_gpu_idle_message() {
        let json = r#"{"type":"context_event","payload":{"event_kind":"GpuIdle"}}"#;
        let message = parse_bridge_message(json).unwrap();
        let (source, event) = message.into_context_event().unwrap();
        assert_eq!(source, ContextSource::RemoteMonitor);
        if let ContextEvent::ErrorDetected { message, severity, .. } = event {
            assert!(message.contains("GPU"));
            assert_eq!(severity, crate::events::ErrorSeverity::Info);
        } else {
            panic!("Expected ErrorDetected event");
        }
    }

    #[test]
    fn invalid_json_returns_none() {
        assert!(parse_bridge_message("not json").is_none());
    }
}
