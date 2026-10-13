//! 变更记录缓冲 + `MutationObserver` 最小面（`web-dom::mutation`，ADR-0016·D5）。
//!
//! **copy-out 纪律（ADR-0016·D2）**：跨 V8 边界前必须 [`MutationBuffer::take_records`]
//! （`mem::take`）取出 **owned** 记录，**释放借用后**再回调——回调期间新产生的记录留待下一轮。

use crate::node::NodeId;

/// 变更种类。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MutationKind {
    /// 子节点增删。
    ChildList,
    /// 属性变更。
    Attributes,
    /// 字符数据变更。
    CharacterData,
}

/// 一条变更记录。
#[derive(Clone, Debug)]
pub struct MutationRecord {
    /// 变更种类。
    pub kind: MutationKind,
    /// 变更目标节点。
    pub target: NodeId,
    /// 新增子节点（`ChildList`）。
    pub added_nodes: Vec<NodeId>,
    /// 移除子节点（`ChildList`）。
    pub removed_nodes: Vec<NodeId>,
    /// 属性名（`Attributes`）。
    pub attribute_name: Option<String>,
    /// 旧值（`attributeOldValue` / `characterDataOldValue`）。
    pub old_value: Option<String>,
}

impl MutationRecord {
    /// 子节点增删记录。
    #[must_use]
    pub fn child_list(target: NodeId, added: Vec<NodeId>, removed: Vec<NodeId>) -> Self {
        Self {
            kind: MutationKind::ChildList,
            target,
            added_nodes: added,
            removed_nodes: removed,
            attribute_name: None,
            old_value: None,
        }
    }

    /// 属性变更记录。
    #[must_use]
    pub fn attributes(target: NodeId, name: &str, old_value: Option<String>) -> Self {
        Self {
            kind: MutationKind::Attributes,
            target,
            added_nodes: Vec::new(),
            removed_nodes: Vec::new(),
            attribute_name: Some(name.to_owned()),
            old_value,
        }
    }
}

/// 变更记录缓冲（追加式；交付前 `take_records` copy-out）。
#[derive(Debug, Default)]
pub struct MutationBuffer {
    records: Vec<MutationRecord>,
}

impl MutationBuffer {
    /// 建空缓冲。
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// 追加一条记录。
    pub fn record(&mut self, record: MutationRecord) {
        self.records.push(record);
    }

    /// 待交付记录（只读）。
    #[must_use]
    pub fn pending(&self) -> &[MutationRecord] {
        &self.records
    }

    /// **copy-out**：`mem::take` 出全部 owned 记录并清空缓冲（跨 V8 边界前调用）。
    pub fn take_records(&mut self) -> Vec<MutationRecord> {
        std::mem::take(&mut self.records)
    }

    /// 待交付条数。
    #[must_use]
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// 是否无待交付记录。
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }
}

/// `observe` 选项。
#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct ObserveOptions {
    /// 观察子节点增删。
    pub child_list: bool,
    /// 观察属性变更。
    pub attributes: bool,
    /// 观察整个子树。
    pub subtree: bool,
    /// 记录属性旧值。
    pub attribute_old_value: bool,
}

/// 一个观察器（回调槽 `callback` 由宿主填，G2 泛型以保持 web-dom 不依赖 engine-v8）。
#[derive(Clone, Debug)]
struct Observer<C> {
    id: u64,
    target: NodeId,
    options: ObserveOptions,
    callback: C,
}

/// 观察器注册表（最小面：`observe` / `disconnect` / 选项查询）。
///
/// 回调槽泛型 `C`（默认 `()`）——`navigation` 侧实例化为 `WeakJsHandle`（C-2），
/// 从而 `web-dom` 类型面**不依赖** `engine-v8`。
#[derive(Debug)]
pub struct ObserverRegistry<C = ()> {
    observers: Vec<Observer<C>>,
    next_id: u64,
}

impl<C> Default for ObserverRegistry<C> {
    fn default() -> Self {
        Self {
            observers: Vec::new(),
            next_id: 0,
        }
    }
}

impl<C> ObserverRegistry<C> {
    /// 建空注册表。
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// 注册一个观察器，返回其 id。
    pub fn observe(&mut self, target: NodeId, options: ObserveOptions, callback: C) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.observers.push(Observer {
            id,
            target,
            options,
            callback,
        });
        id
    }

    /// 注销一个观察器；返回是否存在。
    pub fn disconnect(&mut self, id: u64) -> bool {
        let before = self.observers.len();
        self.observers.retain(|observer| observer.id != id);
        self.observers.len() != before
    }

    /// 某观察器的选项。
    #[must_use]
    pub fn options_of(&self, id: u64) -> Option<ObserveOptions> {
        self.observers
            .iter()
            .find(|observer| observer.id == id)
            .map(|observer| observer.options)
    }

    /// 某观察器的目标节点。
    #[must_use]
    pub fn target_of(&self, id: u64) -> Option<NodeId> {
        self.observers
            .iter()
            .find(|observer| observer.id == id)
            .map(|observer| observer.target)
    }

    /// 某观察器的回调槽（引用；宿主可读取自用）。
    #[must_use]
    pub fn callback_of(&self, id: u64) -> Option<&C> {
        self.observers
            .iter()
            .find(|observer| observer.id == id)
            .map(|observer| &observer.callback)
    }

    /// 全部已注册观察器的 id（升序，供交付时逐个查选项/回调——S13 MO 交付）。
    #[must_use]
    pub fn ids(&self) -> Vec<u64> {
        self.observers.iter().map(|observer| observer.id).collect()
    }

    /// 观察器个数。
    #[must_use]
    pub fn len(&self) -> usize {
        self.observers.len()
    }

    /// 是否无观察器。
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.observers.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::{MutationBuffer, MutationRecord, ObserveOptions, ObserverRegistry};
    use crate::tree::NodeTree;

    #[test]
    fn mutation_recorded() {
        // §5 S2：变更记录缓冲 + copy-out（`take_records` 清空）。
        let mut tree = NodeTree::new();
        let root = tree.root();
        let a = tree.create_element("a", "html", vec![]);
        let b = tree.create_element("b", "html", vec![]);
        tree.append_child(root, a);
        tree.append_child(root, b);

        let mut buffer = MutationBuffer::new();
        assert!(buffer.is_empty());
        buffer.record(MutationRecord::child_list(root, vec![a, b], vec![]));
        buffer.record(MutationRecord::attributes(
            a,
            "class",
            Some("old".to_owned()),
        ));
        assert_eq!(buffer.len(), 2);
        assert_eq!(buffer.pending()[1].attribute_name.as_deref(), Some("class"));

        let taken = buffer.take_records();
        assert_eq!(taken.len(), 2, "copy-out 应取出全部记录");
        assert!(buffer.is_empty(), "take_records 应清空缓冲");
        assert_eq!(taken[0].added_nodes, vec![a, b]);
    }

    #[test]
    fn observer_observe_options_and_disconnect() {
        // §4 web-dom::mutation：observe 选项 + 注销。
        let mut tree = NodeTree::new();
        let root = tree.root();
        let node = tree.create_element("div", "html", vec![]);

        let mut registry: ObserverRegistry = ObserverRegistry::new();
        assert!(registry.is_empty());
        let options = ObserveOptions {
            child_list: true,
            attributes: true,
            subtree: true,
            attribute_old_value: false,
        };
        let id = registry.observe(node, options, ());
        assert_eq!(registry.len(), 1);
        assert_eq!(registry.options_of(id), Some(options));
        assert_eq!(registry.target_of(id), Some(node));
        assert_eq!(registry.callback_of(id), Some(&()));
        assert_eq!(registry.options_of(id + 1), None);
        assert!(registry.disconnect(id));
        assert!(registry.is_empty());
        assert!(!registry.disconnect(id), "重复注销应返回 false");
        // root 未使用告警规避：断言其为有效节点。
        assert!(tree.is_valid(root));
    }
}
