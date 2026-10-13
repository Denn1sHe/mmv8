//! 文档根 + `readyState` + quirks 模式（`web-dom::document`，G2 最小面）。

use crate::error::DomError;
use crate::node::{NodeId, ShadowMode};
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

    /// `Element.attachShadow({mode})` 原生原语（G3 / S10；mode 语义见 [`NodeTree::attach_shadow`]）。
    ///
    /// 同 mode 重复 ⇒ 幂等返回既有根；mode 不同 ⇒ `Err`（G3-B13）。
    ///
    /// # Errors
    /// 见 [`NodeTree::attach_shadow`]（`InvalidNode` / `NotAnElement` / `NotSupported`）。
    pub fn attach_shadow(&mut self, host: NodeId, mode: ShadowMode) -> Result<NodeId, DomError> {
        self.tree.attach_shadow(host, mode)
    }

    /// `Element.shadowRoot`（**仅 `open` 返回**；`closed` ⇒ `None`；G3-B14）。
    ///
    /// Rust 层 [`NodeTree::shadow_root`] 返回 `(NodeId, ShadowMode)`；本访问器**按 `open` 过滤**（JS 面口径）。
    #[must_use]
    pub fn shadow_root(&self, host: NodeId) -> Option<NodeId> {
        match self.tree.shadow_root(host) {
            Some((root, ShadowMode::Open)) => Some(root),
            _ => None,
        }
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

    #[test]
    fn attach_shadow_mode_semantics() {
        use crate::node::ShadowMode;
        let mut doc = Document::new("https://example.test/");
        let host = doc.tree_mut().create_element("x-host", "html", vec![]);
        let root = doc.root();
        doc.tree_mut().append_child(root, host);

        let shadow = doc
            .attach_shadow(host, ShadowMode::Open)
            .expect("first attach");
        // 同 mode 重复 ⇒ 幂等返回既有根（不新建）。
        let again = doc
            .attach_shadow(host, ShadowMode::Open)
            .expect("idempotent");
        assert_eq!(shadow, again, "同 mode 应幂等返回既有根");
        // mode 不同 ⇒ Err。
        assert!(
            doc.attach_shadow(host, ShadowMode::Closed).is_err(),
            "mode 不同必须报错（G3-B13）"
        );
    }

    #[test]
    fn shadow_root_open_only_via_js() {
        use crate::node::ShadowMode;
        let mut doc = Document::new("https://example.test/");
        let root = doc.root();

        let open_host = doc.tree_mut().create_element("x-open", "html", vec![]);
        doc.tree_mut().append_child(root, open_host);
        let open_shadow = doc
            .attach_shadow(open_host, ShadowMode::Open)
            .expect("open");
        assert_eq!(doc.shadow_root(open_host), Some(open_shadow), "open 应可见");

        let closed_host = doc.tree_mut().create_element("x-closed", "html", vec![]);
        doc.tree_mut().append_child(root, closed_host);
        doc.attach_shadow(closed_host, ShadowMode::Closed)
            .expect("closed");
        assert_eq!(
            doc.shadow_root(closed_host),
            None,
            "closed 应返回 None（G3-B14）"
        );
    }
}
