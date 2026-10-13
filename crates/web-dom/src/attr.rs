//! 有序属性表（`web-dom::attr`）。
//!
//! 属性**保序**保存；名字按 HTML 规则**大小写折叠**（不区分大小写，ASCII 小写）。
//! G2 对 `svg`/`mathml` 命名空间的属性大小写亦作同样折叠（最小面；精确命名空间大小写规则
//! 属 G3）。

/// 有序属性表：`(名, 值)`，名已小写折叠。重复 `set` 覆盖值并**保留原位置**。
#[derive(Clone, Debug, Default)]
pub struct AttributeMap {
    entries: Vec<(String, String)>,
}

impl AttributeMap {
    /// 建空表。
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// 设一条属性（名折叠为 ASCII 小写；已存在则覆盖值、保留原顺序）。
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

    /// 取一条属性值。
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&str> {
        let key = name.trim().to_ascii_lowercase();
        self.entries
            .iter()
            .find(|(k, _)| k.as_str() == key)
            .map(|(_, v)| v.as_str())
    }

    /// 是否含某属性。
    #[must_use]
    pub fn contains(&self, name: &str) -> bool {
        self.get(name).is_some()
    }

    /// 删除一条属性并返回旧值。
    pub fn remove(&mut self, name: &str) -> Option<String> {
        let key = name.trim().to_ascii_lowercase();
        let index = self.entries.iter().position(|(k, _)| k.as_str() == key)?;
        Some(self.entries.remove(index).1)
    }

    /// 逐条遍历（保序）。
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.entries.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }

    /// 属性条数。
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

#[cfg(test)]
mod tests {
    use super::AttributeMap;

    #[test]
    fn attr_case_folds_html_names() {
        let mut map = AttributeMap::new();
        map.set("CLASS", "x");
        map.set("Id", "y");
        assert_eq!(map.get("class"), Some("x"));
        assert_eq!(map.get("CLASS"), Some("x"), "读取亦折叠大小写");
        assert_eq!(map.get("id"), Some("y"));
        assert!(map.contains("Class"));

        // 覆盖保留原位置、保序。
        map.set("class", "z");
        assert_eq!(map.get("class"), Some("z"));
        let keys: Vec<&str> = map.iter().map(|(k, _)| k).collect();
        assert_eq!(keys, vec!["class", "id"], "覆盖应保留原位置");

        assert_eq!(map.remove("ID"), Some("y".to_owned()));
        assert!(!map.contains("id"));
    }
}
