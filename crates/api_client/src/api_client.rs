//! API Client core types and event log for the Ribhu API client panel.
//!
//! This crate tracks API request/response events (sourced from the Context Bus)
//! and provides the data model for the `api_client_panel` crate.

use serde::{Deserialize, Serialize};

/// HTTP method enum covering the most common REST methods.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Patch,
    Delete,
    Head,
    Options,
}

impl HttpMethod {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
            Self::Put => "PUT",
            Self::Patch => "PATCH",
            Self::Delete => "DELETE",
            Self::Head => "HEAD",
            Self::Options => "OPTIONS",
        }
    }

    pub fn from_str(method: &str) -> Self {
        match method.to_uppercase().as_str() {
            "GET" => Self::Get,
            "POST" => Self::Post,
            "PUT" => Self::Put,
            "PATCH" => Self::Patch,
            "DELETE" => Self::Delete,
            "HEAD" => Self::Head,
            "OPTIONS" => Self::Options,
            other => {
                log::warn!("Unknown HTTP method '{}', defaulting to GET", other);
                Self::Get
            }
        }
    }
}

impl std::fmt::Display for HttpMethod {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// A single captured API request/response pair.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiLogEntry {
    pub method: HttpMethod,
    pub url: String,
    /// HTTP status code of the response (None if no response received yet).
    pub status: Option<u16>,
    /// Round-trip duration in milliseconds (None if no response yet).
    pub duration_ms: Option<u64>,
}

impl ApiLogEntry {
    pub fn request(method: impl Into<String>, url: impl Into<String>) -> Self {
        Self {
            method: HttpMethod::from_str(&method.into()),
            url: url.into(),
            status: None,
            duration_ms: None,
        }
    }

    pub fn status_class(&self) -> Option<StatusClass> {
        self.status.map(StatusClass::from_code)
    }
}

/// Broad classification of an HTTP status code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusClass {
    Success,
    Redirect,
    ClientError,
    ServerError,
    Unknown,
}

impl StatusClass {
    pub fn from_code(code: u16) -> Self {
        match code {
            200..=299 => Self::Success,
            300..=399 => Self::Redirect,
            400..=499 => Self::ClientError,
            500..=599 => Self::ServerError,
            _ => Self::Unknown,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn http_method_round_trip() {
        assert_eq!(HttpMethod::from_str("GET").as_str(), "GET");
        assert_eq!(HttpMethod::from_str("post").as_str(), "POST");
        assert_eq!(HttpMethod::from_str("delete").as_str(), "DELETE");
    }

    #[test]
    fn status_class_categorizes_correctly() {
        assert_eq!(StatusClass::from_code(200), StatusClass::Success);
        assert_eq!(StatusClass::from_code(404), StatusClass::ClientError);
        assert_eq!(StatusClass::from_code(503), StatusClass::ServerError);
        assert_eq!(StatusClass::from_code(302), StatusClass::Redirect);
    }

    #[test]
    fn log_entry_pending_has_no_status() {
        let entry = ApiLogEntry::request("GET", "https://api.example.com/users");
        assert!(entry.status.is_none());
        assert!(entry.duration_ms.is_none());
        assert_eq!(entry.method, HttpMethod::Get);
    }
}
