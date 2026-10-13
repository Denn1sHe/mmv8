//! web-events 错误类型（`web-events::error`）。

use std::fmt;

/// 事件层错误类别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EventErrorKind {
    /// 未知/不支持的事件类型。
    UnknownType,
    /// 监听器条目非法（如目标不属于当前文档）。
    InvalidListener,
    /// 内部不变量被破坏。
    Internal,
}

/// 事件层错误（`kind` + 人类可读消息）。
#[derive(Clone, Debug)]
pub struct EventError {
    kind: EventErrorKind,
    message: String,
}

impl EventError {
    /// 构造错误。
    pub fn new(kind: EventErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    /// 错误类别。
    #[must_use]
    pub fn kind(&self) -> EventErrorKind {
        self.kind
    }

    /// 错误消息。
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for EventError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "event error ({:?}): {}", self.kind, self.message)
    }
}

impl std::error::Error for EventError {}
