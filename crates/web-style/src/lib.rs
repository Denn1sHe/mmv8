//! web-style —— 样式系统与 CSSOM 适配（G2 仅内联声明值）。
//!
//! 状态：**G2 最小面**。仅解析/保存**内联**声明值（`style="..."` 属性与 `el.style`），
//! **不做级联**、无选择器匹配、无 `getComputedStyle` 计算值——完整样式系统（Stylo 直连）
//! 属 G3（见 ADR-0011）。本 crate **零 unsafe**。

pub mod error;
pub mod inline;

// 类型面再导出（施工图 §1.2 以 `web_style::InlineStyle` 引用）。
pub use error::{StyleError, StyleErrorKind};
pub use inline::{DeclMap, InlineStyle};
