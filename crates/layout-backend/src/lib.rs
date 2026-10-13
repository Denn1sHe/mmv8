//! layout-backend —— 盒树、布局算法与几何/尺寸 API（**直连 Stylo + Taffy + Parley**，ADR-0020/0021）。
//!
//! 状态：**G3 S4 骨架（b）**（施工图 `docs/design-g3-layout-2026-10-13.md` §5 S4）。
//! 本步落**门面与类型**：门面 trait [`backend::LayoutBackend`]（**`: Debug`**，G3-B6）、
//! 几何类型 [`geometry`]、错误 [`error`]、内嵌 UA 表 [`UA_CSS`]（G3-B7）。
//!
//! **尚未落地**（后续步）：具体实现 `StyloBackend`（**S6**，依赖 R17 unsafe 白名单获批）、
//! 样式系统（S5/S7）、盒树（S8）、文本（S9）、shadow（S11）、几何接线（S12）。
//!
//! # 纪律
//! - **零 `unsafe`**（workspace lint `unsafe_code = "forbid"`）。
//! - 上层只依赖 [`LayoutBackend`]（接口不随后端变化，ADR-0011·C14）。

pub mod backend;
pub mod box_tree;
pub mod error;
pub mod font_metrics;
pub mod geometry;
pub mod invalidate;
pub mod opaque;
pub mod serialize;
pub mod style_system;
pub mod stylo_host;
pub mod text;

pub use backend::{DirtyState, LayoutBackend, StyleSheetHandle};
pub use error::{LayoutError, LayoutErrorKind};
pub use geometry::{NodeGeometry, Rect, Size};

/// 内嵌最小 UA 样式表（G3-B7；`include_str!` **编译期嵌入**，无运行期文件依赖）。
pub const UA_CSS: &str = include_str!("../assets/ua.css");

#[cfg(test)]
mod tests {
    use super::UA_CSS;

    /// 规范化空白（含换行）为单空格，便于结构断言。
    fn normalized(css: &str) -> String {
        css.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    /// §3.1 `ua_default_display_block`（**S4 结构断言**）：内嵌 UA 表含必需标签规则，
    /// 且 `div` 默认 `display: block`（G3-B7）。
    ///
    /// （**级联级**真值断言——`#box{width:120px}` ⇒ 计算值——在 **S7** 落；本步只证 UA 表就位。）
    #[test]
    fn ua_default_display_block() {
        let css = normalized(UA_CSS);
        for selector in ["html", "body", "div", "p", "h1", "ul", "li", "span"] {
            assert!(
                css.contains(&format!("{selector} {{")),
                "UA css 应含 `{selector}` 规则；实际 = {css}"
            );
        }
        assert!(
            css.contains("div { display: block; }"),
            "div 默认 display:block（G3-B7）；实际 = {css}"
        );
        assert!(
            css.contains("span { display: inline; }"),
            "span 默认 display:inline；实际 = {css}"
        );
    }
}
