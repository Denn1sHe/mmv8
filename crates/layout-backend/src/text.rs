//! 文字整形（`layout-backend::text`，施工图 §1.4 D7 / §3.2 S8-b）。
//!
//! 栈：`parley`（整形）← `fontique`（发现/回退）← `skrifa`（字形）。
//! **确定性口径（P-3/P-10）**：内嵌回退字体（`assets/Tuffy.ttf`，public domain）+ **只装内嵌字体** 的
//! `FontContext` ⇒ 同架构**逐位相等**、跨架构 **≤0.5px**（G3-B10）。
//!
//! **零 unsafe**（落 `layout-backend/src/`，非桥目录）。

use std::borrow::Cow;
use std::sync::Arc;

use fontique::{Blob, Collection, CollectionOptions, SourceCache};
use parley::{Alignment, AlignmentOptions, FontContext, FontFamily, LayoutContext, StyleProperty};

/// 内嵌回退字体字节（编译期嵌入 ⇒ 跨机**同字节**）。
pub const EMBEDDED_FONT: &[u8] = include_bytes!("../assets/Tuffy.ttf");

/// 内嵌字体家族名（字体内部 family name）。
pub const EMBEDDED_FAMILY: &str = "Tuffy";

/// 整形样本文本（纯 ASCII，内嵌字体覆盖）。
pub const SAMPLE_TEXT: &str = "Hamburgefonstiv";

/// 整形样本字号（px）。
pub const SAMPLE_SIZE: f32 = 16.0;

/// 跨机容差（px；G3-B10：跨架构 ≤0.5px）。
pub const CROSS_ARCH_TOLERANCE_PX: f32 = 0.5;

/// **本机钉死值**（px；S1/P-10 实测：Tuffy `Hamburgefonstiv`@16px，scale=1.0）。
pub const PINNED_EMBEDDED_ADVANCE_PX: f32 = 112.625;

/// `LayoutContext` 画笔占位（parley 对 `()` 有 `Brush` blanket impl）。
type NoBrush = ();

/// 文字整形器：持**只含内嵌字体**的确定性 `FontContext`（不加载系统字体）。
pub struct TextShaper {
    font_context: FontContext,
}

impl std::fmt::Debug for TextShaper {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TextShaper")
            .field("embedded_family", &EMBEDDED_FAMILY)
            .finish_non_exhaustive()
    }
}

impl Default for TextShaper {
    fn default() -> Self {
        Self::new()
    }
}

impl TextShaper {
    /// 建整形器（注册内嵌回退字体；**不加载系统字体** ⇒ 确定性）。
    #[must_use]
    pub fn new() -> Self {
        let mut collection = Collection::new(CollectionOptions {
            shared: false,
            system_fonts: false,
        });
        collection.register_fonts(Blob::new(Arc::new(EMBEDDED_FONT.to_vec())), None);
        Self {
            font_context: FontContext {
                collection,
                source_cache: SourceCache::default(),
            },
        }
    }

    /// 内嵌家族是否已注册。
    pub fn has_embedded_family(&mut self) -> bool {
        self.font_context
            .collection
            .family_id(EMBEDDED_FAMILY)
            .is_some()
    }

    /// 用给定家族整形 `text` 并返回**布局宽度**（px）。
    ///
    /// **D7 回退**：请求家族未注册 ⇒ 落到**内嵌回退字体**（`EMBEDDED_FAMILY`）。
    pub fn shape_width(&mut self, text: &str, family: &str, size: f32) -> f32 {
        let family = if self.font_context.collection.family_id(family).is_some() {
            family
        } else {
            EMBEDDED_FAMILY
        };
        let mut layout_context: LayoutContext<NoBrush> = LayoutContext::new();
        let mut builder = layout_context.ranged_builder(&mut self.font_context, text, 1.0, false);
        builder.push_default(StyleProperty::FontFamily(FontFamily::Source(
            Cow::Borrowed(family),
        )));
        builder.push_default(StyleProperty::FontSize(size));
        let mut layout = builder.build(text);
        layout.break_all_lines(None);
        layout.align(Alignment::Start, AlignmentOptions::default());
        layout.width()
    }

    /// 内嵌字体的整形宽度（`SAMPLE_TEXT`@`SAMPLE_SIZE`；P-10 钉值口径）。
    pub fn embedded_sample_width(&mut self) -> f32 {
        self.shape_width(SAMPLE_TEXT, EMBEDDED_FAMILY, SAMPLE_SIZE)
    }

    /// **逐字形 advance 之和**（`text_shaping_width_matches_advance` 用；内嵌字体逐字校验）。
    pub fn glyph_advance_sum(&mut self, text: &str, family: &str, size: f32) -> f32 {
        let mut layout_context: LayoutContext<NoBrush> = LayoutContext::new();
        let mut builder = layout_context.ranged_builder(&mut self.font_context, text, 1.0, false);
        builder.push_default(StyleProperty::FontFamily(FontFamily::Source(
            Cow::Borrowed(family),
        )));
        builder.push_default(StyleProperty::FontSize(size));
        let mut layout = builder.build(text);
        layout.break_all_lines(None);
        let mut sum = 0.0_f32;
        for line in layout.lines() {
            for item in line.items() {
                if let parley::layout::PositionedLayoutItem::GlyphRun(glyph_run) = item {
                    for glyph in glyph_run.glyphs() {
                        sum += glyph.advance;
                    }
                }
            }
        }
        sum
    }
}

#[cfg(test)]
mod tests {
    use super::{CROSS_ARCH_TOLERANCE_PX, PINNED_EMBEDDED_ADVANCE_PX, TextShaper};

    #[test]
    fn embedded_font_registers_and_shapes() {
        let mut shaper = TextShaper::new();
        assert!(shaper.has_embedded_family(), "内嵌家族须注册");
        assert!(shaper.embedded_sample_width() > 0.0, "整形宽度必须 > 0");
    }

    #[test]
    fn embedded_font_is_used() {
        // §3.6：内嵌字体 ⇒ 文本度量 == 钉死值（P-10 回填）。
        let mut shaper = TextShaper::new();
        let width = shaper.embedded_sample_width();
        assert!(
            (width - PINNED_EMBEDDED_ADVANCE_PX).abs() <= CROSS_ARCH_TOLERANCE_PX,
            "内嵌字体度量应命中钉死值 {PINNED_EMBEDDED_ADVANCE_PX}：实测 {width}"
        );
    }

    #[test]
    fn font_fallback_metrics_nonzero() {
        // §3.6：不存在家族 ⇒ 回退，且度量 > 0（唯一保留 `>0` 的负对照类）。
        let mut shaper = TextShaper::new();
        let width = shaper.shape_width("Hamburgefonstiv", "__iv8_missing_family__", 16.0);
        assert!(width > 0.0, "缺失家族应回退且度量 > 0：实测 {width}");
    }

    #[test]
    fn text_shaping_width_matches_advance() {
        // §3.6：整形宽度 == 字形 advance 之和（内嵌字体逐字校验）。
        let mut shaper = TextShaper::new();
        let width = shaper.embedded_sample_width();
        let advance_sum = shaper.glyph_advance_sum("Hamburgefonstiv", "Tuffy", 16.0);
        assert!(
            (width - advance_sum).abs() < 0.01,
            "整形宽度应 == advance 之和：width={width} sum={advance_sum}"
        );
    }

    #[test]
    fn embedded_metric_is_pinned() {
        let mut shaper = TextShaper::new();
        let width = shaper.embedded_sample_width();
        // 同架构逐位；跨架构 ≤0.5px（G3-B10）。
        let same_arch = cfg!(target_arch = "aarch64");
        if same_arch {
            assert!(
                (width - PINNED_EMBEDDED_ADVANCE_PX).abs() < f32::EPSILON,
                "同架构内嵌度量应逐位相等：实测 {width}"
            );
        } else {
            assert!(
                (width - PINNED_EMBEDDED_ADVANCE_PX).abs() <= CROSS_ARCH_TOLERANCE_PX,
                "跨架构容差 ≤0.5px：实测 {width}"
            );
        }
    }
}
