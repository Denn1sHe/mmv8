//! Shadow 作用域与槽分发（`layout-backend::shadow`，施工图 §1.2 / §3.5 / S11）。
//!
//! 组合面（`composed`）供布局使用：跨 shadow 边界取子（`flattened_children`）+ `<slot>` 分发
//! （`assigned_nodes`）。`:host` / `::slotted` 的**样式作用域**经 `stylo_host` 的
//! `implicit_scope_for_sheet_in_shadow_root`（opaque 侧表反查）接入 stylo 级联。
//!
//! **零 unsafe**（落 `layout-backend/src/`，非桥目录）。

use web_dom::node::NodeId;
use web_dom::tree::NodeTree;

/// 组合树的子节点（跨 shadow 边界 + slot 展开）——布局用。
#[must_use]
pub fn flattened_children(tree: &NodeTree, node: NodeId) -> Vec<NodeId> {
    tree.flattened_children(node)
}

/// `<slot>` 的被分配节点（默认 + 具名）。
#[must_use]
pub fn assigned_nodes(tree: &NodeTree, slot: NodeId) -> Vec<NodeId> {
    tree.assigned_nodes(slot)
}

/// 节点是否位于某影子树内（其祖先链含影子根）。
#[must_use]
pub fn is_in_shadow_tree(tree: &NodeTree, node: NodeId) -> bool {
    tree.containing_shadow_root(node).is_some()
}

/// 节点是否**仅存在于影子树**（light 遍历不可见）——`true` 当节点在影子树内且不等于影子根自身。
#[must_use]
pub fn is_shadow_only(tree: &NodeTree, node: NodeId) -> bool {
    is_in_shadow_tree(tree, node) && !tree.is_shadow_root(node)
}

/// `node` 的宿主元素（若 `node` 是影子根）。
#[must_use]
pub fn shadow_host(tree: &NodeTree, node: NodeId) -> Option<NodeId> {
    tree.shadow_host(node)
}

/// **light 子链**（不含影子内容）——与 [`flattened_children`] 对照（作用域隔离，G3-B8）。
#[must_use]
pub fn light_children(tree: &NodeTree, node: NodeId) -> Vec<NodeId> {
    tree.children(node).collect()
}

#[cfg(test)]
mod tests {
    use super::{assigned_nodes, flattened_children, is_shadow_only, light_children};
    use web_dom::node::{NodeId, ShadowMode};
    use web_dom::tree::NodeTree;

    #[test]
    fn slot_distribution_assigned_nodes() {
        let mut tree = NodeTree::new();
        let root = tree.root();
        let host = tree.create_element("x-host", "html", vec![]);
        tree.append_child(root, host);
        let light_a = tree.create_element("span", "html", vec![]);
        let light_b =
            tree.create_element("b", "html", vec![("slot".to_owned(), "named".to_owned())]);
        tree.append_child(host, light_a);
        tree.append_child(host, light_b);

        let shadow = tree.attach_shadow(host, ShadowMode::Open).expect("shadow");
        let slot = tree.create_element(
            "slot",
            "html",
            vec![("name".to_owned(), "named".to_owned())],
        );
        tree.append_child(shadow, slot);

        let assigned = assigned_nodes(&tree, slot);
        assert_eq!(
            assigned,
            vec![light_b],
            "具名 slot 只收 slot=named 的 host light 子"
        );
        // 组合子：host 的组合子 = 影子树子（slot 展开 ⇒ 分发节点）。
        assert_eq!(
            flattened_children(&tree, host),
            vec![light_b],
            "host 组合子应为分发节点（light_a 未被 slot 收 => 不渲染）"
        );
    }

    #[test]
    fn shadow_node_hidden_from_light_traversal() {
        let mut tree = NodeTree::new();
        let root = tree.root();
        let host = tree.create_element("x-host", "html", vec![]);
        tree.append_child(root, host);
        let shadow = tree.attach_shadow(host, ShadowMode::Open).expect("shadow");
        let inside = tree.create_element("div", "html", vec![]);
        tree.append_child(shadow, inside);

        assert!(is_shadow_only(&tree, inside), "影子内节点应仅存在于影子树");
        assert!(
            !light_children(&tree, host).contains(&inside),
            "light 子链不含影子内容（作用域隔离）"
        );
        assert_eq!(
            light_children(&tree, host),
            Vec::<NodeId>::new(),
            "host 无 light 子"
        );
    }
}
