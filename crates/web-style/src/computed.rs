//! 计算值投影（属性名 → 值字符串）（`web-style::computed`，施工图 §1.1/§1.2；G3 S5）。
//!
//! [`ComputedStyle`] 是**计算值的最小容器**：由 `layout-backend`（stylo，S6/S7）填值，
//! 供 `getComputedStyle(el).getPropertyValue(prop)` 取用。属性名按 CSS 规则**小写折叠**。
//!
//! 本 crate **不做级联**（级联在 stylo 侧）；此处只承载**结果值**。

/// 计算值表：`(属性名, 值字符串)`，属性名已小写折叠、**保序**。重复 `set` 覆盖值并保留原位置。
#[derive(Clone, Debug, Default)]
pub struct ComputedStyle {
    entries: Vec<(String, String)>,
}

impl ComputedStyle {
    /// 建空表。
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// 设一条计算值（`prop` 折叠为 ASCII 小写；已存在则覆盖值、保留原顺序）。
    pub fn set(&mut self, prop: &str, value: &str) {
        let key = prop.trim().to_ascii_lowercase();
        if key.is_empty() {
            return;
        }
        if let Some(entry) = self.entries.iter_mut().find(|(k, _)| k.as_str() == key) {
            value.clone_into(&mut entry.1);
        } else {
            self.entries.push((key, value.to_owned()));
        }
    }

    /// 取一条计算值（属性名大小写不敏感）。
    #[must_use]
    pub fn get(&self, prop: &str) -> Option<&str> {
        let key = prop.trim().to_ascii_lowercase();
        self.entries
            .iter()
            .find(|(k, _)| k.as_str() == key)
            .map(|(_, v)| v.as_str())
    }

    /// 删除一条计算值并返回旧值。
    pub fn remove(&mut self, prop: &str) -> Option<String> {
        let key = prop.trim().to_ascii_lowercase();
        let index = self.entries.iter().position(|(k, _)| k.as_str() == key)?;
        Some(self.entries.remove(index).1)
    }

    /// 逐条遍历（保序）。
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.entries.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }

    /// 序列化为 `prop: value; prop2: value2`（空表 ⇒ 空串；无尾随 `;`）。
    #[must_use]
    pub fn to_css_text(&self) -> String {
        let mut out = String::new();
        for (prop, value) in self.iter() {
            if !out.is_empty() {
                out.push_str("; ");
            }
            out.push_str(prop);
            out.push_str(": ");
            out.push_str(value);
        }
        out
    }

    /// 条目数。
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否为空。
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::ComputedStyle;

    /// §4 `web-style::computed`：`computed_style_roundtrip`（置入-取回-文本序列化）。
    #[test]
    fn computed_style_roundtrip() {
        let mut style = ComputedStyle::new();
        assert!(style.is_empty());
        style.set("Width", "120px");
        style.set("color", "red");
        assert_eq!(style.get("width"), Some("120px"), "属性名应小写折叠");
        assert_eq!(style.get("COLOR"), Some("red"), "取用应大小写不敏感");
        assert_eq!(style.to_css_text(), "width: 120px; color: red");

        // 覆盖保留原位置。
        style.set("width", "200px");
        assert_eq!(style.get("width"), Some("200px"));
        assert_eq!(style.to_css_text(), "width: 200px; color: red");
        assert_eq!(style.len(), 2);

        // 删除。
        assert_eq!(style.remove("color"), Some("red".to_owned()));
        assert_eq!(style.get("color"), None);
        assert_eq!(style.to_css_text(), "width: 200px");
    }

    #[test]
    fn computed_style_empty_text() {
        assert_eq!(ComputedStyle::new().to_css_text(), "");
    }
}
