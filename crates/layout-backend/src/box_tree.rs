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
        };

        let root_id = tree.root();
        let root_taffy = box_tree.build_node(tree, &host, system, root_id)?;
        box_tree.root = Some(root_taffy);

        // 逐元素建树 + 挂父。
        for element in elements_in_document_order(tree) {
            let taffy_node = box_tree.build_node(tree, &host, system, element)?;
            if let Some(parent) = tree
                .parent(element)
                .filter(|p| box_tree.map.contains_key(p))
            {
                let parent_taffy = box_tree.map[&parent];
                box_tree
                    .taffy
                    .add_child(parent_taffy, taffy_node)
                    .map_err(map_taffy_error)?;
            }
        }
        Ok(box_tree)
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

fn is_element(tree: &NodeTree, id: NodeId) -> bool {
    tree.get(id)
        .is_some_and(|node| node.kind == NodeKind::Element)
}

/// 文档序的所有元素（先序）。
fn elements_in_document_order(tree: &NodeTree) -> Vec<NodeId> {
    let mut out = Vec::new();
    let mut stack: Vec<NodeId> = tree.children(tree.root()).collect();
    stack.reverse();
    while let Some(id) = stack.pop() {
        if is_element(tree, id) {
            out.push(id);
        }
        let mut children: Vec<NodeId> = tree.children(id).collect();
        children.reverse();
        stack.extend(children);
    }
    out
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
