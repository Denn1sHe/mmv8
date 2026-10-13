//! 内联声明值（`style="color: red"`）。
//!
//! G2 **只做内联声明**：属性名按 CSS 规则折叠为小写键、按声明顺序保序保存；
//! **不做级联 / 选择器 / 计算值**（完整样式属 G3）。`css_text` / `from_css_text`
//! 提供与 `style` 属性字符串之间的往返（`inline_decl_roundtrip`）。

/// 有序声明表：`(属性名, 值)`，属性名已小写折叠。重复 `set` 覆盖值并**保留原位置**。
#[derive(Clone, Debug, Default)]
pub struct DeclMap {
    entries: Vec<(String, String)>,
}

impl DeclMap {
    /// 建空表。
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// 设一条声明（`name` 折叠为 ASCII 小写；已存在则覆盖值、保留原顺序）。
    pub fn set(&mut self, name: &str, value: &str) {
        let key = name.trim().to_ascii_lowercase();
        if key.is_empty() {
            return;
        }
        if let Some(entry) = self.entries.iter_mut().find(|(k, _)| k.as_str() == key) {
            value.clone_into(&mut entry.1);
        } else {
            self.entries.push((key, value.to_owned()));
        }
    }

    /// 取一条声明值。
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&str> {
        let key = name.trim().to_ascii_lowercase();
        self.entries
            .iter()
            .find(|(k, _)| k.as_str() == key)
            .map(|(_, v)| v.as_str())
    }

    /// 删除一条声明并返回旧值。
    pub fn remove(&mut self, name: &str) -> Option<String> {
        let key = name.trim().to_ascii_lowercase();
        let index = self.entries.iter().position(|(k, _)| k.as_str() == key)?;
        Some(self.entries.remove(index).1)
    }

    /// 逐条遍历（保序）。
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.entries.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }

    /// 条目数。
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否为空表。
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// 元素的内联样式（`element.style`；G2 仅声明值，无级联/计算值）。
#[derive(Clone, Debug, Default)]
pub struct InlineStyle {
    decls: DeclMap,
}

impl InlineStyle {
    /// 建空内联样式。
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// 设一条内联声明。
    pub fn set_property(&mut self, name: &str, value: &str) {
        self.decls.set(name, value);
    }

    /// 取一条内联声明值。
    #[must_use]
    pub fn get_property(&self, name: &str) -> Option<&str> {
        self.decls.get(name)
    }

    /// 删一条内联声明并返回旧值。
    pub fn remove_property(&mut self, name: &str) -> Option<String> {
        self.decls.remove(name)
    }

    /// 底层声明表（只读）。
    #[must_use]
    pub fn decls(&self) -> &DeclMap {
        &self.decls
    }

    /// 序列化为 `style` 属性文本，如 `color: red; width: 3px;`（空表 ⇒ 空串）。
    #[must_use]
    pub fn css_text(&self) -> String {
        let mut out = String::new();
        for (name, value) in self.decls.iter() {
            out.push_str(name);
            out.push_str(": ");
            out.push_str(value);
            out.push_str("; ");
        }
        out.trim_end().to_owned()
    }

    /// 从 `style` 属性文本解析（`a: b; c: d`；忽略空项与**缺冒号**项）。
    #[must_use]
    pub fn from_css_text(text: &str) -> Self {
        let mut style = Self::new();
        for part in text.split(';') {
            let Some((name, value)) = part.split_once(':') else {
                continue;
            };
            let name = name.trim();
            if name.is_empty() {
                continue;
            }
            style.decls.set(name, value.trim());
        }
        style
    }
}

#[cfg(test)]
mod tests {
    use super::{DeclMap, InlineStyle};

    #[test]
    fn inline_decl_roundtrip() {
        // §5 S4：内联声明值置入-取回-文本往返。
        let mut style = InlineStyle::new();
        style.set_property("Color", "red");
        style.set_property("width", "3px");
        assert_eq!(style.get_property("color"), Some("red"), "属性名应小写折叠");
        assert_eq!(style.get_property("WIDTH"), Some("3px"));

        let text = style.css_text();
        let back = InlineStyle::from_css_text(&text);
        assert_eq!(back.get_property("color"), Some("red"));
        assert_eq!(back.get_property("width"), Some("3px"));

        // 覆盖保留原位置；删除生效。
        style.set_property("color", "blue");
        assert_eq!(style.get_property("color"), Some("blue"));
        assert_eq!(style.decls().len(), 2);
        assert_eq!(style.remove_property("color"), Some("blue".to_owned()));
        assert_eq!(style.get_property("color"), None);
        assert_eq!(style.decls().len(), 1);
    }

    #[test]
    fn decl_map_case_folding_and_order() {
        let mut map = DeclMap::new();
        map.set("  Z-Index  ", "1");
        map.set("color", "red");
        assert_eq!(map.get("z-index"), Some("1"), "键应 trim + 小写折叠");
        let keys: Vec<&str> = map.iter().map(|(k, _)| k).collect();
        assert_eq!(keys, vec!["z-index", "color"], "应保序");
        assert!(DeclMap::new().is_empty());
    }
}
