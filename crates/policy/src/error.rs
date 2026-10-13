//! policy 错误类型（`policy::error`）。

use std::fmt;

/// 策略层错误类别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PolicyErrorKind {
    /// URL 非法或为不透明 origin。
    InvalidUrl,
    /// 内部不变量被破坏。
    Internal,
}

/// 策略层错误（`kind` + 人类可读消息）。
#[derive(Clone, Debug)]
pub struct PolicyError {
    kind: PolicyErrorKind,
    message: String,
}

impl PolicyError {
    /// 构造错误。
    pub fn new(kind: PolicyErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    /// 错误类别。
    #[must_use]
    pub fn kind(&self) -> PolicyErrorKind {
        self.kind
    }

    /// 错误消息。
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for PolicyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "policy error ({:?}): {}", self.kind, self.message)
    }
}

impl std::error::Error for PolicyError {}
