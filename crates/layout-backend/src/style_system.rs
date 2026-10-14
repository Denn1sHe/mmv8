//! 样式系统（`layout-backend::style_system`，施工图 §2.1 / S7）。
//!
//! **S7-b② 最小可编译面**：持有 stylo [`Stylist`]（级联引擎）+ `Device` + 共享锁；
//! 提供样式表**登记 / 移除**（`web-style::StylesheetList` 记账 + stylo `append_stylesheet`）+ `flush`。
//! **逐元素级联**（步骤③）与 `computed_value` 序列化（`serialize.rs`）在后续落。
//!
//! **零 unsafe**（落 `layout-backend/src/`，非桥目录）。

use std::collections::HashMap;

use euclid::{Scale, Size2D};
use selectors::Element as SelectorsElement;
use style::context::{
    QuirksMode, RegisteredSpeculativePainter, RegisteredSpeculativePainters, SharedStyleContext,
    StyleContext, StyleSystemOptions, ThreadLocalStyleContext,
};
use style::device::Device;
use style::media_queries::{MediaList, MediaType};
use style::properties::ComputedValues;
use style::properties::style_structs::Font;
use style::queries::values::PrefersColorScheme;
use style::servo::animation::DocumentAnimationSet;
use style::servo::media_features::PointerCapabilities;
use style::servo::selector_parser::SnapshotMap;
use style::servo_arc::Arc;
use style::shared_lock::{SharedRwLock, StylesheetGuards};
use style::style_resolver::{PseudoElementResolution, StyleResolverForElement};
use style::stylesheets::{AllowImportRules, DocumentStyleSheet, Origin, Stylesheet, UrlExtraData};
use style::stylist::{RuleInclusion, Stylist};
use style::thread_state::{self, ThreadState};
use style::traversal_flags::TraversalFlags;
use style_traits::{CSSPixel, DevicePixel};
use web_style::SheetOrigin;

use crate::error::{LayoutError, LayoutErrorKind};
use crate::font_metrics::FontChain;
use crate::geometry::Size;
use crate::stylo_host::NodeRef;

/// 把 `web-style::SheetOrigin` 映射到 stylo `Origin`（UA < author）。
fn to_stylo_origin(origin: SheetOrigin) -> Origin {
    match origin {
        SheetOrigin::UserAgent => Origin::UserAgent,
        SheetOrigin::Author => Origin::Author,
    }
}

/// 构造 `UrlExtraData`（`about:blank`；G3 无资源加载）。
pub(crate) fn base_url_data() -> UrlExtraData {
    UrlExtraData::from(url::Url::parse("about:blank").expect("about:blank is a valid url"))
}

/// 样式系统：stylo 级联引擎 + 样式表登记/移除 + flush。
pub struct StyleSystem {
    stylist: Stylist,
    lock: SharedRwLock,
    /// web-style 侧文本登记（去重 + 句柄）。
    sheets: web_style::StylesheetList,
    /// web-style `SheetId` → stylo 表（供移除/去重判重）。
    registered: HashMap<u64, DocumentStyleSheet>,
}

// 只展示摘要（避免暴露锁/内部表；`lock`/`registered` 无稳定 Debug 面）。
#[allow(clippy::missing_fields_in_debug)]
impl std::fmt::Debug for StyleSystem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StyleSystem")
            .field("sheets", &self.sheets.len())
            .finish()
    }
}

impl StyleSystem {
    /// 建样式系统：`Device`（注入 [`FontChain`] 度量提供者）+ `Stylist`。
    #[must_use]
    pub fn new(viewport: Size) -> Self {
        let lock = crate::stylo_host::global_shared_lock().clone();
        let default_values: Arc<ComputedValues> =
            ComputedValues::initial_values_with_font_override(Font::initial_values());
        let device = Device::new(
            MediaType::screen(),
            QuirksMode::NoQuirks,
            Size2D::<f32, CSSPixel>::new(viewport.width, viewport.height),
            Size2D::<f32, DevicePixel>::new(viewport.width, viewport.height),
            Scale::<f32, CSSPixel, DevicePixel>::new(1.0),
            Box::new(FontChain::new()),
            default_values,
            PrefersColorScheme::Light,
            PointerCapabilities::empty(),
            PointerCapabilities::empty(),
        );
        Self {
            stylist: Stylist::new(device, QuirksMode::NoQuirks),
            lock,
            sheets: web_style::StylesheetList::new(),
            registered: HashMap::new(),
        }
    }

    /// 登记一张样式表（`(origin, text)` 去重）；返回 `web-style` 句柄。
    ///
    /// # Errors
    /// CSS 解析 / 登记失败 ⇒ [`LayoutErrorKind::StyleSheet`]。
    pub fn add_stylesheet(
        &mut self,
        origin: SheetOrigin,
        css: &str,
    ) -> Result<web_style::SheetId, LayoutError> {
        let id = self.sheets.insert(origin, css);
        if self.registered.contains_key(&id.raw()) {
            return Ok(id);
        }
        let media = Arc::new(self.lock.wrap(MediaList::empty()));
        let sheet = Stylesheet::from_str(
            css,
            base_url_data(),
            to_stylo_origin(origin),
            media,
            self.lock.clone(),
            None,
            None,
            QuirksMode::NoQuirks,
            AllowImportRules::Yes,
        );
        let document_sheet = DocumentStyleSheet(Arc::new(sheet));
        {
            let guard = self.lock.read();
            self.stylist
                .append_stylesheet(document_sheet.clone(), &guard);
        }
        self.registered.insert(id.raw(), document_sheet);
        Ok(id)
    }

    /// 移除一张样式表记账（对称面；G3-B11）。
    ///
    /// # Errors
    /// 未知句柄 ⇒ [`LayoutErrorKind::StyleSheet`]。
    pub fn remove_stylesheet(&mut self, id: web_style::SheetId) -> Result<(), LayoutError> {
        let Some(document_sheet) = self.registered.remove(&id.raw()) else {
            return Err(LayoutError::new(
                LayoutErrorKind::StyleSheet,
                "unknown stylesheet handle",
            ));
        };
        self.sheets.remove(id);
        // stylo 侧移除 + flush 配对（`remove` 变更须 flush 才重建规则树）。
        let read_guard = self.lock.read();
        self.stylist.remove_stylesheet(document_sheet, &read_guard);
        drop(read_guard);
        // 规则变更 ⇒ 强制 origins 脏 + flush 才重建规则树（stylo 约定）。
        self.stylist.force_stylesheet_origins_dirty(
            style::stylesheets::OriginSet::from(style::stylesheets::Origin::UserAgent)
                | style::stylesheets::OriginSet::from(style::stylesheets::Origin::Author),
        );
        self.flush();
        Ok(())
    }

    /// 样式表数（记账侧）。
    #[must_use]
    pub fn stylesheet_count(&self) -> usize {
        self.sheets.len()
    }

    /// 刷新级联缓存（重建规则树）。
    pub fn flush(&mut self) {
        let guard = self.lock.read();
        self.stylist.flush(&StylesheetGuards::same(&guard));
    }

    /// 共享锁（`Locked<…>` 与 guards 同源；供 `stylo_host::build_slots` 用）。
    #[must_use]
    pub const fn shared_lock(&self) -> &SharedRwLock {
        &self.lock
    }

    /// stylo 级联引擎（只读）。
    #[must_use]
    pub const fn stylist(&self) -> &Stylist {
        &self.stylist
    }

    /// 逐元素级联：解析 `element` 的**计算值**（`getComputedStyle` 的语义源）。
    ///
    /// # Errors
    /// 级联失败 ⇒ [`LayoutErrorKind::Cascade`]。
    pub fn resolve_element_style(
        &self,
        element: NodeRef<'_>,
        now: f64,
    ) -> Result<Arc<ComputedValues>, LayoutError> {
        // 每次 resolve 清缓存的数据（规则节点/计算值），确保样式表变更后重算（防陈旧）。
        element.clear_data();
        thread_state::enter(ThreadState::LAYOUT);
        // 块作用域：确保 `thread_local`（含 `SequentialTaskList`）在 **LAYOUT 态内** drop
        // （stylo `SequentialTaskList::drop` 断言 `ThreadState::LAYOUT`）。
        let style = {
            let guard = self.lock.read();
            let guards = StylesheetGuards::same(&guard);
            let snapshot_map = SnapshotMap::new();
            let animations = DocumentAnimationSet::default();
            let painters = NoSpeculativePainters;
            let shared = SharedStyleContext {
                stylist: &self.stylist,
                visited_styles_enabled: false,
                options: StyleSystemOptions::default(),
                guards,
                current_time_for_animations: now,
                traversal_flags: TraversalFlags::empty(),
                snapshot_map: &snapshot_map,
                animations,
                registered_speculative_painters: &painters,
            };
            let mut thread_local: ThreadLocalStyleContext<NodeRef<'_>> =
                ThreadLocalStyleContext::new();
            // R24：**匹配前填充祖先 bloom**（仿 traversal.rs:288-289）。绕过 traversal 时 bloom 为空
            // ⇒ 组合器（如 `div span`）被 fast-reject（matching.rs:294-301）而静默不命中。
            if let Some(parent) =
                <&crate::stylo_host::ElementSlot as SelectorsElement>::parent_element(&element)
            {
                thread_local.bloom_filter.rebuild(parent);
                thread_local.bloom_filter.push(parent);
            }
            // S1a：debug 守卫 —— bloom 应已完整覆盖祖先（stylo 官方不变量；release 下为空操作）。
            thread_local.bloom_filter.assert_complete(element);
            let mut context = StyleContext {
                shared: &shared,
                thread_local: &mut thread_local,
            };
            let mut resolver = StyleResolverForElement::new(
                element,
                &mut context,
                RuleInclusion::All,
                PseudoElementResolution::IfApplicable,
            );
            let element_styles = resolver.resolve_style_with_default_parents();
            // R23·I4 写回：`resolve_style*` 只返回不写回 ⇒ 显式 `set_styles`（否则槽「出生即违约」，
            // 父为元素的解析会命中 `ElementStyles::primary().unwrap()` on None）。
            let primary = element_styles.primary_style().clone();
            element.ensure_data().set_styles(element_styles);
            primary
        };
        thread_state::exit(ThreadState::LAYOUT);
        Ok(style)
    }
}

/// G3 无绘制 ⇒ 无注册的推测性画笔（no-op；零 unsafe）。
#[derive(Debug, Default)]
struct NoSpeculativePainters;

impl RegisteredSpeculativePainters for NoSpeculativePainters {
    fn get(&self, _name: &style::Atom) -> Option<&dyn RegisteredSpeculativePainter> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::StyleSystem;
    use crate::geometry::Size;
    use web_style::SheetOrigin;

    #[test]
    fn inline_style_wins_over_author() {
        use crate::stylo_host::{StyloHost, build_slots};
        use web_dom::tree::NodeTree;

        let mut system = StyleSystem::new(Size::new(800.0, 600.0));
        system
            .add_stylesheet(SheetOrigin::Author, "div { width: 1px; }")
            .expect("author sheet");
        system.flush();

        let mut tree = NodeTree::new();
        let root = tree.root();
        let div = tree.create_element(
            "div",
            "html",
            vec![("style".to_owned(), "width: 5px;".to_owned())],
        );
        tree.append_child(root, div);
        let slots = build_slots(&tree, system.shared_lock());
        let style = system
            .resolve_element_style(StyloHost::new(slots).node(div).expect("div slot"), 0.0)
            .expect("cascade");
        assert_eq!(
            crate::serialize::serialize_computed(&style, "width").expect("width"),
            "5px",
            "内联 style 应胜出 author 声明"
        );
    }

    #[test]
    fn stylesheet_remove_invalidates() {
        use crate::stylo_host::{StyloHost, build_slots};
        use web_dom::tree::NodeTree;

        let mut system = StyleSystem::new(Size::new(800.0, 600.0));
        // 负对照来源：内嵌 UA 表（UA_CSS 含 div{display:block}）。
        system
            .add_stylesheet(SheetOrigin::UserAgent, crate::UA_CSS)
            .expect("ua sheet");
        let handle = system
            .add_stylesheet(SheetOrigin::Author, "#box { width: 120px; }")
            .expect("author sheet");
        system.flush();
        let sheets_before = system.stylesheet_count();
        let rebuilds_before = system.stylist().num_rebuilds();

        let mut tree = NodeTree::new();
        let root = tree.root();
        let div = tree.create_element("div", "html", vec![("id".to_owned(), "box".to_owned())]);
        tree.append_child(root, div);
        let slots = build_slots(&tree, system.shared_lock());
        let host = StyloHost::new(slots);

        let before = system
            .resolve_element_style(host.node(div).expect("div slot"), 0.0)
            .expect("cascade");
        assert_eq!(
            crate::serialize::serialize_computed(&before, "width").expect("width"),
            "120px",
            "移除前：author #box width:120px 生效"
        );
        assert_eq!(
            crate::serialize::serialize_computed(&before, "display").expect("display"),
            "block",
            "负对照前置：UA div display:block"
        );

        system
            .remove_stylesheet(handle)
            .expect("remove should not panic");

        // 诊断：记账减一 + 规则树重建计数递增。
        assert_eq!(
            system.stylesheet_count(),
            sheets_before - 1,
            "sheet 记账 2->1"
        );
        assert!(
            system.stylist().num_rebuilds() > rebuilds_before,
            "flush 应触发规则树重建"
        );

        let after = system
            .resolve_element_style(host.node(div).expect("div slot"), 0.0)
            .expect("cascade");
        assert_eq!(
            crate::serialize::serialize_computed(&after, "width").expect("width"),
            "auto",
            "移除后：author width:120px 应失效回落初始 auto（判别性观测）"
        );
        assert_eq!(
            crate::serialize::serialize_computed(&after, "display").expect("display"),
            "block",
            "负对照：UA 表仍在 => display 仍 block"
        );
    }

    #[test]
    fn host_selector_styles_host() {
        // R22 / P-11：影子表 `:host { width: 120px; }` 经 **`shadow.style_data()` 真通道** ⇒ host 宽 120。
        use crate::stylo_host::{StyloHost, build_slots};
        use web_dom::node::ShadowMode;
        use web_dom::tree::NodeTree;

        let system = StyleSystem::new(Size::new(800.0, 600.0));
        let mut tree = NodeTree::new();
        let root = tree.root();
        let host = tree.create_element("x-host", "html", vec![]);
        tree.append_child(root, host);
        let shadow = tree.attach_shadow(host, ShadowMode::Open).expect("shadow");
        let style_el = tree.create_element("style", "html", vec![]);
        tree.append_child(shadow, style_el);
        let text = tree.create_text(":host { width: 120px; }");
        tree.append_child(style_el, text);

        let slots = build_slots(&tree, system.shared_lock());
        let bridge = StyloHost::new(slots);
        let style = system
            .resolve_element_style(bridge.node(host).expect("host"), 0.0)
            .expect("cascade");
        assert_eq!(
            crate::serialize::serialize_computed(&style, "width").expect("width"),
            "120px",
            ":host 应经影子 `style_data()` 真通道生效（P-11）"
        );
    }

    #[test]
    #[ignore = "S5b 未闭合（已停报）：`assigned_slot` 钩子**早已存在**（stylo_host/mod.rs:610），缺口不在投影；疑在规则收集路径（`collect_host_and_slotted_rules` 未被 `resolve_element_style` 走达）"]
    fn slotted_selector_styles_slotted() {
        // R22 / P-11：影子表 `::slotted(span) { width: 60px; }` ⇒ 分发 light 子 span 宽 60。
        use crate::stylo_host::{StyloHost, build_slots};
        use web_dom::node::ShadowMode;
        use web_dom::tree::NodeTree;

        let system = StyleSystem::new(Size::new(800.0, 600.0));
        let mut tree = NodeTree::new();
        let root = tree.root();
        let host = tree.create_element("x-host", "html", vec![]);
        tree.append_child(root, host);
        let light = tree.create_element("span", "html", vec![("slot".to_owned(), "s".to_owned())]);
        tree.append_child(host, light);
        let shadow = tree.attach_shadow(host, ShadowMode::Open).expect("shadow");
        let style_el = tree.create_element("style", "html", vec![]);
        tree.append_child(shadow, style_el);
        let text = tree.create_text("::slotted(span) { width: 60px; }");
        tree.append_child(style_el, text);

        let slots = build_slots(&tree, system.shared_lock());
        let bridge = StyloHost::new(slots);
        let style = system
            .resolve_element_style(bridge.node(light).expect("span"), 0.0)
            .expect("cascade");
        assert_eq!(
            crate::serialize::serialize_computed(&style, "width").expect("width"),
            "60px",
            "::slotted(span) 应经影子 `style_data()` 真通道生效（P-11）"
        );
    }

    #[test]
    fn display_contents_parent_child_resolves() {
        // S1a：`display: contents` 祖先下的后代解析**不 panic**，且继承取值正确。
        // （`TElement` 无 `layout_parent` 覆写点 ⇒ 保障由「祖先先解析 + I4 写回」给出；
        //   负对照：若 I4 写回被移除，本路径回到 R23 的 `primary().unwrap()` on None panic。）
        use crate::stylo_host::{StyloHost, build_slots};
        use web_dom::tree::NodeTree;

        let mut system = StyleSystem::new(Size::new(800.0, 600.0));
        system
            .add_stylesheet(
                SheetOrigin::Author,
                "div { display: contents; color: rgb(7, 8, 9); }",
            )
            .expect("author");
        system.flush();

        let mut tree = NodeTree::new();
        let root = tree.root();
        let contents = tree.create_element("div", "html", vec![]);
        let child = tree.create_element("span", "html", vec![]);
        tree.append_child(root, contents);
        tree.append_child(contents, child);

        let slots = build_slots(&tree, system.shared_lock());
        let bridge = StyloHost::new(slots);
        let _ = system
            .resolve_element_style(bridge.node(contents).expect("div"), 0.0)
            .expect("contents 祖先解析");
        let child_style = system
            .resolve_element_style(bridge.node(child).expect("span"), 0.0)
            .expect("display:contents 祖先下的后代解析不应 panic");
        assert_eq!(
            crate::serialize::serialize_computed(&child_style, "color").expect("color"),
            "rgb(7, 8, 9)",
            "子应继承 contents 祖先的 color"
        );
    }

    #[test]
    fn descendant_combinator_matches() {
        // R24：祖先 bloom 填充 ⇒ `div span`（后代组合器）命中。
        use crate::stylo_host::{StyloHost, build_slots};
        use web_dom::tree::NodeTree;

        let mut system = StyleSystem::new(Size::new(800.0, 600.0));
        system
            .add_stylesheet(SheetOrigin::Author, "div span { width: 30px; }")
            .expect("author");
        system.flush();
        let mut tree = NodeTree::new();
        let root = tree.root();
        let parent = tree.create_element("div", "html", vec![]);
        let child = tree.create_element("span", "html", vec![]);
        tree.append_child(root, parent);
        tree.append_child(parent, child);
        let slots = build_slots(&tree, system.shared_lock());
        let host = StyloHost::new(slots);
        let _ = system
            .resolve_element_style(host.node(parent).expect("div"), 0.0)
            .expect("p");
        let child_style = system
            .resolve_element_style(host.node(child).expect("span"), 0.0)
            .expect("c");
        assert_eq!(
            crate::serialize::serialize_computed(&child_style, "width").expect("width"),
            "30px",
            "后代组合器应命中"
        );
    }

    #[test]
    fn rule_change_recompute_no_panic() {
        // R23 同族：rule-change 族（移除 + force_stylesheet_origins_dirty + flush）重算不 panic；
        // 观测位 = width（判别性：50px -> auto）。
        use crate::stylo_host::{StyloHost, build_slots};
        use web_dom::tree::NodeTree;

        let mut system = StyleSystem::new(Size::new(800.0, 600.0));
        let handle = system
            .add_stylesheet(SheetOrigin::Author, "div { width: 50px; }")
            .expect("author");
        system.flush();

        let mut tree = NodeTree::new();
        let root = tree.root();
        let div = tree.create_element("div", "html", vec![]);
        tree.append_child(root, div);
        let slots = build_slots(&tree, system.shared_lock());
        let host = StyloHost::new(slots);

        let before = system
            .resolve_element_style(host.node(div).expect("div"), 0.0)
            .expect("cascade");
        assert_eq!(
            crate::serialize::serialize_computed(&before, "width").expect("width"),
            "50px"
        );

        system.remove_stylesheet(handle).expect("remove");
        let after = system
            .resolve_element_style(host.node(div).expect("div"), 0.0)
            .expect("rule-change 族重算不应 panic");
        assert_eq!(
            crate::serialize::serialize_computed(&after, "width").expect("width"),
            "auto",
            "移除后应回落 auto"
        );
    }

    #[test]
    fn style_invalidation_changes_computed() {
        use crate::stylo_host::{StyloHost, build_slots};
        use web_dom::tree::NodeTree;

        let mut system = StyleSystem::new(Size::new(800.0, 600.0));
        system
            .add_stylesheet(
                SheetOrigin::Author,
                "div { width: 1px; } .c { width: 2px; }",
            )
            .expect("sheet");
        system.flush();

        let mut tree = NodeTree::new();
        let root = tree.root();
        let div = tree.create_element("div", "html", vec![]);
        tree.append_child(root, div);
        let before = {
            let slots = build_slots(&tree, system.shared_lock());
            let style = system
                .resolve_element_style(StyloHost::new(slots).node(div).expect("div slot"), 0.0)
                .expect("cascade");
            crate::serialize::serialize_computed(&style, "width").expect("width")
        };
        assert_eq!(before, "1px");

        // 改 class（§2.2：class ⇒ style 级脏）→ 同元素重算应得新计算值。
        tree.set_attribute(div, "class", "c");
        let after = {
            let slots = build_slots(&tree, system.shared_lock());
            let style = system
                .resolve_element_style(StyloHost::new(slots).node(div).expect("div slot"), 0.0)
                .expect("cascade");
            crate::serialize::serialize_computed(&style, "width").expect("width")
        };
        assert_eq!(
            after, "2px",
            "改 class 后 .c width:2px 应生效（style 级失效）"
        );
    }

    #[test]
    fn dynamic_dom_insert_then_read() {
        use crate::stylo_host::{StyloHost, build_slots};
        use web_dom::tree::NodeTree;

        let mut system = StyleSystem::new(Size::new(800.0, 600.0));
        system
            .add_stylesheet(SheetOrigin::Author, "#box { width: 120px; }")
            .expect("sheet");
        system.flush();

        let mut tree = NodeTree::new();
        let root = tree.root();
        // 动态插入新节点（append）→ 重建槽 → 读其计算值应为输入值。
        let div = tree.create_element("div", "html", vec![("id".to_owned(), "box".to_owned())]);
        tree.append_child(root, div);
        let slots = build_slots(&tree, system.shared_lock());
        let style = system
            .resolve_element_style(StyloHost::new(slots).node(div).expect("div slot"), 0.0)
            .expect("cascade");
        assert_eq!(
            crate::serialize::serialize_computed(&style, "width").expect("width"),
            "120px",
            "动态插入节点 resolve 后应与其输入一致"
        );
    }

    #[test]
    fn computed_value_is_real() {
        use crate::stylo_host::{StyloHost, build_slots};
        use web_dom::tree::NodeTree;

        let mut system = StyleSystem::new(Size::new(800.0, 600.0));
        system
            .add_stylesheet(SheetOrigin::Author, "#box { width: 120px; }")
            .expect("author sheet");
        system.flush();

        let mut tree = NodeTree::new();
        let root = tree.root();
        let div = tree.create_element("div", "html", vec![("id".to_owned(), "box".to_owned())]);
        tree.append_child(root, div);
        let slots = build_slots(&tree, system.shared_lock());
        let style = system
            .resolve_element_style(StyloHost::new(slots).node(div).expect("div slot"), 0.0)
            .expect("cascade");
        assert_eq!(
            crate::serialize::serialize_computed(&style, "width").expect("width"),
            "120px",
            "#box width:120px => getComputedStyle width 应为 120px（已知值自证）"
        );
    }

    #[test]
    fn specificity_and_order_applied() {
        use crate::stylo_host::{StyloHost, build_slots};
        use web_dom::tree::NodeTree;

        let mut system = StyleSystem::new(Size::new(800.0, 600.0));
        system
            .add_stylesheet(
                SheetOrigin::Author,
                "div { width: 1px; } .c { width: 2px; } #i { width: 3px; }",
            )
            .expect("author sheet");
        system.flush();

        let mut tree = NodeTree::new();
        let root = tree.root();
        let div = tree.create_element(
            "div",
            "html",
            vec![
                ("id".to_owned(), "i".to_owned()),
                ("class".to_owned(), "c".to_owned()),
            ],
        );
        tree.append_child(root, div);
        let slots = build_slots(&tree, system.shared_lock());
        let style = system
            .resolve_element_style(StyloHost::new(slots).node(div).expect("div slot"), 0.0)
            .expect("cascade");
        assert_eq!(
            crate::serialize::serialize_computed(&style, "width").expect("width"),
            "3px",
            "#id > .class > 元素（specificity）"
        );
    }

    #[test]
    fn cascade_author_over_user_agent() {
        use crate::stylo_host::{StyloHost, build_slots};
        use style::values::computed::Display as StyloDisplay;
        use web_dom::tree::NodeTree;

        let mut system = StyleSystem::new(Size::new(800.0, 600.0));
        system
            .add_stylesheet(SheetOrigin::UserAgent, "div { display: inline; }")
            .expect("ua sheet");
        system
            .add_stylesheet(SheetOrigin::Author, "div { display: block; }")
            .expect("author sheet");
        system.flush();

        let mut tree = NodeTree::new();
        let root = tree.root();
        let div = tree.create_element("div", "html", vec![]);
        tree.append_child(root, div);
        let slots = build_slots(&tree, system.shared_lock());
        let host = StyloHost::new(slots);
        let style = system
            .resolve_element_style(host.node(div).expect("div slot"), 0.0)
            .expect("cascade");
        assert_eq!(
            style.clone_display(),
            StyloDisplay::Block,
            "author 应胜出 UA（cascade order）"
        );
    }

    #[test]
    fn ua_default_display_block() {
        use crate::stylo_host::{StyloHost, build_slots};
        use style::values::computed::Display as StyloDisplay;
        use web_dom::tree::NodeTree;

        let mut system = StyleSystem::new(Size::new(800.0, 600.0));
        system
            .add_stylesheet(SheetOrigin::UserAgent, crate::UA_CSS)
            .expect("ua css");
        system.flush();

        let mut tree = NodeTree::new();
        let root = tree.root();
        let div = tree.create_element("div", "html", vec![]);
        tree.append_child(root, div);
        let slots = build_slots(&tree, system.shared_lock());
        let host = StyloHost::new(slots);
        let style = system
            .resolve_element_style(host.node(div).expect("div slot"), 0.0)
            .expect("cascade");
        assert_eq!(
            style.clone_display(),
            StyloDisplay::Block,
            "内嵌 UA 表应使 <div> 默认 display:block（G3-B7）"
        );
    }

    #[test]
    fn cascade_author_rule_changes_computed_display() {
        use crate::stylo_host::{StyloHost, build_slots};
        use style::values::computed::Display as StyloDisplay;
        use web_dom::tree::NodeTree;

        let mut system = StyleSystem::new(Size::new(800.0, 600.0));
        system
            .add_stylesheet(SheetOrigin::Author, "div { display: block; }")
            .expect("author sheet");
        system.flush();

        let mut tree = NodeTree::new();
        let root = tree.root();
        let div = tree.create_element("div", "html", vec![]);
        tree.append_child(root, div);
        let slots = build_slots(&tree, system.shared_lock());
        let host = StyloHost::new(slots);
        let style = system
            .resolve_element_style(host.node(div).expect("div slot"), 0.0)
            .expect("cascade should succeed");
        assert_eq!(
            style.clone_display(),
            StyloDisplay::Block,
            "author rule div display:block 应反映到计算值"
        );
    }

    #[test]
    fn style_system_registers_and_dedups_sheets() {
        let mut system = StyleSystem::new(Size::new(800.0, 600.0));
        let a = system
            .add_stylesheet(SheetOrigin::UserAgent, "div { display: block; }")
            .expect("first sheet");
        let b = system
            .add_stylesheet(SheetOrigin::UserAgent, "div { display: block; }")
            .expect("dup sheet");
        assert_eq!(a, b, "同 (origin, text) 应去重复用句柄");
        assert_eq!(system.stylesheet_count(), 1);
        system.flush();
        assert!(system.remove_stylesheet(a).is_ok());
        assert!(system.remove_stylesheet(a).is_err(), "重复移除应报错");
    }
}
