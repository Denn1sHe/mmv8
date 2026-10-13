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
    /// 操作在当前状态下不受支持（对齐规范 `NotSupportedError`；如 `attach_shadow`
    /// 对已存在**不同 mode** 影子根的宿主 —— G3-B13）。
    NotSupported,
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

    /// 构造 `NotSupported` 错误（`attach_shadow` mode 冲突等）。
    pub fn not_supported(message: impl Into<String>) -> Self {
        Self::new(DomErrorKind::NotSupported, message)
    }

    /// 构造 `NotAnElement` 错误。
    pub fn not_an_element(message: impl Into<String>) -> Self {
        Self::new(DomErrorKind::NotAnElement, message)
    }

    /// 构造 `InvalidNode` 错误。
    pub fn invalid_node(message: impl Into<String>) -> Self {
        Self::new(DomErrorKind::InvalidNode, message)
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
