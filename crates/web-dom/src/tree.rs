//! 节点树 arena（`web-dom::tree`，ADR-0016·D1）。
//!
//! **单线程 arena**（`Vec<Option<NodeData>>` + free list）。全部按**下标**索引、无自引用指针
//! ⇒ **零 unsafe**。类型面不出现任何 v8 类型（C-2）。
//!
//! **删除 ≠ 回收**（ADR-0016·D4）：[`NodeTree::remove_child`] 只离树；[`NodeTree::dispose`]
//! 才回收槽（入 free list + **generation bump**），使 [`NodeId`] 复用防呆生效
//! （`nodeid_generation_reuse`）。

use crate::node::{ElementData, NodeData, NodeId, NodeKind};

/// 节点树：**单线程 arena**（`Vec<Option<NodeData>>` + free list）。
#[derive(Debug)]
pub struct NodeTree {
    nodes: Vec<Option<NodeData>>,
    generations: Vec<u32>,
    free: Vec<u32>,
    root: NodeId,
    len: usize,
}

impl Default for NodeTree {
    fn default() -> Self {
        Self::new()
    }
}

impl NodeTree {
    /// 建一棵仅含文档根（`NodeKind::Document`，index 0）的树。
    #[must_use]
    pub fn new() -> Self {
        let root = NodeId::new(0, 0);
        Self {
            nodes: vec![Some(NodeData::new(NodeKind::Document))],
            generations: vec![0],
            free: Vec::new(),
            root,
            len: 1,
        }
    }

    /// 文档根。
    #[must_use]
    pub fn root(&self) -> NodeId {
        self.root
    }

    /// 以指定 `generation` 基线建树（供**导航重绑**：跨导航 `NodeId` **不别名**，ADR-0018·D2）。
    ///
    /// 基线单调 ⇒ 新树的根 `NodeId` 与旧树不同（旧文档节点在新树中判失效）。
    #[must_use]
    pub fn with_generation_base(base: u32) -> Self {
        let mut tree = Self::new();
        if let Some(generation) = tree.generations.first_mut() {
            *generation = base;
        }
        tree.root = NodeId::new(0, base);
        tree
    }

    /// 分配一个槽（复用 free list；复用时沿用 dispose 已 bump 的 generation）。
    fn alloc(&mut self, data: NodeData) -> NodeId {
        if let Some(index) = self.free.pop() {
            let generation = self.generations.get(index as usize).copied().unwrap_or(0);
            if let Some(slot) = self.nodes.get_mut(index as usize) {
                *slot = Some(data);
            }
            self.len += 1;
            NodeId::new(index, generation)
        } else {
            let index = u32::try_from(self.nodes.len()).expect("web-dom arena 超过 u32 上限");
            self.nodes.push(Some(data));
            self.generations.push(0);
            self.len += 1;
            NodeId::new(index, 0)
        }
    }

    /// 只读取节点（generation 校验）。
    #[must_use]
    fn node_ref(&self, id: NodeId) -> Option<&NodeData> {
        let index = id.index() as usize;
        if self.generations.get(index).copied() != Some(id.generation()) {
            return None;
        }
        self.nodes.get(index).and_then(Option::as_ref)
    }

    /// 可变取节点（generation 校验）。
    fn node_mut(&mut self, id: NodeId) -> Option<&mut NodeData> {
        let index = id.index() as usize;
        if self.generations.get(index).copied() != Some(id.generation()) {
            return None;
        }
        self.nodes.get_mut(index).and_then(Option::as_mut)
    }

    /// 节点是否有效（存在且 generation 匹配）。
    #[must_use]
    pub fn is_valid(&self, id: NodeId) -> bool {
        self.node_ref(id).is_some()
    }

    /// 取节点数据。
    #[must_use]
    pub fn get(&self, id: NodeId) -> Option<&NodeData> {
        self.node_ref(id)
    }

    /// 取节点数据（可变）。
    pub fn get_mut(&mut self, id: NodeId) -> Option<&mut NodeData> {
        self.node_mut(id)
    }

    /// 建元素节点（标签名折叠为小写；`attrs` 按序置入）。
    pub fn create_element(
        &mut self,
        name: &str,
        namespace: &str,
        attrs: Vec<(String, String)>,
    ) -> NodeId {
        let mut element = ElementData::new(name, namespace);
        for (key, value) in attrs {
            element.attrs.set(&key, &value);
        }
        let mut data = NodeData::new(NodeKind::Element);
        data.element = Some(element);
        self.alloc(data)
    }

    /// 建文本节点。
    pub fn create_text(&mut self, data: &str) -> NodeId {
        let mut node = NodeData::new(NodeKind::Text);
        node.text = Some(data.to_owned());
        self.alloc(node)
    }

    /// 建注释节点。
    pub fn create_comment(&mut self, data: &str) -> NodeId {
        let mut node = NodeData::new(NodeKind::Comment);
        node.text = Some(data.to_owned());
        self.alloc(node)
    }

    /// 建文档类型节点（`<!DOCTYPE ...>`，供 `TreeSink::append_doctype_to_document`）。
    pub fn create_doctype(&mut self, data: &str) -> NodeId {
        let mut node = NodeData::new(NodeKind::DocumentType);
        node.text = Some(data.to_owned());
        self.alloc(node)
    }

    /// 取父节点。
    #[must_use]
    pub fn parent(&self, id: NodeId) -> Option<NodeId> {
        self.node_ref(id).and_then(|node| node.parent)
    }

    /// 按序迭代子节点（惰性、只读借用本树）。
    pub fn children(&self, id: NodeId) -> impl Iterator<Item = NodeId> + '_ {
        let first = self.node_ref(id).and_then(|node| node.first_child);
        std::iter::successors(first, move |&cur| {
            self.node_ref(cur).and_then(|node| node.next_sibling)
        })
    }

    /// 子节点个数（O(子节点数)）。
    #[must_use]
    pub fn child_count(&self, id: NodeId) -> usize {
        self.children(id).count()
    }

    /// 汇总子树文本（供脚本宿主取 `<script>` 体内文本）。
    #[must_use]
    pub fn text_content(&self, id: NodeId) -> String {
        let mut out = String::new();
        self.collect_text(id, &mut out);
        out
    }

    fn collect_text(&self, id: NodeId, out: &mut String) {
        let Some(node) = self.node_ref(id) else {
            return;
        };
        if let Some(text) = node.text.as_ref() {
            out.push_str(text);
        }
        for child in self.children(id).collect::<Vec<_>>() {
            self.collect_text(child, out);
        }
    }

    /// 把 `child` 从其当前父节点摘下（更新父子/兄弟链）。
    fn detach(&mut self, child: NodeId) {
        let Some(parent) = self.node_ref(child).and_then(|node| node.parent) else {
            return;
        };
        let (prev, next) = match self.node_ref(child) {
            Some(node) => (node.prev_sibling, node.next_sibling),
            None => return,
        };
        if let Some(node) = self.node_mut(child) {
            node.parent = None;
            node.prev_sibling = None;
            node.next_sibling = None;
        }
        if let Some(prev) = prev {
            if let Some(node) = self.node_mut(prev) {
                node.next_sibling = next;
            }
        } else if let Some(node) = self.node_mut(parent) {
            node.first_child = next;
        }
        if let Some(next) = next {
            if let Some(node) = self.node_mut(next) {
                node.prev_sibling = prev;
            }
        } else if let Some(node) = self.node_mut(parent) {
            node.last_child = prev;
        }
    }

    /// 把 `child` 链为 `parent` 的末子（前提：`child` 已离树）。
    fn link_as_last(&mut self, parent: NodeId, child: NodeId) {
        let last = self.node_ref(parent).and_then(|node| node.last_child);
        if let Some(node) = self.node_mut(child) {
            node.parent = Some(parent);
            node.prev_sibling = last;
            node.next_sibling = None;
        }
        if let Some(last) = last {
            if let Some(node) = self.node_mut(last) {
                node.next_sibling = Some(child);
            }
        } else if let Some(node) = self.node_mut(parent) {
            node.first_child = Some(child);
        }
        if let Some(node) = self.node_mut(parent) {
            node.last_child = Some(child);
        }
    }

    /// 把 `child` 链到 `reference` 之前（前提：`child` 已离树、`reference` 是 `parent` 的子）。
    fn link_before(&mut self, parent: NodeId, child: NodeId, reference: NodeId) {
        let prev = self.node_ref(reference).and_then(|node| node.prev_sibling);
        if let Some(node) = self.node_mut(child) {
            node.parent = Some(parent);
            node.next_sibling = Some(reference);
            node.prev_sibling = prev;
        }
        if let Some(prev) = prev {
            if let Some(node) = self.node_mut(prev) {
                node.next_sibling = Some(child);
            }
        } else if let Some(node) = self.node_mut(parent) {
            node.first_child = Some(child);
        }
        if let Some(node) = self.node_mut(reference) {
            node.prev_sibling = Some(child);
        }
    }

    /// `ancestor` 是否为 `node` 的祖先（含自反为假；用于防环）。
    #[must_use]
    fn is_ancestor_of(&self, ancestor: NodeId, node: NodeId) -> bool {
        let mut current = self.parent(node);
        while let Some(parent) = current {
            if parent == ancestor {
                return true;
            }
            current = self.parent(parent);
        }
        false
    }

    /// 加为末子。若 `child` 已挂在别处则先摘下；非法（含 `parent == child` 或**成环**）⇒ no-op。
    pub fn append_child(&mut self, parent: NodeId, child: NodeId) {
        if parent == child
            || !self.is_valid(parent)
            || !self.is_valid(child)
            || self.is_ancestor_of(child, parent)
        {
            return;
        }
        self.detach(child);
        self.link_as_last(parent, child);
    }

    /// 插到 `reference` 之前；`reference` 为 `None` 等价 [`NodeTree::append_child`]。
    ///
    /// `reference` 不是 `parent` 的子 ⇒ 退化为 append（不抛错）；成环 ⇒ no-op。
    pub fn insert_before(&mut self, parent: NodeId, child: NodeId, reference: Option<NodeId>) {
        let Some(reference) = reference else {
            self.append_child(parent, child);
            return;
        };
        if parent == child
            || child == reference
            || !self.is_valid(parent)
            || !self.is_valid(child)
            || !self.is_valid(reference)
            || self.is_ancestor_of(child, parent)
        {
            return;
        }
        if self.parent(reference) != Some(parent) {
            self.append_child(parent, child);
            return;
        }
        self.detach(child);
        self.link_before(parent, child, reference);
    }

    /// 从 `parent` 摘下 `child`（**只离树、不回收槽** —— ADR-0016·D4）。
    pub fn remove_child(&mut self, parent: NodeId, child: NodeId) {
        if self.parent(child) == Some(parent) {
            self.detach(child);
        }
    }

    /// 回收 `id` 所在**子树**：离树 + 释放 arena 槽（入 `free`）+ **generation bump**。
    ///
    /// 根节点不可回收（no-op）。旧 [`NodeId`] 于复用槽必须判失效（`nodeid_generation_reuse`）。
    pub fn dispose(&mut self, id: NodeId) {
        if !self.is_valid(id) || id == self.root {
            return;
        }
        self.detach(id);
        let mut stack = vec![id];
        while let Some(current) = stack.pop() {
            let index = current.index() as usize;
            stack.extend(self.children(current).collect::<Vec<_>>());
            if let Some(slot) = self.nodes.get_mut(index) {
                if slot.is_some() {
                    *slot = None;
                    if let Some(generation) = self.generations.get_mut(index) {
                        *generation = generation.wrapping_add(1);
                    }
                    self.free.push(current.index());
                    self.len = self.len.saturating_sub(1);
                }
            }
        }
    }

    /// 设元素属性（非元素 ⇒ no-op）。
    pub fn set_attribute(&mut self, id: NodeId, name: &str, value: &str) {
        if let Some(element) = self.node_mut(id).and_then(|node| node.element.as_mut()) {
            element.attrs.set(name, value);
        }
    }

    /// 取元素属性值（非元素 ⇒ `None`）。
    #[must_use]
    pub fn get_attribute(&self, id: NodeId, name: &str) -> Option<&str> {
        self.node_ref(id)
            .and_then(|node| node.element.as_ref())
            .and_then(|element| element.attrs.get(name))
    }

    /// 存活节点数。
    #[must_use]
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否无节点（含根，故恒 `false`，仅为 lint 对称）。
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

#[cfg(test)]
mod tests {
    use super::NodeTree;
    use crate::node::NodeKind;
    use proptest::prelude::*;

    fn tree_with_siblings() -> (NodeTree, [crate::node::NodeId; 3]) {
        let mut tree = NodeTree::new();
        let root = tree.root();
        let body = tree.create_element("body", "html", vec![]);
        let a = tree.create_element("a", "html", vec![]);
        let b = tree.create_element("b", "html", vec![]);
        tree.append_child(root, body);
        tree.append_child(body, a);
        tree.append_child(body, b);
        (tree, [body, a, b])
    }

    #[test]
    fn arena_create_append_insert_remove() {
        let (mut tree, [body, a, b]) = tree_with_siblings();
        assert_eq!(tree.child_count(body), 2);
        // a 之前插 c ⇒ c, a, b。
        let c = tree.create_element("c", "html", vec![]);
        tree.insert_before(body, c, Some(a));
        let ids: Vec<_> = tree.children(body).collect();
        assert_eq!(ids, vec![c, a, b], "insert_before 顺序应正确");
        // 摘 a ⇒ c, b。
        tree.remove_child(body, a);
        let ids: Vec<_> = tree.children(body).collect();
        assert_eq!(ids, vec![c, b]);
        assert_eq!(tree.parent(a), None, "摘下的节点应无父");
        assert!(tree.is_valid(a), "remove_child 不回收槽 ⇒ id 仍有效");
        // 加回末位 ⇒ c, b, a。
        tree.append_child(body, a);
        let ids: Vec<_> = tree.children(body).collect();
        assert_eq!(ids, vec![c, b, a]);
        // 文本内容汇总。
        let text = tree.create_text("hi");
        tree.append_child(a, text);
        assert_eq!(tree.text_content(body), "hi");
        assert_eq!(
            tree.get(tree.root()).map(|n| n.kind),
            Some(NodeKind::Document)
        );
    }

    #[test]
    fn nodeid_generation_reuse() {
        // §4：dispose 后复用槽 ⇒ 旧 NodeId 判失效（ADR-0016·D4）。
        let mut tree = NodeTree::new();
        let root = tree.root();
        let a = tree.create_element("a", "html", vec![]);
        tree.append_child(root, a);
        let (index, generation) = (a.index(), a.generation());

        tree.dispose(a);
        assert_eq!(tree.parent(a), None);
        assert!(!tree.is_valid(a), "dispose 后旧 id 应失效");

        // 新节点复用同一槽 ⇒ generation 已 bump，旧 id 仍失效、新 id 有效。
        let fresh = tree.create_element("b", "html", vec![]);
        assert_eq!(fresh.index(), index, "应复用同一 arena 槽");
        assert_ne!(fresh.generation(), generation, "复用槽必须 bump generation");
        assert!(!tree.is_valid(a), "旧 id 不得因复用而复活");
        assert!(tree.is_valid(fresh));

        // dispose 整个子树：子节点一并回收。
        let parent = tree.create_element("p", "html", vec![]);
        let child = tree.create_element("span", "html", vec![]);
        tree.append_child(root, parent);
        tree.append_child(parent, child);
        tree.dispose(parent);
        assert!(!tree.is_valid(parent));
        assert!(!tree.is_valid(child), "子树应一并回收");
        assert_eq!(tree.parent(parent), None);
    }

    #[test]
    fn dispose_root_is_noop() {
        let mut tree = NodeTree::new();
        let root = tree.root();
        tree.dispose(root);
        assert!(tree.is_valid(root), "根不可回收");
    }

    #[test]
    fn append_child_cycle_is_noop() {
        // 防环：把祖先挂到其后代下应被拒（否则遍历会死循环）。
        let mut tree = NodeTree::new();
        let root = tree.root();
        let parent = tree.create_element("div", "html", vec![]);
        let child = tree.create_element("span", "html", vec![]);
        tree.append_child(root, parent);
        tree.append_child(parent, child);
        // parent 是 child 的祖先 ⇒ 把 parent 挂到 child 下必须 no-op。
        tree.append_child(child, parent);
        assert_eq!(tree.parent(parent), Some(root), "祖先不得移入后代");
        assert_eq!(tree.parent(child), Some(parent));
        assert_eq!(tree.child_count(child), 0);
        // 自挂亦 no-op。
        tree.append_child(parent, parent);
        assert_eq!(tree.parent(parent), Some(root));
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(64))]

        #[test]
        fn tree_ops_state_machine(ops in proptest::collection::vec(0u8..6, 1..40)) {
            // 随机结构操作后：父子/兄弟链自洽（children 与 parent 双向一致）。
            let mut tree = NodeTree::new();
            let root = tree.root();
            let mut created: Vec<crate::node::NodeId> = Vec::new();
            for op in ops {
                match op {
                    0 | 1 => {
                        let id = tree.create_element("x", "html", vec![]);
                        tree.append_child(root, id);
                        created.push(id);
                    }
                    2 => {
                        if let Some(&target) = created.last() {
                            let id = tree.create_text("t");
                            tree.append_child(target, id);
                            created.push(id);
                        }
                    }
                    3 => {
                        if created.len() >= 2 {
                            let a = created[created.len() - 1];
                            let b = created[created.len() - 2];
                            tree.append_child(b, a);
                        }
                    }
                    4 => {
                        if let Some(&target) = created.last() {
                            tree.remove_child(root, target);
                        }
                    }
                    _ => {
                        if let Some(&target) = created.last() {
                            tree.dispose(target);
                            created.pop();
                        }
                    }
                }
            }
            // 不变量：每个有效节点的每个子节点的 parent 指回该节点。
            for id in created.iter().copied().chain(std::iter::once(root)) {
                if !tree.is_valid(id) {
                    continue;
                }
                for child in tree.children(id).collect::<Vec<_>>() {
                    prop_assert_eq!(tree.parent(child), Some(id));
                }
            }
            prop_assert!(!tree.is_empty(), "含根的树恒非空");
        }
    }
}
