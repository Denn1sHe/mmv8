//! 盒树（`layout-backend::box_tree`，施工图 §2.1 / §3.2 / S8）。
//!
//! styled DOM → **Taffy 建树**（`stylo_taffy::to_taffy_style` 首进生产路径，P-2 桥）+ 计算布局 + 几何读数。
//! **G3 最小面**：块级元素建树 + `to_taffy_style` 的 `display/size/margin/padding/border` 映射；
//! 匿名盒与内联文本整形（`text.rs`）在后续步。
//!
//! **零 unsafe**（落 `layout-backend/src/`，非桥目录）。

use std::collections::HashMap;

use taffy::prelude::{AvailableSpace, TaffyTree};
use taffy::{Size as TaffySize, Style as TaffyStyle};
use web_dom::node::{NodeId, NodeKind};
use web_dom::tree::NodeTree;

use crate::error::{LayoutError, LayoutErrorKind};
use crate::geometry::Size;
use crate::style_system::StyleSystem;
use crate::stylo_host::{StyloHost, build_slots};
use crate::text::{EMBEDDED_FAMILY, SAMPLE_SIZE, TextShaper};

/// 一个节点的几何读数（`offsetWidth`/`clientWidth`/`gBCR` 的源）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NodeGeometry {
    /// border-box 宽（`offsetWidth` 源；四舍五入到整数）。
    pub offset_width: f32,
    /// border-box 高（`offsetHeight` 源）。
    pub offset_height: f32,
    /// 左右 border 之和。
    pub border_x: f32,
    /// 上下 border 之和。
    pub border_y: f32,
    /// 视口坐标 top（`gBCR().top` 源）。
    pub top: f32,
    /// 视口坐标 left（`gBCR().left` 源）。
    pub left: f32,
}

impl NodeGeometry {
    /// `clientWidth` = border-box − 左右 border（padding box；G3 无滚动条）。
    #[must_use]
    pub fn client_width(self) -> f32 {
        (self.offset_width - self.border_x).max(0.0)
    }

    /// `clientHeight` = border-box − 上下 border。
    #[must_use]
    pub fn client_height(self) -> f32 {
        (self.offset_height - self.border_y).max(0.0)
    }
}

/// 盒树：Taffy 树 + DOM↔Taffy 节点映射 + 每元素几何。
pub struct BoxTree {
    taffy: TaffyTree,
    map: HashMap<NodeId, taffy::NodeId>,
    root: Option<taffy::NodeId>,
    geometry: HashMap<NodeId, NodeGeometry>,
    shaper: TextShaper,
}

#[allow(clippy::missing_fields_in_debug)] // 摘要（节点数）；taffy/映射无稳定 Debug 面。
impl std::fmt::Debug for BoxTree {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BoxTree")
            .field("nodes", &self.map.len())
            .finish()
    }
}

/// 把 `stylo_taffy::to_taffy_style` 的 `Style<Atom>` 拷进 Taffy 树用的 `Style`（S 无关字段）。
fn to_tree_style(computed: &style::properties::ComputedValues) -> TaffyStyle {
    let bridged = stylo_taffy::to_taffy_style(computed);
    TaffyStyle {
        display: bridged.display,
        size: bridged.size,
        margin: bridged.margin,
        padding: bridged.padding,
        border: bridged.border,
        ..TaffyStyle::default()
    }
}

impl BoxTree {
    /// 从 styled DOM 建 Taffy 树（块级递归；`display:none` 经 `to_taffy_style` 映射为 `None`）。
    ///
    /// # Errors
    /// 级联失败 ⇒ [`LayoutErrorKind::Cascade`]。
    pub fn build(tree: &NodeTree, system: &StyleSystem) -> Result<Self, LayoutError> {
        let slots = build_slots(tree, system.shared_lock());
        let host = StyloHost::new(slots);
        let mut box_tree = Self {
            taffy: TaffyTree::new(),
            map: HashMap::new(),
            root: None,
            geometry: HashMap::new(),
            shaper: TextShaper::new(),
        };

        let root_dom = tree.root();
        let root_taffy = box_tree.build_node(tree, &host, system, root_dom)?;
        box_tree.root = Some(root_taffy);
        box_tree.append_children(tree, &host, system, root_dom, root_taffy)?;
        Ok(box_tree)
    }

    /// 递归挂子：**连续内联级子元素包进匿名块盒**（CSS 2.1 §9.2.1.1；§5 S8）。
    fn append_children(
        &mut self,
        tree: &NodeTree,
        host: &StyloHost<'_>,
        system: &StyleSystem,
        parent_dom: NodeId,
        parent_taffy: taffy::NodeId,
    ) -> Result<(), LayoutError> {
        // composed/flattened 子（跨 shadow 边界 + slot 展开）——影子内容与分发节点参与真实布局（S11）。
        let children: Vec<NodeId> = tree.flattened_children(parent_dom);
        let mut inline_run: Vec<taffy::NodeId> = Vec::new();
        for child in children {
            if !is_element(tree, child) {
                continue;
            }
            let taffy_node = self.build_node(tree, host, system, child)?;
            if is_inline_level(tree, host, system, child)? {
                inline_run.push(taffy_node);
                // `G3X-S6`：**内联级子亦须下钻**——此前只入匿名块运行、**不递归**，
                // 致其子树（如**动态影子内容**）**从未进盒树**（无几何）。
                self.append_children(tree, host, system, child, taffy_node)?;
            } else {
                self.flush_inline_run(parent_taffy, &mut inline_run)?;
                self.taffy
                    .add_child(parent_taffy, taffy_node)
                    .map_err(map_taffy_error)?;
                self.append_children(tree, host, system, child, taffy_node)?;
            }
        }
        self.flush_inline_run(parent_taffy, &mut inline_run)
    }

    /// 把一段**连续内联级**子盒包进一个**匿名块盒**再挂到父（无合法空跑）。
    fn flush_inline_run(
        &mut self,
        parent_taffy: taffy::NodeId,
        inline_run: &mut Vec<taffy::NodeId>,
    ) -> Result<(), LayoutError> {
        if inline_run.is_empty() {
            return Ok(());
        }
        let anonymous = self
            .taffy
            .new_leaf(TaffyStyle {
                display: taffy::Display::Block,
                ..TaffyStyle::default()
            })
            .map_err(map_taffy_error)?;
        for child in inline_run.drain(..) {
            self.taffy
                .add_child(anonymous, child)
                .map_err(map_taffy_error)?;
        }
        self.taffy
            .add_child(parent_taffy, anonymous)
            .map_err(map_taffy_error)
    }

    fn build_node(
        &mut self,
        tree: &NodeTree,
        host: &StyloHost<'_>,
        system: &StyleSystem,
        id: NodeId,
    ) -> Result<taffy::NodeId, LayoutError> {
        let is_root = id == tree.root();
        let mut style = match host.node(id) {
            Some(node) if is_element(tree, id) => {
                let computed = system.resolve_element_style(node, 0.0)?;
                to_tree_style(&computed)
            }
            _ => TaffyStyle::default(),
        };
        // 文档根 = 初始包含块：块容器（G3 无定位/滚动，视口原点 (0,0)）。
        if is_root {
            style.display = taffy::Display::Block;
        }
        // 内联文字：元素含文本子节点 ⇒ 用整形宽度作为其盒宽（§3.2 inline_text_shaped_width）。
        if let Some(text) = first_text(tree, id) {
            let width = self.shaper.shape_width(&text, EMBEDDED_FAMILY, SAMPLE_SIZE);
            style.size.width = taffy::Dimension::length(width);
        }
        let taffy_node = self.taffy.new_leaf(style).map_err(map_taffy_error)?;
        self.map.insert(id, taffy_node);
        Ok(taffy_node)
    }

    /// 计算布局并缓存每元素几何。
    ///
    /// # Errors
    /// Taffy 布局失败 ⇒ [`LayoutErrorKind::Taffy`]。
    pub fn compute_layout(&mut self, viewport: Size) -> Result<(), LayoutError> {
        let Some(root) = self.root else {
            return Ok(());
        };
        self.taffy
            .compute_layout(
                root,
                TaffySize {
                    width: AvailableSpace::Definite(viewport.width),
                    height: AvailableSpace::Definite(viewport.height),
                },
            )
            .map_err(map_taffy_error)?;

        self.geometry.clear();
        for (dom, taffy_node) in &self.map {
            let layout = self.taffy.layout(*taffy_node).map_err(map_taffy_error)?;
            let border_x = layout.border.left + layout.border.right;
            let border_y = layout.border.top + layout.border.bottom;
            self.geometry.insert(
                *dom,
                NodeGeometry {
                    offset_width: layout.size.width,
                    offset_height: layout.size.height,
                    border_x,
                    border_y,
                    top: layout.location.y,
                    left: layout.location.x,
                },
            );
        }
        Ok(())
    }

    /// 某元素的几何（`compute_layout` 后可用）。
    #[must_use]
    pub fn geometry(&self, id: NodeId) -> Option<NodeGeometry> {
        self.geometry.get(&id).copied()
    }
}

/// 元素的首个文本子节点内容（非空 ⇒ `Some`）。
fn first_text(tree: &NodeTree, id: NodeId) -> Option<String> {
    tree.children(id)
        .filter_map(|child| tree.get(child))
        .find(|node| node.kind == NodeKind::Text)
        .and_then(|node| node.text.clone())
        .filter(|text| !text.is_empty())
}

fn is_element(tree: &NodeTree, id: NodeId) -> bool {
    tree.get(id)
        .is_some_and(|node| node.kind == NodeKind::Element)
}

/// 元素是否**内联级**（计算 `display` ∈ {inline, inline-block, inline-flex, inline-grid}）。
fn is_inline_level(
    _tree: &NodeTree,
    host: &StyloHost<'_>,
    system: &StyleSystem,
    id: NodeId,
) -> Result<bool, LayoutError> {
    let Some(node) = host.node(id) else {
        return Ok(false);
    };
    let computed = system.resolve_element_style(node, 0.0)?;
    Ok(matches!(
        computed.clone_display(),
        style::values::computed::Display::Inline | style::values::computed::Display::InlineBlock
    ))
}

#[allow(clippy::needless_pass_by_value)] // `map_err` 需按值签名。
fn map_taffy_error(error: taffy::TaffyError) -> LayoutError {
    LayoutError::new(LayoutErrorKind::Taffy, error.to_string())
}

#[cfg(test)]
mod tests {
    use super::BoxTree;
    use crate::geometry::Size;
    use crate::style_system::StyleSystem;
    use web_dom::tree::NodeTree;
    use web_style::SheetOrigin;

    const VIEWPORT: Size = Size::new(800.0, 600.0);

    fn ua_system() -> StyleSystem {
        let mut system = StyleSystem::new(VIEWPORT);
        system
            .add_stylesheet(SheetOrigin::UserAgent, crate::UA_CSS)
            .expect("ua sheet");
        system.flush();
        system
    }

    #[test]
    fn block_width_matches_css() {
        let system = ua_system();
        let mut tree = NodeTree::new();
        let root = tree.root();
        let div = tree.create_element(
            "div",
            "html",
            vec![("style".to_owned(), "width:100px;height:50px".to_owned())],
        );
        tree.append_child(root, div);

        let mut boxes = BoxTree::build(&tree, &system).expect("build");
        boxes.compute_layout(VIEWPORT).expect("compute");
        let geometry = boxes.geometry(div).expect("geometry");
        assert!(
            (geometry.offset_width.round() - 100.0).abs() < 0.01,
            "offsetWidth == width:100px"
        );
        assert!(
            (geometry.offset_height.round() - 50.0).abs() < 0.01,
            "offsetHeight == height:50px"
        );
    }

    #[test]
    fn margin_and_border_in_border_box() {
        let system = ua_system();
        let mut tree = NodeTree::new();
        let root = tree.root();
        let div = tree.create_element(
            "div",
            "html",
            vec![(
                "style".to_owned(),
                "width:100px;height:50px;border:2px solid;padding:3px;box-sizing:border-box"
                    .to_owned(),
            )],
        );
        tree.append_child(root, div);

        let mut boxes = BoxTree::build(&tree, &system).expect("build");
        boxes.compute_layout(VIEWPORT).expect("compute");
        let geometry = boxes.geometry(div).expect("geometry");
        assert!(
            (geometry.offset_width.round() - 100.0).abs() < 0.01,
            "border-box 宽 100"
        );
        assert!(
            (geometry.client_width().round() - 96.0).abs() < 0.01,
            "clientWidth = 100 - 2*2"
        );
    }

    #[test]
    fn rect_reflects_position() {
        let system = ua_system();
        let mut tree = NodeTree::new();
        let root = tree.root();
        let first = tree.create_element(
            "div",
            "html",
            vec![("style".to_owned(), "height:50px".to_owned())],
        );
        let second = tree.create_element(
            "div",
            "html",
            vec![("style".to_owned(), "height:10px".to_owned())],
        );
        tree.append_child(root, first);
        tree.append_child(root, second);

        let mut boxes = BoxTree::build(&tree, &system).expect("build");
        boxes.compute_layout(VIEWPORT).expect("compute");
        let rect = boxes.geometry(second).expect("geometry");
        assert!(
            (rect.top.round() - 50.0).abs() < 0.01,
            "第二块 top == 上方块高 50"
        );
        assert!((rect.left.round() - 0.0).abs() < 0.01, "left == 0");
    }

    #[test]
    fn inline_text_shaped_width() {
        let system = ua_system();
        let mut tree = NodeTree::new();
        let root = tree.root();
        let span = tree.create_element("span", "html", vec![]);
        let text = tree.create_text("Hamburgefonstiv");
        tree.append_child(root, span);
        tree.append_child(span, text);

        let mut boxes = BoxTree::build(&tree, &system).expect("build");
        boxes.compute_layout(VIEWPORT).expect("compute");
        let geometry = boxes.geometry(span).expect("geometry");
        // offsetWidth 按 CSSOM View 四舍五入到整数：112.625 -> 113。
        assert!(
            (geometry.offset_width - crate::text::PINNED_EMBEDDED_ADVANCE_PX.round()).abs() < 0.01,
            "内联整形宽应为 P-10 钉值 {}（四舍五入 {}）；实测 {}",
            crate::text::PINNED_EMBEDDED_ADVANCE_PX,
            crate::text::PINNED_EMBEDDED_ADVANCE_PX.round(),
            geometry.offset_width
        );
    }

    #[test]
    fn shadow_node_participates_in_layout() {
        use web_dom::node::ShadowMode;
        let system = ua_system();
        let mut tree = NodeTree::new();
        let root = tree.root();
        let host = tree.create_element("x-host", "html", vec![]);
        tree.append_child(root, host);
        let shadow = tree.attach_shadow(host, ShadowMode::Open).expect("shadow");
        let inside = tree.create_element(
            "div",
            "html",
            vec![("style".to_owned(), "width:60px;height:40px".to_owned())],
        );
        tree.append_child(shadow, inside);

        let mut boxes = BoxTree::build(&tree, &system).expect("build");
        boxes.compute_layout(VIEWPORT).expect("compute");
        let geometry = boxes.geometry(inside).expect("shadow 内节点几何应可读");
        assert!(
            (geometry.offset_width.round() - 60.0).abs() < 0.01
                && (geometry.offset_height.round() - 40.0).abs() < 0.01,
            "影子内块级节点应参与布局：{}x{}",
            geometry.offset_width,
            geometry.offset_height
        );
    }

    #[test]
    fn anonymous_box_wraps_inline_run() {
        let system = ua_system();
        let mut tree = NodeTree::new();
        let root = tree.root();
        let block = tree.create_element(
            "div",
            "html",
            vec![("style".to_owned(), "height:20px".to_owned())],
        );
        let span = tree.create_element(
            "span",
            "html",
            vec![("style".to_owned(), "height:30px".to_owned())],
        );
        let tail = tree.create_element(
            "div",
            "html",
            vec![("style".to_owned(), "height:20px".to_owned())],
        );
        tree.append_child(root, block);
        tree.append_child(root, span);
        tree.append_child(root, tail);

        let mut boxes = BoxTree::build(&tree, &system).expect("build");
        boxes.compute_layout(VIEWPORT).expect("compute");
        let block_rect = boxes.geometry(block).expect("block");
        let span_rect = boxes.geometry(span).expect("span");
        let tail_rect = boxes.geometry(tail).expect("tail");
        // 匿名盒把 span 归入块流：block(0) < span(≥20) < tail(≥span)。
        assert!(block_rect.top < span_rect.top, "块首 < 内联片");
        assert!(span_rect.top < tail_rect.top, "内联片 < 后续块（顺序保持）");
    }

    #[test]
    fn non_rendered_element_is_zero() {
        let system = ua_system();
        let mut tree = NodeTree::new();
        let root = tree.root();
        let div = tree.create_element(
            "div",
            "html",
            vec![("style".to_owned(), "display:none".to_owned())],
        );
        tree.append_child(root, div);

        let mut boxes = BoxTree::build(&tree, &system).expect("build");
        boxes.compute_layout(VIEWPORT).expect("compute");
        let geometry = boxes.geometry(div).expect("geometry");
        assert!(
            (geometry.offset_width - 0.0).abs() < 0.01,
            "display:none => offsetWidth 0"
        );
        assert!(
            (geometry.offset_height - 0.0).abs() < 0.01,
            "display:none => offsetHeight 0"
        );
    }
}
