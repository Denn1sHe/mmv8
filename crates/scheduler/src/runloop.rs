//! 单线程消息泵 + 双时钟（`scheduler::runloop`，ADR-0017·D3/D6/D8）。
//!
//! `run_until` **有界**：任务数 / 任务边界墙钟。**不重入**：运行期再次进入 ⇒
//! [`SchedulerErrorKind::Reentrancy`](crate::error::SchedulerErrorKind::Reentrancy)（**非 panic**）。
//! **逻辑模式**下无立即任务时**虚拟跳转**到最近 due 定时器（不 sleep）。**parked-IO** 使
//! [`RunOutcome::WaitingIo`] 可达。

use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use crate::budget::RunBudget;
use crate::clock::{Clock, ClockMode};
use crate::error::SchedulerError;
use crate::queue::TaskQueues;
use crate::task::{TaskExecutor, TaskSource, TaskToken, TaskVerdict};

/// 一次 `run_until` 的终止结论。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RunOutcome {
    /// 本轮无任务可跑且无停泊任务/未完成 I/O。
    Idle,
    /// 达到 `max_tasks`。
    BudgetExhausted,
    /// 达到 `deadline`（**任务边界**检查）。
    DeadlineExceeded,
    /// 队列空且存在**停泊任务**或未完成 I/O（§6：不凭空完成请求）。
    WaitingIo,
    /// 运行循环已关闭。
    Closed,
}

/// 下一步动作。
#[derive(Debug)]
enum Pick {
    /// 运行该任务。
    Task(TaskSource, TaskToken),
    /// 逻辑模式：无立即任务，跳到该逻辑毫秒（虚拟、不 sleep）。
    JumpTo(f64),
    /// 无可运行任务。
    None,
}

/// 单线程消息泵 + 双时钟。
#[derive(Debug)]
pub struct RunLoop {
    queues: TaskQueues,
    timer_deadlines: BTreeMap<TaskToken, f64>,
    parked: BTreeSet<TaskToken>,
    clock: Box<dyn Clock>,
    in_run: bool,
    closed: bool,
}

impl RunLoop {
    /// 用给定时钟建运行循环。
    #[must_use]
    pub fn new(clock: Box<dyn Clock>) -> Self {
        Self {
            queues: TaskQueues::new(),
            timer_deadlines: BTreeMap::new(),
            parked: BTreeSet::new(),
            clock,
            in_run: false,
            closed: false,
        }
    }

    /// 入队一个任务（源内 FIFO）。亦把该令牌从停泊集合移除（完成后再入队）。
    pub fn schedule(&mut self, source: TaskSource, token: TaskToken) {
        self.parked.remove(&token);
        self.queues.push(source, token);
    }

    /// 入队一个**定时器**任务，记录其逻辑到期毫秒；Timer 队列按到期**升序**维护（队首即最近 due）。
    pub fn schedule_timer(&mut self, token: TaskToken, due_ms: f64) {
        self.parked.remove(&token);
        self.timer_deadlines.insert(token, due_ms);
        let existing = self.queues.tokens_of(TaskSource::Timer);
        let mut position = existing.len();
        for (index, existing_token) in existing.iter().enumerate() {
            let existing_due = self.timer_due(*existing_token);
            if existing_due > due_ms {
                position = index;
                break;
            }
        }
        self.queues.insert_at(TaskSource::Timer, position, token);
    }

    /// 关闭运行循环并清空全部队列/停泊/定时器。
    pub fn close(&mut self) {
        self.closed = true;
        self.queues.clear();
        self.timer_deadlines.clear();
        self.parked.clear();
    }

    /// 队列是否为空（**把「有停泊任务」计为「队列空」** —— ADR-0017·D6）。
    #[must_use]
    pub fn is_idle(&self) -> bool {
        self.queues.is_empty()
    }

    /// 当前逻辑时间（毫秒）。
    #[must_use]
    pub fn logical_now_ms(&self) -> f64 {
        self.clock.logical_now_ms()
    }

    /// 时钟模式。
    #[must_use]
    pub fn clock_mode(&self) -> ClockMode {
        self.clock.mode()
    }

    /// 有界推进：**不重入**；逻辑模式无立即任务时虚拟跳转。
    ///
    /// # Errors
    /// 运行期再次进入 ⇒ [`SchedulerError`]（`Reentrancy`）；任务执行器返回错误时透传。
    pub fn run_until(
        &mut self,
        budget: RunBudget,
        exec: &mut dyn TaskExecutor,
    ) -> Result<RunOutcome, SchedulerError> {
        if self.closed {
            return Ok(RunOutcome::Closed);
        }
        if self.in_run {
            return Err(SchedulerError::reentrancy());
        }
        self.in_run = true;
        let outcome = self.run_inner(budget, exec);
        self.in_run = false;
        outcome
    }

    fn run_inner(
        &mut self,
        budget: RunBudget,
        exec: &mut dyn TaskExecutor,
    ) -> Result<RunOutcome, SchedulerError> {
        let start = self.clock.monotonic_now();
        let deadline = budget.deadline.map(|delta| start + delta);
        let mut ran: u64 = 0;
        loop {
            if budget.max_tasks != 0 && ran >= budget.max_tasks {
                return Ok(RunOutcome::BudgetExhausted);
            }
            if let Some(limit) = deadline {
                if self.clock.monotonic_now() >= limit {
                    return Ok(RunOutcome::DeadlineExceeded);
                }
            }
            match self.pick_runnable() {
                Pick::Task(source, token) => {
                    let _ = self.queues.pop_front(source);
                    let verdict = exec.run_task(source, token)?;
                    ran += 1;
                    self.apply_verdict(source, token, verdict);
                }
                Pick::JumpTo(due_ms) => {
                    let now = self.clock.logical_now_ms();
                    let delta_ms = (due_ms - now).max(0.0);
                    self.clock
                        .advance(Duration::from_secs_f64(delta_ms / 1000.0));
                }
                Pick::None => return Ok(self.classify_idle(exec)),
            }
        }
    }

    fn classify_idle(&self, exec: &dyn TaskExecutor) -> RunOutcome {
        if !self.parked.is_empty() || exec.has_parked() || exec.has_pending_io() {
            RunOutcome::WaitingIo
        } else {
            RunOutcome::Idle
        }
    }

    fn timer_due(&self, token: TaskToken) -> f64 {
        self.timer_deadlines.get(&token).copied().unwrap_or(0.0)
    }

    /// 按跨源顺序取下一个可运行任务；Timer 需 `due <= now`，否则逻辑模式记跳转目标。
    fn pick_runnable(&self) -> Pick {
        let now = self.clock.logical_now_ms();
        for source in TaskSource::ALL {
            let Some(token) = self.queues.front(source) else {
                continue;
            };
            if source == TaskSource::Timer {
                let due = self.timer_due(token);
                if due > now {
                    return if self.clock.mode() == ClockMode::Logical {
                        Pick::JumpTo(due)
                    } else {
                        Pick::None
                    };
                }
            }
            return Pick::Task(source, token);
        }
        Pick::None
    }

    fn apply_verdict(&mut self, source: TaskSource, token: TaskToken, verdict: TaskVerdict) {
        match verdict {
            TaskVerdict::Done | TaskVerdict::Abort => {}
            TaskVerdict::Requeue => self.queues.push(source, token),
            TaskVerdict::Park => {
                self.parked.insert(token);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{RunLoop, RunOutcome};
    use crate::budget::RunBudget;
    use crate::clock::LogicalClock;
    use crate::error::{SchedulerError, SchedulerErrorKind};
    use crate::task::{TaskExecutor, TaskSource, TaskToken, TaskVerdict};
    use std::time::Duration;

    struct RecordingExec {
        log: Vec<(TaskSource, TaskToken)>,
        verdict: TaskVerdict,
        pending_io: bool,
        parked: bool,
    }

    impl RecordingExec {
        fn new(verdict: TaskVerdict) -> Self {
            Self {
                log: Vec::new(),
                verdict,
                pending_io: false,
                parked: false,
            }
        }
    }

    impl TaskExecutor for RecordingExec {
        fn run_task(
            &mut self,
            source: TaskSource,
            token: TaskToken,
        ) -> Result<TaskVerdict, SchedulerError> {
            self.log.push((source, token));
            Ok(self.verdict)
        }

        fn has_pending_io(&self) -> bool {
            self.pending_io
        }

        fn has_parked(&self) -> bool {
            self.parked
        }
    }

    fn runloop() -> RunLoop {
        RunLoop::new(Box::new(LogicalClock::new()))
    }

    #[test]
    fn run_until_budget() {
        // §4：任务数上限 ⇒ BudgetExhausted（§3.5 `run_until_respects_max_tasks`）。
        let mut rl = runloop();
        for raw in 0u64..5 {
            rl.schedule(TaskSource::Timer, TaskToken::new(raw));
        }
        let mut exec = RecordingExec::new(TaskVerdict::Done);
        let outcome = rl.run_until(RunBudget::tasks(2), &mut exec).expect("run");
        assert_eq!(outcome, RunOutcome::BudgetExhausted);
        assert_eq!(exec.log.len(), 2, "应恰好跑 2 个任务");
        assert_eq!(rl.queues.len(), 3, "余 3 个");
    }

    #[test]
    fn run_until_deadline() {
        // §4：墙钟上限 ⇒ DeadlineExceeded（§3.5 `run_until_respects_deadline`）。
        let mut rl = runloop();
        rl.schedule(TaskSource::Timer, TaskToken::new(1));
        let mut exec = RecordingExec::new(TaskVerdict::Done);
        let outcome = rl
            .run_until(RunBudget::with_deadline(0, Duration::ZERO), &mut exec)
            .expect("run");
        assert_eq!(outcome, RunOutcome::DeadlineExceeded);
        assert!(exec.log.is_empty(), "deadline=0 不应跑任务");

        // 宽裕的 deadline ⇒ 跑完转 Idle。
        let mut exec = RecordingExec::new(TaskVerdict::Done);
        let outcome = rl
            .run_until(
                RunBudget::with_deadline(0, Duration::from_secs(10)),
                &mut exec,
            )
            .expect("run");
        assert_eq!(outcome, RunOutcome::Idle);
        assert_eq!(exec.log.len(), 1);
    }

    #[test]
    fn runloop_rejects_reentry() {
        // §4：运行期再次进入 ⇒ 可判定错误（非 panic）。
        let mut rl = runloop();
        assert!(!rl.in_run);
        // 白盒：伪造「运行时」状态，驱动重入分支。
        rl.in_run = true;
        let mut exec = RecordingExec::new(TaskVerdict::Done);
        let err = rl
            .run_until(RunBudget::unlimited(), &mut exec)
            .expect_err("运行期再入应被拒");
        assert_eq!(err.kind(), SchedulerErrorKind::Reentrancy);
        rl.in_run = false;
        // 正常一次运行后，守卫复位。
        rl.run_until(RunBudget::unlimited(), &mut exec)
            .expect("run");
        assert!(!rl.in_run, "运行结束应复位守卫");
    }

    #[test]
    fn run_until_returns_waiting_io_when_parked() {
        // §4：队列空 + 停泊/未完成 I/O ⇒ WaitingIo（ADR-0017·D6）。
        let mut rl = runloop();
        assert!(rl.is_idle());

        let mut parked = RecordingExec::new(TaskVerdict::Done);
        parked.parked = true;
        assert_eq!(
            rl.run_until(RunBudget::unlimited(), &mut parked)
                .expect("run"),
            RunOutcome::WaitingIo,
            "有停泊任务 ⇒ WaitingIo"
        );

        let mut io = RecordingExec::new(TaskVerdict::Done);
        io.pending_io = true;
        assert_eq!(
            rl.run_until(RunBudget::unlimited(), &mut io).expect("run"),
            RunOutcome::WaitingIo,
            "有未完成 I/O ⇒ WaitingIo"
        );

        let mut idle = RecordingExec::new(TaskVerdict::Done);
        assert_eq!(
            rl.run_until(RunBudget::unlimited(), &mut idle)
                .expect("run"),
            RunOutcome::Idle,
            "无停泊/无 I/O ⇒ Idle"
        );
    }

    #[test]
    fn park_verdict_makes_waiting_io_reachable() {
        // ADR-0017·D6：任务可出队停泊 ⇒ WaitingIo 可达（而非 Requeue 自旋）。
        let mut rl = runloop();
        rl.schedule(TaskSource::Network, TaskToken::new(7));
        let mut exec = RecordingExec::new(TaskVerdict::Park);
        let outcome = rl
            .run_until(RunBudget::unlimited(), &mut exec)
            .expect("run");
        assert_eq!(outcome, RunOutcome::WaitingIo);
        assert_eq!(exec.log.len(), 1);
        assert!(rl.is_idle(), "停泊任务视作队列空");
    }

    #[test]
    fn task_source_cross_order_defined() {
        // §4：源间选择顺序按枚举定义（可观测）。
        let mut rl = runloop();
        // 乱序入队，跨源。
        rl.schedule(TaskSource::ScriptBlocking, TaskToken::new(6));
        rl.schedule(TaskSource::Microtask, TaskToken::new(5));
        rl.schedule(TaskSource::Timer, TaskToken::new(1));
        rl.schedule(TaskSource::Message, TaskToken::new(4));
        rl.schedule(TaskSource::UserInput, TaskToken::new(3));
        rl.schedule(TaskSource::Network, TaskToken::new(2));

        let mut exec = RecordingExec::new(TaskVerdict::Done);
        let outcome = rl
            .run_until(RunBudget::unlimited(), &mut exec)
            .expect("run");
        assert_eq!(outcome, RunOutcome::Idle);
        let order: Vec<TaskSource> = exec.log.iter().map(|(source, _)| *source).collect();
        assert_eq!(order, TaskSource::ALL.to_vec(), "跨源顺序应等于枚举顺序");
    }

    #[test]
    fn logical_clock_fires_timer() {
        // §4 / ADR-0017·D3：无立即任务 ⇒ 虚拟跳转到最近 due 定时器并执行（不 sleep）。
        let mut rl = runloop();
        rl.schedule_timer(TaskToken::new(9), 100.0);
        let wall_before = std::time::Instant::now();
        let mut exec = RecordingExec::new(TaskVerdict::Done);
        let outcome = rl
            .run_until(RunBudget::unlimited(), &mut exec)
            .expect("run");
        assert_eq!(outcome, RunOutcome::Idle);
        assert_eq!(exec.log, vec![(TaskSource::Timer, TaskToken::new(9))]);
        assert!(
            (rl.logical_now_ms() - 100.0).abs() < 1e-6,
            "应虚拟跳到 100ms"
        );
        assert!(
            wall_before.elapsed() < Duration::from_millis(200),
            "虚拟跳转不得 sleep"
        );

        // 多个定时器：按到期升序触发。
        let mut rl = runloop();
        rl.schedule_timer(TaskToken::new(3), 300.0);
        rl.schedule_timer(TaskToken::new(1), 100.0);
        rl.schedule_timer(TaskToken::new(2), 200.0);
        let mut exec = RecordingExec::new(TaskVerdict::Done);
        rl.run_until(RunBudget::unlimited(), &mut exec)
            .expect("run");
        let tokens: Vec<u64> = exec.log.iter().map(|(_, token)| token.get()).collect();
        assert_eq!(tokens, vec![1, 2, 3], "定时器应按到期序触发");
        assert!((rl.logical_now_ms() - 300.0).abs() < 1e-6);
    }

    #[test]
    fn requeue_recycles_with_budget() {
        let mut rl = runloop();
        rl.schedule(TaskSource::Microtask, TaskToken::new(1));
        let mut exec = RecordingExec::new(TaskVerdict::Requeue);
        let outcome = rl.run_until(RunBudget::tasks(3), &mut exec).expect("run");
        assert_eq!(outcome, RunOutcome::BudgetExhausted);
        assert_eq!(exec.log.len(), 3, "Requeue 应循环执行受预算截断");
    }

    #[test]
    fn close_is_terminal() {
        let mut rl = runloop();
        rl.schedule(TaskSource::Timer, TaskToken::new(1));
        rl.close();
        let mut exec = RecordingExec::new(TaskVerdict::Done);
        let outcome = rl
            .run_until(RunBudget::unlimited(), &mut exec)
            .expect("run");
        assert_eq!(outcome, RunOutcome::Closed);
        assert!(exec.log.is_empty(), "关闭后不应执行任何任务");
        assert!(rl.is_idle(), "关闭后队列应为空");
    }
}
