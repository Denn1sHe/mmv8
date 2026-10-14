//! 线程局部 **opaque 侧表**（`layout-backend::opaque`，施工图 §1.2.1；ADR-0020 决策 2）。
//!
//! `selectors::Element::opaque(&self) -> OpaqueElement` 是**必需方法、无默认实现**；
//! 而 Shadow 作用域经**无 `self` 的关联函数** `TElement::implicit_scope_for_sheet_in_shadow_root`
//! 反查宿主 ⇒ 必须能从 `OpaqueElement` 回到节点。
//!
//! **零 unsafe 论证**：`OpaqueElement::new(&*anchor)` 为**安全**函数；我方**从不**调用
//! `unsafe fn as_const_ptr` —— 只把 `OpaqueElement` 当 `Eq + Hash` **键**使用 ⇒ **无 unsafe 面**。
//! 锚点为 `Box<NodeAnchor>`（堆地址稳定，规避 `Vec<Option<_>>` append 时 realloc）。

use std::cell::{Cell, RefCell};
use std::collections::HashMap;

use selectors::OpaqueElement;
use web_dom::node::NodeId;

/// 树身份（同一 `NodeTree` 实例在一次 `resolve` 内的不透明标识）。
pub type TreeId = u64;

/// 稳定地址锚点（`OpaqueElement::new(&*anchor)` 的取址对象）。
///
/// 字段为**稳定身份记录**（`tree`/`id`）；当前仅作 `OpaqueElement` 键的稳定载体，
/// 树身份校验/作用域反查留待 S11（`shadow`）时读取。
#[allow(dead_code)]
struct NodeAnchor {
    tree: TreeId,
    id: NodeId,
}

/// 线程局部「opaque 侧表」。
#[derive(Default)]
pub struct OpaqueSideTable {
    /// 每节点一个**稳定地址**锚点（`Box` ⇒ 地址不随容器 rehash 而变）。
    anchors: HashMap<NodeId, Box<NodeAnchor>>,
    /// `OpaqueElement` → `NodeId`（关联函数据此反查）。
    by_opaque: HashMap<OpaqueElement, NodeId>,
    /// 当前 `resolve` 的树身份（关联函数**无 `self`** ⇒ 由线程局部提供）。
    active_tree: Cell<Option<TreeId>>,
}

impl OpaqueSideTable {
    /// 置当前 `resolve` 的树身份。
    pub fn set_active_tree(&self, tree: TreeId) {
        self.active_tree.set(Some(tree));
    }

    /// 当前树身份。
    #[must_use]
    pub fn active_tree(&self) -> Option<TreeId> {
        self.active_tree.get()
    }

    /// 清空侧表（每次 `resolve` 起 / `reset()` 导航重绑；ADR-0018·D2）。
    pub fn clear(&mut self) {
        self.anchors.clear();
        self.by_opaque.clear();
        self.active_tree.set(None);
    }

    /// `selectors::Element::opaque()`：取/建锚点，返回其**稳定地址**的 `OpaqueElement`。
    pub fn opaque_for(&mut self, id: NodeId) -> OpaqueElement {
        let tree = self.active_tree.get().unwrap_or(0);
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
}

thread_local! {
    /// 线程局部侧表（关联函数**无 `self`** ⇒ 必须由线程局部提供）。
    static SIDE: RefCell<OpaqueSideTable> = RefCell::new(OpaqueSideTable::default());
}

/// 在本线程侧表上执行闭包。
///
/// **`G3X-S3c2`（⑦ 同族 · 泛型返回值约束）**：返回类型 `R` **无默认值可造** ⇒ 无法"可判定错误"化
/// （同 `ElementDataWrapper` 的情形）。**不叠加借**由本模块设计保证：侧表**仅在** `with_side` 内被借，
/// 且 `with_side` **不得嵌套**（嵌套 = 编程错误；本模块不提供嵌套用法）。
pub fn with_side<R>(f: impl FnOnce(&mut OpaqueSideTable) -> R) -> R {
    SIDE.with(|side| f(&mut side.borrow_mut()))
}

/// 清空本线程侧表（`reset()`）。
pub fn reset() {
    // `G3X-S3c2`（非 Result，(β)）：**不 panic** —— 冲突 ⇒ `debug_assert!` 可见 + 跳过清理。
    SIDE.with(|side| {
        if let Ok(mut table) = side.try_borrow_mut() {
            table.clear();
        } else {
            debug_assert!(false, "opaque::reset：侧表借用冲突（宿主重入）");
        }
    });
}

#[cfg(test)]
mod tests {
    use super::{TreeId, reset, with_side};

    #[test]
    fn opaque_roundtrip_and_stability() {
        reset();
        set_active_tree_for_test(1);
        let id = web_dom::node::NodeId::new(7, 0);
        let a = opaque_for_test(id);
        let b = opaque_for_test(id);
        assert_eq!(a, b, "同 id 的 OpaqueElement 必须逐位相等（锚点地址稳定）");
        assert_eq!(node_of_for_test(a), Some(id));
        // 触发 rehash 后仍稳定。
        with_side(|side| {
            for index in 0..500u32 {
                let _ = side.opaque_for(web_dom::node::NodeId::new(index, 0));
            }
        });
        assert_eq!(opaque_for_test(id), a, "rehash 后旧锚点地址必须仍稳定");
        assert_eq!(node_of_for_test(a), Some(id));
    }

    #[test]
    fn distinct_opaque_no_collision() {
        reset();
        set_active_tree_for_test(1);
        let o1 = opaque_for_test(web_dom::node::NodeId::new(1, 0));
        let o2 = opaque_for_test(web_dom::node::NodeId::new(2, 0));
        assert_ne!(o1, o2);
        assert_eq!(node_of_for_test(o1), Some(web_dom::node::NodeId::new(1, 0)));
        assert_eq!(node_of_for_test(o2), Some(web_dom::node::NodeId::new(2, 0)));
    }

    fn set_active_tree_for_test(tree: TreeId) {
        with_side(|side| side.set_active_tree(tree));
    }
    fn opaque_for_test(id: web_dom::node::NodeId) -> selectors::OpaqueElement {
        with_side(|side| side.opaque_for(id))
    }
    fn node_of_for_test(opaque: selectors::OpaqueElement) -> Option<web_dom::node::NodeId> {
        with_side(|side| side.node_of(opaque))
    }
}
