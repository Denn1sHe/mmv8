//! `computed_value` 序列化（`layout-backend::serialize`，施工图 §1.1 / §3.4 G3-B12③）。
//!
//! **属性白名单**：白名单内属性从 stylo `ComputedValues` 取计算值并序列化为 CSS 串；
//! **白名单外 ⇒ `Err(LayoutErrorKind::UnsupportedProperty)`**（S4 已落变体）。
//!
//! **零 unsafe**（落 `layout-backend/src/`，非桥目录）。

use style::properties::ComputedValues;
use style_traits::ToCss;

use crate::error::LayoutError;

/// 白名单：`computed_value` 支持的属性（G3 最小面；`getComputedStyle` 语义）。
///
/// 白名单外属性一律 [`crate::error::LayoutErrorKind::UnsupportedProperty`]（§3.4 G3-B12③）。
pub const PROPERTY_WHITELIST: &[&str] = &["display", "width", "height", "color", "font-size"];

/// 属性是否在白名单内（大小写不敏感；`-` 折叠不含）。
#[must_use]
pub fn is_whitelisted(property: &str) -> bool {
    let name = property.trim().to_ascii_lowercase();
    PROPERTY_WHITELIST.contains(&name.as_str())
}

/// 取一个属性的**计算值**并序列化为 CSS 串（`getComputedStyle(...).getPropertyValue(prop)` 语义）。
///
/// # Errors
/// 属性不在白名单 ⇒ [`crate::error::LayoutErrorKind::UnsupportedProperty`]。
pub fn serialize_computed(
    computed: &ComputedValues,
    property: &str,
) -> Result<String, LayoutError> {
    let name = property.trim().to_ascii_lowercase();
    match name.as_str() {
        "display" => Ok(computed.clone_display().to_css_string()),
        "width" => Ok(computed.get_position().width.to_css_string()),
        "height" => Ok(computed.get_position().height.to_css_string()),
        "color" => Ok(computed.clone_color().to_css_string()),
        "font-size" => Ok(computed.clone_font_size().to_css_string()),
        _ => Err(LayoutError::unsupported_property(property)),
    }
}

/// 序列化一条**内联**声明值（供 `element.style` / `getPropertyValue` 回读；白名单同 [`serialize_computed`]）。
///
/// # Errors
/// 属性不在白名单 ⇒ [`crate::error::LayoutErrorKind::UnsupportedProperty`]。
pub fn serialize_inline(
    inline: &web_style::InlineStyle,
    property: &str,
) -> Result<String, LayoutError> {
    if !is_whitelisted(property) {
        return Err(LayoutError::unsupported_property(property));
    }
    Ok(inline.get_property(property).unwrap_or("").to_owned())
}

#[cfg(test)]
mod tests {
    use super::{PROPERTY_WHITELIST, is_whitelisted, serialize_computed};
    use crate::error::LayoutErrorKind;
    use style::properties::ComputedValues;
    use style::properties::style_structs::Font;

    fn initial() -> style::servo_arc::Arc<ComputedValues> {
        ComputedValues::initial_values_with_font_override(Font::initial_values())
    }

    #[test]
    fn whitelist_gate_rejects_unknown_property() {
        assert!(is_whitelisted("Display"));
        assert!(is_whitelisted("font-size"));
        assert!(!is_whitelisted("z-index"));
        let computed = initial();
        let error = serialize_computed(&computed, "z-index").expect_err("z-index 不在白名单");
        assert_eq!(error.kind(), LayoutErrorKind::UnsupportedProperty);
    }

    #[test]
    fn serializes_whitelisted_initial_values() {
        let computed = initial();
        // 初始 display = inline；初始 width = auto（CSS 初始化值）。
        assert_eq!(
            serialize_computed(&computed, "display").expect("display"),
            "inline"
        );
        let width = serialize_computed(&computed, "width").expect("width");
        assert_eq!(width, "auto", "计算值 width 初始应为 auto");
        assert_eq!(PROPERTY_WHITELIST.len(), 5);
    }
}
