//! `stylo_host` —— **stylo FFI 薄桥**：把 `web-dom::NodeTree` **只读**投影成 stylo 消费的类型
//! （施工图 §1.2 / §2.1；ADR-0024 目录级白名单 `crates/layout-backend/src/stylo_host/`）。
//!
//! - [`selectors::Element`]（匹配面；**无 `unsafe` 方法**）——本文件。
//! - `style::dom::{TDocument,TNode,TElement,TShadowRoot}`——见 `dom` 子模块（含 5 个必需 `unsafe fn`）。
//!
//! `opaque()` 走线程局部侧表（[`crate::opaque`]，§1.2.1；**零 unsafe**）。
//! **投影只读**（DOM 树）；每节点 stylo 元素数据（[`ElementSlot`]）**急切持有**（P-1b 定形），
//! 由宿主按节点数一次性分配、`NodeRef` 持 `&'a [ElementSlot]` ⇒ `ensure_data` 返回值的生命周期挂 `'a`。

use std::cell::Cell;

use selectors::attr::{AttrSelectorOperation, CaseSensitivity, NamespaceConstraint};
use selectors::bloom::BloomFilter;
use selectors::matching::{ElementSelectorFlags, MatchingContext};
use selectors::{Element as SelectorsElement, OpaqueElement};
use style::data::{ElementData, ElementDataMut, ElementDataRef, ElementDataWrapper};
use style::selector_parser::SelectorImpl;
use web_dom::node::{ElementData as DomElementData, NodeId, NodeKind};
use web_dom::tree::NodeTree;

use crate::opaque;

/// 每节点 stylo 元素数据槽（**急切持有** `ElementDataWrapper`；P-1b 定形 `signatures:5 bodies:0`）。
///
/// 由宿主 [`StyloHost`] 按节点数一次性分配；`NodeRef` 持 `&'a [ElementSlot]` ⇒
/// `ensure_data(&self) -> ElementDataMut<'_>` 返回生命周期挂 `'a` —— **零 unsafe**
/// （stylo `ElementDataWrapper::borrow_mut(&self)` 为安全方法；`ElementData: Default`）。
#[derive(Debug, Default)]
pub struct ElementSlot {
    data: ElementDataWrapper,
    snapshot_handled: Cell<bool>,
    dirty_descendants: Cell<bool>,
}

impl ElementSlot {
    /// 建一个空槽。
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// 急切借出可变数据（安全：`borrow_mut` 内部互斥）。
    pub fn ensure_data(&self) -> ElementDataMut<'_> {
        self.data.borrow_mut()
    }

    /// 只读借出数据。
    #[must_use]
    pub fn borrow_data(&self) -> ElementDataRef<'_> {
        self.data.borrow()
    }

    /// 清空数据（覆盖为 `ElementData::default()`；`DerefMut` 安全写）。
    pub fn clear_data(&self) {
        *self.data.borrow_mut() = ElementData::default();
    }

    /// 置/取「已处理快照」位。
    pub fn set_snapshot_handled(&self) {
        self.snapshot_handled.set(true);
    }

    /// 「已处理快照」位。
    #[must_use]
    pub fn snapshot_handled(&self) -> bool {
        self.snapshot_handled.get()
    }

    /// 置/清「有脏后代」位。
    pub fn set_dirty_descendants(&self) {
        self.dirty_descendants.set(true);
    }

    /// 清「有脏后代」位。
    pub fn unset_dirty_descendants(&self) {
        self.dirty_descendants.set(false);
    }

    /// 「有脏后代」位。
    #[must_use]
    pub fn has_dirty_descendants(&self) -> bool {
        self.dirty_descendants.get()
    }
}

/// 桥：持节点树 + 每节点 stylo 元素数据槽。
#[derive(Debug)]
pub struct StyloHost<'a> {
    tree: &'a NodeTree,
    slots: &'a [ElementSlot],
}

impl<'a> StyloHost<'a> {
    /// 由节点树与按节点数分配的 [`ElementSlot`] 切片构造桥。
    #[must_use]
    pub const fn new(tree: &'a NodeTree, slots: &'a [ElementSlot]) -> Self {
        Self { tree, slots }
    }

    /// 节点树（只读）。
    #[must_use]
    pub const fn tree(&self) -> &'a NodeTree {
        self.tree
    }

    /// 取一个节点引用。
    #[must_use]
    pub const fn node(&self, id: NodeId) -> NodeRef<'a> {
        NodeRef {
            tree: self.tree,
            slots: self.slots,
            id,
        }
    }
}

/// `web-dom` 节点上的**只读**视图（`selectors::Element` + stylo `dom` 投影）。
#[derive(Clone, Copy, Debug)]
pub struct NodeRef<'a> {
    /// 节点树（只读借用）。
    pub tree: &'a NodeTree,
    /// 每节点 stylo 元素数据槽（按 `NodeId::index()` 索引）。
    pub slots: &'a [ElementSlot],
    /// 节点身份。
    pub id: NodeId,
}

impl<'a> NodeRef<'a> {
    /// 构造。
    #[must_use]
    pub const fn new(tree: &'a NodeTree, slots: &'a [ElementSlot], id: NodeId) -> Self {
        Self { tree, slots, id }
    }

    /// 本节点的数据槽（`index` 越界 ⇒ `None`）。
    #[must_use]
    pub fn slot(&self) -> Option<&'a ElementSlot> {
        self.slots.get(self.id.index() as usize)
    }

    /// 元素数据（非元素 ⇒ `None`）。
    #[must_use]
    pub fn element_data(&self) -> Option<&'a DomElementData> {
        self.tree
            .get(self.id)
            .and_then(|node| node.element.as_ref())
    }

    /// 标签名（小写）。
    #[must_use]
    pub fn local_name(&self) -> Option<&'a str> {
        self.element_data().map(|element| element.name.as_str())
    }

    /// 命名空间。
    #[must_use]
    pub fn namespace(&self) -> Option<&'a str> {
        self.element_data()
            .map(|element| element.namespace.as_str())
    }

    /// 属性值。
    #[must_use]
    pub fn attr(&self, name: &str) -> Option<&'a str> {
        self.element_data()
            .and_then(|element| element.attrs.get(name))
    }

    /// 是否元素节点。
    #[must_use]
    pub fn is_element(&self) -> bool {
        self.tree
            .get(self.id)
            .is_some_and(|node| node.kind == NodeKind::Element)
    }

    /// 是否文本节点。
    #[must_use]
    pub fn is_text_node(&self) -> bool {
        self.tree
            .get(self.id)
            .is_some_and(|node| node.kind == NodeKind::Text)
    }

    /// 是否 `<slot>` 元素。
    #[must_use]
    pub fn is_slot(&self) -> bool {
        self.local_name() == Some("slot")
    }
}

impl SelectorsElement for NodeRef<'_> {
    type Impl = SelectorImpl;

    fn opaque(&self) -> OpaqueElement {
        // §1.2.1：`OpaqueElement::new(&*anchor)`；从不 deref 指针 ⇒ 零 unsafe。
        opaque::with_side(|side| side.opaque_for(self.id))
    }

    fn parent_element(&self) -> Option<Self> {
        let parent = self.tree.parent(self.id)?;
        self.tree
            .get(parent)
            .filter(|node| node.kind == NodeKind::Element)
            .map(|_| NodeRef::new(self.tree, self.slots, parent))
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
            .map(|host| NodeRef::new(self.tree, self.slots, host))
    }

    fn is_pseudo_element(&self) -> bool {
        false
    }

    fn prev_sibling_element(&self) -> Option<Self> {
        let mut prev = self.tree.get(self.id).and_then(|node| node.prev_sibling);
        while let Some(id) = prev {
            let node = self.tree.get(id)?;
            if node.kind == NodeKind::Element {
                return Some(NodeRef::new(self.tree, self.slots, id));
            }
            prev = node.prev_sibling;
        }
        None
    }

    fn next_sibling_element(&self) -> Option<Self> {
        let mut next = self.tree.get(self.id).and_then(|node| node.next_sibling);
        while let Some(id) = next {
            let node = self.tree.get(id)?;
            if node.kind == NodeKind::Element {
                return Some(NodeRef::new(self.tree, self.slots, id));
            }
            next = node.next_sibling;
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
            .map(|child| NodeRef::new(self.tree, self.slots, child))
    }

    fn is_html_element_in_html_document(&self) -> bool {
        self.namespace() == Some("html")
    }

    fn has_local_name(
        &self,
        local_name: &<Self::Impl as selectors::SelectorImpl>::BorrowedLocalName,
    ) -> bool {
        self.local_name() == Some(local_name.as_ref())
    }

    fn has_namespace(
        &self,
        ns: &<Self::Impl as selectors::SelectorImpl>::BorrowedNamespaceUrl,
    ) -> bool {
        self.namespace() == Some(ns.as_ref())
    }

    fn is_same_type(&self, other: &Self) -> bool {
        self.local_name() == other.local_name() && self.namespace() == other.namespace()
    }

    fn attr_matches(
        &self,
        ns: &NamespaceConstraint<&<Self::Impl as selectors::SelectorImpl>::NamespaceUrl>,
        local_name: &<Self::Impl as selectors::SelectorImpl>::LocalName,
        operation: &AttrSelectorOperation<&<Self::Impl as selectors::SelectorImpl>::AttrValue>,
    ) -> bool {
        if !matches!(ns, NamespaceConstraint::Any) {
            return false;
        }
        match self.attr(local_name.as_ref()) {
            Some(value) => operation.eval_str(value),
            None => false,
        }
    }

    fn match_non_ts_pseudo_class(
        &self,
        _pc: &<Self::Impl as selectors::SelectorImpl>::NonTSPseudoClass,
        _context: &mut MatchingContext<Self::Impl>,
    ) -> bool {
        false
    }

    fn match_pseudo_element(
        &self,
        _pe: &<Self::Impl as selectors::SelectorImpl>::PseudoElement,
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
        let host = self.tree.parent(self.id)?;
        let shadow = self.tree.element_shadow(host)?;
        for slot in self.tree.children(shadow) {
            let is_slot = self
                .tree
                .get(slot)
                .is_some_and(|n| n.element.as_ref().is_some_and(|e| e.name == "slot"));
            if is_slot && self.tree.assigned_nodes(slot).contains(&self.id) {
                return Some(NodeRef::new(self.tree, self.slots, slot));
            }
        }
        None
    }

    fn has_id(
        &self,
        id: &<Self::Impl as selectors::SelectorImpl>::Identifier,
        case_sensitivity: CaseSensitivity,
    ) -> bool {
        self.attr("id")
            .is_some_and(|value| case_sensitivity.eq(value.as_bytes(), id.as_ref().as_bytes()))
    }

    fn has_class(
        &self,
        name: &<Self::Impl as selectors::SelectorImpl>::Identifier,
        case_sensitivity: CaseSensitivity,
    ) -> bool {
        self.element_data().is_some_and(|element| {
            element
                .class_list()
                .iter()
                .any(|class| case_sensitivity.eq(class.as_bytes(), name.as_ref().as_bytes()))
        })
    }

    fn has_custom_state(
        &self,
        _name: &<Self::Impl as selectors::SelectorImpl>::Identifier,
    ) -> bool {
        false
    }

    fn imported_part(
        &self,
        _name: &<Self::Impl as selectors::SelectorImpl>::Identifier,
    ) -> Option<<Self::Impl as selectors::SelectorImpl>::Identifier> {
        None
    }

    fn is_part(&self, _name: &<Self::Impl as selectors::SelectorImpl>::Identifier) -> bool {
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
        false
    }
}

#[cfg(test)]
mod tests {
    use super::{ElementSlot, StyloHost};
    use selectors::Element as _;
    use web_dom::tree::NodeTree;

    #[test]
    fn host_projects_elements_selectors_surface() {
        let mut tree = NodeTree::new();
        let root = tree.root();
        let html = tree.create_element("html", "html", vec![]);
        let div = tree.create_element("div", "html", vec![("id".to_owned(), "box".to_owned())]);
        tree.append_child(root, html);
        tree.append_child(html, div);

        let slots: Vec<ElementSlot> = (0..8).map(|_| ElementSlot::new()).collect();
        let host = StyloHost::new(&tree, &slots);

        let node = host.node(div);
        assert!(node.is_element());
        assert_eq!(node.local_name(), Some("div"));
        assert_eq!(node.attr("id"), Some("box"));
        assert!(!node.is_root(), "div 非根元素");
        assert!(host.node(html).is_root(), "html 的父是 Document ⇒ 根元素");
        assert!(!host.node(root).is_root(), "Document 自身非根元素");
    }
}
