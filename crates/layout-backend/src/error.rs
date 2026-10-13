//! 布局后端错误（`layout-backend::error`，施工图 §1.2 + G3 类图）。
//!
//! `LayoutError`（`kind` + 消息）与 `LayoutErrorKind` 供门面 [`crate::backend::LayoutBackend`] 使用。
//! 每个 `pub` 门面方法的错误路径由其对应 kind 表达（施工图 §4「每方法 ≥1 错误路径」）。

use std::fmt;

/// 布局错误类别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LayoutErrorKind {
    /// 样式表登记 / 移除失败（未知句柄、解析失败）。
    StyleSheet,
    /// 级联 / 计算值失败。
    Cascade,
    /// stylo 侧错误（投影 / 遍历）。
    Stylo,
    /// taffy 侧错误（建树 / 布局）。
    Taffy,
    /// 字体 / 整形失败。
    Font,
    /// 节点不属于当前树或已失效（generation 不匹配）。
    UnknownNode,
    /// 尚未布局（未 `resolve`，或该节点不可渲染）。
    NotLaidOut,
    /// 属性不在 `computed_value` 白名单内（G3-B12③）。
    UnsupportedProperty,
    /// 内部不变量被破坏。
    Internal,
}

/// 布局错误（`kind` + 人类可读消息）。
#[derive(Clone, Debug)]
pub struct LayoutError {
    kind: LayoutErrorKind,
    message: String,
}

impl LayoutError {
    /// 构造错误。
    pub fn new(kind: LayoutErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    /// 节点未知 / 失效。
    pub fn unknown_node(message: impl Into<String>) -> Self {
        Self::new(LayoutErrorKind::UnknownNode, message)
    }

    /// 尚未布局。
    pub fn not_laid_out(message: impl Into<String>) -> Self {
        Self::new(LayoutErrorKind::NotLaidOut, message)
    }

    /// 属性不在白名单（G3-B12③）。
    pub fn unsupported_property(message: impl Into<String>) -> Self {
        Self::new(LayoutErrorKind::UnsupportedProperty, message)
    }

    /// 错误类别。
    #[must_use]
    pub fn kind(&self) -> LayoutErrorKind {
        self.kind
    }

    /// 错误消息。
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for LayoutError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "layout error ({:?}): {}", self.kind, self.message)
    }
}

impl std::error::Error for LayoutError {}

#[cfg(test)]
mod tests {
    use super::{LayoutError, LayoutErrorKind};

    #[test]
    fn layout_error_kinds_and_helpers() {
        let err = LayoutError::not_laid_out("no resolve yet");
        assert_eq!(err.kind(), LayoutErrorKind::NotLaidOut);
        assert_eq!(err.message(), "no resolve yet");
        assert!(err.to_string().contains("NotLaidOut"));
        assert_eq!(
            LayoutError::unknown_node("x").kind(),
            LayoutErrorKind::UnknownNode
        );
        assert_eq!(
            LayoutError::unsupported_property("z-index").kind(),
            LayoutErrorKind::UnsupportedProperty
        );
    }
}
