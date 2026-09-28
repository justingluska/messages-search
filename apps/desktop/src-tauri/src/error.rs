//! The one error shape every command returns: `{ kind, message }`.
//! `kind` lets the UI branch (setup screen, "not found", bad query) without
//! parsing text. See docs/ARCHITECTURE.md → Command contract.

use serde::Serialize;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ErrorKind {
    /// Full Disk Access is missing (chat.db or Attachments unreadable).
    Permission,
    NotFound,
    Invalid,
    Internal,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CmdError {
    pub kind: ErrorKind,
    pub message: String,
}

pub type CmdResult<T> = Result<T, CmdError>;

impl CmdError {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        CmdError {
            kind,
            message: message.into(),
        }
    }
    pub fn permission(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Permission, message)
    }
    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::NotFound, message)
    }
    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Invalid, message)
    }
    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Internal, message)
    }
}

impl std::fmt::Display for CmdError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}: {}", self.kind, self.message)
    }
}

impl std::error::Error for CmdError {}

impl From<tauri::Error> for CmdError {
    fn from(e: tauri::Error) -> Self {
        CmdError::internal(e.to_string())
    }
}

impl From<std::io::Error> for CmdError {
    fn from(e: std::io::Error) -> Self {
        if e.kind() == std::io::ErrorKind::PermissionDenied {
            CmdError::permission(e.to_string())
        } else {
            CmdError::internal(e.to_string())
        }
    }
}


impl From<ms_engine::EngineError> for CmdError {
    fn from(e: ms_engine::EngineError) -> Self {
        CmdError::internal(e.to_string())
    }
}

impl From<ms_core::Error> for CmdError {
    fn from(e: ms_core::Error) -> Self {
        CmdError::internal(e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_as_kind_and_message() {
        let json = serde_json::to_value(CmdError::not_found("chat 7")).unwrap();
        assert_eq!(json["kind"], "notFound");
        assert_eq!(json["message"], "chat 7");
        let json = serde_json::to_value(CmdError::permission("x")).unwrap();
        assert_eq!(json["kind"], "permission");
    }
}
