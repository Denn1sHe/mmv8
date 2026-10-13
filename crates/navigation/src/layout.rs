//! 布局门面（`navigation::layout`，施工图 §2.5 / §3.4 / S12）。
//!
//! 持 `StyleSystem` + `BoxTree` + 两级失效簿记；**同步读几何语义（§2.5）**：脚本任务内首次读
//! 几何/计算值前，若为脏 ⇒ 隐式 `relayout_if_dirty()`（sync flush），随后读数即最新。
//!
//! **零 unsafe**（落 `navigation/src/`，非桥目录）。

use layout_backend::box_tree::{BoxTree, NodeGeometry};
use layout_backend::geometry::Size;
use layout_backend::invalidate::{DomChange, Invalidation};
use layout_backend::style_system::StyleSystem;
use web_dom::node::NodeId;
use web_dom::tree::NodeTree;
use web_style::SheetOrigin;

use crate::error::{PageError, PageErrorKind};

/// 布局门面：几何真值 + §2.5 同步读。
pub struct LayoutFacade {
    system: StyleSystem,
    viewport: Size,
    invalidation: Invalidation,
    boxes: Option<BoxTree>,
}

impl std::fmt::Debug for LayoutFacade {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LayoutFacade")
            .field("laid_out", &self.boxes.is_some())
            .finish_non_exhaustive()
    }
}

impl LayoutFacade {
    /// 建门面（视口给定；初始脏 ⇒ 首次读数即布局）。
    #[must_use]
    pub fn new(viewport: Size) -> Self {
        let mut invalidation = Invalidation::new();
        invalidation.apply(DomChange::ChildList);
        Self {
            system: StyleSystem::new(viewport),
            viewport,
            invalidation,
            boxes: None,
        }
    }

    /// 登记作者 / UA 样式表。
    ///
    /// # Errors
    /// CSS 解析 / 登记失败 ⇒ [`PageErrorKind::Internal`]。
    pub fn register_sheet(&mut self, origin: SheetOrigin, css: &str) -> Result<(), PageError> {
        self.system
            .add_stylesheet(origin, css)
            .map_err(|err| PageError::new(PageErrorKind::Internal, err.message().to_owned()))?;
        self.system.flush();
        self.mark_dirty(DomChange::ChildList);
        Ok(())
    }

    /// 标一次变更（§2.2 变更→脏级映射）。
    pub fn mark_dirty(&mut self, change: DomChange<'_>) {
        self.invalidation.apply(change);
    }

    /// **§2.5 同步读**：脏 ⇒ 隐式重建布局（sync flush）；否则复用已算布局。
    ///
    /// # Errors
    /// 级联 / 布局失败 ⇒ [`PageErrorKind::Internal`]。
    pub fn relayout_if_dirty(&mut self, tree: &NodeTree) -> Result<(), PageError> {
        let state = self.invalidation.state();
        let dirty = state.style || state.layout;
        if !dirty && self.boxes.is_some() {
            return Ok(());
        }
        let mut boxes = BoxTree::build(tree, &self.system)
            .map_err(|err| PageError::new(PageErrorKind::Internal, err.message().to_owned()))?;
        boxes
            .compute_layout(self.viewport)
            .map_err(|err| PageError::new(PageErrorKind::Internal, err.message().to_owned()))?;
        self.boxes = Some(boxes);
        let _ = self.invalidation.take();
        Ok(())
    }

    /// 几何读数（**先隐式 flush**，§2.5）。
    ///
    /// # Errors
    /// 未布局 / 节点不可渲染 ⇒ [`PageErrorKind::Internal`]。
    pub fn geometry(&mut self, tree: &NodeTree, node: NodeId) -> Result<NodeGeometry, PageError> {
        self.relayout_if_dirty(tree)?;
        self.boxes
            .as_ref()
            .and_then(|boxes| boxes.geometry(node))
            .ok_or_else(|| PageError::new(PageErrorKind::Internal, "节点无几何（未布局）"))
    }

    /// 计算值序列化（**先隐式 flush**，§2.5；`getComputedStyle` 源）。
    ///
    /// # Errors
    /// 属性不在白名单 ⇒ [`PageErrorKind::Internal`]（上层映射 `UnsupportedProperty` 语义）。
    pub fn computed_value(
        &mut self,
        tree: &NodeTree,
        node: NodeId,
        property: &str,
    ) -> Result<String, PageError> {
        self.relayout_if_dirty(tree)?;
        let slots = layout_backend::stylo_host::build_slots(tree, self.system.shared_lock());
        let host = layout_backend::stylo_host::StyloHost::new(slots);
        // R23 配对义务①：**祖先先解析**（parent-first）——后代选择器 / 继承依赖祖先已解析的数据。
        let mut path = vec![node];
        let mut cursor = tree.parent(node);
        while let Some(parent) = cursor {
            path.push(parent);
            cursor = tree.parent(parent);
        }
        path.reverse();
        let mut latest = None;
        for id in path {
            if let Some(element) = host.node(id) {
                latest = Some(
                    self.system
                        .resolve_element_style(element, 0.0)
                        .map_err(|err| {
                            PageError::new(PageErrorKind::Internal, err.message().to_owned())
                        })?,
                );
            }
        }
        let computed = latest.ok_or_else(|| PageError::new(PageErrorKind::Internal, "节点无效"))?;
        layout_backend::serialize::serialize_computed(&computed, property)
            .map_err(|err| PageError::new(PageErrorKind::Internal, err.message().to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use super::LayoutFacade;
    use layout_backend::geometry::Size;
    use web_dom::tree::NodeTree;
    use web_style::SheetOrigin;

    #[test]
    fn geometry_sync_read_after_mutation() {
        let mut facade = LayoutFacade::new(Size::new(800.0, 600.0));
        facade
            .register_sheet(SheetOrigin::UserAgent, layout_backend::UA_CSS)
            .expect("ua");
        facade
            .register_sheet(SheetOrigin::Author, "#box { width: 120px; height: 40px; }")
            .expect("author");

        let mut tree = NodeTree::new();
        let root = tree.root();
        let div = tree.create_element("div", "html", vec![("id".to_owned(), "box".to_owned())]);
        tree.append_child(root, div);

        let first = facade.geometry(&tree, div).expect("geometry");
        assert!(
            (first.offset_width.round() - 120.0).abs() < 0.01,
            "首读 120"
        );

        // 改 style（inline width）→ 标脏 → 同步读应反映新值（§2.5，非陈旧）。
        tree.set_attribute(div, "style", "width: 200px; height: 40px;");
        facade.mark_dirty(layout_backend::invalidate::DomChange::InlineStyle { property: "width" });
        let second = facade.geometry(&tree, div).expect("geometry");
        assert!(
            (second.offset_width.round() - 200.0).abs() < 0.01,
            "变更加标脏 => 同步读应为 200（实测 {}）",
            second.offset_width
        );
    }

    #[test]
    fn layout_invalidation_changes_geometry() {
        // §3.3 几何条：改 inline width（尺寸相关）=> 几何层变（两级成对）。
        let mut facade = LayoutFacade::new(Size::new(800.0, 600.0));
        facade
            .register_sheet(SheetOrigin::UserAgent, layout_backend::UA_CSS)
            .expect("ua");
        let mut tree = NodeTree::new();
        let root = tree.root();
        let div = tree.create_element(
            "div",
            "html",
            vec![("style".to_owned(), "width:80px;height:10px".to_owned())],
        );
        tree.append_child(root, div);

        let before = facade.geometry(&tree, div).expect("geometry");
        tree.set_attribute(div, "style", "width:200px;height:10px");
        facade.mark_dirty(layout_backend::invalidate::DomChange::InlineStyle { property: "width" });
        let after = facade.geometry(&tree, div).expect("geometry");
        assert!(
            (before.offset_width - 80.0).abs() < 0.01 && (after.offset_width - 200.0).abs() < 0.01,
            "尺寸变更 => 几何层变（before {} / after {}）",
            before.offset_width,
            after.offset_width
        );
    }

    #[test]
    fn layout_only_change_leaves_computed() {
        // §3.3：仅容器尺寸变 => 子几何变、子「计算值 width」不变（R23 后恢复）。
        let mut facade = LayoutFacade::new(Size::new(800.0, 600.0));
        facade
            .register_sheet(SheetOrigin::UserAgent, layout_backend::UA_CSS)
            .expect("ua");
        let mut tree = NodeTree::new();
        let root = tree.root();
        let container = tree.create_element("div", "html", vec![("id".to_owned(), "c".to_owned())]);
        let child = tree.create_element(
            "div",
            "html",
            vec![("style".to_owned(), "width:40px;height:10px".to_owned())],
        );
        tree.append_child(root, container);
        tree.append_child(container, child);

        let computed_before = facade.computed_value(&tree, child, "width").expect("cv");
        facade
            .register_sheet(SheetOrigin::Author, "#c { width: 500px; }")
            .expect("author");
        let computed_after = facade.computed_value(&tree, child, "width").expect("cv");
        assert_eq!(
            computed_before, computed_after,
            "仅容器尺寸变 => 子计算值 width 不变（{computed_before} vs {computed_after}）"
        );
    }

    #[test]
    fn parent_first_resolution_inheritance() {
        // R23 配对义务①：祖先先解析（parent-first）=> 后代选择器匹配 + 继承取值正确（防退化回归）。
        let mut facade = LayoutFacade::new(Size::new(800.0, 600.0));
        facade
            .register_sheet(SheetOrigin::UserAgent, layout_backend::UA_CSS)
            .expect("ua");
        facade
            .register_sheet(
                SheetOrigin::Author,
                "div { width: 30px; color: rgb(1, 2, 3); }",
            )
            .expect("author");
        let mut tree = NodeTree::new();
        let root = tree.root();
        let parent = tree.create_element("div", "html", vec![]);
        let child = tree.create_element("span", "html", vec![]);
        tree.append_child(root, parent);
        tree.append_child(parent, child);

        let width = facade.computed_value(&tree, child, "width").expect("cv");
        let _ = width;
        // 继承断言（parent-first）：父设 color，子应继承（依赖父已解析 => 防退化）。
        let parent_color = facade.computed_value(&tree, parent, "color").expect("cv");
        let child_color = facade.computed_value(&tree, child, "color").expect("cv");
        assert_eq!(
            child_color, parent_color,
            "子应继承父的 color（parent-first；{child_color} vs {parent_color}）"
        );
    }

    #[test]
    fn computed_value_via_facade() {
        let mut facade = LayoutFacade::new(Size::new(800.0, 600.0));
        facade
            .register_sheet(SheetOrigin::UserAgent, layout_backend::UA_CSS)
            .expect("ua");
        facade
            .register_sheet(SheetOrigin::Author, "#box { width: 120px; }")
            .expect("author");
        let mut tree = NodeTree::new();
        let root = tree.root();
        let div = tree.create_element("div", "html", vec![("id".to_owned(), "box".to_owned())]);
        tree.append_child(root, div);

        assert_eq!(
            facade.computed_value(&tree, div, "width").expect("width"),
            "120px"
        );
        assert!(
            facade.computed_value(&tree, div, "z-index").is_err(),
            "白名单外应报错"
        );
    }
}
