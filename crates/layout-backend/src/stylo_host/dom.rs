//! `style::dom::{TDocument,TNode,TElement,TShadowRoot}` 投影（`stylo_host::dom`）。
//!
//! 含 `TElement` 的 **5 个必需 `unsafe fn`**（`stylo-0.20.0/dom.rs:648/675/680/739/744`）：
//! 逐方法 `#[allow(unsafe_code)]` + `// SAFETY:`，**函数体零 unsafe**（P-1b 定形）。
//! `opaque()` / `implicit_scope_for_sheet_in_shadow_root` 走 [`crate::opaque`] 侧表（§1.2.1）。
//!
//! 元素句柄 = `&'a ElementSlot<'a>`（8B；见 [`crate::stylo_host`] 模块头）。

use selectors::OpaqueElement;
use selectors::matching::ElementSelectorFlags;
use selectors::sink::Push;
use style::applicable_declarations::ApplicableDeclarationBlock;
use style::context::{QuirksMode, SharedStyleContext};
use style::data::{ElementDataMut, ElementDataRef};
use style::dom::{LayoutIterator, NodeInfo, OpaqueNode, TDocument, TElement, TNode, TShadowRoot};
use style::properties::{AnimationDeclarations, PropertyDeclarationBlock};
use style::selector_parser::{PseudoElement, SelectorImpl};
use style::servo_arc::{Arc, ArcBorrow};
use style::shared_lock::{Locked, SharedRwLock};
use style::stylesheets::scope_rule::ImplicitScopeRoot;
use style::stylist::CascadeData;
use style::values::AtomIdent;
use style::values::computed::Display;
use style::{Atom as WeakAtom, LocalName, Namespace};
use stylo_dom::ElementState;
use web_dom::node::NodeKind;

use crate::opaque;

use super::ElementSlot;

impl NodeInfo for &ElementSlot<'_> {
    fn is_element(&self) -> bool {
        ElementSlot::is_element(self)
    }

    fn is_text_node(&self) -> bool {
        ElementSlot::is_text_node(self)
    }
}

impl<'a> TDocument for &'a ElementSlot<'a> {
    type ConcreteNode = Self;

    fn as_node(&self) -> Self::ConcreteNode {
        *self
    }

    fn is_html_document(&self) -> bool {
        true
    }

    fn quirks_mode(&self) -> QuirksMode {
        QuirksMode::NoQuirks
    }

    fn shared_lock(&self) -> &SharedRwLock {
        &style::global_style_data::GLOBAL_STYLE_DATA.shared_lock
    }
}

impl<'a> TShadowRoot for &'a ElementSlot<'a> {
    type ConcreteNode = Self;

    fn as_node(&self) -> Self::ConcreteNode {
        *self
    }

    fn host(&self) -> <Self::ConcreteNode as TNode>::ConcreteElement {
        let host = self
            .tree()
            .shadow_host(self.node_id())
            .expect("shadow root must have a host");
        self.resolve(host).expect("host slot must exist")
    }

    fn style_data<'b>(&self) -> Option<&'b CascadeData>
    where
        Self: 'b,
    {
        // 影子作用域样式数据留待 S11（`shadow::ShadowSupport`）。
        None
    }
}

impl<'a> TNode for &'a ElementSlot<'a> {
    type ConcreteElement = Self;
    type ConcreteDocument = Self;
    type ConcreteShadowRoot = Self;

    fn parent_node(&self) -> Option<Self> {
        let id = self.tree().parent(self.node_id())?;
        self.resolve(id)
    }

    fn first_child(&self) -> Option<Self> {
        let id = self.tree().children(self.node_id()).next()?;
        self.resolve(id)
    }

    fn last_child(&self) -> Option<Self> {
        let id = self.tree().children(self.node_id()).last()?;
        self.resolve(id)
    }

    fn prev_sibling(&self) -> Option<Self> {
        let id = self
            .tree()
            .get(self.node_id())
            .and_then(|node| node.prev_sibling)?;
        self.resolve(id)
    }

    fn next_sibling(&self) -> Option<Self> {
        let id = self
            .tree()
            .get(self.node_id())
            .and_then(|node| node.next_sibling)?;
        self.resolve(id)
    }

    fn owner_doc(&self) -> Self::ConcreteDocument {
        self.resolve(self.tree().root())
            .expect("root slot must exist")
    }

    fn is_in_document(&self) -> bool {
        true
    }

    fn traversal_parent(&self) -> Option<Self::ConcreteElement> {
        <Self as selectors::Element>::parent_element(self)
    }

    fn opaque(&self) -> OpaqueNode {
        OpaqueNode(usize::try_from(self.node_id().raw()).unwrap_or(usize::MAX))
    }

    fn debug_id(self) -> usize {
        usize::try_from(self.node_id().raw()).unwrap_or(usize::MAX)
    }

    fn as_element(&self) -> Option<Self::ConcreteElement> {
        self.tree()
            .get(self.node_id())
            .is_some_and(|n| n.kind == NodeKind::Element)
            .then_some(*self)
    }

    fn as_document(&self) -> Option<Self::ConcreteDocument> {
        self.tree()
            .get(self.node_id())
            .is_some_and(|n| n.kind == NodeKind::Document)
            .then_some(*self)
    }

    fn as_shadow_root(&self) -> Option<Self::ConcreteShadowRoot> {
        self.tree().is_shadow_root(self.node_id()).then_some(*self)
    }
}

impl<'a> TElement for &'a ElementSlot<'a> {
    type ConcreteNode = Self;
    type TraversalChildrenIterator = std::vec::IntoIter<Self>;

    fn as_node(&self) -> Self::ConcreteNode {
        *self
    }

    fn traversal_children(&self) -> LayoutIterator<Self::TraversalChildrenIterator> {
        let children: Vec<Self> = self
            .tree()
            .children(self.node_id())
            .filter(|id| {
                self.tree()
                    .get(*id)
                    .is_some_and(|n| n.kind == NodeKind::Element)
            })
            .filter_map(|id| self.resolve(id))
            .collect();
        LayoutIterator(children.into_iter())
    }

    fn is_html_element(&self) -> bool {
        self.namespace_str() == Some("html")
    }

    fn is_mathml_element(&self) -> bool {
        self.namespace_str() == Some("mathml")
    }

    fn is_svg_element(&self) -> bool {
        self.namespace_str() == Some("svg")
    }

    fn style_attribute(&self) -> Option<ArcBorrow<'_, Locked<PropertyDeclarationBlock>>> {
        None
    }

    fn animation_rule(
        &self,
        _context: &SharedStyleContext,
    ) -> Option<Arc<Locked<PropertyDeclarationBlock>>> {
        None
    }

    fn transition_rule(
        &self,
        _context: &SharedStyleContext,
    ) -> Option<Arc<Locked<PropertyDeclarationBlock>>> {
        None
    }

    fn animation_declarations(&self, _context: &SharedStyleContext) -> AnimationDeclarations {
        AnimationDeclarations::default()
    }

    fn state(&self) -> ElementState {
        ElementState::empty()
    }

    fn has_part_attr(&self) -> bool {
        false
    }

    fn exports_any_part(&self) -> bool {
        false
    }

    fn id(&self) -> Option<&WeakAtom> {
        None
    }

    fn each_class<F>(&self, mut callback: F)
    where
        F: FnMut(&AtomIdent),
    {
        if let Some(element) = self.element_data() {
            for class in element.class_list() {
                let atom = AtomIdent::from(class);
                callback(&atom);
            }
        }
    }

    fn each_custom_state<F>(&self, _callback: F)
    where
        F: FnMut(&AtomIdent),
    {
    }

    fn each_attr_name<F>(&self, mut callback: F)
    where
        F: FnMut(&LocalName),
    {
        if let Some(element) = self.element_data() {
            for (name, _) in element.attrs.iter() {
                let atom = LocalName::from(name);
                callback(&atom);
            }
        }
    }

    fn has_dirty_descendants(&self) -> bool {
        ElementSlot::has_dirty_descendants(self)
    }

    fn has_snapshot(&self) -> bool {
        false
    }

    fn handled_snapshot(&self) -> bool {
        ElementSlot::snapshot_handled(self)
    }

    // SAFETY: 签名强制 unsafe；函数体零 unsafe（仅置 Cell<bool>，无内存风险；P-1b 定形）。
    #[allow(unsafe_code)]
    unsafe fn set_handled_snapshot(&self) {
        ElementSlot::set_snapshot_handled(self);
    }

    // SAFETY: 签名强制 unsafe；函数体零 unsafe（仅置 Cell<bool>，无内存风险）。
    #[allow(unsafe_code)]
    unsafe fn set_dirty_descendants(&self) {
        ElementSlot::set_dirty_descendants(self);
    }

    // SAFETY: 签名强制 unsafe；函数体零 unsafe（仅清 Cell<bool>，无内存风险）。
    #[allow(unsafe_code)]
    unsafe fn unset_dirty_descendants(&self) {
        ElementSlot::unset_dirty_descendants(self);
    }

    fn store_children_to_process(&self, _n: isize) {}

    fn did_process_child(&self) -> isize {
        0
    }

    // SAFETY: 签名强制 unsafe；函数体零 unsafe（内部互斥由 stylo borrow_mut 保证，不 deref 裸指针）。
    #[allow(unsafe_code)]
    unsafe fn ensure_data(&self) -> ElementDataMut<'_> {
        ElementSlot::ensure_data(self)
    }

    // SAFETY: 签名强制 unsafe；函数体零 unsafe（「清空」= DerefMut 写 ElementData::default()，安全）。
    #[allow(unsafe_code)]
    unsafe fn clear_data(&self) {
        ElementSlot::clear_data(self);
    }

    fn has_data(&self) -> bool {
        true
    }

    fn borrow_data(&self) -> Option<ElementDataRef<'_>> {
        Some(ElementSlot::borrow_data(self))
    }

    fn mutate_data(&self) -> Option<ElementDataMut<'_>> {
        Some(ElementSlot::ensure_data(self))
    }

    fn skip_item_display_fixup(&self) -> bool {
        false
    }

    fn may_have_animations(&self) -> bool {
        false
    }

    fn has_animations(&self, _context: &SharedStyleContext) -> bool {
        false
    }

    fn has_css_animations(
        &self,
        _context: &SharedStyleContext,
        _pseudo: Option<PseudoElement>,
    ) -> bool {
        false
    }

    fn has_css_transitions(
        &self,
        _context: &SharedStyleContext,
        _pseudo: Option<PseudoElement>,
    ) -> bool {
        false
    }

    fn shadow_root(&self) -> Option<<Self::ConcreteNode as TNode>::ConcreteShadowRoot> {
        let root = self.tree().element_shadow(self.node_id())?;
        self.resolve(root)
    }

    fn containing_shadow(&self) -> Option<<Self::ConcreteNode as TNode>::ConcreteShadowRoot> {
        let root = self.tree().containing_shadow_root(self.node_id())?;
        self.resolve(root)
    }

    fn lang_attr(&self) -> Option<style::selector_parser::AttrValue> {
        None
    }

    fn match_element_lang(
        &self,
        _override_lang: Option<Option<style::selector_parser::AttrValue>>,
        _value: &style::selector_parser::Lang,
    ) -> bool {
        false
    }

    fn is_html_document_body_element(&self) -> bool {
        self.name() == Some("body")
    }

    fn synthesize_presentational_hints_for_legacy_attributes<V>(
        &self,
        _visited_handling: selectors::matching::VisitedHandlingMode,
        _hints: &mut V,
    ) where
        V: Push<ApplicableDeclarationBlock>,
    {
    }

    fn local_name(&self) -> &<SelectorImpl as selectors::SelectorImpl>::BorrowedLocalName {
        self.local_name_atom()
    }

    fn namespace(&self) -> &<SelectorImpl as selectors::SelectorImpl>::BorrowedNamespaceUrl {
        self.namespace_atom()
    }

    fn query_container_size(
        &self,
        _display: &Display,
    ) -> euclid::default::Size2D<Option<app_units::Au>> {
        euclid::default::Size2D::new(None, None)
    }

    fn has_selector_flags(&self, _flags: ElementSelectorFlags) -> bool {
        false
    }

    fn relative_selector_search_direction(&self) -> ElementSelectorFlags {
        ElementSelectorFlags::empty()
    }

    fn implicit_scope_for_sheet_in_shadow_root(
        opaque_host: OpaqueElement,
        _sheet_index: usize,
    ) -> Option<ImplicitScopeRoot> {
        // 经线程局部侧表确认 opaque 为合法锚点（§1.2.1 的 opaque 机制步骤），再返回影子作用域根（宿主）。
        opaque::with_side(|side| side.node_of(opaque_host))?;
        Some(ImplicitScopeRoot::ShadowHost(opaque_host))
    }

    fn get_attr(&self, attr: &LocalName, _namespace: &Namespace) -> Option<String> {
        self.attr(attr.as_ref()).map(str::to_owned)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stylo_host::{StyloHost, build_slots};
    use web_dom::node::ShadowMode;
    use web_dom::tree::NodeTree;

    #[test]
    fn dom_projection_and_implicit_scope() {
        let mut tree = NodeTree::new();
        let root = tree.root();
        let host = tree.create_element("x-host", "html", vec![]);
        tree.append_child(root, host);
        let shadow = tree.attach_shadow(host, ShadowMode::Open).unwrap();
        assert!(tree.is_shadow_root(shadow));

        let slots = build_slots(&tree);
        let host_ctx = StyloHost::new(slots);
        let host_ref = host_ctx.node(host).expect("host slot");

        // `TElement::local_name` 走 interned atom 槽。
        assert_eq!(
            <&ElementSlot as TElement>::local_name(&host_ref),
            &web_atoms::LocalName::from("x-host")
        );
        assert!(TElement::is_html_element(&host_ref));
        assert!(TElement::shadow_root(&host_ref).is_some());

        // `opaque()` → `implicit_scope_for_sheet_in_shadow_root`（无 self 关联函数）经侧表反查（§1.2.1）。
        crate::opaque::reset();
        crate::opaque::with_side(|side| side.set_active_tree(1));
        let opaque_host = <&ElementSlot as selectors::Element>::opaque(&host_ref);
        let scope =
            <&ElementSlot as TElement>::implicit_scope_for_sheet_in_shadow_root(opaque_host, 0);
        assert!(
            matches!(scope, Some(ImplicitScopeRoot::ShadowHost(_))),
            "影子表的作用域根应为宿主（经 opaque 侧表反查）"
        );
    }
}
