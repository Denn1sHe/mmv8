//! web-dom 错误类型（`web-dom::error`）。

use std::fmt;

/// DOM 层错误类别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DomErrorKind {
    /// 节点 id 不属于本树或已失效（generation 不匹配 / 槽已回收）。
    InvalidNode,
    /// 操作要求元素节点，但目标不是元素。
    NotAnElement,
    /// 结构性借用冲突（`try_borrow` / `try_borrow_mut` 失败 —— ADR-0016·D2）。
    BorrowConflict,
    /// 内部不变量被破坏。
    Internal,
}

/// DOM 层错误（`kind` + 人类可读消息）。
#[derive(Clone, Debug)]
pub struct DomError {
    kind: DomErrorKind,
    message: String,
}

impl DomError {
    /// 构造错误。
    pub fn new(kind: DomErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    /// 错误类别。
    #[must_use]
    pub fn kind(&self) -> DomErrorKind {
        self.kind
    }

    /// 错误消息。
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for DomError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "dom error ({:?}): {}", self.kind, self.message)
    }
}

impl std::error::Error for DomError {}
