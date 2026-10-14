//! `markup5ever::TreeSink` 落到 `web-dom`（`Handle = NodeId`，ADR-0017·D10）。**零 unsafe**。
//!
//! 因 `TreeSink::elem_name<'a>(&'a self, ..) -> ExpandedName<'a>` 的返回须**借住 interned atom**，
//! 而 `NodeTree` 存的是 `String`，故 `DomSink` **自带 `HashMap<NodeId, QualName>`**（原子名缓存），
//! `elem_name` 从该缓存返回（ADR-0017·D10 定稿形态）。

use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use html5ever::tendril::StrTendril;
use html5ever::tree_builder::{ElementFlags, NextParserState, NodeOrText, QuirksMode, TreeSink};
use html5ever::{Attribute, ExpandedName, QualName};

use web_dom::node::{NodeId, NodeKind};
use web_dom::tree::NodeTree;

/// `TreeSink` 落到 `web-dom` 的适配器。持 `Rc<RefCell<NodeTree>>` + `QualName` 缓存。
#[derive(Debug)]
pub struct DomSink {
    tree: Rc<RefCell<NodeTree>>,
    qual_names: HashMap<NodeId, QualName>,
    document: NodeId,
    quirks: QuirksMode,
}

impl DomSink {
    /// 以共享节点树建 sink（文档根 = 树的根）。
    #[must_use]
    pub fn new(tree: Rc<RefCell<NodeTree>>) -> Self {
        // `G3X-S3c1`（⑥，(β) 口径）：**不 panic** —— 借用冲突 ⇒ `debug_assert!` 可见 + 保守默认（根）。
        let document = if let Ok(tree_ref) = tree.try_borrow() {
            tree_ref.root()
        } else {
            debug_assert!(false, "DomSink::new：NodeTree 借用冲突（宿主重入）");
            NodeId::new(0, 0)
        };
        Self {
            tree,
            qual_names: HashMap::new(),
            document,
            quirks: QuirksMode::NoQuirks,
        }
    }

    /// 当前 quirks 模式。
    #[must_use]
    pub fn quirks_mode(&self) -> QuirksMode {
        self.quirks
    }

    /// 放回共享节点树（消费 sink）。
    #[must_use]
    pub fn into_tree(self) -> Rc<RefCell<NodeTree>> {
        self.tree
    }

    /// 把 `NodeOrText::AppendText` 落到树上（相邻文本节点合并）。
    fn append_text(&mut self, parent: NodeId, text: &StrTendril) {
        let last = if let Ok(tree_ref) = self.tree.try_borrow() {
            tree_ref.get(parent).and_then(|node| node.last_child)
        } else {
            debug_assert!(false, "DomSink::append_text：NodeTree 借用冲突（宿主重入）");
            None
        };
        let merged = last.is_some_and(|last| {
            let mut tree = self.tree.borrow_mut();
            if tree.get(last).map(|node| node.kind) != Some(NodeKind::Text) {
                return false;
            }
            if let Some(node) = tree.get_mut(last) {
                if let Some(buffer) = node.text.as_mut() {
                    buffer.push_str(text);
                    return true;
                }
            }
            false
        });
        if !merged {
            let id = self.tree.borrow_mut().create_text(text.as_ref());
            self.tree.borrow_mut().append_child(parent, id);
        }
    }
}

impl TreeSink for DomSink {
    type Handle = NodeId;
    type Output = ();

    fn finish(self) {}

    fn parse_error(&mut self, _msg: Cow<'static, str>) {
        // G2 不暴露解析错误计数（最小面）。
    }

    fn get_document(&mut self) -> NodeId {
        self.document
    }

    fn elem_name<'a>(&'a self, target: &'a NodeId) -> ExpandedName<'a> {
        self.qual_names
            .get(target)
            .expect("elem_name 只应在元素节点上调用")
            .expanded()
    }

    fn create_element(
        &mut self,
        name: QualName,
        attrs: Vec<Attribute>,
        _flags: ElementFlags,
    ) -> NodeId {
        let local = name.local.as_ref().to_owned();
        let namespace = name.ns.as_ref().to_owned();
        let pairs: Vec<(String, String)> = attrs
            .iter()
            .map(|attr| (attr.name.local.as_ref().to_owned(), attr.value.to_string()))
            .collect();
        let id = self
            .tree
            .borrow_mut()
            .create_element(&local, &namespace, pairs);
        self.qual_names.insert(id, name);
        id
    }

    fn create_comment(&mut self, text: StrTendril) -> NodeId {
        self.tree.borrow_mut().create_comment(&text)
    }

    fn create_pi(&mut self, target: StrTendril, data: StrTendril) -> NodeId {
        // G2 无 PI 节点：以注释近似（登记最小面）。
        self.tree
            .borrow_mut()
            .create_comment(&format!("?{target} {data}?"))
    }

    fn append(&mut self, parent: &NodeId, child: NodeOrText<NodeId>) {
        match child {
            NodeOrText::AppendNode(node) => {
                self.tree.borrow_mut().append_child(*parent, node);
            }
            NodeOrText::AppendText(text) => self.append_text(*parent, &text),
        }
    }

    fn append_based_on_parent_node(
        &mut self,
        element: &NodeId,
        prev_element: &NodeId,
        child: NodeOrText<NodeId>,
    ) {
        let has_parent = self.tree.try_borrow().map_or_else(
            |_| {
                debug_assert!(
                    false,
                    "DomSink::append_based_on_parent_node：NodeTree 借用冲突"
                );
                false
            },
            |tree_ref| tree_ref.parent(*element).is_some(),
        );
        if has_parent {
            self.append_before_sibling(prev_element, child);
        } else {
            self.append(element, child);
        }
    }

    fn append_doctype_to_document(
        &mut self,
        name: StrTendril,
        public_id: StrTendril,
        system_id: StrTendril,
    ) {
        let data = format!("{name} {public_id} {system_id}");
        let id = self.tree.borrow_mut().create_doctype(data.trim());
        self.tree.borrow_mut().append_child(self.document, id);
    }

    fn mark_script_already_started(&mut self, _node: &NodeId) {}

    fn pop(&mut self, _node: &NodeId) {}

    fn get_template_contents(&mut self, target: &NodeId) -> NodeId {
        // G2 不支持 template contents：返回元素本身（登记最小面）。
        *target
    }

    fn same_node(&self, x: &NodeId, y: &NodeId) -> bool {
        x == y
    }

    fn set_quirks_mode(&mut self, mode: QuirksMode) {
        self.quirks = mode;
    }

    fn append_before_sibling(&mut self, sibling: &NodeId, new_node: NodeOrText<NodeId>) {
        let parent = self
            .tree
            .try_borrow()
            .ok()
            .and_then(|tree_ref| tree_ref.parent(*sibling));
        let Some(parent) = parent else {
            return;
        };
        match new_node {
            NodeOrText::AppendNode(node) => {
                self.tree
                    .borrow_mut()
                    .insert_before(parent, node, Some(*sibling));
            }
            NodeOrText::AppendText(text) => {
                let id = self.tree.borrow_mut().create_text(text.as_ref());
                self.tree
                    .borrow_mut()
                    .insert_before(parent, id, Some(*sibling));
            }
        }
    }

    fn add_attrs_if_missing(&mut self, target: &NodeId, attrs: Vec<Attribute>) {
        for attr in &attrs {
            let name = attr.name.local.as_ref();
            // 借用冲突 ⇒ 保守**跳过该属性**（不 mutate），避免在冲突态下改树。
            let already = self.tree.try_borrow().map_or_else(
                |_| {
                    debug_assert!(false, "DomSink::add_attrs_if_missing：NodeTree 借用冲突");
                    true
                },
                |tree_ref| tree_ref.get_attribute(*target, name).is_some(),
            );
            if already {
                continue;
            }
            self.tree
                .borrow_mut()
                .set_attribute(*target, name, attr.value.as_ref());
        }
    }

    fn associate_with_form(
        &mut self,
        _target: &NodeId,
        _form: &NodeId,
        _nodes: (&NodeId, Option<&NodeId>),
    ) {
    }

    fn remove_from_parent(&mut self, target: &NodeId) {
        let parent = self
            .tree
            .try_borrow()
            .ok()
            .and_then(|tree_ref| tree_ref.parent(*target));
        if let Some(parent) = parent {
            self.tree.borrow_mut().remove_child(parent, *target);
        }
    }

    fn reparent_children(&mut self, node: &NodeId, new_parent: &NodeId) {
        let children: Vec<NodeId> = self.tree.try_borrow().map_or_else(
            |_| {
                debug_assert!(false, "DomSink::reparent_children：NodeTree 借用冲突");
                Vec::new()
            },
            |tree_ref| tree_ref.children(*node).collect(),
        );
        for child in children {
            self.tree.borrow_mut().remove_child(*node, child);
            self.tree.borrow_mut().append_child(*new_parent, child);
        }
    }

    fn is_mathml_annotation_xml_integration_point(&self, _handle: &NodeId) -> bool {
        false
    }

    fn set_current_line(&mut self, _line_number: u64) {}

    fn complete_script(&mut self, _node: &NodeId) -> NextParserState {
        NextParserState::Continue
    }
}
