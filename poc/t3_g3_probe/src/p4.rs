//! P-4（探针 B）—— **真实 `web-dom` 模型**上的 shadow 作用域 + opaque 机制。
//!
//! 判定（施工图 §7 P-4；三者同时成立）：
//! 1. **作用域隔离**：light 遍历**不含**影子内容；
//! 2. **`:host` / `::slotted` 经 implicit scope 生效**（选择器匹配，见 `p4_match` 模块，P-4b）；
//! 3. **含 opaque 机制步骤 + 零 unsafe**。
//!
//! 本模块落 **①（隔离）+ ③（opaque 反查）** 的**真实模型**部分，并构造真实模型供 ② 使用；
//! ② 由 `p4_match` 模块（P-4b）用 `selectors::matching` 在**同一模型**上判定。
//!
//! ## opaque 桥接说明
//! [`crate::opaque`] 的侧表键/值为裸 `u64`（`NodeId::raw()` 语义）；此处用
//! `web_dom::node::NodeId::raw()` 桥接（生产 `layout-backend` 直接以 `web_dom` NodeId 为键）。

use crate::opaque;
use web_dom::node::{NodeId, ShadowMode};
use web_dom::tree::NodeTree;

/// P-4 真实模型：`<x-host>` + 其影子根（`<div>` + `<slot>`）+ light 子 `<span>`。
#[derive(Debug)]
pub struct ShadowModel {
    /// 真实节点树。
    pub tree: NodeTree,
    /// 宿主元素 `<x-host>`。
    pub host: NodeId,
    /// 影子根。
    pub shadow: NodeId,
    /// 影子树内的 `<div>`（影子内容）。
    pub shadow_div: NodeId,
    /// 影子树内的 `<slot>`（默认）。
    pub slot: NodeId,
    /// 宿主的 light 子 `<span>`（被默认 slot 分发）。
    pub light_span: NodeId,
    /// 影子树身份（供 opaque 侧表 `active_tree`）。
    pub tree_id: opaque::TreeId,
}

/// 构造 P-4 真实模型（`attach_shadow` + 影子内容 + light 子）。
#[must_use]
pub fn build_model() -> ShadowModel {
    let mut tree = NodeTree::new();
    let root = tree.root();
    let host = tree.create_element("x-host", "html", vec![]);
    tree.append_child(root, host);
    let light_span = tree.create_element("span", "html", vec![]);
    tree.append_child(host, light_span);

    let shadow = tree
        .attach_shadow(host, ShadowMode::Open)
        .expect("attach_shadow should succeed on a valid element host");
    let shadow_div = tree.create_element("div", "html", vec![]);
    let slot = tree.create_element("slot", "html", vec![]);
    tree.append_child(shadow, shadow_div);
    tree.append_child(shadow, slot);

    ShadowModel {
        tree,
        host,
        shadow,
        shadow_div,
        slot,
        light_span,
        tree_id: 1,
    }
}

/// P-4 判定结果（① / ③ 部分）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct P4Report {
    /// ① light 遍历不含影子内容。
    pub isolated: bool,
    /// `flattened_children` 跨 shadow 且展开 slot。
    pub flattened_crosses: bool,
    /// ③ opaque 反查命中宿主（无 self 关联函数经线程局部侧表）。
    pub opaque_resolves_host: bool,
}

/// 跑 ①（隔离）+ ③（opaque 反查）。
#[must_use]
pub fn run() -> P4Report {
    let model = build_model();
    let tree = &model.tree;

    // ① light 遍历（children）不含影子内容；影子根独立于 light 树。
    let light: Vec<NodeId> = tree.children(model.host).collect();
    let isolated = light == vec![model.light_span]
        && tree.parent(model.shadow_div) == Some(model.shadow)
        && tree.parent(model.shadow).is_none();

    // flattened 跨 shadow：slot 展开为被分配的 light 子。
    let flattened: Vec<NodeId> = tree.flattened_children(model.host);
    let flattened_crosses = flattened == vec![model.shadow_div, model.light_span];

    // ③ opaque 机制：`opaque()`（侧表）→ **无 self 关联函数**反查宿主。
    opaque::reset_side();
    let opaque_host = opaque::with_side(|side| {
        side.set_active_tree(model.tree_id);
        side.opaque_for(model.tree_id, model.host.raw())
    });
    let resolved =
        opaque::implicit_scope_for_sheet_in_shadow_root(opaque_host, /*sheet_index=*/ 0);
    let opaque_resolves_host = resolved == Some(model.host.raw());

    P4Report {
        isolated,
        flattened_crosses,
        opaque_resolves_host,
    }
}

#[cfg(test)]
mod tests {
    use super::{build_model, run};

    /// ① 作用域隔离：light 遍历不含影子内容（真实 `web-dom` 模型）。
    #[test]
    fn shadow_isolated_from_light_traversal_real_model() {
        let report = run();
        assert!(report.isolated, "① light 遍历必须不含影子内容");
        assert!(
            report.flattened_crosses,
            "flattened 必须跨 shadow 且展开 slot"
        );
    }

    /// ③ opaque 机制步骤 + 反查命中（真实 `NodeId`）。
    #[test]
    fn opaque_implicit_scope_resolves_host_real_model() {
        assert!(
            run().opaque_resolves_host,
            "③ opaque → 关联函数须反查命中宿主"
        );
    }

    /// 模型自洽：影子根独立、slot 分发正确。
    #[test]
    fn model_shape_is_consistent() {
        let model = build_model();
        assert_ne!(model.host, model.shadow);
        assert_eq!(model.tree.shadow_host(model.shadow), Some(model.host));
        assert_eq!(
            model.tree.assigned_nodes(model.slot),
            vec![model.light_span]
        );
    }
}
