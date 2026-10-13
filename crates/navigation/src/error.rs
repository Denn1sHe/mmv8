//! navigation 页面层错误（`PageError` / `PageErrorKind`，ADR-0017·D11）。
//!
//! `PageError` 是页面层统一失败类型；与 `scheduler::SchedulerError`
//! （`From<SchedulerError>`）与 `engine_v8::JsError` 的映射在接线步骤（S13）补齐。

use std::fmt;

/// 页面层错误类别（ADR-0017·D11 变体清单，G2 拟定）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PageErrorKind {
    /// URL 非法或不透明 origin。
    InvalidUrl,
    /// 导航被作废（旧代次）。
    NavigationAborted,
    /// 导航后访问旧文档节点（ADR-0018·D2）。
    StaleDocumentNode,
    /// 显式 target 不属于当前文档（ADR-0019）。
    TargetNotInDocument,
    /// parser 已结束（`readyState != Loading`）后调用 `document.write`（ADR-0017·D4，具名）。
    DocumentWriteAfterLoad,
    /// 模块顶层 `await` 不受支持（ADR-0017 模块条款）。
    ModuleTopLevelAwaitUnsupported,
    /// 脚本 / 模块执行被 watchdog 打断（ADR-0017·D2；映射自 `JsErrorKind::Terminated`）。
    Terminated,
    /// 重入（运行循环 / `PyO3` 控制面）。
    Reentrancy,
    /// 内部不变量被破坏。
    Internal,
}

/// 页面层错误（`kind` + 人类可读消息）。
#[derive(Clone, Debug)]
pub struct PageError {
    kind: PageErrorKind,
    message: String,
}

impl PageError {
    /// 构造错误。
    #[must_use]
    pub fn new(kind: PageErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    /// 错误类别。
    #[must_use]
    pub fn kind(&self) -> PageErrorKind {
        self.kind
    }

    /// 错误消息。
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for PageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "page error ({:?}): {}", self.kind, self.message)
    }
}

impl std::error::Error for PageError {}
