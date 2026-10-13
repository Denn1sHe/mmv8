//! layout-backend —— 盒树、布局算法与几何/尺寸 API（**直连 Stylo + Taffy + Parley**，ADR-0020/0021）。
//!
//! 状态：**G3 S4 骨架（a）**：几何类型 [`geometry`] + 错误 [`error`]。
//! 门面 trait `LayoutBackend`、内嵌 UA 表 `UA_CSS` 见 **S4（b）**（施工图 `docs/design-g3-layout-2026-10-13.md` §5 S4）。
//!
//! # 纪律
//! - **零 `unsafe`**（workspace lint `unsafe_code = "forbid"`）。

pub mod error;
pub mod geometry;

pub use error::{LayoutError, LayoutErrorKind};
pub use geometry::{NodeGeometry, Rect, Size};
