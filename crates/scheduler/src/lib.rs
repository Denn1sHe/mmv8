//! scheduler —— 任务源队列、双时钟（逻辑/系统）与有界运行循环（G2 核心）。
//!
//! 状态：**G2 落地**（施工图 §1.2 / §2.4 / §3.5，ADR-0017·D3/D6/D8）。
//! - 任务源队列（`TaskSource` 内 FIFO、源间按枚举顺序）。
//! - 有界 [`RunLoop::run_until`]：任务数 / 任务边界墙钟；**不重入**。
//! - 双时钟 [`Clock`]：逻辑模式**虚拟跳转**（不 sleep）；系统模式墙钟。
//! - **parked-IO**：出队停泊使 [`RunOutcome::WaitingIo`] 可达。
//! - **自持 [`SchedulerError`]**（不引用 `navigation::PageError` ⇒ 无依赖环）。
//! - 本 crate **零 unsafe**、**零依赖**。

pub mod budget;
pub mod clock;
pub mod error;
pub mod queue;
pub mod runloop;
pub mod task;

pub use budget::RunBudget;
pub use clock::{Clock, ClockMode, LogicalClock, SystemClock};
pub use error::{SchedulerError, SchedulerErrorKind};
pub use queue::TaskQueues;
pub use runloop::{RunLoop, RunOutcome};
pub use task::{TaskExecutor, TaskSource, TaskToken, TaskVerdict};
