//! `opaque` —— **线程局部 opaque 侧表**（ADR-0020 决策 2 / 施工图 §1.2.1）。
//!
//! ## 为什么需要它
//!
//! `selectors::Element::opaque(&self) -> OpaqueElement` 是**必需方法、无默认实现**
//! （`selectors-0.40.0/tree.rs:50`）；`OpaqueElement(NonNull<()>)`（同文件 `:17`）**唯一反解**
//! 是 `unsafe fn as_const_ptr<T>(&self) -> *const T`（`:41`）。而 Shadow 作用域经**无 `self`
//! 的关联函数** `TElement::implicit_scope_for_sheet_in_shadow_root(opaque_host, sheet_index)`
//! （`stylo-0.20.0/dom.rs:976`；调用点 `stylist.rs:3150`）反查宿主 ⇒ 必须能从 `OpaqueElement`
//! 回到节点。blitz 因以 `NodeId` 作 opaque、需 `Slab` 反查而 `todo!()`（`blitz-dom/src/stylo.rs:577`）。
//!
//! ## 零 unsafe 论证
//!
//! - `OpaqueElement::new(&*anchor)` 是**安全**函数（取 `&T`；`selectors-0.40.0/tree.rs:21`）。
//! - 我方**从不**调用 `unsafe fn as_const_ptr` —— 只把 `OpaqueElement` 当 `Copy + Eq + Hash`
//!   **键**使用 ⇒ **无 unsafe 面**。
//! - **锚点稳定**：锚点为 `Box<NodeAnchor>`（堆分配地址稳定，规避 `HashMap` rehash /
//!   `Vec<Option<_>>` append 时 realloc 导致的地址漂移）。
//! - **生命周期**：侧表在**每次 `resolve` 起**设 `active_tree`；`reset()`（导航）时 [`clear`]。
//!
//! [`clear`]: OpaqueSideTable::clear

use std::cell::{Cell, RefCell};
use std::collections::HashMap;

use selectors::OpaqueElement;

/// DOM 节点标识（探针用裸 `u64`；生产系统用 `web_dom::node::NodeId`）。
pub type NodeId = u64;
/// 树身份（探针用裸 `u64`；生产系统用 `TreeId`）。
pub type TreeId = u64;

/// 稳定地址锚点：`Box` ⇒ 堆地址不随容器 rehash / realloc 而变。
struct NodeAnchor {
    #[allow(dead_code)] // 探针记录树身份；生产 `implicit_scope...` 会校验它。
    tree: TreeId,
    #[allow(dead_code)]
    id: NodeId,
}

/// 线程局部「opaque 侧表」。
///
/// 把 `OpaqueElement` 当作**不透明键**（`Copy + Eq + Hash`），**从不反解指针**。
#[derive(Default)]
pub struct OpaqueSideTable {
    /// 每节点一个**稳定地址**锚点（`Box` ⇒ 地址不随 `NodeTree.nodes` 增长而变）。
    anchors: HashMap<NodeId, Box<NodeAnchor>>,
    /// `OpaqueElement` → `NodeId`（关联函数据此反查）。
    by_opaque: HashMap<OpaqueElement, NodeId>,
    /// 当前 `resolve` 的树身份（关联函数**无 `self`** ⇒ 由线程局部提供）。
    active_tree: Cell<Option<TreeId>>,
}

impl OpaqueSideTable {
    /// 置当前 `resolve` 的树身份（`reset()` 时清空）。
    pub fn set_active_tree(&self, tree: TreeId) {
        self.active_tree.set(Some(tree));
    }

    /// 清空侧表（导航 `reset()`；ADR-0018·D2）。
    pub fn clear(&mut self) {
        self.anchors.clear();
        self.by_opaque.clear();
        self.active_tree.set(None);
    }

    /// `selectors::Element::opaque()`：取/建锚点，返回其**稳定地址**的 `OpaqueElement`。
    ///
    /// 对同一 `id` 重复调用返回**同一** `OpaqueElement`（地址稳定 ⇒ 键稳定）。
    pub fn opaque_for(&mut self, tree: TreeId, id: NodeId) -> OpaqueElement {
        let anchor: &NodeAnchor = self
            .anchors
            .entry(id)
            .or_insert_with(|| Box::new(NodeAnchor { tree, id }));
        let opaque = OpaqueElement::new(anchor);
        self.by_opaque.insert(opaque, id);
        opaque
    }

    /// 关联函数反查：`OpaqueElement` → `NodeId`（**纯查表，不 deref 指针 ⇒ 零 unsafe**）。
    #[must_use]
    pub fn node_of(&self, opaque: OpaqueElement) -> Option<NodeId> {
        self.by_opaque.get(&opaque).copied()
    }

    /// 只读形态：取既有锚点的 `OpaqueElement`（不插入）。
    #[must_use]
    pub fn existing_opaque(&self, id: NodeId) -> Option<OpaqueElement> {
        self.anchors.get(&id).map(|a| OpaqueElement::new(&**a))
    }
}

thread_local! {
    /// 线程局部侧表（关联函数**无 `self`** ⇒ 必须由线程局部提供，ADR-0020 决策 1）。
    static SIDE: RefCell<OpaqueSideTable> = RefCell::new(OpaqueSideTable::default());
}

/// 在本线程侧表上执行闭包（供 `Element::opaque()` / `resolve` 编排调用）。
pub fn with_side<R>(f: impl FnOnce(&mut OpaqueSideTable) -> R) -> R {
    SIDE.with(|s| f(&mut s.borrow_mut()))
}

/// 在本线程侧表上执行**只读**闭包。
pub fn with_side_ref<R>(f: impl FnOnce(&OpaqueSideTable) -> R) -> R {
    SIDE.with(|s| f(&s.borrow()))
}

/// 清空本线程侧表（`reset()`）。
pub fn reset_side() {
    SIDE.with(|s| s.borrow_mut().clear());
}

/// **镜像** stylo 的**无 `self` 关联函数**：
/// `TElement::implicit_scope_for_sheet_in_shadow_root(opaque_host, sheet_index)`
/// （`stylo-0.20.0/dom.rs:976`）。经线程局部 `SIDE` 反查宿主节点。
///
/// 这是 P-1「反查命中」判定的**核心证物**：它证明**无需 `self`**、**无需 deref 指针**
/// 即可把 `OpaqueElement` 反解为节点 —— 只需线程局部侧表。
#[must_use]
pub fn implicit_scope_for_sheet_in_shadow_root(
    opaque_host: OpaqueElement,
    _sheet_index: usize,
) -> Option<NodeId> {
    with_side_ref(|side| {
        // 必须在某次 `resolve` 中（`active_tree` 已设），否则无从谈起。
        side.active_tree.get()?;
        side.node_of(opaque_host)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// P-1 反查命中：`opaque()` 产出 → 关联函数经线程局部侧表反查命中。
    #[test]
    fn opaque_roundtrip_hits() {
        reset_side();
        let opaque = with_side(|s| {
            s.set_active_tree(7);
            s.opaque_for(7, 42)
        });
        // 关联函数（无 self）反查命中。
        assert_eq!(
            implicit_scope_for_sheet_in_shadow_root(opaque, 0),
            Some(42),
            "opaque → 关联函数反查必须命中 NodeId(42)"
        );
    }

    /// 锚点稳定：同一 id 重复调用返回**同一** `OpaqueElement`（地址稳定 ⇒ 键稳定）。
    #[test]
    fn anchor_address_is_stable() {
        reset_side();
        let (a, b) = with_side(|s| (s.opaque_for(1, 10), s.opaque_for(1, 10)));
        assert_eq!(a, b, "同 id 的 OpaqueElement 必须逐位相等（锚点地址稳定）");
        // 插入大量其它锚点（触发 HashMap rehash）后仍相等。
        let c = with_side(|s| {
            for id in 100..1200u64 {
                let _ = s.opaque_for(1, id);
            }
            s.opaque_for(1, 10)
        });
        assert_eq!(a, c, "大量插入触发 rehash 后，旧锚点地址必须仍稳定");
    }

    /// 未设 `active_tree`（不在 `resolve` 中）时反查返回 `None`（穷尽纪律）。
    #[test]
    fn no_active_tree_yields_none() {
        reset_side();
        let opaque = with_side(|s| s.opaque_for(1, 5));
        reset_side(); // 清空 ⇒ active_tree=None
        assert_eq!(
            implicit_scope_for_sheet_in_shadow_root(opaque, 0),
            None,
            "未处于 resolve 中时不得命中"
        );
    }

    /// 负对照：**另一个** OpaqueElement（不同锚点）不得反查到 42。
    #[test]
    fn distinct_opaque_does_not_collide() {
        reset_side();
        let (o1, o2) = with_side(|s| {
            s.set_active_tree(1);
            (s.opaque_for(1, 42), s.opaque_for(1, 43))
        });
        assert_ne!(o1, o2);
        assert_eq!(implicit_scope_for_sheet_in_shadow_root(o1, 0), Some(42));
        assert_eq!(implicit_scope_for_sheet_in_shadow_root(o2, 0), Some(43));
    }
}
