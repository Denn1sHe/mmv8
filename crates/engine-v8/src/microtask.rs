//! 微任务策略与预算（S8）。

use std::time::Duration;

/// 微任务策略：`Explicit`（宿主显式 checkpoint；**G1 默认**）或 `Auto`。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MicrotaskPolicy {
    /// 宿主显式调用 `perform_checkpoint` 才跑微任务（§6：宿主显式管理）。
    Explicit,
    /// 脚本调用深度归零时自动跑（G1 不用）。
    Auto,
}

/// 预算：一次 `perform_checkpoint` 允许跑的最大微任务数（防死循环，§6「预算」）。
///
/// # G1 诚实登记（对齐 §5 S8 的「不许假绿」）
///
/// V8 的 `perform_microtask_checkpoint` 是**同步且不可抢占**的，且**不暴露**本次执行的微任务
/// **条数** ⇒ `max_jobs` 这一**条数**预算在 V8 上**无法直接兑现**。G1 的实际截断手段是
/// **墙钟守卫**（watchdog 线程 + `IsolateHandle::terminate_execution`，见
/// [`crate::realm::Realm::perform_checkpoint`]）。
/// `max_jobs` 因此保留为**声明性的契约字段**（供将来出现 job 钩子时启用）。
#[derive(Clone, Copy, Debug)]
pub struct MicrotaskBudget {
    /// 声明性的条数上限（见类型级「诚实登记」；G1 不直接据此截断）。
    pub max_jobs: u32,
}

impl Default for MicrotaskBudget {
    fn default() -> Self {
        Self { max_jobs: 100_000 }
    }
}

/// 墙钟守卫的默认宽限（G1 的**实际**截断判据；见 [`MicrotaskBudget`] 的诚实登记）。
pub const DEFAULT_MICROTASK_GRACE: Duration = Duration::from_millis(500);
