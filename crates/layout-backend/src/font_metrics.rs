//! 字体度量提供者（`layout-backend::font_metrics`，施工图 §1.4 **D7**）。
//!
//! 为 stylo [`Device`](style::device::Device) 提供 [`FontMetrics`]（`ex`/`ch`/`cap` 等 CSS 单位与
//! 文本度量所需）。**G3 最小面**：持有 `fontique` 集合（**系统发现 + 内嵌回退字体**，与 P-3/P-10 同源），
//! 度量按 `base_size` 的**确定性比例**合成；**真实字形度量**（skrifa/parley 逐字测量）接线留 **S9 `text.rs`**。
//!
//! - **零 unsafe**：本文件落 `layout-backend/src/`（**非** `stylo_host/` 桥目录）⇒ 受守卫规则 1 +
//!   门禁 b 管辖（「除桥内 5 签名外无 unsafe 文本」）。
//! - **确定性（D7/G3-B10）**：内嵌回退字体 + 比例度量 ⇒ 跨机 `f32` 稳定（容差 ≤0.5px 口径）。

use std::sync::Arc;

use fontique::{Blob, Collection, CollectionOptions};
use style::device::servo::FontMetricsProvider;
use style::font_metrics::FontMetrics;
use style::properties::style_structs::Font;
use style::values::computed::font::GenericFontFamily;
use style::values::computed::{CSSPixelLength, Length};
use style::values::specified::font::QueryFontMetricsFlags;

/// 内嵌回退字体（**public domain**，见 `assets/LICENSE-Tuffy.txt`；与 P-3/P-10 同源）。
pub const EMBEDDED_FALLBACK_FONT: &[u8] = include_bytes!("../assets/Tuffy.ttf");

/// 默认基准字号（`medium` = 16px；未命中 generic 时的回退）。
pub const DEFAULT_BASE_FONT_SIZE_PX: f32 = 16.0;

/// 字体链（`fontique` 集合）——同时作为 stylo 的 [`FontMetricsProvider`]。
///
/// 集合以 **`system_fonts: false`** 构建（确定性）+ 注册**内嵌回退字体**；真实页面可在附加
/// `load_system_fonts()` 后复用（S9 决定）。
pub struct FontChain {
    collection: Collection,
}

impl std::fmt::Debug for FontChain {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FontChain")
            .field("embedded_font_bytes", &EMBEDDED_FALLBACK_FONT.len())
            .finish()
    }
}

impl Default for FontChain {
    fn default() -> Self {
        Self::new()
    }
}

impl FontChain {
    /// 建字体链：注册内嵌回退字体（确定性口径 D7）。
    #[must_use]
    pub fn new() -> Self {
        let mut collection = Collection::new(CollectionOptions {
            shared: false,
            system_fonts: false,
        });
        collection.register_fonts(Blob::new(Arc::new(EMBEDDED_FALLBACK_FONT.to_vec())), None);
        Self { collection }
    }

    /// 字体集合（可变；`fontique` 查询需 `&mut`，供 S9 `text.rs` 整形使用）。
    pub fn collection(&mut self) -> &mut Collection {
        &mut self.collection
    }

    /// 家族是否已注册（探针/测试用）。
    pub fn has_family(&mut self, name: &str) -> bool {
        self.collection.family_id(name).is_some()
    }
}

/// 比例度量：由 `base_size` 推得（G3 最小面；真实字形度量留 S9）。
fn proportional_metrics(base_size: CSSPixelLength) -> FontMetrics {
    let size = base_size.px();
    FontMetrics {
        x_height: Some(Length::new(size * 0.5)),
        zero_advance_measure: Some(Length::new(size * 0.5)),
        cap_height: Some(Length::new(size * 0.7)),
        ic_width: Some(Length::new(size)),
        ascent: Length::new(size * 0.8),
        script_percent_scale_down: None,
        script_script_percent_scale_down: None,
    }
}

impl FontMetricsProvider for FontChain {
    fn query_font_metrics(
        &self,
        _vertical: bool,
        _font: &Font,
        base_size: CSSPixelLength,
        _flags: QueryFontMetricsFlags,
    ) -> FontMetrics {
        proportional_metrics(base_size)
    }

    fn base_size_for_generic(&self, _generic: GenericFontFamily) -> Length {
        Length::new(DEFAULT_BASE_FONT_SIZE_PX)
    }
}

#[cfg(test)]
mod tests {
    use super::{DEFAULT_BASE_FONT_SIZE_PX, FontChain};
    use style::device::servo::FontMetricsProvider;
    use style::properties::style_structs::Font;
    use style::values::computed::CSSPixelLength;
    use style::values::computed::font::GenericFontFamily;
    use style::values::specified::font::QueryFontMetricsFlags;

    #[test]
    fn font_chain_registers_embedded_font() {
        let mut chain = FontChain::new();
        assert!(
            chain.has_family("Tuffy"),
            "内嵌回退字体须注册（确定性口径 D7）"
        );
    }

    #[test]
    fn metrics_are_deterministic_and_proportional() {
        let chain = FontChain::new();
        let font = Font::initial_values();
        let metrics = chain.query_font_metrics(
            false,
            &font,
            CSSPixelLength::new(20.0),
            QueryFontMetricsFlags::empty(),
        );
        assert!(
            (metrics.ascent.px() - 16.0).abs() < f32::EPSILON,
            "ascent = 0.8 * base"
        );
        assert!((metrics.cap_height.unwrap().px() - 14.0).abs() < f32::EPSILON);
        let base = chain.base_size_for_generic(GenericFontFamily::None);
        assert!((base.px() - DEFAULT_BASE_FONT_SIZE_PX).abs() < f32::EPSILON);
    }
}
