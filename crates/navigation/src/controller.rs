//! 导航协调器（`navigation::controller`，ADR-0018·D2/D6）。
//!
//! 持 URL / origin / `readyState` / load blockers / **导航代次**（`policy::NavigationEpoch`）。
//! **本控制器只动导航状态**；文档 / 树 / 绑定的**重建**由 `BrowsingContext::load` 编排
//! （S13，ADR-0018·D2）。

use std::collections::BTreeSet;

use policy::{NavigationEpoch, Origin};
use web_dom::node::NodeId;

use crate::error::{PageError, PageErrorKind};

/// 文档就绪状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReadyState {
    /// `"loading"`。
    Loading,
    /// `"interactive"`（解析完成、defer/module 脚本执行前）。
    Interactive,
    /// `"complete"`（blockers 清零 + load）。
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

/// load blocker 种类（实际 id 记账在 `BTreeSet<u64>`）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LoadBlockerKind {
    /// 外部脚本。
    ExternalScript,
    /// 样式表。
    Stylesheet,
    /// 图片。
    Image,
    /// 其他。
    Other,
}

/// 导航协调器。
#[derive(Debug)]
pub struct NavigationController {
    url: String,
    origin: Origin,
    ready_state: ReadyState,
    epoch: NavigationEpoch,
    load_blockers: BTreeSet<u64>,
    document: NodeId,
    next_blocker: u64,
    parse_finished: bool,
}

impl NavigationController {
    /// 以初始 URL 建协调器（解析 origin；失败 ⇒ [`PageErrorKind::InvalidUrl`]）。
    ///
    /// # Errors
    /// URL 非法或不透明 origin ⇒ [`PageErrorKind::InvalidUrl`]。
    pub fn new(url: &str) -> Result<Self, PageError> {
        let origin = parse_origin(url)?;
        Ok(Self {
            url: url.to_owned(),
            origin,
            ready_state: ReadyState::Loading,
            epoch: NavigationEpoch::initial(),
            load_blockers: BTreeSet::new(),
            document: NodeId::new(0, 0),
            next_blocker: 0,
            parse_finished: false,
        })
    }

    /// 开启一次导航：`epoch += 1`、重置 `readyState = Loading`、清 blockers。
    ///
    /// # Errors
    /// URL 非法或不透明 origin ⇒ [`PageErrorKind::InvalidUrl`]。
    pub fn begin_navigation(&mut self, url: &str) -> Result<NavigationEpoch, PageError> {
        let origin = parse_origin(url)?;
        self.epoch = self.epoch.next();
        url.clone_into(&mut self.url);
        self.origin = origin;
        self.ready_state = ReadyState::Loading;
        self.load_blockers.clear();
        self.parse_finished = false;
        Ok(self.epoch)
    }

    /// 当前代次。
    #[must_use]
    pub fn epoch(&self) -> NavigationEpoch {
        self.epoch
    }

    /// 当前 URL。
    #[must_use]
    pub fn url(&self) -> &str {
        &self.url
    }

    /// 当前 origin（已规范化）。
    #[must_use]
    pub fn origin(&self) -> &Origin {
        &self.origin
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

    /// 当前文档根节点。
    #[must_use]
    pub fn document(&self) -> NodeId {
        self.document
    }

    /// 绑定文档根节点（供 S13 重建树后回填）。
    pub fn set_document(&mut self, document: NodeId) {
        self.document = document;
    }

    /// 标记解析完成：`Loading ⇒ Interactive`（若仍为 Loading）。
    pub fn mark_parse_finished(&mut self) {
        self.parse_finished = true;
        if self.ready_state == ReadyState::Loading {
            self.ready_state = ReadyState::Interactive;
        }
    }

    /// 登记一个 load blocker，返回其 id（`kind` 为记账口径）。
    pub fn add_blocker(&mut self, _kind: LoadBlockerKind) -> u64 {
        let id = self.next_blocker;
        self.next_blocker += 1;
        self.load_blockers.insert(id);
        id
    }

    /// 注销一个 load blocker。
    pub fn remove_blocker(&mut self, id: u64) {
        self.load_blockers.remove(&id);
    }

    /// 尝试推进到 `Complete`：仅当**解析完成且无 blocker** 时返回 `true`。
    pub fn try_finish(&mut self) -> bool {
        if !self.parse_finished || !self.load_blockers.is_empty() {
            return false;
        }
        self.ready_state = ReadyState::Complete;
        true
    }

    /// 该代次是否为当前代次（旧代次异步消息据此作废）。
    #[must_use]
    pub fn is_current(&self, epoch: NavigationEpoch) -> bool {
        epoch == self.epoch
    }

    /// 未完成 blocker 数。
    #[must_use]
    pub fn blocker_count(&self) -> usize {
        self.load_blockers.len()
    }
}

/// 解析 origin，失败 ⇒ [`PageErrorKind::InvalidUrl`]。
fn parse_origin(url: &str) -> Result<Origin, PageError> {
    Origin::parse(url).ok_or_else(|| {
        PageError::new(
            PageErrorKind::InvalidUrl,
            format!("无法解析 origin（非法或不透明）：{url}"),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::{LoadBlockerKind, NavigationController, ReadyState};
    use crate::error::PageErrorKind;

    #[test]
    fn navigation_bumps_epoch() {
        // §3.4：begin_navigation 后 epoch 严格 +1。
        let mut controller = NavigationController::new("https://a.example/").expect("new");
        let first = controller.epoch();
        let second = controller
            .begin_navigation("https://b.example/x")
            .expect("nav");
        assert_eq!(second.get(), first.get() + 1);
        assert_eq!(controller.epoch(), second);
        assert_eq!(controller.url(), "https://b.example/x");
    }

    #[test]
    fn stale_epoch_task_dropped() {
        // §3.4：旧代次完成消息据此作废（is_current 判定）。
        let mut controller = NavigationController::new("https://a.example/").expect("new");
        let old = controller.epoch();
        let new = controller
            .begin_navigation("https://a.example/next")
            .expect("nav");
        assert!(controller.is_current(new));
        assert!(!controller.is_current(old), "旧代次应失效");
    }

    #[test]
    fn ready_state_transitions() {
        // §3.4：Loading → Interactive → Complete。
        let mut controller = NavigationController::new("https://a.example/").expect("new");
        assert_eq!(controller.ready_state(), ReadyState::Loading);
        controller.mark_parse_finished();
        assert_eq!(controller.ready_state(), ReadyState::Interactive);
        assert!(controller.try_finish());
        assert_eq!(controller.ready_state(), ReadyState::Complete);
        assert_eq!(controller.ready_state().as_str(), "complete");
    }

    #[test]
    fn load_waits_for_blockers() {
        // §3.4：有未完成 blocker 时 try_finish()==false。
        let mut controller = NavigationController::new("https://a.example/").expect("new");
        let blocker = controller.add_blocker(LoadBlockerKind::ExternalScript);
        controller.mark_parse_finished();
        assert!(!controller.try_finish(), "有 blocker 不得完成");
        assert_eq!(controller.blocker_count(), 1);
        controller.remove_blocker(blocker);
        assert!(controller.try_finish(), "清 blocker 后应可完成");
    }

    #[test]
    fn origin_computed_from_url() {
        // §3.4 / ADR-0018·D6：规范化（default-port 折叠、大小写、端口断言）。
        let controller = NavigationController::new("https://a.example:8443/x?q#f").expect("new");
        let origin = controller.origin();
        assert_eq!(origin.scheme(), "https");
        assert_eq!(origin.host(), "a.example");
        assert_eq!(origin.port(), Some(8443));

        let folded = NavigationController::new("https://A.EXAMPLE:443/").expect("new");
        assert_eq!(folded.origin().port(), None, "default port 应折叠");

        let bad = NavigationController::new("not a url").expect_err("非法 URL 应报错");
        assert_eq!(bad.kind(), PageErrorKind::InvalidUrl);
    }
}
