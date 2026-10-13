//! Live 集合最小面（`web-dom::collection`）。
//!
//! `LiveCollection` 按**查询谓词惰性求值**（每次 `collect`/`item` 现算，故「live」）。
//! G2 **不做** Range、不做完整 `HTMLCollection`（属 G3）。

use crate::node::{NodeId, NodeKind};
use crate::tree::NodeTree;

/// 集合查询谓词。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CollectionQuery {
    /// 所有**元素**后代。
    All,
    /// 标签名匹配的**元素**后代（名折叠为小写）。
    TagName(String),
}

/// 一个 live 集合：**每次求值现算**（子树遍历），不缓存结果。
#[derive(Clone, Debug)]
pub struct LiveCollection {
    query: CollectionQuery,
}

impl LiveCollection {
    /// 所有元素后代。
    #[must_use]
    pub fn all() -> Self {
        Self {
            query: CollectionQuery::All,
        }
    }

    /// 按标签名（`getElementsByTagName` 口径，名折叠为小写）。
    #[must_use]
    pub fn by_tag_name(name: &str) -> Self {
        Self {
            query: CollectionQuery::TagName(name.to_ascii_lowercase()),
        }
    }

    /// 该节点是否命中查询（仅元素命中）。
    #[must_use]
    pub fn matches(&self, tree: &NodeTree, id: NodeId) -> bool {
        let Some(node) = tree.get(id) else {
            return false;
        };
        if node.kind != NodeKind::Element {
            return false;
        }
        match &self.query {
            CollectionQuery::All => true,
            CollectionQuery::TagName(name) => node
                .element
                .as_ref()
                .is_some_and(|element| element.name == *name),
        }
    }

    /// 惰性求值：返回 `root` 子树内命中的节点（文档序）。
    #[must_use]
    pub fn collect(&self, tree: &NodeTree, root: NodeId) -> Vec<NodeId> {
        let mut out = Vec::new();
        self.walk(tree, root, &mut out);
        out
    }

    fn walk(&self, tree: &NodeTree, node: NodeId, out: &mut Vec<NodeId>) {
        for child in tree.children(node).collect::<Vec<_>>() {
            if self.matches(tree, child) {
                out.push(child);
            }
            self.walk(tree, child, out);
        }
    }

    /// 命中个数。
    #[must_use]
    pub fn len(&self, tree: &NodeTree, root: NodeId) -> usize {
        self.collect(tree, root).len()
    }

    /// 是否无命中。
    #[must_use]
    pub fn is_empty(&self, tree: &NodeTree, root: NodeId) -> bool {
        self.len(tree, root) == 0
    }

    /// 取第 `index` 个命中节点（0 起）。
    #[must_use]
    pub fn item(&self, tree: &NodeTree, root: NodeId, index: usize) -> Option<NodeId> {
        self.collect(tree, root).into_iter().nth(index)
    }
}

#[cfg(test)]
mod tests {
    use super::LiveCollection;
    use crate::tree::NodeTree;

    #[test]
    fn live_collection_minimal() {
        // §5 S2：Live 集合按谓词惰性求值（live：增删节点后结果随之变化）。
        let mut tree = NodeTree::new();
        let root = tree.root();
        let div1 = tree.create_element("div", "html", vec![]);
        let div2 = tree.create_element("div", "html", vec![]);
        let span = tree.create_element("span", "html", vec![]);
        tree.append_child(root, div1);
        tree.append_child(root, div2);
        tree.append_child(div1, span);

        let selection = LiveCollection::by_tag_name("div");
        assert_eq!(selection.len(&tree, root), 2);
        assert_eq!(selection.item(&tree, root, 0), Some(div1));
        assert_eq!(selection.item(&tree, root, 1), Some(div2));
        assert_eq!(selection.item(&tree, root, 2), None);

        let everything = LiveCollection::all();
        assert_eq!(everything.len(&tree, root), 3, "所有元素后代");

        // live：新增一个 div ⇒ 计数随之变化。
        let div3 = tree.create_element("div", "html", vec![]);
        tree.append_child(root, div3);
        assert_eq!(selection.len(&tree, root), 3, "live 集合应反映新增");
        assert!(!selection.is_empty(&tree, root));

        // 大小写折叠：大写标签名等价。
        assert_eq!(LiveCollection::by_tag_name("DIV").len(&tree, root), 3);
    }
}
