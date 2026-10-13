//! 可暂停 / 恢复的解析驱动（ADR-0006 / ADR-0017·D7）。
//!
//! **必须自建循环**（不得用一次性 `parse_document`）；`</script>` 暂停点由 `TreeBuilder` 经
//! `TokenSinkResult::Script(node)` 返回（U-1 源码级成立）。`document.write` 产出经
//! [`ParserHost::enqueue_document_write`] 以 **`push_front`** 表达插入点，**上层不直接操纵
//! `BufferQueue`**（ADR-0006）。

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use html5ever::ParseOpts;
use html5ever::tendril::StrTendril;
use html5ever::tokenizer::{BufferQueue, Tokenizer, TokenizerResult};
use html5ever::tree_builder::{QuirksMode, TreeBuilder, TreeSink};

use web_dom::node::NodeId;
use web_dom::tree::NodeTree;

use crate::sink::DomSink;

/// 解析驱动结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ParseOutcome {
    /// 当前输入已消化完（不阻塞）。
    Done,
    /// 在 `</script>` 处暂停，携带**脚本节点**（`Sink::Handle = NodeId`）。
    BlockedOnScript(NodeId),
}

/// 自建的 HTML 解析宿主（`Tokenizer<TreeBuilder<NodeId, DomSink>>` + 输入队列 + 写缓冲）。
pub struct ParserHost {
    tokenizer: Tokenizer<TreeBuilder<NodeId, DomSink>>,
    input: BufferQueue,
    pending_writes: VecDeque<StrTendril>,
}

impl ParserHost {
    /// 以共享节点树与解析选项建宿主。
    #[must_use]
    pub fn new(tree: Rc<RefCell<NodeTree>>, opts: ParseOpts) -> Self {
        let tree_builder = TreeBuilder::new(DomSink::new(tree), opts.tree_builder);
        let tokenizer = Tokenizer::new(tree_builder, opts.tokenizer);
        Self {
            tokenizer,
            input: BufferQueue::default(),
            pending_writes: VecDeque::new(),
        }
    }

    /// 喂入一个 chunk；返回 `Done` 或 `BlockedOnScript(node)`。
    ///
    /// 暂停点**对 `type` 无感**：`<script type=module>` 同样返回 `BlockedOnScript`——由调用方判
    /// `type` 分流（§2.1 模块脚本条款）。
    pub fn feed(&mut self, chunk: &str) -> ParseOutcome {
        self.input.push_back(StrTendril::from(chunk));
        self.drive()
    }

    /// `document.write` 产出：登记待注入文本（**上层不直接操纵 `BufferQueue`**）。
    ///
    /// 多条写入由 [`ParserHost::resume`] **逆序 `push_front`** 注入以保证与**调用序**一致。
    pub fn enqueue_document_write(&mut self, text: &str) {
        self.pending_writes.push_back(StrTendril::from(text));
    }

    /// 脚本执行完毕，恢复解析。注入 `pending_writes` 后继续 `feed`。
    pub fn resume(&mut self) -> ParseOutcome {
        // `pending_writes` 是调用序 [a, b]；逐条 `push_front` 会倒置 ⇒ 逆序注入。
        while let Some(text) = self.pending_writes.pop_back() {
            self.input.push_front(text);
        }
        self.drive()
    }

    /// 当前 quirks 模式（由 `TreeSink::set_quirks_mode` 记录）。
    #[must_use]
    pub fn quirks_mode(&self) -> QuirksMode {
        self.tokenizer.sink.sink.quirks_mode()
    }

    /// 收尾：消化剩余输入、`Tokenizer::end()`，再 `TreeSink::finish`。
    pub fn finish(mut self) {
        while let TokenizerResult::Script(_) = self.tokenizer.feed(&mut self.input) {}
        self.tokenizer.end();
        self.tokenizer.sink.sink.finish();
    }

    fn drive(&mut self) -> ParseOutcome {
        match self.tokenizer.feed(&mut self.input) {
            TokenizerResult::Done => ParseOutcome::Done,
            TokenizerResult::Script(node) => ParseOutcome::BlockedOnScript(node),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ParseOutcome, ParserHost};
    use html5ever::ParseOpts;
    use std::cell::RefCell;
    use std::rc::Rc;
    use web_dom::node::{NodeId, NodeKind};
    use web_dom::tree::NodeTree;

    fn make_host() -> (Rc<RefCell<NodeTree>>, ParserHost) {
        let tree = Rc::new(RefCell::new(NodeTree::new()));
        let parser = ParserHost::new(Rc::clone(&tree), ParseOpts::default());
        (tree, parser)
    }

    fn find_by_tag(tree: &NodeTree, root: NodeId, tag: &str) -> Option<NodeId> {
        for child in tree.children(root) {
            if let Some(element) = tree.get(child).and_then(|node| node.element.as_ref()) {
                if element.name == tag {
                    return Some(child);
                }
            }
            if let Some(found) = find_by_tag(tree, child, tag) {
                return Some(found);
            }
        }
        None
    }

    /// 按**文档序**（前序 DFS）收集元素标签名。
    fn element_names_in_order(tree: &NodeTree, root: NodeId) -> Vec<String> {
        let mut names = Vec::new();
        for child in tree.children(root) {
            if let Some(element) = tree.get(child).and_then(|node| node.element.as_ref()) {
                names.push(element.name.clone());
            }
            names.extend(element_names_in_order(tree, child));
        }
        names
    }

    #[test]
    fn streaming_parse_incremental_nodes() {
        // §3.1：未喂完即产出（证增量）。
        let (tree, mut host) = make_host();
        assert_eq!(host.feed("<div>hi"), ParseOutcome::Done, "未完整标签不暂停");
        let snapshot = tree.borrow();
        assert!(snapshot.len() > 1, "增量解析应已建节点（含 html/body/div）");
        assert!(find_by_tag(&snapshot, snapshot.root(), "div").is_some());
    }

    #[test]
    fn parser_handles_split_tags_across_chunks() {
        // §3.1：标签跨 chunk 边界仍正确（`<di` + `v>` ⇒ `<div>`）。
        let (tree, mut host) = make_host();
        assert_eq!(host.feed("<di"), ParseOutcome::Done);
        assert_eq!(host.feed("v>x</div>"), ParseOutcome::Done);
        let snapshot = tree.borrow();
        assert!(
            find_by_tag(&snapshot, snapshot.root(), "div").is_some(),
            "跨 chunk 的 <div> 应被正确解析"
        );
    }

    #[test]
    fn tree_building_builds_expected_shape() {
        // §3.1：`<html><body><p>x` ⇒ 结构正确，html/head/body 按规范补全。
        let (tree, mut host) = make_host();
        host.feed("<p>x");
        host.finish();
        let snapshot = tree.borrow();
        let root = snapshot.root();
        for tag in ["html", "head", "body", "p"] {
            assert!(
                find_by_tag(&snapshot, root, tag).is_some(),
                "规范应补全 <{tag}>"
            );
        }
    }

    #[test]
    fn sink_maps_element_text_comment() {
        // §3.1：`TreeSink` 回调落到 NodeTree 的 Element/Text/Comment。
        let (tree, mut host) = make_host();
        host.feed("<div><!--c-->text</div>");
        let snapshot = tree.borrow();
        let root = snapshot.root();
        let div = find_by_tag(&snapshot, root, "div").expect("应有 div");
        let kinds: Vec<NodeKind> = snapshot
            .children(div)
            .filter_map(|child| snapshot.get(child).map(|node| node.kind))
            .collect();
        assert!(kinds.contains(&NodeKind::Comment), "应有注释子节点");
        assert!(kinds.contains(&NodeKind::Text), "应有文本子节点");
    }

    #[test]
    fn blocking_script_pauses_parser() {
        // §3.2：内联 `<script>` ⇒ feed 返回 BlockedOnScript(node)（P-2 全栈回归）。
        let (tree, mut host) = make_host();
        let outcome = host.feed("<html><body><script>var x = 1;</script>");
        let ParseOutcome::BlockedOnScript(node) = outcome else {
            panic!("应在 </script> 暂停，实得 {outcome:?}");
        };
        let snapshot = tree.borrow();
        assert_eq!(
            snapshot.get(node).map(|n| n.kind),
            Some(NodeKind::Element),
            "暂停点应携带 <script> 元素本身"
        );
        assert_eq!(
            snapshot
                .get(node)
                .and_then(|n| n.element.as_ref())
                .map(|e| e.name.as_str()),
            Some("script")
        );
    }

    #[test]
    fn document_write_injection_order() {
        // §3.3 / ADR-0006：多条 write 注入序 == 调用序。
        // **强化：断言 DOM 实际节点顺序**（`<a>` 必须先于 `<b>`）——注入序一旦倒置即必红。
        let (tree, mut host) = make_host();
        host.feed("<body>");
        host.enqueue_document_write("<a></a>");
        host.enqueue_document_write("<b></b>");
        host.resume();
        host.finish();
        let snapshot = tree.borrow();
        let root = snapshot.root();
        let names = element_names_in_order(&snapshot, root);
        let a_index = names.iter().position(|name| name == "a").expect("应有 <a>");
        let b_index = names.iter().position(|name| name == "b").expect("应有 <b>");
        assert!(
            a_index < b_index,
            "注入序应为 a 先于 b（实得文档序 {names:?}）"
        );
    }
}
