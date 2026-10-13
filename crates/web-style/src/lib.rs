//! web-style —— 样式系统与 CSSOM 适配（G2 内联声明值；G3 增样式表登记 + 计算值投影）。
//!
//! 状态：**G3 S5**。新增**样式表文本登记**（[`sheet`]：`Stylesheet` / `StylesheetList`）与
//! **计算值投影**（[`computed`]：`ComputedStyle`）；G2 的**内联声明值**（[`inline`]）保留。
//! **不做级联 / 选择器匹配**（级联在 `layout-backend` 的 stylo 侧，S6/S7）。本 crate **零 unsafe**。

pub mod computed;
pub mod error;
pub mod inline;
pub mod sheet;

// 类型面再导出（施工图 §1.2 以 `web_style::{InlineStyle, SheetOrigin, Stylesheet, ComputedStyle}` 引用）。
pub use computed::ComputedStyle;
pub use error::{StyleError, StyleErrorKind};
pub use inline::{DeclMap, InlineStyle};
pub use sheet::{SheetId, SheetOrigin, Stylesheet, StylesheetList};
