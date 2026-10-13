//! P-4b —— 在**真实 `web-dom` 模型**上判定「`:host` / `::slotted` 经（shadow 作用域）生效」。
//!
//! 做法：为 `web-dom::NodeTree` 上的 [`NodeRef`] 实现 `selectors::Element`
//! （`Impl = style::selector_parser::SelectorImpl`），再用 `selectors::matching::matches_selector`
//! 判定 `:host` / `::slotted(span)` 是否命中。`opaque()` 走 §1.2.1 线程局部侧表（[`crate::opaque`]）。
//!
//! > `selectors::Element` **无任何 `unsafe` 方法** ⇒ 本模块**零 unsafe**（crate `#![forbid(unsafe_code)]`）。

use selectors::attr::{AttrSelectorOperation, CaseSensitivity, NamespaceConstraint};
use selectors::bloom::BloomFilter;
use selectors::context::{QuirksMode, SelectorCaches};
use selectors::matching::{
    matches_selector, ElementSelectorFlags, MatchingContext, MatchingForInvalidation, MatchingMode,
    NeedsSelectorFlags,
};
use selectors::parser::SelectorImpl as SelectorsSelectorImpl;
use selectors::{Element as SelectorsElement, OpaqueElement};
use style::selector_parser::{SelectorImpl, SelectorParser};
use style::stylesheets::UrlExtraData;
use web_dom::node::{NodeId, NodeKind};
use web_dom::tree::NodeTree;

use crate::opaque::{self, TreeId};

/// `web-dom` 节点上的元素引用（投影的**只读**视图）。
#[derive(Clone, Copy, Debug)]
pub struct NodeRef<'a> {
    /// 节点树（只读借用）。
    pub tree: &'a NodeTree,
    /// 节点身份。
    pub id: NodeId,
    /// 树身份（opaque 侧表用）。
    pub tree_id: TreeId,
}

impl<'a> NodeRef<'a> {
    /// 构造。
    #[must_use]
    pub const fn new(tree: &'a NodeTree, id: NodeId, tree_id: TreeId) -> Self {
        Self { tree, id, tree_id }
    }

    fn element_data(&self) -> Option<&'a web_dom::node::ElementData> {
        self.tree
            .get(self.id)
            .and_then(|node| node.element.as_ref())
    }

    fn local_name(&self) -> Option<&'a str> {
        self.element_data().map(|element| element.name.as_str())
    }

    fn namespace(&self) -> Option<&'a str> {
        self.element_data()
            .map(|element| element.namespace.as_str())
    }

    fn attr(&self, name: &str) -> Option<&'a str> {
        self.element_data()
            .and_then(|element| element.attrs.get(name))
    }

    /// `<slot>` 元素。
    fn is_slot(&self) -> bool {
        self.local_name() == Some("slot")
    }
}

impl SelectorsElement for NodeRef<'_> {
    type Impl = SelectorImpl;

    fn opaque(&self) -> OpaqueElement {
        // §1.2.1：`OpaqueElement::new(&*anchor)`；从不 deref 指针 ⇒ 零 unsafe。
        opaque::with_side(|side| side.opaque_for(self.tree_id, self.id.raw()))
    }

    fn parent_element(&self) -> Option<Self> {
        let parent = self.tree.parent(self.id)?;
        self.tree
            .get(parent)
            .filter(|node| node.kind == NodeKind::Element)
            .map(|_| NodeRef::new(self.tree, parent, self.tree_id))
    }

    fn parent_node_is_shadow_root(&self) -> bool {
        self.tree
            .parent(self.id)
            .is_some_and(|parent| self.tree.is_shadow_root(parent))
    }

    fn containing_shadow_host(&self) -> Option<Self> {
        let root = self.tree.containing_shadow_root(self.id)?;
        self.tree
            .shadow_host(root)
            .map(|host| NodeRef::new(self.tree, host, self.tree_id))
    }

    fn is_pseudo_element(&self) -> bool {
        false
    }

    fn prev_sibling_element(&self) -> Option<Self> {
        let mut prev = self.tree.get(self.id).and_then(|node| node.prev_sibling);
        while let Some(id) = prev {
            match self.tree.get(id) {
                Some(node) if node.kind == NodeKind::Element => {
                    return Some(NodeRef::new(self.tree, id, self.tree_id));
                }
                Some(node) => prev = node.prev_sibling,
                None => return None,
            }
        }
        None
    }

    fn next_sibling_element(&self) -> Option<Self> {
        let mut next = self.tree.get(self.id).and_then(|node| node.next_sibling);
        while let Some(id) = next {
            match self.tree.get(id) {
                Some(node) if node.kind == NodeKind::Element => {
                    return Some(NodeRef::new(self.tree, id, self.tree_id));
                }
                Some(node) => next = node.next_sibling,
                None => return None,
            }
        }
        None
    }

    fn first_element_child(&self) -> Option<Self> {
        self.tree
            .children(self.id)
            .find(|child| {
                self.tree
                    .get(*child)
                    .is_some_and(|n| n.kind == NodeKind::Element)
            })
            .map(|child| NodeRef::new(self.tree, child, self.tree_id))
    }

    fn is_html_element_in_html_document(&self) -> bool {
        self.namespace() == Some("html")
    }

    fn has_local_name(
        &self,
        local_name: &<Self::Impl as SelectorsSelectorImpl>::BorrowedLocalName,
    ) -> bool {
        self.local_name() == Some(local_name.as_ref())
    }

    fn has_namespace(
        &self,
        ns: &<Self::Impl as SelectorsSelectorImpl>::BorrowedNamespaceUrl,
    ) -> bool {
        self.namespace() == Some(ns.as_ref())
    }

    fn is_same_type(&self, other: &Self) -> bool {
        self.local_name() == other.local_name() && self.namespace() == other.namespace()
    }

    fn attr_matches(
        &self,
        ns: &NamespaceConstraint<&<Self::Impl as SelectorsSelectorImpl>::NamespaceUrl>,
        local_name: &<Self::Impl as SelectorsSelectorImpl>::LocalName,
        operation: &AttrSelectorOperation<&<Self::Impl as SelectorsSelectorImpl>::AttrValue>,
    ) -> bool {
        if !matches!(ns, NamespaceConstraint::Any) {
            // 探针：无命名空间属性（G3 最小面），特定 ns 约束一律不命中。
            return false;
        }
        match self.attr(local_name.as_ref()) {
            Some(value) => operation.eval_str(value),
            None => false,
        }
    }

    fn match_non_ts_pseudo_class(
        &self,
        _pc: &<Self::Impl as SelectorsSelectorImpl>::NonTSPseudoClass,
        _context: &mut MatchingContext<Self::Impl>,
    ) -> bool {
        false
    }

    fn match_pseudo_element(
        &self,
        _pe: &<Self::Impl as SelectorsSelectorImpl>::PseudoElement,
        _context: &mut MatchingContext<Self::Impl>,
    ) -> bool {
        false
    }

    fn apply_selector_flags(&self, _flags: ElementSelectorFlags) {}

    fn is_link(&self) -> bool {
        false
    }

    fn is_html_slot_element(&self) -> bool {
        self.is_slot()
    }

    fn assigned_slot(&self) -> Option<Self> {
        // E 为宿主 H 的 light 子；返回 H 影子树内分配给它、且包含 E 的 `<slot>`。
        let host = self.tree.parent(self.id)?;
        let shadow = self.tree.element_shadow(host)?;
        for slot in self.tree.children(shadow) {
            if self
                .tree
                .get(slot)
                .is_some_and(|n| n.element.as_ref().is_some_and(|e| e.name == "slot"))
                && self.tree.assigned_nodes(slot).contains(&self.id)
            {
                return Some(NodeRef::new(self.tree, slot, self.tree_id));
            }
        }
        None
    }

    fn has_id(
        &self,
        id: &<Self::Impl as SelectorsSelectorImpl>::Identifier,
        case_sensitivity: CaseSensitivity,
    ) -> bool {
        self.attr("id")
            .is_some_and(|value| case_sensitivity.eq(value.as_bytes(), id.as_ref().as_bytes()))
    }

    fn has_class(
        &self,
        name: &<Self::Impl as SelectorsSelectorImpl>::Identifier,
        case_sensitivity: CaseSensitivity,
    ) -> bool {
        self.element_data().is_some_and(|element| {
            element
                .class_list()
                .iter()
                .any(|class| case_sensitivity.eq(class.as_bytes(), name.as_ref().as_bytes()))
        })
    }

    fn has_custom_state(&self, _name: &<Self::Impl as SelectorsSelectorImpl>::Identifier) -> bool {
        false
    }

    fn imported_part(
        &self,
        _name: &<Self::Impl as SelectorsSelectorImpl>::Identifier,
    ) -> Option<<Self::Impl as SelectorsSelectorImpl>::Identifier> {
        None
    }

    fn is_part(&self, _name: &<Self::Impl as SelectorsSelectorImpl>::Identifier) -> bool {
        false
    }

    fn is_empty(&self) -> bool {
        self.tree
            .children(self.id)
            .all(|child| match self.tree.get(child) {
                Some(node) => match node.kind {
                    NodeKind::Element => false,
                    NodeKind::Text | NodeKind::Comment => {
                        node.text.as_deref().is_none_or(str::is_empty)
                    }
                    _ => true,
                },
                None => true,
            })
    }

    fn is_root(&self) -> bool {
        self.tree.parent(self.id).is_some_and(|parent| {
            self.tree
                .get(parent)
                .is_some_and(|n| n.kind == NodeKind::Document)
        })
    }

    fn add_element_unique_hashes(&self, _filter: &mut BloomFilter) -> bool {
        // 探针：bloom 为可选快速拒绝优化；此处不注入（不影响匹配正确性）。
        false
    }
}

/// 判定 `css`（单选择器）是否命中 `target`；`shadow_host` 非空时以该宿主为 `:host` 上下文。
///
/// # Panics
/// 选择器文本非法时 panic（探针用；生产返回 `Result`）。
#[must_use]
pub fn matches(
    css: &str,
    tree: &NodeTree,
    target: NodeId,
    tree_id: TreeId,
    shadow_host: Option<NodeId>,
) -> bool {
    let url = UrlExtraData::from(url::Url::parse("about:blank").expect("valid url"));
    let list = SelectorParser::parse_author_origin_no_namespace(css, &url)
        .expect("probe selector should parse");
    let selector = &list.slice()[0];

    let mut caches = SelectorCaches::default();
    let mut context = MatchingContext::<SelectorImpl>::new(
        MatchingMode::Normal,
        None,
        &mut caches,
        QuirksMode::NoQuirks,
        NeedsSelectorFlags::No,
        MatchingForInvalidation::No,
    );
    let target_ref = NodeRef::new(tree, target, tree_id);
    let host_ref = shadow_host.map(|host| NodeRef::new(tree, host, tree_id));
    context.with_shadow_host(host_ref, |ctx| {
        matches_selector(selector, 0, None, &target_ref, ctx)
    })
}

#[cfg(test)]
mod tests {
    use super::matches;
    use crate::p4::build_model;

    #[test]
    fn host_selector_matches_host() {
        let model = build_model();
        assert!(
            matches(
                ":host",
                &model.tree,
                model.host,
                model.tree_id,
                Some(model.host)
            ),
            ":host 应以宿主为上下文命中宿主"
        );
    }

    #[test]
    fn host_selector_does_not_match_other_element() {
        let model = build_model();
        assert!(
            !matches(
                ":host",
                &model.tree,
                model.light_span,
                model.tree_id,
                Some(model.host)
            ),
            ":host 不得命中非宿主元素"
        );
    }

    #[test]
    fn slotted_selector_matches_slotted_light_child() {
        let model = build_model();
        assert!(
            matches(
                "::slotted(span)",
                &model.tree,
                model.light_span,
                model.tree_id,
                Some(model.host)
            ),
            "::slotted(span) 应命中被分发的 light 子 <span>"
        );
    }

    #[test]
    fn slotted_selector_rejects_shadow_content() {
        let model = build_model();
        assert!(
            !matches(
                "::slotted(div)",
                &model.tree,
                model.shadow_div,
                model.tree_id,
                Some(model.host)
            ),
            "::slotted 不得命中影子树内节点"
        );
    }
}
