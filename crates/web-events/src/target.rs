//! 目标监听器注册表（`web-events::target`）。
//!
//! 注册/移除按 `(type, callback, capture)` 匹配（ADR-0019·D3）；`once` 与 `removeEventListener`
//! 共用「**移除即置 dead**」路径（ADR-0019·D2）。`add` 去重：已存在同键的**存活**条目 ⇒ `no-op`。
//! `listeners` 只返回**候选** id；**活性在调用时**由编排层查（[`ListenerRegistry::is_live`]）。

use engine_v8::WeakJsHandle;
use web_dom::node::NodeId;

use crate::listener::{ListenerId, ListenerOptions, ListenerRef};

#[derive(Clone, Debug)]
struct Entry {
    id: ListenerId,
    target: NodeId,
    type_: String,
    listener: ListenerRef,
    dead: bool,
}

/// 目标 → 监听器列表（单线程；`(target, type)` 键下的注册序）。
#[derive(Debug, Default)]
pub struct ListenerRegistry {
    entries: Vec<Entry>,
    next_id: u64,
}

impl ListenerRegistry {
    /// 建空注册表。
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// 注册监听器；已存在同 `(type, callback, capture)` 的**存活**条目 ⇒ `no-op`，返回其 id。
    pub fn add(
        &mut self,
        target: NodeId,
        type_: &str,
        callback: WeakJsHandle,
        options: ListenerOptions,
    ) -> ListenerId {
        if let Some(existing) = self.entries.iter().find(|entry| {
            !entry.dead
                && entry.target == target
                && entry.type_.as_str() == type_
                && entry.listener.callback == callback
                && entry.listener.options.capture == options.capture
        }) {
            return existing.id;
        }
        let id = ListenerId::from_raw(self.next_id);
        self.next_id += 1;
        self.entries.push(Entry {
            id,
            target,
            type_: type_.to_owned(),
            listener: ListenerRef { callback, options },
            dead: false,
        });
        id
    }

    /// 移除：**只按 `(type, callback, capture)` 匹配**（忽略 `passive`/`once`）；命中即置 dead。
    pub fn remove(&mut self, target: NodeId, type_: &str, callback: WeakJsHandle, capture: bool) {
        for entry in &mut self.entries {
            if !entry.dead
                && entry.target == target
                && entry.type_.as_str() == type_
                && entry.listener.callback == callback
                && entry.listener.options.capture == capture
            {
                entry.dead = true;
            }
        }
    }

    /// 取该 `(target, type)` 的**候选**监听器 id（存活者，注册序）。
    #[must_use]
    pub fn listeners(&self, target: NodeId, type_: &str) -> Vec<ListenerId> {
        self.entries
            .iter()
            .filter(|entry| !entry.dead && entry.target == target && entry.type_.as_str() == type_)
            .map(|entry| entry.id)
            .collect()
    }

    /// 该 id 当前是否仍有效（未被 `once`/`remove` 置 dead）。
    #[must_use]
    pub fn is_live(&self, id: ListenerId) -> bool {
        self.entries
            .iter()
            .any(|entry| entry.id == id && !entry.dead)
    }

    /// 取该 id 的监听器条目（仅存活者）。
    #[must_use]
    pub fn get(&self, id: ListenerId) -> Option<ListenerRef> {
        self.entries
            .iter()
            .find(|entry| entry.id == id && !entry.dead)
            .map(|entry| entry.listener)
    }

    /// 置某 id 为 dead（`once` 触发后由编排层调用；`remove` 走同一路径语义）。
    pub fn mark_dead(&mut self, id: ListenerId) {
        for entry in &mut self.entries {
            if entry.id == id {
                entry.dead = true;
            }
        }
    }

    /// 存活监听器个数。
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.iter().filter(|entry| !entry.dead).count()
    }

    /// 是否无存活监听器。
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::ListenerRegistry;
    use crate::listener::ListenerOptions;
    use engine_v8::WeakJsHandle;
    use web_dom::tree::NodeTree;

    fn target() -> (NodeTree, web_dom::node::NodeId, WeakJsHandle) {
        let tree = NodeTree::new();
        let root = tree.root();
        (tree, root, WeakJsHandle::new(1, 0))
    }

    #[test]
    fn duplicate_add_is_noop() {
        // §4 / ADR-0019·D3：同 (type, callback, capture) 去重。
        let (_tree, root, cb) = target();
        let mut registry = ListenerRegistry::new();
        let first = registry.add(root, "click", cb, ListenerOptions::default());
        let second = registry.add(root, "click", cb, ListenerOptions::default());
        assert_eq!(first, second, "重复 add 应 no-op 并返回同一 id");
        assert_eq!(registry.len(), 1);

        // 不同 capture ⇒ 视为不同条目。
        let capture = ListenerOptions {
            capture: true,
            ..ListenerOptions::default()
        };
        let third = registry.add(root, "click", cb, capture);
        assert_ne!(first, third);
        assert_eq!(registry.len(), 2);

        // 不同 type/callback 亦不同。
        let other_type = registry.add(root, "keydown", cb, ListenerOptions::default());
        let other_cb = registry.add(
            root,
            "click",
            WeakJsHandle::new(2, 0),
            ListenerOptions::default(),
        );
        assert_ne!(other_type, other_cb);
        assert_eq!(registry.len(), 4);
    }

    #[test]
    fn remove_ignores_passive_once() {
        // §4 / ADR-0019·D3：remove 只按 (type, callback, capture)，忽略 passive/once。
        let (_tree, root, cb) = target();
        let mut registry = ListenerRegistry::new();
        let passive = ListenerOptions {
            passive: true,
            once: true,
            ..ListenerOptions::default()
        };
        let id = registry.add(root, "click", cb, passive);
        assert!(registry.is_live(id));
        registry.remove(root, "click", cb, false);
        assert!(!registry.is_live(id), "remove 应命中（忽略 passive/once）");
        assert!(registry.is_empty());
        // 移除后可再次添加（新 id）。
        let again = registry.add(root, "click", cb, ListenerOptions::default());
        assert_ne!(again, id);
        assert!(registry.is_live(again));
    }
}
