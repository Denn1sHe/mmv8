//! 任务源与执行器契约（`scheduler::task`，ADR-0017·D3/D6）。
//!
//! `TaskToken` 对 scheduler **不透明**（由生产者分配并自行映射到具体工作）；scheduler 只驱动、
//! 不理解语义，故 **不依赖** `navigation`/`network`。

use crate::error::SchedulerError;

/// 任务源。**枚举顺序即跨源选择顺序**（ADR-0017·D3）。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum TaskSource {
    /// 定时器。
    Timer,
    /// 网络完成。
    Network,
    /// 用户输入。
    UserInput,
    /// 消息/`postMessage`。
    Message,
    /// 微任务。
    Microtask,
    /// 阻塞脚本（`ScriptBlocking`）。
    ScriptBlocking,
}

impl TaskSource {
    /// 全部源，**按跨源选择顺序**排列（供 `RunLoop` 与测试使用）。
    pub const ALL: [TaskSource; 6] = [
        TaskSource::Timer,
        TaskSource::Network,
        TaskSource::UserInput,
        TaskSource::Message,
        TaskSource::Microtask,
        TaskSource::ScriptBlocking,
    ];
}

/// 不透明任务令牌（由生产者分配并自行映射到具体工作）。
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct TaskToken(u64);

impl TaskToken {
    /// 由原始 `u64` 构造。
    #[must_use]
    pub const fn new(raw: u64) -> Self {
        Self(raw)
    }

    /// 取原始 `u64`。
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// 任务执行后的处置（ADR-0017·D6）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TaskVerdict {
    /// 完成（出队）。
    Done,
    /// **出队停泊**（等某 I/O/promise；完成时由宿主再 `schedule`）。
    Park,
    /// 重新入队（如仍需继续）。
    Requeue,
    /// 放弃（出队）。
    Abort,
}

/// 执行器：把「令牌」翻译成具体工作（由 `BrowsingContext` 的独立 `Exec` 实现）。
///
/// scheduler 只驱动、不理解语义；返回 [`SchedulerError`]（**不引用** `navigation::PageError`）。
pub trait TaskExecutor {
    /// 执行一个任务。
    ///
    /// # Errors
    /// 任务执行失败时返回 [`SchedulerError`]；`RunLoop` 会复位重入守卫后透传。
    fn run_task(
        &mut self,
        source: TaskSource,
        token: TaskToken,
    ) -> Result<TaskVerdict, SchedulerError>;

    /// 是否存在未完成 I/O（用于 `RunOutcome::WaitingIo`）。
    fn has_pending_io(&self) -> bool;

    /// 是否存在**停泊任务**（`RunLoop` 据此把队列空判为 `WaitingIo`）。
    fn has_parked(&self) -> bool {
        false
    }
}
