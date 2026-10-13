//! 失效簿记（`layout-backend::invalidate`，施工图 §2.2 / §3.3）。
//!
//! **两级脏位**（style / layout）+ **脏节点集** + **变更→脏级映射**（§2.2 表逐行）+ restyle root。
//! 纯逻辑（不依赖 stylo/taffy），由 `MutationBuffer`（S13）/ `set_viewport` 驱动。

use std::collections::HashSet;

use web_dom::node::NodeId;
use web_dom::tree::NodeTree;

use crate::backend::DirtyState;

/// 一次 DOM 变更的**类别**（`apply` 的输入；§2.2 表的左列）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DomChange<'a> {
    /// `class` 属性变更。
    ClassName,
    /// `id` 属性变更。
    Id,
    /// 一般属性变更（非 `style`）。
    Attribute,
    /// 内联 `style` 中某个**属性**变更（是否含 layout 级取决于 [`is_layout_affecting_property`]）。
    InlineStyle {
        /// 变更的内联属性名（大小写不敏感）。
        property: &'a str,
    },
    /// 子节点增删（`append_child` / `remove_child`）。
    ChildList,
    /// 文本内容变更。
    CharacterData,
    /// 视口变更（`set_viewport`）。
    Viewport,
    /// Shadow 结构（`attach_shadow` / slot 结构）变更。
    Shadow,
}

/// 该内联属性名是否**影响布局**（§2.2：尺寸相关 ⇒ layout 级也脏）。
///
/// 尺寸相关：`width`/`height`/`min|max-*`/`margin*`/`padding*`/`border*`/`font-size`/`line-height`/
/// `display`/`position`/`top|right|bottom|left`/`flex*`/`grid*`/`top` 等（保守：非已知「纯样式」属性
/// （`color`/`background`/`opacity`/`visibility`/`z-index`/`text-*` 等）一律视为影响布局）。
#[must_use]
pub fn is_layout_affecting_property(property: &str) -> bool {
    const PURE_STYLE: &[&str] = &[
        "color",
        "background",
        "background-color",
        "background-image",
        "background-position",
        "background-size",
        "background-repeat",
        "opacity",
        "visibility",
        "z-index",
        "text-align",
        "text-decoration",
        "text-transform",
        "text-shadow",
        "color-scheme",
        "cursor",
        "outline-color",
        "outline-style",
        "border-color",
    ];
    let name = property.trim().to_ascii_lowercase();
    !PURE_STYLE.contains(&name.as_str())
}

/// 两级失效簿记（`DirtyState` + 脏节点集）。
#[derive(Debug, Default)]
pub struct Invalidation {
    style_dirty: bool,
    layout_dirty: bool,
    dirty_nodes: HashSet<NodeId>,
}

impl Invalidation {
    /// 建空簿记。
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// 标一个节点的样式级脏。
    pub fn mark_style(&mut self, node: NodeId) {
        self.style_dirty = true;
        self.dirty_nodes.insert(node);
    }

    /// 标一个节点的布局级脏。
    pub fn mark_layout(&mut self, node: NodeId) {
        self.layout_dirty = true;
        self.dirty_nodes.insert(node);
    }

    /// 标**全量**布局脏（`set_viewport`；无特定节点）。
    pub fn mark_all_layout(&mut self) {
        self.layout_dirty = true;
    }

    /// 按变更类别施加两级脏（§2.2 表逐行）。
    pub fn apply(&mut self, change: DomChange<'_>) {
        match change {
            // class / id / 一般属性 ⇒ 仅 style 级。
            DomChange::ClassName | DomChange::Id | DomChange::Attribute => self.style_dirty = true,
            // 内联 style 属性 ⇒ style 级；尺寸相关另加 layout 级。
            DomChange::InlineStyle { property } => {
                self.style_dirty = true;
                if is_layout_affecting_property(property) {
                    self.layout_dirty = true;
                }
            }
            // 结构/文本 ⇒ 两级。
            DomChange::ChildList | DomChange::CharacterData | DomChange::Shadow => {
                self.style_dirty = true;
                self.layout_dirty = true;
            }
            // 视口 ⇒ 仅 layout 级（全量）。
            DomChange::Viewport => self.layout_dirty = true,
        }
    }

    /// 当前脏状态（只读）。
    #[must_use]
    pub fn state(&self) -> DirtyState {
        DirtyState {
            style: self.style_dirty,
            layout: self.layout_dirty,
        }
    }

    /// 取走脏状态（消费；`resolve` 起调用）。
    pub fn take(&mut self) -> DirtyState {
        let state = DirtyState {
            style: self.style_dirty,
            layout: self.layout_dirty,
        };
        self.style_dirty = false;
        self.layout_dirty = false;
        self.dirty_nodes.clear();
        state
    }

    /// 脏节点集（只读）。
    #[must_use]
    pub fn dirty_nodes(&self) -> &HashSet<NodeId> {
        &self.dirty_nodes
    }

    /// **restyle root**：脏节点中最靠上的（其祖先链无更脏节点者）——限制重算子树（§2.2）。
    ///
    /// 无脏节点 ⇒ `None`。
    #[must_use]
    pub fn restyle_roots(&self, tree: &NodeTree) -> Vec<NodeId> {
        let mut roots = Vec::new();
        for &node in &self.dirty_nodes {
            let mut current = tree.parent(node);
            let mut has_dirty_ancestor = false;
            while let Some(parent) = current {
                if self.dirty_nodes.contains(&parent) {
                    has_dirty_ancestor = true;
                    break;
                }
                current = tree.parent(parent);
            }
            if !has_dirty_ancestor {
                roots.push(node);
            }
        }
        roots.sort_by_key(|node| node.raw());
        roots
    }
}

#[cfg(test)]
mod tests {
    use super::{DomChange, Invalidation, is_layout_affecting_property};
    use crate::backend::DirtyState;
    use web_dom::node::NodeId;
    use web_dom::tree::NodeTree;

    #[test]
    fn change_to_dirty_level_mapping_matches_spec_table() {
        // §2.2 表逐行。
        let mut inv = Invalidation::new();
        inv.apply(DomChange::ClassName);
        assert_eq!(
            inv.state(),
            DirtyState {
                style: true,
                layout: false
            },
            "class ⇒ 仅 style"
        );
        inv.apply(DomChange::Id);
        assert_eq!(
            inv.state(),
            DirtyState {
                style: true,
                layout: false
            },
            "id ⇒ 仅 style"
        );
        inv.apply(DomChange::Attribute);
        assert_eq!(
            inv.state(),
            DirtyState {
                style: true,
                layout: false
            },
            "一般属性 ⇒ 仅 style"
        );

        let mut inv = Invalidation::new();
        inv.apply(DomChange::InlineStyle { property: "color" });
        assert_eq!(
            inv.state(),
            DirtyState {
                style: true,
                layout: false
            },
            "内联非尺寸 ⇒ 仅 style"
        );
        inv.apply(DomChange::InlineStyle { property: "WIDTH" });
        assert_eq!(
            inv.state(),
            DirtyState {
                style: true,
                layout: true
            },
            "内联尺寸 ⇒ style+layout"
        );

        let mut inv = Invalidation::new();
        inv.apply(DomChange::Viewport);
        assert_eq!(
            inv.state(),
            DirtyState {
                style: false,
                layout: true
            },
            "set_viewport ⇒ 仅 layout"
        );

        let mut inv = Invalidation::new();
        inv.apply(DomChange::ChildList);
        assert_eq!(
            inv.state(),
            DirtyState {
                style: true,
                layout: true
            },
            "结构变更 ⇒ 两级"
        );
        inv.apply(DomChange::CharacterData);
        inv.apply(DomChange::Shadow);
        assert_eq!(
            inv.state(),
            DirtyState {
                style: true,
                layout: true
            }
        );
    }

    #[test]
    fn take_consumes_and_resets() {
        let mut inv = Invalidation::new();
        inv.mark_style(NodeId::new(1, 0));
        inv.mark_layout(NodeId::new(2, 0));
        let taken = inv.take();
        assert_eq!(
            taken,
            DirtyState {
                style: true,
                layout: true
            }
        );
        assert_eq!(inv.state(), DirtyState::default(), "take 后应清空");
        assert!(inv.dirty_nodes().is_empty());
    }

    #[test]
    fn restyle_roots_are_topmost_dirty() {
        let mut tree = NodeTree::new();
        let root = tree.root();
        let parent = tree.create_element("div", "html", vec![]);
        let child = tree.create_element("span", "html", vec![]);
        tree.append_child(root, parent);
        tree.append_child(parent, child);

        let mut inv = Invalidation::new();
        inv.mark_style(parent);
        assert_eq!(inv.restyle_roots(&tree), vec![parent], "父脏 ⇒ 根为父");

        inv.mark_style(child);
        assert_eq!(
            inv.restyle_roots(&tree),
            vec![parent],
            "父与子都脏 ⇒ 根仅为父（子被父覆盖）"
        );
    }

    #[test]
    fn layout_affecting_property_classification() {
        assert!(is_layout_affecting_property("width"));
        assert!(is_layout_affecting_property("Margin-Top"));
        assert!(is_layout_affecting_property("font-size"));
        assert!(is_layout_affecting_property("未知属性"));
        assert!(!is_layout_affecting_property("color"));
        assert!(!is_layout_affecting_property("opacity"));
    }
}
