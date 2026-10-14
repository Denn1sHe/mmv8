//! `stylo_host` —— **stylo FFI 薄桥**：把 `web-dom::NodeTree` **只读**投影成 stylo 消费的类型
//! （施工图 §1.2 / §2.1 / §6 R18；ADR-0024 目录级白名单 `crates/layout-backend/src/stylo_host/`）。
//!
//! ## 元素句柄 = `&'a ElementSlot<'a>`（8 字节）
//! stylo 的样式共享缓存断言 `size_of::<SharingCache<E>>() == size_of::<TypelessSharingCache>()`
//! （`stylo-0.20.0/sharing/mod.rs:605`）⇒ **`TElement` 类型 `E` 必须指针大小（8B）**。
//! 故元素 = **对槽的引用** `NodeRef<'a> = &'a ElementSlot<'a>`（同 blitz 的 `&'a Node`）。
//!
//! **安全两阶段构造**（[`build_slots`]）：先建 `Vec<ElementSlot>` → `Box::leak` 得**稳定锚** →
//! 用 `Cell` 内层可变性把「槽数组自引用」回填（**全共享借用，零 unsafe**）。
//! **一次性泄漏、跨 resolve 复用、重建即重分配**（登记；G5 前如需回收另立优化项）。
//!
//! `opaque()` 走线程局部侧表（[`crate::opaque`]，§1.2.1；**零 unsafe**）。

pub mod dom;

use std::cell::Cell;

use crate::font_metrics::FontChain;
use euclid::{Scale, Size2D};
use selectors::attr::{AttrSelectorOperation, CaseSensitivity, NamespaceConstraint};
use selectors::bloom::BloomFilter;
use selectors::matching::{ElementSelectorFlags, MatchingContext};
use selectors::{Element as SelectorsElement, OpaqueElement};
use std::sync::OnceLock;
use style::context::QuirksMode;
use style::data::{ElementData, ElementDataMut, ElementDataRef, ElementDataWrapper};
use style::device::Device;
use style::media_queries::MediaList;
use style::media_queries::MediaType;
use style::properties::ComputedValues;
use style::properties::style_structs::Font;
use style::queries::values::PrefersColorScheme;
use style::selector_parser::SelectorImpl;
use style::servo::media_features::PointerCapabilities;
use style::servo_arc::Arc as ServoArc;
use style::stylesheet_set::DocumentStylesheetSet;
use style::stylesheets::{
    AllowImportRules, CustomMediaMap, DocumentStyleSheet, Origin, Stylesheet,
};
use style::stylist::{CascadeData, CascadeDataDifference, StylistSheet};
use style_traits::{CSSPixel, DevicePixel};
use web_dom::node::{ElementData as DomElementData, NodeId, NodeKind};
use web_dom::tree::NodeTree;

use crate::opaque;

/// 进程级**唯一**共享锁（＝ `TDocument::shared_lock()` 同一把；`Locked<…>` 与 `StylesheetGuards`
/// 必须同源，否则 stylo `read_with` 断言 —— R21：统一为 `GLOBAL_STYLE_DATA.shared_lock`，不再自建）。
#[must_use]
pub fn global_shared_lock() -> &'static style::shared_lock::SharedRwLock {
    &style::global_style_data::GLOBAL_STYLE_DATA.shared_lock
}

/// 进程级共享 `Device`（影子级 `CascadeData::rebuild` 需要）。
///
/// **具名偏差（P-11 登记）**：此处用固定 800×600 视口构造（媒体查询按此判定）；主样式系统仍用
/// 真实视口。影子表内媒体查询的视口判定**暂不精确** ⇒ 后续将真实 `Device` 贯穿 `build_slots` 时消除。
#[must_use]
pub fn global_device() -> &'static Device {
    static DEVICE: OnceLock<&'static Device> = OnceLock::new();
    DEVICE.get_or_init(|| {
        let default_values: style::servo_arc::Arc<ComputedValues> =
            ComputedValues::initial_values_with_font_override(Font::initial_values());
        Box::leak(Box::new(Device::new(
            MediaType::screen(),
            QuirksMode::NoQuirks,
            Size2D::<f32, CSSPixel>::new(800.0, 600.0),
            Size2D::<f32, DevicePixel>::new(800.0, 600.0),
            Scale::<f32, CSSPixel, DevicePixel>::new(1.0),
            Box::new(FontChain::new()),
            default_values,
            PrefersColorScheme::Light,
            PointerCapabilities::empty(),
            PointerCapabilities::empty(),
        )))
    })
}

/// 收集某子树内 `<style>` 元素的文本（影子根级作者表来源）。
fn collect_style_texts(tree: &NodeTree, node: NodeId, out: &mut Vec<String>) {
    for child in tree.children(node) {
        let is_style = tree
            .get(child)
            .and_then(|data| data.element.as_ref())
            .is_some_and(|element| element.name == "style");
        if is_style {
            let text = tree.text_content(child);
            if !text.is_empty() {
                out.push(text);
            }
        }
        collect_style_texts(tree, child, out);
    }
}

/// 为单个影子根构建 `CascadeData`（**全公开 API，零补丁**）：`:host`/`::slotted` 的真匹配通道。
fn build_shadow_cascade(
    texts: &[String],
    lock: &style::shared_lock::SharedRwLock,
    base: &style::stylesheets::UrlExtraData,
) -> Option<&'static CascadeData> {
    let device = global_device();
    let mut set: DocumentStylesheetSet<StylistSheet> = DocumentStylesheetSet::new();
    let guard = lock.read();
    let custom_media = CustomMediaMap::default();
    for css in texts {
        let media = ServoArc::new(lock.wrap(MediaList::empty()));
        let sheet = Stylesheet::from_str(
            css,
            base.clone(),
            Origin::Author,
            media,
            lock.clone(),
            None,
            None,
            QuirksMode::NoQuirks,
            AllowImportRules::Yes,
        );
        set.append_stylesheet(
            Some(device),
            &custom_media,
            DocumentStyleSheet(ServoArc::new(sheet)),
            &guard,
        );
    }
    let mut cascade = Box::new(CascadeData::new());
    let mut difference = CascadeDataDifference::default();
    let (mut flusher, _invalidation) = set.flush();
    let origin_flusher = flusher.flush_origin(Origin::Author);
    cascade
        .rebuild(
            device,
            QuirksMode::NoQuirks,
            origin_flusher,
            &guard,
            &mut difference,
        )
        .ok()?;
    Some(Box::leak(cascade))
}

/// 元素句柄：**对槽的引用**（8 字节；见模块头的不变量说明）。
pub type NodeRef<'a> = &'a ElementSlot<'a>;

/// 每节点槽：stylo 元素数据 + 归属（`tree`/`id`）+ **自引用槽数组**（导航用）。
///
/// - **急切持有** `ElementDataWrapper`（P-1b 定形：`ensure_data`/`clear_data` 函数体零 unsafe）；
/// - `slots` 经 `Cell` 回填（安全两阶段构造）；父/子/兄弟导航据此解析。
pub struct ElementSlot<'a> {
    tree: &'a NodeTree,
    id: NodeId,
    slots: Cell<Option<&'a [ElementSlot<'a>]>>,
    data: ElementDataWrapper,
    snapshot_handled: Cell<bool>,
    dirty_descendants: Cell<bool>,
    local_name: web_atoms::LocalName,
    namespace: web_atoms::Namespace,
    id_atom: Option<style::Atom>,
    style_attribute: Option<
        style::servo_arc::Arc<
            style::shared_lock::Locked<style::properties::PropertyDeclarationBlock>,
        >,
    >,
    /// **影子根级 `CascadeData`**（P-11 / R22）：仅影子根槽为 `Some`；
    /// `Box::leak` 同寿（随槽数组一次性泄漏，与 R18 泄漏模型一致）；
    /// **不改槽地址/`'a`**（仅改 `size_of::<ElementSlot>`）。
    shadow_cascade: Cell<Option<&'static CascadeData>>,
}

impl std::fmt::Debug for ElementSlot<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ElementSlot")
            .field("id", &self.id)
            .field("local_name", &format_args!("{}", self.local_name))
            .finish_non_exhaustive()
    }
}

impl PartialEq for ElementSlot<'_> {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self.tree, other.tree) && self.id == other.id
    }
}

impl Eq for ElementSlot<'_> {}

impl std::hash::Hash for ElementSlot<'_> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

impl<'a> ElementSlot<'a> {
    /// 归属树。
    #[must_use]
    pub const fn tree(&self) -> &'a NodeTree {
        self.tree
    }

    /// 节点身份。
    #[must_use]
    pub const fn node_id(&self) -> NodeId {
        self.id
    }

    /// 槽数组（构造期回填；导航用）。
    ///
    /// # Panics
    /// 槽未回填（未经 [`build_slots`] 构造）时 panic。
    #[must_use]
    pub fn slots(&self) -> &'a [ElementSlot<'a>] {
        self.slots
            .get()
            .expect("stylo_host: slot array must be filled after build_slots")
    }

    /// 按 `NodeId` 取同树槽（越界 ⇒ `None`）。
    #[must_use]
    pub fn resolve(&self, id: NodeId) -> Option<NodeRef<'a>> {
        self.slots().get(id.index() as usize)
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
    pub fn name(&self) -> Option<&'a str> {
        self.element_data().map(|element| element.name.as_str())
    }

    /// 命名空间。
    #[must_use]
    pub fn namespace_str(&self) -> Option<&'a str> {
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
    pub fn is_slot_element(&self) -> bool {
        self.name() == Some("slot")
    }

    /// interned 标签名（`TElement::local_name` 的返回）。
    #[must_use]
    pub fn local_name_atom(&self) -> &web_atoms::LocalName {
        &self.local_name
    }

    /// interned 命名空间。
    #[must_use]
    pub fn namespace_atom(&self) -> &web_atoms::Namespace {
        &self.namespace
    }

    /// interned `id` 原子（`TElement::id` 的返回；无 `id` ⇒ `None`）。
    #[must_use]
    pub fn id_atom(&self) -> Option<&style::Atom> {
        self.id_atom.as_ref()
    }

    /// 内联 `style` 属性块（`style="…"` 解析结果；无 ⇒ `None`）。
    #[must_use]
    pub fn style_attribute(
        &self,
    ) -> Option<
        &style::servo_arc::Arc<
            style::shared_lock::Locked<style::properties::PropertyDeclarationBlock>,
        >,
    > {
        self.style_attribute.as_ref()
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

    /// 是否已持**有效主样式**（R23·I1：`has_data`/`borrow_data`/`clear_data` 一致，
    /// 均由 `styles.primary.is_some()` 派生）。
    #[must_use]
    pub fn has_styles(&self) -> bool {
        // `G3X-S3c1`（⑦）**结构约束**：`stylo::data::ElementDataWrapper` **仅 `borrow`/`borrow_mut`，无 `try_borrow`**
        // （stylo-0.20.0/data.rs:332/344）⇒ 桥内无法"可判定错误"化；**登记为上游 API 约束**（非"豁免"）。
        self.data.borrow().has_styles()
    }

    /// 清空数据（覆盖为 `ElementData::default()`；`DerefMut` 安全写）。
    pub fn clear_data(&self) {
        *self.data.borrow_mut() = ElementData::default();
    }

    /// 置「已处理快照」位。
    pub fn set_snapshot_handled(&self) {
        self.snapshot_handled.set(true);
    }

    /// 「已处理快照」位。
    #[must_use]
    pub fn snapshot_handled(&self) -> bool {
        self.snapshot_handled.get()
    }

    /// 置「有脏后代」位。
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

/// 桥：持**稳定锚**槽数组（[`build_slots`] 产出）。
#[derive(Debug)]
pub struct StyloHost<'a> {
    slots: &'a [ElementSlot<'a>],
}

impl<'a> StyloHost<'a> {
    /// 由槽数组构造桥。
    #[must_use]
    pub const fn new(slots: &'a [ElementSlot<'a>]) -> Self {
        Self { slots }
    }

    /// 取一个节点句柄（越界 ⇒ `None`）。
    #[must_use]
    pub fn node(&self, id: NodeId) -> Option<NodeRef<'a>> {
        self.slots.get(id.index() as usize)
    }
}

/// **安全两阶段**构建槽数组（`Box::leak` 得稳定锚 → `Cell` 自填自引用）。
///
/// **前提**：一次 `resolve` 内不 `dispose`（`NodeId::generation` 保持 0）；槽位按 `NodeId::index()` 对齐。
/// **泄漏登记**：每 host 一次性（跨 resolve 复用；重建即重分配）；G5 前如需回收另立优化项。
///
/// # Panics
/// 槽数组构造中若 `about:blank` URL 解析失败即 panic（内建常量，不应发生）。
#[must_use]
pub fn build_slots<'a>(
    tree: &'a NodeTree,
    lock: &style::shared_lock::SharedRwLock,
) -> &'a [ElementSlot<'a>] {
    let len = tree.len();
    let mut slots: Vec<ElementSlot<'_>> = Vec::with_capacity(len);
    for index in 0..len {
        let Ok(index_u32) = u32::try_from(index) else {
            break;
        };
        let id = NodeId::new(index_u32, 0);
        let element = tree.get(id).and_then(|node| node.element.as_ref());
        let id_atom = element
            .and_then(|element| element.attrs.get("id"))
            .map(style::Atom::from);
        let style_attribute = element
            .and_then(|element| element.attrs.get("style"))
            .map(|text| {
                let block = style::properties::parse_style_attribute(
                    text,
                    &style::stylesheets::UrlExtraData::from(
                        url::Url::parse("about:blank").expect("about:blank"),
                    ),
                    None,
                    style::context::QuirksMode::NoQuirks,
                    style::stylesheets::CssRuleType::Style,
                );
                style::servo_arc::Arc::new(lock.wrap(block))
            });
        let (local_name, namespace) = element.map_or_else(
            || {
                (
                    web_atoms::LocalName::from(""),
                    web_atoms::Namespace::from(""),
                )
            },
            |element| {
                (
                    web_atoms::LocalName::from(element.name.as_str()),
                    web_atoms::Namespace::from(element.namespace.as_str()),
                )
            },
        );
        slots.push(ElementSlot {
            tree,
            id,
            slots: Cell::new(None),
            data: ElementDataWrapper::default(),
            snapshot_handled: Cell::new(false),
            dirty_descendants: Cell::new(false),
            local_name,
            namespace,
            id_atom,
            style_attribute,
            shadow_cascade: Cell::new(None),
        });
    }

    let leaked: &mut [ElementSlot<'_>] = Box::leak(slots.into_boxed_slice());
    let slots: &[ElementSlot<'_>] = leaked;
    for slot in slots {
        slot.slots.set(Some(slots));
    }
    // P-11 / R22：为每个影子根装配**影子级 `CascadeData`**（`:host` / `::slotted` 真匹配通道）。
    if !tree.is_shadow_root(tree.root()) {
        let base = crate::style_system::base_url_data();
        let mut index: u32 = 0;
        while let Ok(len_u32) = u32::try_from(tree.len()) {
            if index >= len_u32 {
                break;
            }
            let id = NodeId::new(index, 0);
            index += 1;
            if !tree.is_shadow_root(id) {
                continue;
            }
            let mut texts = Vec::new();
            collect_style_texts(tree, id, &mut texts);
            if texts.is_empty() {
                continue;
            }
            if let Some(slot) = slots.get(id.index() as usize)
                && let Some(cascade) = build_shadow_cascade(&texts, lock, &base)
            {
                slot.shadow_cascade.set(Some(cascade));
            }
        }
    }
    slots
}

impl<'a> SelectorsElement for &'a ElementSlot<'a> {
    type Impl = SelectorImpl;

    fn opaque(&self) -> OpaqueElement {
        // §1.2.1：经线程局部侧表取稳定锚（零 unsafe）。
        opaque::with_side(|side| side.opaque_for(self.id))
    }

    fn parent_element(&self) -> Option<Self> {
        let parent = self.tree.parent(self.id)?;
        self.resolve(parent).filter(|slot| slot.is_element())
    }

    fn parent_node_is_shadow_root(&self) -> bool {
        self.tree
            .parent(self.id)
            .is_some_and(|parent| self.tree.is_shadow_root(parent))
    }

    fn containing_shadow_host(&self) -> Option<Self> {
        let root = self.tree.containing_shadow_root(self.id)?;
        let host = self.tree.shadow_host(root)?;
        self.resolve(host)
    }

    fn is_pseudo_element(&self) -> bool {
        false
    }

    fn prev_sibling_element(&self) -> Option<Self> {
        let mut prev = self.tree.get(self.id).and_then(|node| node.prev_sibling);
        while let Some(id) = prev {
            let node = self.tree.get(id)?;
            if node.kind == NodeKind::Element {
                return self.resolve(id);
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
                return self.resolve(id);
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
            .and_then(|child| self.resolve(child))
    }

    fn is_html_element_in_html_document(&self) -> bool {
        self.namespace_str() == Some("html")
    }

    fn has_local_name(
        &self,
        local_name: &<Self::Impl as selectors::SelectorImpl>::BorrowedLocalName,
    ) -> bool {
        self.name() == Some(local_name.as_ref())
    }

    fn has_namespace(
        &self,
        ns: &<Self::Impl as selectors::SelectorImpl>::BorrowedNamespaceUrl,
    ) -> bool {
        self.namespace_str() == Some(ns.as_ref())
    }

    fn is_same_type(&self, other: &Self) -> bool {
        self.name() == other.name() && self.namespace_str() == other.namespace_str()
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
        self.is_slot_element()
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
                return self.resolve(slot);
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

    fn add_element_unique_hashes(&self, filter: &mut BloomFilter) -> bool {
        style::bloom::each_relevant_element_hash(*self, |hash| {
            filter.insert_hash(hash & selectors::bloom::BLOOM_HASH_MASK);
        });
        true
    }
}

#[cfg(test)]
mod tests {
    use super::{NodeRef, StyloHost, build_slots};
    use selectors::Element as _;
    use web_dom::tree::NodeTree;

    #[test]
    fn single_shared_lock_identity() {
        use style::dom::TDocument;
        let tree = NodeTree::new();
        let root = tree.root();
        let slots = build_slots(&tree, super::global_shared_lock());
        let host = StyloHost::new(slots);
        let node = host.node(root).expect("root slot");
        assert!(
            std::ptr::eq(super::global_shared_lock(), TDocument::shared_lock(&node)),
            "全局锁必须与 TDocument::shared_lock() 同一把（R21 防分叉）"
        );
    }

    #[test]
    fn element_handle_is_pointer_sized() {
        // stylo 共享缓存不变量：E 必须 8 字节。
        assert_eq!(
            std::mem::size_of::<NodeRef<'_>>(),
            std::mem::size_of::<usize>(),
            "TElement 句柄必须指针大小"
        );
    }

    #[test]
    fn host_projects_elements_and_navigates() {
        let mut tree = NodeTree::new();
        let root = tree.root();
        let html = tree.create_element("html", "html", vec![]);
        let body = tree.create_element("body", "html", vec![]);
        let div = tree.create_element("div", "html", vec![("id".to_owned(), "box".to_owned())]);
        tree.append_child(root, html);
        tree.append_child(html, body);
        tree.append_child(body, div);

        let slots = build_slots(&tree, crate::stylo_host::global_shared_lock());
        let host = StyloHost::new(slots);

        let node = host.node(div).expect("div slot");
        assert!(node.is_element());
        assert_eq!(node.name(), Some("div"));
        assert_eq!(node.attr("id"), Some("box"));
        // 导航完整：父 = body。
        assert_eq!(
            node.parent_element().map(super::ElementSlot::name),
            Some(Some("body"))
        );
        assert!(!node.is_root());
        assert!(host.node(html).expect("html slot").is_root());
    }
}
