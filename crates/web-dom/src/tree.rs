//! 节点树 arena（`web-dom::tree`，ADR-0016·D1）。
//!
//! **单线程 arena**（`Vec<Option<NodeData>>` + free list）。全部按**下标**索引、无自引用指针
//! ⇒ **零 unsafe**。类型面不出现任何 v8 类型（C-2）。
//!
//! **删除 ≠ 回收**（ADR-0016·D4）：[`NodeTree::remove_child`] 只离树；[`NodeTree::dispose`]
//! 才回收槽（入 free list + **generation bump**），使 [`NodeId`] 复用防呆生效
//! （`nodeid_generation_reuse`）。

use crate::error::DomError;
use crate::node::{ElementData, NodeData, NodeId, NodeKind, ShadowMode, ShadowRootData};

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

    // ================= G3 D6：Shadow DOM 最小面（S2，§1.2） =================

    /// 取 `host` 的影子根 id（若有；不过滤 `mode`）。
    #[must_use]
    pub fn element_shadow(&self, host: NodeId) -> Option<NodeId> {
        self.node_ref(host)
            .and_then(|node| node.element.as_ref())
            .and_then(|element| element.shadow)
    }

    /// 给 `host`（Element）挂影子根。**mode 语义定死（G3-B13）**：
    ///
    /// - 已存在且 **mode 相同** ⇒ **幂等**返回既有根（**不新建**节点）；
    /// - 已存在且 **mode 不同** ⇒ `Err(DomErrorKind::NotSupported)`（对齐规范 `NotSupportedError`）；
    /// - `host` 不是本树有效节点 ⇒ `Err(InvalidNode)`；不是元素 ⇒ `Err(NotAnElement)`。
    ///
    /// 影子根是**独立节点**（`NodeKind::ShadowRoot`），**不进入** host 的 light 子链
    /// ⇒ light 遍历天然不含影子内容（作用域隔离，G3-B8）。
    ///
    /// # Errors
    /// - `host` 不是本树有效节点 ⇒ [`DomErrorKind::InvalidNode`](crate::error::DomErrorKind::InvalidNode)；
    /// - `host` 不是元素（含文本 / 文档根 / 影子根本身）⇒
    ///   [`DomErrorKind::NotAnElement`](crate::error::DomErrorKind::NotAnElement)；
    /// - `host` 已存在**不同 mode** 的影子根 ⇒
    ///   [`DomErrorKind::NotSupported`](crate::error::DomErrorKind::NotSupported)。
    pub fn attach_shadow(&mut self, host: NodeId, mode: ShadowMode) -> Result<NodeId, DomError> {
        let Some(node) = self.node_ref(host) else {
            return Err(DomError::invalid_node(
                "attach_shadow: host 不是本树的有效节点",
            ));
        };
        if node.kind != NodeKind::Element {
            return Err(DomError::not_an_element(
                "attach_shadow: host 必须是元素（ShadowRoot 不是 Element）",
            ));
        }
        if let Some(existing) = node.element.as_ref().and_then(|element| element.shadow) {
            let existing_mode = self
                .node_ref(existing)
                .and_then(|root| root.shadow_root)
                .map(|data| data.mode);
            return match existing_mode {
                Some(m) if m == mode => Ok(existing),
                _ => Err(DomError::not_supported(
                    "attach_shadow: host 已存在不同 mode 的影子根",
                )),
            };
        }
        let mut data = NodeData::new(NodeKind::ShadowRoot);
        data.shadow_root = Some(ShadowRootData { mode, host });
        let root = self.alloc(data);
        if let Some(element) = self.node_mut(host).and_then(|node| node.element.as_mut()) {
            element.shadow = Some(root);
        }
        Ok(root)
    }

    /// 影子根（**Rust 面返回 `(NodeId, ShadowMode)`**；JS 层按 `open` 过滤 —— G3-B14）。
    #[must_use]
    pub fn shadow_root(&self, host: NodeId) -> Option<(NodeId, ShadowMode)> {
        let root = self.element_shadow(host)?;
        let mode = self
            .node_ref(root)
            .and_then(|node| node.shadow_root)
            .map(|data| data.mode)?;
        Some((root, mode))
    }

    /// 影子根的宿主元素（`root` 非影子根 ⇒ `None`）。
    #[must_use]
    pub fn shadow_host(&self, root: NodeId) -> Option<NodeId> {
        self.node_ref(root)
            .and_then(|node| node.shadow_root)
            .map(|data| data.host)
    }

    /// 该节点是否影子根。
    #[must_use]
    pub fn is_shadow_root(&self, id: NodeId) -> bool {
        self.node_ref(id)
            .is_some_and(|node| node.kind == NodeKind::ShadowRoot)
    }

    /// 沿父链上溯，返回 `node` 所在的**影子根**（无则 `None`）。
    #[must_use]
    pub fn containing_shadow_root(&self, node: NodeId) -> Option<NodeId> {
        let mut current = Some(node);
        while let Some(id) = current {
            if self.is_shadow_root(id) {
                return Some(id);
            }
            current = self.parent(id);
        }
        None
    }

    /// `slot` 的**被分配节点**（默认 + 具名简单匹配；**按需计算，无持久表** —— D6/R10）。
    ///
    /// - `slot` 非 `<slot>` 元素、或不在影子树内 ⇒ 空。
    /// - 默认 slot（`name` 缺省/空）收**无 `slot` 属性**（含空值与文本节点）的 host light 子；
    ///   具名 slot（`name="x"`）收 `slot="x"` 的 host light 子。
    #[must_use]
    pub fn assigned_nodes(&self, slot: NodeId) -> Vec<NodeId> {
        let Some(slot_name) = self.slot_name(slot) else {
            return Vec::new();
        };
        let Some(root) = self.containing_shadow_root(slot) else {
            return Vec::new();
        };
        let Some(host) = self.shadow_host(root) else {
            return Vec::new();
        };
        let default_slot = slot_name.is_empty();
        let mut out = Vec::new();
        for child in self.children(host).collect::<Vec<_>>() {
            let assigned = match self.light_slot_attr(child).as_deref() {
                // 无 `slot` 属性（含文本节点）或空值 ⇒ 仅默认 slot 收。
                None | Some("") => default_slot,
                // 具名 `slot` 属性 ⇒ 精确匹配具名 slot。
                Some(name) => !default_slot && name == slot_name,
            };
            if assigned {
                out.push(child);
            }
        }
        out
    }

    /// **composed/flattened 遍历**（跨 shadow 边界）供布局使用。
    ///
    /// - `node` 为影子根 ⇒ 其子节点（slot 展开）；
    /// - `node` 为有影子根的元素 ⇒ 影子根的子节点（slot 展开）；
    /// - 否则 ⇒ light 子节点。
    ///
    /// slot 展开：`<slot>` ⇒ 其被分配节点；无被分配节点 ⇒ slot **回退内容**（其自身 light 子）。
    #[must_use]
    pub fn flattened_children(&self, node: NodeId) -> Vec<NodeId> {
        if self.is_shadow_root(node) {
            return self.expand_slots(self.children(node).collect());
        }
        if let Some(root) = self.element_shadow(node) {
            if self.is_valid(root) {
                return self.expand_slots(self.children(root).collect());
            }
        }
        self.children(node).collect()
    }

    /// `<slot>` 元素的 `name` 属性（缺省空串）。非 `<slot>` 元素 ⇒ `None`。
    #[must_use]
    fn slot_name(&self, slot: NodeId) -> Option<String> {
        let element = self.node_ref(slot)?.element.as_ref()?;
        if element.name != "slot" {
            return None;
        }
        Some(element.attrs.get("name").unwrap_or("").to_owned())
    }

    /// 节点是否 `<slot>` 元素。
    #[must_use]
    fn is_slot_element(&self, node: NodeId) -> bool {
        self.node_ref(node)
            .and_then(|data| data.element.as_ref())
            .is_some_and(|element| element.name == "slot")
    }

    /// 节点的 `slot` 属性（元素；非元素 ⇒ `None` ≡ 无该属性）。
    #[must_use]
    fn light_slot_attr(&self, node: NodeId) -> Option<String> {
        let element = self.node_ref(node)?.element.as_ref()?;
        Some(element.attrs.get("slot").unwrap_or("").to_owned())
    }

    /// 对一组子节点做 slot 展开（slot ⇒ 被分配节点，或回退内容）。
    fn expand_slots(&self, children: Vec<NodeId>) -> Vec<NodeId> {
        let mut out = Vec::new();
        for child in children {
            if self.is_slot_element(child) {
                let assigned = self.assigned_nodes(child);
                if assigned.is_empty() {
                    out.extend(self.children(child));
                } else {
                    out.extend(assigned);
                }
            } else {
                out.push(child);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::NodeTree;
    use crate::error::DomErrorKind;
    use crate::node::{NodeId, NodeKind, ShadowMode};
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

    #[test]
    fn attach_shadow_mode_semantics() {
        // §3.5 / G3-B13：同 mode 幂等返回既有根；不同 mode ⇒ Err(NotSupported)。
        let mut tree = NodeTree::new();
        let root = tree.root();
        let host = tree.create_element("div", "html", vec![]);
        tree.append_child(root, host);

        let r1 = tree.attach_shadow(host, ShadowMode::Open).unwrap();
        assert!(tree.is_shadow_root(r1), "影子根应为 NodeKind::ShadowRoot");
        assert_eq!(tree.shadow_root(host), Some((r1, ShadowMode::Open)));
        assert_eq!(tree.shadow_host(r1), Some(host));
        let len_after_first = tree.len();

        // 同 mode 重复 ⇒ 幂等返回既有根，**不新建**节点。
        let r2 = tree.attach_shadow(host, ShadowMode::Open).unwrap();
        assert_eq!(r2, r1, "同 mode 重复应幂等返回既有根");
        assert_eq!(tree.len(), len_after_first, "幂等不得新建节点");

        // 不同 mode ⇒ Err(NotSupported)，且不得改动既有根。
        let err = tree.attach_shadow(host, ShadowMode::Closed).unwrap_err();
        assert_eq!(err.kind(), DomErrorKind::NotSupported);
        assert_eq!(
            tree.shadow_root(host),
            Some((r1, ShadowMode::Open)),
            "失败不得改动既有根"
        );

        // 非元素 host ⇒ Err(NotAnElement)；非法节点 ⇒ Err(InvalidNode)。
        let text = tree.create_text("x");
        tree.append_child(host, text);
        assert_eq!(
            tree.attach_shadow(text, ShadowMode::Open)
                .unwrap_err()
                .kind(),
            DomErrorKind::NotAnElement
        );
        assert_eq!(
            tree.attach_shadow(NodeId::new(9999, 0), ShadowMode::Open)
                .unwrap_err()
                .kind(),
            DomErrorKind::InvalidNode
        );
        // 文档根也不是元素。
        assert_eq!(
            tree.attach_shadow(root, ShadowMode::Open)
                .unwrap_err()
                .kind(),
            DomErrorKind::NotAnElement
        );
    }

    #[test]
    fn assigned_nodes_default_and_named() {
        // §3.5：默认 + 具名简单匹配；按需计算、无持久表。
        let mut tree = NodeTree::new();
        let root = tree.root();
        let host = tree.create_element("x-host", "html", vec![]);
        tree.append_child(root, host);
        let div = tree.create_element("div", "html", vec![]);
        let span = tree.create_element("span", "html", vec![("slot".to_owned(), "x".to_owned())]);
        let bold = tree.create_element("b", "html", vec![("slot".to_owned(), "y".to_owned())]);
        let text = tree.create_text("t");
        for child in [div, span, bold, text] {
            tree.append_child(host, child);
        }

        let shadow = tree.attach_shadow(host, ShadowMode::Open).unwrap();
        let default_slot = tree.create_element("slot", "html", vec![]);
        let named_slot =
            tree.create_element("slot", "html", vec![("name".to_owned(), "x".to_owned())]);
        tree.append_child(shadow, default_slot);
        tree.append_child(shadow, named_slot);

        assert_eq!(
            tree.assigned_nodes(default_slot),
            vec![div, text],
            "默认 slot 收无 slot 属性者（含文本）"
        );
        assert_eq!(
            tree.assigned_nodes(named_slot),
            vec![span],
            "具名 slot x 收 slot=x"
        );
        // 非 `<slot>` 元素 / 不在影子树 ⇒ 空。
        assert!(tree.assigned_nodes(div).is_empty(), "非 slot 元素应为空");
        assert!(tree.assigned_nodes(bold).is_empty(), "不在影子树应为空");
        assert!(tree.assigned_nodes(root).is_empty());
    }

    #[test]
    fn flattened_children_crosses_shadow() {
        // §4：flattened 跨 shadow 边界 + slot 展开；light 不含影子内容。
        let mut tree = NodeTree::new();
        let root = tree.root();
        let host = tree.create_element("x-host", "html", vec![]);
        tree.append_child(root, host);
        let span = tree.create_element("span", "html", vec![]);
        tree.append_child(host, span);

        let shadow = tree.attach_shadow(host, ShadowMode::Open).unwrap();
        let inner = tree.create_element("div", "html", vec![]);
        let slot = tree.create_element("slot", "html", vec![]);
        tree.append_child(shadow, inner);
        tree.append_child(shadow, slot);

        // light 遍历：仅 host 自身 light 子（不含影子内容）。
        assert_eq!(tree.children(host).collect::<Vec<_>>(), vec![span]);
        // flattened：影子根子节点（slot 展开为被分配 light 子）⇒ 跨 shadow。
        assert_eq!(
            tree.flattened_children(host),
            vec![inner, span],
            "flattened 应跨 shadow 且展开 slot"
        );
        // 影子根自身的 flattened 亦然。
        assert_eq!(tree.flattened_children(shadow), vec![inner, span]);
        // 普通元素（无影子）flattened == children。
        assert_eq!(tree.flattened_children(inner), Vec::new());
        assert_eq!(tree.flattened_children(root), vec![host]);
    }

    #[test]
    fn shadow_node_hidden_from_light_traversal() {
        // §3.5 判别力核心（G3-B8）：light 遍历不含影子内容；影子根为独立节点。
        let mut tree = NodeTree::new();
        let root = tree.root();
        let host = tree.create_element("div", "html", vec![]);
        tree.append_child(root, host);
        let shadow = tree.attach_shadow(host, ShadowMode::Open).unwrap();
        let secret = tree.create_element("p", "html", vec![]);
        tree.append_child(shadow, secret);

        assert_eq!(tree.child_count(host), 0, "影子内容不得进入 light 子链");
        assert!(
            tree.children(host).all(|child| child != secret),
            "light 遍历不得含影子节点"
        );
        assert_eq!(tree.parent(secret), Some(shadow), "影子内容父为影子根");
        assert_eq!(tree.parent(shadow), None, "影子根独立于 light 树（无父）");
        // 但 flattened 可见（参与布局）。
        assert_eq!(tree.flattened_children(host), vec![secret]);
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
