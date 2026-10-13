//! 调度器错误（`scheduler::error`，ADR-0017·D1）。
//!
//! `scheduler` **自持**错误类型，**不引用** `navigation::PageError`——避免
//! `navigation ↔ scheduler` 依赖环；`navigation` 侧以 `PageError: From<SchedulerError>` 承接。

use std::fmt;

/// 调度器错误类别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SchedulerErrorKind {
    /// 运行期再次进入 `run_until`（**非 panic**，可判定）。
    Reentrancy,
    /// 运行循环已关闭。
    Closed,
    /// 内部不变量被破坏。
    Internal,
}

/// 调度器错误（`kind` + 人类可读消息）。
#[derive(Clone, Debug)]
pub struct SchedulerError {
    kind: SchedulerErrorKind,
    message: String,
}

impl SchedulerError {
    /// 构造错误。
    pub fn new(kind: SchedulerErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    /// 重入错误（运行期再次进入 `run_until`）。
    #[must_use]
    pub fn reentrancy() -> Self {
        Self::new(
            SchedulerErrorKind::Reentrancy,
            "run_until 运行期再次进入被拒",
        )
    }

    /// 错误类别。
    #[must_use]
    pub fn kind(&self) -> SchedulerErrorKind {
        self.kind
    }

    /// 错误消息。
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for SchedulerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "scheduler error ({:?}): {}", self.kind, self.message)
    }
}

impl std::error::Error for SchedulerError {}
