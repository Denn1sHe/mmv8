//! 样式表登记（`web-style::sheet`，施工图 §1.1/§1.2）。
//!
//! 状态：**S4 仅落 [`SheetOrigin`]**（`layout-backend` 门面 trait 需要它，才能编译门面）。
//! `Stylesheet` 文本登记与 `remove` 语义在 **S5** 补齐（施工图 §5 S5）。

/// 样式表来源（级联 origin；UA < author）。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum SheetOrigin {
    /// 用户代理（UA）内嵌样式表。
    UserAgent,
    /// 作者样式表（`<style>` / `<link>` / 导出的作者 CSS）。
    Author,
}
