//! `NodeId ↔ 弱句柄` 双向映射（C-3，S9）。
//!
//! key 用 `u64`，**不依赖 `web-dom`**（保持 crate 边界）。
//!
//! **持 [`WeakJsHandle`] 而非 `JsHandle`**（ADR-0013 决策 6 / C-2）：`node_map` 若持强句柄会
//! **永久钉住** wrapper，重演 ADR-0007 的泄漏（且泄漏点在白名单 crate 内部、门禁不可见）。

use std::collections::HashMap;

use crate::abi::WeakJsHandle;

/// `NodeId ↔ 弱句柄` 双向映射。
#[derive(Debug, Default)]
pub struct NodeMap {
    by_node: HashMap<u64, WeakJsHandle>,
    by_handle: HashMap<WeakJsHandle, u64>,
}

impl NodeMap {
    /// 建空映射。
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// 绑定 `node → handle`。**同一 node 复用同一 wrapper**：若该 node 已有绑定，返回其既有
    /// `WeakJsHandle` 且**不覆盖**（C-2「同一 Realm 复用同一 wrapper」）；否则插入并返回 `None`。
    pub fn bind(&mut self, node: u64, handle: WeakJsHandle) -> Option<WeakJsHandle> {
        if let Some(existing) = self.by_node.get(&node) {
            return Some(*existing);
        }
        // 若该 handle 已绑定到另一个 node，先解除其旧的反向映射，保持两向一致。
        if let Some(previous_node) = self.by_handle.insert(handle, node) {
            self.by_node.remove(&previous_node);
        }
        self.by_node.insert(node, handle);
        None
    }

    /// 取 node 的弱句柄。
    #[must_use]
    pub fn handle_for(&self, node: u64) -> Option<WeakJsHandle> {
        self.by_node.get(&node).copied()
    }

    /// **强制绑定**（**覆盖**既有）：供 `wrapper_for` 在弱句柄被回收后**重建** wrapper
    /// （ADR-0016·D3 决策 3）。
    ///
    /// 与 [`NodeMap::bind`] 的「同一 node 复用、不覆盖」不同：本方法**总是**把 `node` 绑到新
    /// `handle`，并保持两向一致（清 `node` 的旧正向、清 `handle` 的旧反向）。
    pub fn bind_force(&mut self, node: u64, handle: WeakJsHandle) {
        self.unbind(node);
        if let Some(previous_node) = self.by_handle.insert(handle, node) {
            self.by_node.remove(&previous_node);
        }
        self.by_node.insert(node, handle);
    }

    /// 取弱句柄对应的 node。
    #[must_use]
    pub fn node_for(&self, handle: WeakJsHandle) -> Option<u64> {
        self.by_handle.get(&handle).copied()
    }

    /// 解除 node 的绑定（两向一致地移除）。
    pub fn unbind(&mut self, node: u64) {
        if let Some(handle) = self.by_node.remove(&node) {
            self.by_handle.remove(&handle);
        }
    }

    /// 当前绑定数。
    #[must_use]
    pub fn len(&self) -> usize {
        self.by_node.len()
    }

    /// 是否为空。
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_node.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::NodeMap;
    use crate::abi::WeakJsHandle;

    #[test]
    fn node_map_identity() {
        // 同一 node 两次 bind ⇒ 返回同一 WeakJsHandle，且 size 不增。
        let mut map = NodeMap::new();
        let weak = WeakJsHandle::new(0, 1);
        assert_eq!(map.bind(7, weak), None, "首次绑定应返回 None");
        assert_eq!(map.bind(7, weak), Some(weak), "重复绑定应复用既有 wrapper");
        assert_eq!(map.len(), 1, "重复绑定不得新增条目");
        assert_eq!(map.handle_for(7), Some(weak));
        assert_eq!(map.node_for(weak), Some(7));
    }

    #[test]
    fn unbind_removes_both() {
        // unbind 后两向都查不到。
        let mut map = NodeMap::new();
        let weak = WeakJsHandle::new(0, 1);
        assert_eq!(map.bind(7, weak), None);
        map.unbind(7);
        assert_eq!(map.handle_for(7), None, "正向应已移除");
        assert_eq!(map.node_for(weak), None, "反向应已移除");
        assert!(map.is_empty());
    }

    #[test]
    fn rebinding_handle_keeps_both_ways_consistent() {
        // 同一 handle 绑到新 node：旧 node 的正向映射必须一并清除，保持两向一致。
        let mut map = NodeMap::new();
        let weak = WeakJsHandle::new(2, 5);
        map.bind(1, weak);
        map.bind(2, weak);
        assert_eq!(map.handle_for(1), None, "旧 node 正向映射应被清除");
        assert_eq!(map.node_for(weak), Some(2));
        assert_eq!(map.len(), 1);
    }

    #[test]
    fn bind_force_overwrites_after_collection() {
        // ADR-0016·D3：弱句柄回收后重建 wrapper ⇒ bind_force 覆盖旧绑定。
        let mut map = NodeMap::new();
        let old = WeakJsHandle::new(0, 1);
        let fresh = WeakJsHandle::new(3, 2);
        map.bind(7, old);
        map.bind_force(7, fresh);
        assert_eq!(map.handle_for(7), Some(fresh), "应覆盖为新弱句柄");
        assert_eq!(map.node_for(old), None, "旧弱句柄反向应清除");
        assert_eq!(map.node_for(fresh), Some(7));
        assert_eq!(map.len(), 1);

        // bind（非 force）仍保持「复用不覆盖」语义。
        let another = WeakJsHandle::new(9, 4);
        assert_eq!(map.bind(7, another), Some(fresh), "bind 应复用既有 wrapper");
        assert_eq!(map.handle_for(7), Some(fresh));
    }
}
