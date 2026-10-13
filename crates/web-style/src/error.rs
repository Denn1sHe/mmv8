//! web-style 错误类型（G2 最小面）。

use std::fmt;

/// 样式层错误类别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StyleErrorKind {
    /// 声明文本非法（缺少 `:` 等）。
    InvalidDeclaration,
    /// 内部不变量被破坏。
    Internal,
}

/// 样式层错误（`kind` + 人类可读消息）。
#[derive(Clone, Debug)]
pub struct StyleError {
    kind: StyleErrorKind,
    message: String,
}

impl StyleError {
    /// 构造错误。
    pub fn new(kind: StyleErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    /// 错误类别。
    #[must_use]
    pub fn kind(&self) -> StyleErrorKind {
        self.kind
    }

    /// 错误消息。
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for StyleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "style error ({:?}): {}", self.kind, self.message)
    }
}

impl std::error::Error for StyleError {}
