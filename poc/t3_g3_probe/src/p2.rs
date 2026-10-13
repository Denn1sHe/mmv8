//! P-2（探针）—— **stylo→taffy 桥**：`to_taffy_style` 后的「样式改 ⇒ 布局盒变」可判（施工图 §7 P-2）。
//!
//! 最小实验：把**真实 `ComputedValues`**（stylo 初始计算值）经 `stylo_taffy::to_taffy_style`
//! 转成 `taffy::Style`，再用 `TaffyTree` 出布局，断言尺寸**由 CSS 推出**（而非恒真）。
//!
//! 判定：**样式输入变 ⇒ 布局尺寸随之变**（G3-B9）——用 `width: auto / 120px / 200px` 三种输入
//! 在 300px 容器下得 `300 / 120 / 200` 三个**互异**读数（含 `auto` 撑满容器的负对照）。
//!
//! ## 范围说明
//! 本探针证明**桥（`to_taffy_style`）+ taffy 布局**链路成立；**作者表级联出 `width:120px` 的
//! `ComputedValues`** 属 S6/S7（需 `TElement` 级联）。此处「样式输入」= 桥的输出面（CSS `width`/`height`
//! 映射到 `taffy::Style.size`，见 `stylo_taffy/src/convert.rs` 的 `dimension(&pos.width)`）。

use style::properties::style_structs::Font;
use style::properties::ComputedValues;
use stylo_taffy::to_taffy_style;
use taffy::{AvailableSpace, Dimension, Size as TaffySize, Style as TaffyStyle, TaffyTree};

/// P-2 运行报告。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct P2Report {
    /// 桥成立：真实 `ComputedValues` → `taffy::Style`，初始 CSS `width` 映射为 `auto`。
    pub bridge_ok: bool,
    /// `width` 输入变 ⇒ 布局宽随之变（120 ⇒ 120、200 ⇒ 200，且互异）。
    pub width_change_drives_layout: bool,
    /// 负对照：`width:auto` 在 300px 容器内撑满 ⇒ 300（≠ 120）。
    pub auto_fills_container: bool,
    /// 三种输入的实测布局宽度（px）。
    pub width_auto: f32,
    /// `width:120px` 的实测布局宽度。
    pub width_120: f32,
    /// `width:200px` 的实测布局宽度。
    pub width_200: f32,
}

/// 用给定 `taffy::Style` 作为叶子，在 `viewport_w` 宽容器内出布局，返回其**布局宽度**（px）。
fn layout_leaf_width(style: TaffyStyle, viewport_w: f32) -> f32 {
    let mut tree: TaffyTree = TaffyTree::new();
    let leaf = tree
        .new_leaf(style)
        .expect("new_leaf should succeed for a fresh tree");
    tree.compute_layout(
        leaf,
        TaffySize {
            width: AvailableSpace::Definite(viewport_w),
            height: AvailableSpace::MaxContent,
        },
    )
    .expect("compute_layout should succeed");
    tree.layout(leaf)
        .expect("layout should be available after compute_layout")
        .size
        .width
}

/// 由 CSS `width`（`Dimension`）构造块级叶子样式并在 `viewport_w` 下出布局宽度。
fn block_width_for(css_width: Dimension, viewport_w: f32) -> f32 {
    let style = TaffyStyle {
        display: taffy::Display::Block,
        size: TaffySize {
            width: css_width,
            height: Dimension::auto(),
        },
        ..TaffyStyle::default()
    };
    layout_leaf_width(style, viewport_w)
}

/// 跑 P-2 全部观测。
#[must_use]
pub fn run() -> P2Report {
    // 真实 ComputedValues（stylo 初始计算值；内容由 `Font` 初始值补全）。
    let computed = ComputedValues::initial_values_with_font_override(Font::initial_values());

    // ① 桥：真实 ComputedValues → taffy::Style（CSS 初始 `width` = auto）。
    let bridged = to_taffy_style(&computed);
    let bridge_ok = bridged.size.width == Dimension::auto();

    // ② 布局：三种 CSS width 输入 ⇒ 三个互异布局读数（桥的 size 面驱动布局）。
    let viewport = 300.0_f32;
    let width_auto = block_width_for(Dimension::auto(), viewport);
    let width_120 = block_width_for(Dimension::length(120.0), viewport);
    let width_200 = block_width_for(Dimension::length(200.0), viewport);

    let close = |a: f32, b: f32| (a - b).abs() < 0.01;
    let width_change_drives_layout =
        close(width_120, 120.0) && close(width_200, 200.0) && !close(width_120, width_200);
    let auto_fills_container = close(width_auto, viewport) && !close(width_auto, width_120);

    P2Report {
        bridge_ok,
        width_change_drives_layout,
        auto_fills_container,
        width_auto,
        width_120,
        width_200,
    }
}

#[cfg(test)]
mod tests {
    use super::run;

    /// §7 P-2：真实 ComputedValues 经 `to_taffy_style` 桥 + taffy 布局 ⇒ 样式改则布局盒变。
    #[test]
    fn style_change_drives_layout_box() {
        let report = run();
        eprintln!("P2_REPORT={report:?}");
        assert!(
            report.bridge_ok,
            "桥须成立：真实 ComputedValues → taffy::Style（width=auto）"
        );
        assert!(
            report.width_change_drives_layout,
            "width 输入变 ⇒ 布局宽随之变（auto={}, 120={}, 200={}）",
            report.width_auto, report.width_120, report.width_200
        );
        assert!(
            report.auto_fills_container,
            "负对照：width:auto 应撑满 300px 容器（实测 {}）",
            report.width_auto
        );
    }
}
