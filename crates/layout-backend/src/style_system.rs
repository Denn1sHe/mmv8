//! 样式系统（`layout-backend::style_system`，施工图 §2.1 / S7）。
//!
//! **S7-b② 最小可编译面**：持有 stylo [`Stylist`]（级联引擎）+ `Device` + 共享锁；
//! 提供样式表**登记 / 移除**（`web-style::StylesheetList` 记账 + stylo `append_stylesheet`）+ `flush`。
//! **逐元素级联**（步骤③）与 `computed_value` 序列化（`serialize.rs`）在后续落。
//!
//! **零 unsafe**（落 `layout-backend/src/`，非桥目录）。

use std::collections::HashMap;

use euclid::{Scale, Size2D};
use style::context::QuirksMode;
use style::device::Device;
use style::media_queries::{MediaList, MediaType};
use style::properties::ComputedValues;
use style::properties::style_structs::Font;
use style::queries::values::PrefersColorScheme;
use style::servo::media_features::PointerCapabilities;
use style::servo_arc::Arc;
use style::shared_lock::{SharedRwLock, StylesheetGuards};
use style::stylesheets::{AllowImportRules, DocumentStyleSheet, Origin, Stylesheet, UrlExtraData};
use style::stylist::Stylist;
use style_traits::{CSSPixel, DevicePixel};
use web_style::SheetOrigin;

use crate::error::{LayoutError, LayoutErrorKind};
use crate::font_metrics::FontChain;
use crate::geometry::Size;

/// 把 `web-style::SheetOrigin` 映射到 stylo `Origin`（UA < author）。
fn to_stylo_origin(origin: SheetOrigin) -> Origin {
    match origin {
        SheetOrigin::UserAgent => Origin::UserAgent,
        SheetOrigin::Author => Origin::Author,
    }
}

/// 构造 `UrlExtraData`（`about:blank`；G3 无资源加载）。
fn base_url_data() -> UrlExtraData {
    UrlExtraData::from(url::Url::parse("about:blank").expect("about:blank is a valid url"))
}

/// 样式系统：stylo 级联引擎 + 样式表登记/移除 + flush。
pub struct StyleSystem {
    stylist: Stylist,
    lock: SharedRwLock,
    /// web-style 侧文本登记（去重 + 句柄）。
    sheets: web_style::StylesheetList,
    /// web-style `SheetId` → stylo 表（供移除/去重判重）。
    registered: HashMap<u64, DocumentStyleSheet>,
}

// 只展示摘要（避免暴露锁/内部表；`lock`/`registered` 无稳定 Debug 面）。
#[allow(clippy::missing_fields_in_debug)]
impl std::fmt::Debug for StyleSystem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StyleSystem")
            .field("sheets", &self.sheets.len())
            .finish()
    }
}

impl StyleSystem {
    /// 建样式系统：`Device`（注入 [`FontChain`] 度量提供者）+ `Stylist`。
    #[must_use]
    pub fn new(viewport: Size) -> Self {
        let lock = SharedRwLock::new_leaked();
        let default_values: Arc<ComputedValues> =
            ComputedValues::initial_values_with_font_override(Font::initial_values());
        let device = Device::new(
            MediaType::screen(),
            QuirksMode::NoQuirks,
            Size2D::<f32, CSSPixel>::new(viewport.width, viewport.height),
            Size2D::<f32, DevicePixel>::new(viewport.width, viewport.height),
            Scale::<f32, CSSPixel, DevicePixel>::new(1.0),
            Box::new(FontChain::new()),
            default_values,
            PrefersColorScheme::Light,
            PointerCapabilities::empty(),
            PointerCapabilities::empty(),
        );
        Self {
            stylist: Stylist::new(device, QuirksMode::NoQuirks),
            lock,
            sheets: web_style::StylesheetList::new(),
            registered: HashMap::new(),
        }
    }

    /// 登记一张样式表（`(origin, text)` 去重）；返回 `web-style` 句柄。
    ///
    /// # Errors
    /// CSS 解析 / 登记失败 ⇒ [`LayoutErrorKind::StyleSheet`]。
    pub fn add_stylesheet(
        &mut self,
        origin: SheetOrigin,
        css: &str,
    ) -> Result<web_style::SheetId, LayoutError> {
        let id = self.sheets.insert(origin, css);
        if self.registered.contains_key(&id.raw()) {
            return Ok(id);
        }
        let media = Arc::new(self.lock.wrap(MediaList::empty()));
        let sheet = Stylesheet::from_str(
            css,
            base_url_data(),
            to_stylo_origin(origin),
            media,
            self.lock.clone(),
            None,
            None,
            QuirksMode::NoQuirks,
            AllowImportRules::Yes,
        );
        let document_sheet = DocumentStyleSheet(Arc::new(sheet));
        {
            let guard = self.lock.read();
            self.stylist
                .append_stylesheet(document_sheet.clone(), &guard);
        }
        self.registered.insert(id.raw(), document_sheet);
        Ok(id)
    }

    /// 移除一张样式表记账（对称面；G3-B11）。
    ///
    /// # Errors
    /// 未知句柄 ⇒ [`LayoutErrorKind::StyleSheet`]。
    pub fn remove_stylesheet(&mut self, id: web_style::SheetId) -> Result<(), LayoutError> {
        if self.registered.remove(&id.raw()).is_none() {
            return Err(LayoutError::new(
                LayoutErrorKind::StyleSheet,
                "unknown stylesheet handle",
            ));
        }
        self.sheets.remove(id);
        // stylo sheet-set 移除 + flush 配对留待步骤③（`Stylist` 的 sheet 变更 API）。
        Ok(())
    }

    /// 样式表数（记账侧）。
    #[must_use]
    pub fn stylesheet_count(&self) -> usize {
        self.sheets.len()
    }

    /// 刷新级联缓存（重建规则树）。
    pub fn flush(&mut self) {
        let guard = self.lock.read();
        self.stylist.flush(&StylesheetGuards::same(&guard));
    }

    /// stylo 级联引擎（只读）。
    #[must_use]
    pub const fn stylist(&self) -> &Stylist {
        &self.stylist
    }
}

#[cfg(test)]
mod tests {
    use super::StyleSystem;
    use crate::geometry::Size;
    use web_style::SheetOrigin;

    #[test]
    fn style_system_registers_and_dedups_sheets() {
        let mut system = StyleSystem::new(Size::new(800.0, 600.0));
        let a = system
            .add_stylesheet(SheetOrigin::UserAgent, "div { display: block; }")
            .expect("first sheet");
        let b = system
            .add_stylesheet(SheetOrigin::UserAgent, "div { display: block; }")
            .expect("dup sheet");
        assert_eq!(a, b, "同 (origin, text) 应去重复用句柄");
        assert_eq!(system.stylesheet_count(), 1);
        system.flush();
        assert!(system.remove_stylesheet(a).is_ok());
        assert!(system.remove_stylesheet(a).is_err(), "重复移除应报错");
    }
}
