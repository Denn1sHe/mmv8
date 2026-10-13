//! 样式表文本登记（`web-style::sheet`，施工图 §1.1/§1.2；G3 S5）。
//!
//! - [`SheetOrigin`]：来源（UA < author）。
//! - [`Stylesheet`]：一张样式表（来源 + 文本）。
//! - [`StylesheetList`]：**去重登记 + 稳定句柄 + 移除**（G3-B11；供 `layout-backend::StyleSystem` 持有）。
//!
//! 本步只做**文本登记/去重/移除**；真正的**解析与级联**由 `layout-backend`（S6/S7，stylo）承担。

/// 样式表来源（级联 origin；UA < author）。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum SheetOrigin {
    /// 用户代理（UA）内嵌样式表。
    UserAgent,
    /// 作者样式表（`<style>` / `<link>` / 导出的作者 CSS）。
    Author,
}

/// 一张样式表：来源 + 文本（施工图 §1.2）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Stylesheet {
    /// 来源。
    pub origin: SheetOrigin,
    /// 原文（未解析；解析在 stylo 侧）。
    pub text: String,
}

impl Stylesheet {
    /// 构造。
    #[must_use]
    pub fn new(origin: SheetOrigin, text: impl Into<String>) -> Self {
        Self {
            origin,
            text: text.into(),
        }
    }

    /// 文本是否为空（去重键的一部分）。
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }
}

/// 样式表句柄（[`StylesheetList`] 分配；稳定，不因移除而位移）。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct SheetId(u64);

impl SheetId {
    /// 取不透明原始值。
    #[must_use]
    pub const fn raw(self) -> u64 {
        self.0
    }
}

/// 样式表列表：**按 `(origin, text)` 去重**登记、稳定句柄、可移除（G3-B11）。
#[derive(Clone, Debug, Default)]
pub struct StylesheetList {
    sheets: Vec<(SheetId, Stylesheet)>,
    next: u64,
}

impl StylesheetList {
    /// 建空列表。
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// 登记一张样式表；**内容相同（`origin` + `text` 全等）⇒ 复用既有句柄**（去重，不新增）。
    pub fn insert(&mut self, origin: SheetOrigin, text: impl Into<String>) -> SheetId {
        let text = text.into();
        if let Some((id, _)) = self
            .sheets
            .iter()
            .find(|(_, sheet)| sheet.origin == origin && sheet.text == text)
        {
            return *id;
        }
        let id = SheetId(self.next);
        self.next += 1;
        self.sheets.push((id, Stylesheet { origin, text }));
        id
    }

    /// 取一张样式表。
    #[must_use]
    pub fn get(&self, id: SheetId) -> Option<&Stylesheet> {
        self.sheets
            .iter()
            .find(|(sheet_id, _)| *sheet_id == id)
            .map(|(_, sheet)| sheet)
    }

    /// 移除一张样式表；返回是否存在（供 `remove_stylesheet` 语义，G3-B11）。
    pub fn remove(&mut self, id: SheetId) -> bool {
        let Some(index) = self.sheets.iter().position(|(sheet_id, _)| *sheet_id == id) else {
            return false;
        };
        self.sheets.remove(index);
        true
    }

    /// 逐条遍历（保序）。
    pub fn iter(&self) -> impl Iterator<Item = &Stylesheet> {
        self.sheets.iter().map(|(_, sheet)| sheet)
    }

    /// 句柄列表（保序）。
    #[must_use]
    pub fn ids(&self) -> Vec<SheetId> {
        self.sheets.iter().map(|(id, _)| *id).collect()
    }

    /// 条目数。
    #[must_use]
    pub fn len(&self) -> usize {
        self.sheets.len()
    }

    /// 是否为空。
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.sheets.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::{SheetOrigin, Stylesheet, StylesheetList};

    /// §4 `web-style::sheet`：来源区分 + 内容去重 + 移除。
    #[test]
    fn stylesheet_origin_and_dedup() {
        let mut list = StylesheetList::new();
        assert!(list.is_empty());

        let ua = list.insert(SheetOrigin::UserAgent, "div { display: block; }");
        // 同 (origin, text) 重复 ⇒ 去重复用句柄，不新增。
        let ua_again = list.insert(SheetOrigin::UserAgent, "div { display: block; }");
        assert_eq!(ua, ua_again, "同内容应去重复用句柄");
        assert_eq!(list.len(), 1);

        // 同文本、不同 origin ⇒ 视为不同样式表。
        let author = list.insert(SheetOrigin::Author, "div { display: block; }");
        assert_ne!(ua, author, "origin 不同不得去重");
        assert_eq!(list.len(), 2);

        assert_eq!(
            list.get(author).map(|s| s.origin),
            Some(SheetOrigin::Author)
        );
        assert_eq!(
            list.get(ua).map(|s| s.text.as_str()),
            Some("div { display: block; }")
        );

        // 移除语义（G3-B11）：存在 ⇒ true 且消失；重复移除 ⇒ false。
        assert!(list.remove(ua));
        assert_eq!(list.len(), 1);
        assert!(list.get(ua).is_none());
        assert!(!list.remove(ua), "重复移除应返回 false");
        assert_eq!(list.ids().len(), 1);
    }

    #[test]
    fn stylesheet_value_roundtrip() {
        let sheet = Stylesheet::new(SheetOrigin::Author, "");
        assert!(sheet.is_empty());
        let sheet = Stylesheet::new(SheetOrigin::Author, "p { color: red; }");
        assert_eq!(sheet.origin, SheetOrigin::Author);
        assert!(!sheet.is_empty());
    }
}
