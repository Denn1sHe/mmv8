//! 运行预算（`scheduler::budget`，ADR-0017·D2/D3）。

use std::time::Duration;

/// 一次 `run_until` 的预算。
///
/// - `max_tasks == 0` ⇒ **不限任务数**（约定；便于「只设 deadline」的调用点）。
/// - `deadline == None` ⇒ 不限墙钟。`deadline` 在**任务边界**检查（ADR-0017·D2）。
#[derive(Clone, Copy, Debug)]
pub struct RunBudget {
    /// 最大任务数（0 = 不限）。
    pub max_tasks: u64,
    /// 墙钟上限（`None` = 不限）。
    pub deadline: Option<Duration>,
}

impl RunBudget {
    /// 不限（不限任务数、不限墙钟）。
    #[must_use]
    pub const fn unlimited() -> Self {
        Self {
            max_tasks: 0,
            deadline: None,
        }
    }

    /// 限任务数。
    #[must_use]
    pub const fn tasks(max_tasks: u64) -> Self {
        Self {
            max_tasks,
            deadline: None,
        }
    }

    /// 限任务数 + 墙钟。
    #[must_use]
    pub const fn with_deadline(max_tasks: u64, deadline: Duration) -> Self {
        Self {
            max_tasks,
            deadline: Some(deadline),
        }
    }
}

impl Default for RunBudget {
    fn default() -> Self {
        Self::unlimited()
    }
}
