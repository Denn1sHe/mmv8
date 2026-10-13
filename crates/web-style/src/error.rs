//! web-style 错误类型（G2 最小面；G3 S5 增样式表/计算值错误）。

use std::fmt;

/// 样式层错误类别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StyleErrorKind {
    /// 声明文本非法（缺少 `:` 等）。
    InvalidDeclaration,
    /// 样式表句柄未知（移除 / 查询不存在者）。
    UnknownSheet,
    /// 属性不在 `computed_value` 白名单内（G3-B12③；与 `layout-backend::LayoutErrorKind` 对应）。
    UnsupportedProperty,
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

    /// 未知样式表句柄。
    pub fn unknown_sheet(message: impl Into<String>) -> Self {
        Self::new(StyleErrorKind::UnknownSheet, message)
    }

    /// 属性不在白名单（G3-B12③）。
    pub fn unsupported_property(message: impl Into<String>) -> Self {
        Self::new(StyleErrorKind::UnsupportedProperty, message)
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

#[cfg(test)]
mod tests {
    use super::{StyleError, StyleErrorKind};

    #[test]
    fn style_error_new_kinds() {
        assert_eq!(
            StyleError::unknown_sheet("no such sheet").kind(),
            StyleErrorKind::UnknownSheet
        );
        assert_eq!(
            StyleError::unsupported_property("z-index").kind(),
            StyleErrorKind::UnsupportedProperty
        );
        assert!(
            StyleError::new(StyleErrorKind::Internal, "x")
                .to_string()
                .contains("Internal")
        );
    }
}
