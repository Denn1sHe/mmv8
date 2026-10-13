//! 文档根 + `readyState` + quirks 模式（`web-dom::document`，G2 最小面）。

use crate::node::NodeId;
use crate::tree::NodeTree;

/// 文档就绪状态（`document.readyState`）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReadyState {
    /// `"loading"`。
    Loading,
    /// `"interactive"`。
    Interactive,
    /// `"complete"`。
    Complete,
}

impl ReadyState {
    /// `document.readyState` 字符串。
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Loading => "loading",
            Self::Interactive => "interactive",
            Self::Complete => "complete",
        }
    }
}

/// quirks 模式（`document.compatMode` 口径）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum QuirksMode {
    /// 标准模式（`"CSS1Compat"`）。
    NoQuirks,
    /// 混杂模式（`"BackCompat"`）。
    Quirks,
    /// 有限混杂模式（G2 保留，暂不外露）。
    LimitedQuirks,
}

/// 文档根：持一棵 [`NodeTree`] 与就绪/quirks/URL 状态。
#[derive(Debug)]
pub struct Document {
    tree: NodeTree,
    ready_state: ReadyState,
    quirks: QuirksMode,
    url: String,
}

impl Document {
    /// 建一份空文档（根为 `Document` 节点；`readyState = Loading`、`NoQuirks`）。
    #[must_use]
    pub fn new(url: &str) -> Self {
        Self {
            tree: NodeTree::new(),
            ready_state: ReadyState::Loading,
            quirks: QuirksMode::NoQuirks,
            url: url.to_owned(),
        }
    }

    /// 节点树（只读）。
    #[must_use]
    pub fn tree(&self) -> &NodeTree {
        &self.tree
    }

    /// 节点树（可变）。
    pub fn tree_mut(&mut self) -> &mut NodeTree {
        &mut self.tree
    }

    /// 文档根节点。
    #[must_use]
    pub fn root(&self) -> NodeId {
        self.tree.root()
    }

    /// 就绪状态。
    #[must_use]
    pub fn ready_state(&self) -> ReadyState {
        self.ready_state
    }

    /// 设置就绪状态。
    pub fn set_ready_state(&mut self, state: ReadyState) {
        self.ready_state = state;
    }

    /// quirks 模式。
    #[must_use]
    pub fn quirks(&self) -> QuirksMode {
        self.quirks
    }

    /// 设置 quirks 模式。
    pub fn set_quirks(&mut self, quirks: QuirksMode) {
        self.quirks = quirks;
    }

    /// 文档 URL（未规范化，规范化见 `policy::Origin`）。
    #[must_use]
    pub fn url(&self) -> &str {
        &self.url
    }
}

#[cfg(test)]
mod tests {
    use super::{Document, QuirksMode, ReadyState};

    #[test]
    fn document_defaults_and_transitions() {
        let mut doc = Document::new("https://example.test/");
        assert_eq!(doc.ready_state(), ReadyState::Loading);
        assert_eq!(doc.quirks(), QuirksMode::NoQuirks);
        assert_eq!(doc.url(), "https://example.test/");
        doc.set_ready_state(ReadyState::Interactive);
        assert_eq!(doc.ready_state().as_str(), "interactive");
        let root = doc.root();
        let el = doc.tree_mut().create_element("p", "html", vec![]);
        doc.tree_mut().append_child(root, el);
        assert_eq!(doc.tree().child_count(root), 1);
    }
}
