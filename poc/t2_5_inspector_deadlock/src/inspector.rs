//! `ChannelImpl` / `V8InspectorClientImpl` 的探针实现。
//!
//! ## 这一层为什么是「死锁入口」
//!
//! `V8InspectorClientImpl::run_message_loop_on_pause`（`v8-152.2.0/src/inspector.rs:584`）
//! 的默认实现是**空函数体**，签名：
//!
//! ```text
//! fn run_message_loop_on_pause(&self, context_group_id: i32) {}
//! ```
//!
//! V8 在暂停时**同步**调用它。也就是说：这个函数体里发生的一切，
//! 都发生在**页面线程的调用栈上**，且此时页面线程正**停在暂停点**。
//! 若宿主在这里做「等待另一个线程」的事，而那个线程又在等页面线程，
//! 就是互等。本探针的 F1（GIL）与 F3（第二线程）实验都在这里构造。
//!
//! ## 实测约束：泵回调里**拿不到 V8 scope**（D5 的构造方式由此定）
//!
//! `run_message_loop_on_pause` 的签名里**只有** `&self` 和一个 `i32`，
//! **没有** `PinScope`、没有 `Isolate`、没有 `Context`。而 `v8` 152.2.0
//! **未公开**任何从零构造 `Local<Context>` 的 API：
//!
//! - `Local::from_non_null` / `Local::as_non_null` 都是 `pub(crate)`（`handle.rs:203`）；
//! - `Local::new` 需要一个已有的 `&PinScope`（`handle.rs:136`）—— 循环依赖；
//! - `Isolate` 没有公开的 `get_current_context`（只有 `HandleScope` 的
//!   `pub(crate)` 版本，`scope.rs:513`）。
//!
//! ⇒ **在泵回调内直接重入 eval 是不可构造的**（不是「危险」，是**做不到**）。
//!
//! 故 D5 改由**宿主函数回调**（`Function::new`）构造：脚本在暂停点前后
//! 调用注入的 `__iv8_reenter_eval()`，该回调**自带** `&mut PinScope`
//! （T2.0 已实测的写法），在其中对**同一 Context** 同步 `Script::compile` + `run`。
//! 这与「宿主回调中再次同步 eval 同一 Context」在语义上等价 ——
//! 被测的是「**回调内重入同一 Context 的 eval**」这条禁令的后果，
//! 而触发点从 Inspector 泵换成了普通宿主函数，**不改变被测性质**。
//! 该偏离已显式记录（README「偏离设计之处」+ 回传报告）。

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use v8::inspector::{ChannelImpl, StringBuffer, V8InspectorClientImpl};

use crate::probe::Combo;
use crate::watchdog::PumpSignal;

/// 协议通道的探针实现：记录 V8 发来的所有 Inspector 协议消息。
///
/// **为什么必须记录**：D3 要求「每个『不死锁』结论必须有可观测证据」。
/// 「暂停期间 V8 确实通过 Channel 说过话」是「暂停真的发生了」的最强证据——
/// 若 Channel 全空，则可能暂停根本没触发，后续所有结论都无从谈起。
pub struct ProbeChannel {
    sink: ChannelSink,
}

impl ProbeChannel {
    /// 新建一个记录型通道。
    pub fn new() -> Self {
        Self {
            sink: ChannelSink::default(),
        }
    }

    /// 取出共享的接收端（供外部断言读取）。
    ///
    /// 返回克隆而非引用：通道对象会被 move 进 `V8InspectorSession`，
    /// 之后无法再访问它，但其内部状态是 `Arc`，克隆后仍可读。
    pub fn sink(&self) -> ChannelSink {
        self.sink.clone()
    }
}

/// 通道的共享读取端。
#[derive(Clone, Default)]
pub struct ChannelSink {
    received: Arc<Mutex<Vec<String>>>,
    notification_count: Arc<AtomicU64>,
}

impl ChannelSink {
    /// 已收到的消息总条数（响应 + 通知）。
    pub fn len(&self) -> usize {
        self.received.lock().map(|v| v.len()).unwrap_or(0)
    }

    /// 已收到的通知（非响应）条数。
    ///
    /// 「有通知」比「有消息」更重要：`Debugger.enable` 的**响应**在任何
    /// 组合下都会来（它是协议握手），而 `Debugger.paused` 这类**通知**
    /// 只在暂停真的发生时才会来。
    pub fn notifications(&self) -> u64 {
        self.notification_count.load(Ordering::SeqCst)
    }

    /// 是否收到过 `Debugger.paused` 通知。
    ///
    /// 这是「暂停真的发生且 V8 真的把它报告给了宿主」的**判定性证据**。
    pub fn saw_debugger_paused(&self) -> bool {
        self.received
            .lock()
            .map(|v| v.iter().any(|m| m.contains("Debugger.paused")))
            .unwrap_or(false)
    }

    /// 记录一条消息。
    fn record(&self, prefix: &str, body: &str) {
        // 截断超长消息：协议响应可能很大，而探针只需证明「有这条消息」。
        // 保留前 240 字符足以辨识 `Debugger.paused` 这类关键通知。
        let mut truncated = body.to_string();
        truncated.truncate(240);
        if let Ok(mut guard) = self.received.lock() {
            guard.push(format!("{prefix}:{truncated}"));
        }
    }
}

/// 读取 `UniquePtr<StringBuffer>` 的内容。
///
/// `UniquePtr` 是「移交所有权」的智能指针，`as_mut()` 取出可变引用。
/// **注意**：V8 侧在回调返回后仍持有该 buffer，故这里**只读不取走**
/// （不调 `take()`），符合 API 契约。
///
/// 拿不到 buffer 时返回可辨识占位串而**不 panic**：
/// panic 跨 FFI 边界传播 = 进程 abort（实测 D5 就可能触发 FATAL），
/// 在一个专门测 FATAL 的探针里主动制造 abort 是不可接受的。
fn buffer_to_string(mut message: v8::UniquePtr<StringBuffer>) -> String {
    match message.as_mut() {
        Some(buffer) => buffer.string().to_string(),
        None => "<null StringBuffer>".to_string(),
    }
}

impl ChannelImpl for ProbeChannel {
    fn send_response(&self, call_id: i32, message: v8::UniquePtr<StringBuffer>) {
        let body = buffer_to_string(message);
        self.sink.record(&format!("RESP#{call_id}"), &body);
    }

    fn send_notification(&self, message: v8::UniquePtr<StringBuffer>) {
        let body = buffer_to_string(message);
        self.sink.notification_count.fetch_add(1, Ordering::SeqCst);
        self.sink.record("NOTIFY", &body);
    }

    fn flush_protocol_notifications(&self) {
        // 探针不实现 CDP 的批处理语义（边界：不实现任何 CDP 域），
        // 但留一条痕迹以证明该回调确实被调用过。
        self.sink.record("FLUSH", "");
    }
}

/// 重入 eval（D5）的实测结果分类。
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum ReentrantOutcome {
    /// 未尝试。
    #[default]
    NotAttempted,
    /// eval 正常返回一个值。
    ReturnedValue,
    /// eval 返回 `None`（有异常待取或被执行终止打断）——**可捕获**。
    ReturnedNone,
    /// eval 抛出了可捕获的 panic（探针用 `catch_unwind` 兜住了）。
    Panicked,
    /// 未能取得执行所需的 scope / 源码（**构造失败，非 V8 行为**）。
    ScopeUnavailable,
}

impl ReentrantOutcome {
    /// 输出为稳定字符串（证据行用）。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotAttempted => "NOT_ATTEMPTED",
            Self::ReturnedValue => "RETURNED_VALUE",
            Self::ReturnedNone => "RETURNED_NONE",
            Self::Panicked => "PANICKED",
            Self::ScopeUnavailable => "SCOPE_UNAVAILABLE",
        }
    }
}

/// 实验过程中累积的观测结果。
///
/// 全部用原子量 / `Mutex`，因为**写入方在不同线程**：
/// 页面线程（泵回调 / 宿主函数回调内）、watchdog 线程、第二线程都可能写。
#[derive(Debug, Default)]
pub struct ProbeObservations {
    /// `run_message_loop_on_pause` 是否被进入过。
    pub pump_entered: AtomicBool,
    /// `run_message_loop_on_pause` 是否正常返回。
    pub pump_returned: AtomicBool,
    /// 泵**真正退出**的时刻（相对实验开始的毫秒数）。
    ///
    /// 必须单独记录：`RESUMED_AT` 的语义是「暂停到解除用了多久」，
    /// 若在报告时才读 `Instant::elapsed()`，量到的会是「泵退出 +
    /// 所有收尾线程 join 完毕」的总时长 —— 本机实测偏差可达 6 秒
    /// （`msg_only` 组合的第二线程会一直跑到 2×watchdog），
    /// 那会让「不死锁」看起来像「卡了 6 秒」。
    pub pump_exited_at_ms: AtomicU64,
    /// `quit_message_loop_on_pause` 是否被调用过。
    pub quit_called: AtomicBool,
    /// 泵内是否**发起过**获取 GIL 的尝试。
    pub gil_attempted: AtomicBool,
    /// 泵内发起 `with_gil` 的相对时刻（ms）。
    pub gil_attempt_at_ms: AtomicU64,
    /// 泵内是否**真正拿到** GIL。
    pub gil_acquired: AtomicBool,
    /// 泵内拿到 GIL 的相对时刻（ms）。
    pub gil_acquired_at_ms: AtomicU64,
    /// 泵内发起 GIL 尝试时，是否观测到 Python 侧正在持锁（D4 争用证据）。
    pub gil_contended: AtomicBool,
    /// Python 侧是否在握手期内进入等待状态（D4 的**测量有效性**前置）。
    ///
    /// `false` 表示握手超时 ⇒ 争用条件**未成立** ⇒ 该次运行的
    /// `GIL_BLOCKED_MS` 与分类都不可用于判定（必须显式暴露，
    /// 不能悄悄按一个未成立的条件出结论）。
    pub python_handshake_ready: AtomicBool,
    /// 重入 eval 是否被尝试（D5）。
    pub reentrant_attempted: AtomicBool,
    /// 重入 eval 的结果分类。
    reentrant_outcome: Mutex<ReentrantOutcome>,
    /// 第二线程是否成功拿到 `Locker`（D6）。
    pub locker_acquired: AtomicBool,
    /// 是否观测到「页面线程暂停期间 Locker 被占用」（D6 锁序死锁的前提）。
    pub locker_contended: AtomicBool,
    /// 「页面任务已被推进」标记：宿主注入的 `__iv8_mark_page_task` 被调用时置位。
    ///
    /// D7 的核心读数：若在**暂停期间**它仍为 `false` ⇒ 页面任务未被推进。
    pub page_task_ran: AtomicBool,
    /// D7 的判定结果（暂停期间读到的「未推进」结论）。
    pub page_task_stalled: AtomicBool,
    /// D7 是否真的测到了（区分「测到未推进」与「没测出来」）。
    pub page_task_measured: AtomicBool,
}

impl ProbeObservations {
    /// 新建一组全零观测。
    pub fn new() -> Self {
        Self::default()
    }

    /// 记录重入 eval 的结果。
    pub fn set_reentrant_outcome(&self, outcome: ReentrantOutcome) {
        if let Ok(mut slot) = self.reentrant_outcome.lock() {
            *slot = outcome;
        }
    }

    /// 读取重入 eval 的结果。
    pub fn reentrant_outcome(&self) -> ReentrantOutcome {
        self.reentrant_outcome
            .lock()
            .map(|v| *v)
            .unwrap_or(ReentrantOutcome::NotAttempted)
    }

    /// GIL 被阻塞的时长（拿到 GIL 的时刻 − 发起时刻）。
    ///
    /// 只在两者都记录过时有值。`None` 表示「没发起」或「一直没拿到」
    /// ——后者正是**死锁**的形态，调用方须据此给出 `TIMEOUT` 而非 `CLEAN_RESUME`。
    pub fn gil_blocked_ms(&self) -> Option<u64> {
        if !self.gil_attempted.load(Ordering::SeqCst) || !self.gil_acquired.load(Ordering::SeqCst) {
            return None;
        }
        Some(
            self.gil_acquired_at_ms
                .load(Ordering::SeqCst)
                .saturating_sub(self.gil_attempt_at_ms.load(Ordering::SeqCst)),
        )
    }

    /// 结算 D7：把「暂停期间是否观察到页面任务被推进」转成结论。
    ///
    /// 必须在**暂停期间**调用（此时读 `page_task_ran` 才有意义）。
    /// 读不到明确结论时**不**置 `page_task_stalled`，而是置 `page_task_measured=false`
    /// —— 让上层能区分「测到未推进」与「根本没测到」，避免假绿。
    pub fn settle_page_task_stalled(&self) {
        self.page_task_measured.store(true, Ordering::SeqCst);
        self.page_task_stalled
            .store(!self.page_task_ran.load(Ordering::SeqCst), Ordering::SeqCst);
    }
}

/// 宿主客户端的探针实现：所有实验行为都在这里分派。
///
/// ## 关键设计：为什么行为由 `Combo` 开关而不是「每组合一个类型」
///
/// 8 组实验组合共享同一份 V8 构造流程（isolate → context → inspector →
/// session → `Debugger.enable` → `schedule_pause_on_next_statement` → run script）。
/// 若每个组合一个类型，就得把这套构造复制 8 遍，而**复制本身是 bug 来源**：
/// D5 很可能让 V8 FATAL，构造代码一旦有一处不一致，就无法归因 FATAL 来自哪里。
/// 故用「一个类型 + `Combo` 开关」，保证**除被测行为外一切都相同**——
/// 这正是 D2「成对证据」成立的前提。
pub struct ProbeClient {
    /// 当前实验组合（决定泵内行为）。
    combo: Combo,
    /// 共享观测结果。
    obs: Arc<ProbeObservations>,
    /// 与 watchdog 共享的退出信号。
    signal: Arc<PumpSignal>,
    /// 实验开始的时刻（用于把泵内事件换算成相对毫秒）。
    started_at: Instant,
    /// F1=yes 时，泵内要 eval 的 Python 表达式。
    python_probe_source: Option<String>,
    /// 受控消息泵「正常泵完」的时长（ms）。
    ///
    /// `None` = 不设预算，只能被 watchdog 打断（即「必然挂起」的 D0 自证场景）。
    /// `Some(_)` = 泵完这么久后自行返回，构成「不死锁」对照。
    ///
    /// 该值由 main 按 watchdog 时长计算，保证**远小于** watchdog，
    /// 否则「不死锁」对照也会被 watchdog 打断，两类结论就分不开了。
    pump_budget_ms: Option<u64>,
    /// 与 Python 侧线程的同步点（D4 争用判定用）。
    rendezvous: Option<Arc<crate::pyside::GilRendezvous>>,
}

impl ProbeClient {
    /// 构造一个宿主客户端。
    pub fn new(
        combo: Combo,
        obs: Arc<ProbeObservations>,
        signal: Arc<PumpSignal>,
        started_at: Instant,
        python_probe_source: Option<String>,
        pump_budget_ms: Option<u64>,
        rendezvous: Option<Arc<crate::pyside::GilRendezvous>>,
    ) -> Self {
        Self {
            combo,
            obs,
            signal,
            started_at,
            python_probe_source,
            pump_budget_ms,
            rendezvous,
        }
    }
}

impl V8InspectorClientImpl for ProbeClient {
    /// **死锁入口**：V8 暂停时同步调用宿主。
    ///
    /// 组合行为：
    /// - F1：尝试获取 GIL（D4）。注意本函数**不检查** abort 标志——
    ///   `Python::with_gil` 自身不可中断，这正是 D4 要测的风险本身。
    /// - D7：读取页面任务标记（**只读，不推进**）。
    /// - 最后进入受控消息泵本体，该本体**必须**能被 watchdog 打断。
    ///
    /// F2（重入 eval）**不在这里**——理由见模块文档「实测约束」。
    fn run_message_loop_on_pause(&self, context_group_id: i32) {
        let _ = context_group_id;
        self.obs.pump_entered.store(true, Ordering::SeqCst);

        if self.combo.acquires_gil() {
            self.acquire_gil_in_pump();
        }

        // D7：在**暂停期间**读页面任务标记。
        // 技术方案 §13 要求暂停期间「不推进普通页面任务」，
        // 故此时 `page_task_ran` 应仍为 false。
        self.obs.settle_page_task_stalled();

        // 受控消息泵本体。技术方案 §13（架构师 Q-8 定的最小语义）：
        // 暂停期间**只**泵 Inspector 协议消息与宿主显式标记的控制消息，
        // **不推进**普通页面任务、**不跑** microtask checkpoint、**不派发**网络事件。
        self.pump_until_quit();

        self.obs.pump_returned.store(true, Ordering::SeqCst);
        self.obs.pump_exited_at_ms.store(
            self.started_at.elapsed().as_millis() as u64,
            Ordering::SeqCst,
        );
    }

    /// 解除暂停。宿主在决定「继续执行」时调用。
    fn quit_message_loop_on_pause(&self) {
        self.obs.quit_called.store(true, Ordering::SeqCst);
        // 同时置位 abort_pump，让正卡在等待循环里的泵退出。
        // 这是「解除暂停」在探针里的**唯一**有效手段：
        // V8 侧的 `quitMessageLoopOnPause` 会让本次 `runMessageLoopOnPause`
        // 返回，但探针的等待循环必须自己看到信号才会返回。
        self.signal.set_abort_pump();
    }

    /// 返回 0 = 让 V8 自己选 ID（trait 文档：`0 = let V8 pick a unique id itself`）。
    ///
    /// 探针不关心 ID 唯一性；用 V8 自选可避免引入跨实验组合的全局状态。
    fn generate_unique_id(&self) -> i64 {
        0
    }
}

impl ProbeClient {
    /// 受控消息泵的等待循环。
    ///
    /// ## 两种退出路径（缺一不可，否则结论无法二分）
    ///
    /// 1. **正常泵完并返回**（`pump_budget_ms = Some(_)`）：模拟「宿主泵完一轮
    ///    协议消息、决定继续执行」⇒ 返回后脚本继续 ⇒ `CLEAN_RESUME`。
    ///    **这是「不死锁」对照的来源**，必须有。
    /// 2. **只被 watchdog 打断**（`pump_budget_ms = None`，即 D0 自证场景）：
    ///    「若无 watchdog 则永远不返回」⇒ `TIMEOUT`。
    ///
    /// 若只有第 2 条路径，则**全部**组合都会是 `TIMEOUT`，
    /// 那样就回答不了「边界在哪」—— 这正是 D2 禁止的情形
    /// （「只有『全部不死锁』或『全部死锁』都回答不了边界在哪」）。
    ///
    /// ## 「不推进页面任务」
    ///
    /// 本循环**绝不**推进任何队列：不调 `perform_microtask_checkpoint`、
    /// 不派发网络事件、不跑定时器 —— 这正是 D7 要验证的行为。
    fn pump_until_quit(&self) {
        let deadline = self.pump_budget_ms.map(std::time::Duration::from_millis);
        let started = Instant::now();

        loop {
            if self.signal.should_abort_pump() {
                return;
            }
            // 正常路径：泵完预算时长后返回，表示「消息泵跑完一轮，可以继续」。
            if let Some(budget) = deadline {
                if started.elapsed() >= budget {
                    return;
                }
            }
            // 真实实现会在这里阻塞等协议消息。用短睡眠模拟「等到了但无事可做」。
            // 1ms 分片：既不忙等烧 CPU，又让 watchdog 响应延迟 ≤ 1ms。
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }

    /// F1 路径：在泵内同步获取 GIL 并执行一个 Python 表达式（D4）。
    ///
    /// 这一步是**整个 GIL 死锁路径的被测点**。若 Python 侧此刻持 GIL
    /// 并等页面线程，本次 `with_gil` 就会一直阻塞 —— 直到 Python 侧的
    /// 兜底超时或 watchdog 到点。
    fn acquire_gil_in_pump(&self) {
        self.obs.gil_attempted.store(true, Ordering::SeqCst);
        // D4 的争用判定：在**发起 with_gil 之前**读一次 Python 侧的持锁标志。
        // 这一行是「真互等」与「伪互等」的唯一区分点，也是 D2 成对证据的依据。
        if let Some(rv) = &self.rendezvous {
            self.obs
                .gil_contended
                .store(rv.python_side_holding(), Ordering::SeqCst);
        }
        let source = self
            .python_probe_source
            .clone()
            .unwrap_or_else(|| "1 + 1".to_string());
        crate::pyside::run_python_in_pump(&source, &self.obs, self.started_at);
    }
}

// ===========================================================================
// 宿主函数回调（T2.0 已实测的注入写法）
// ===========================================================================

/// 宿主函数回调如何拿到共享状态：`Isolate` 的 **slot** 机制。
///
/// ## 为什么必须用 slot 而不是「捕获闭包」
///
/// `v8::Function::new` 的回调参数是 `impl MapFnTo<FunctionCallback>`，
/// 而 `MapFnTo` 要求 `Self: UnitType`（`support.rs:524-527`）——
/// **只接受不捕获环境的闭包**。故 T2.0 那种 `move |...| { obs... }`
/// 的写法在 152.2.0 上编译不过（`E0277: the trait bound ... MapFnTo<...>
/// is not satisfied`）。
///
/// `Isolate::set_slot` / `get_slot`（`scope.rs:2123-2150`）正是为此场景
/// 准备的：文档原文「associate state with an Isolate … The value will be
/// dropped when the isolate is dropped」。回调内 `scope.get_slot::<T>()`
/// 取回即可，无需捕获。
///
/// 这也让本探针**零 unsafe**（D9）：不碰 `FunctionCallbackInfo` 的裸数据指针。
///
/// ## 安全性：slot 的生命周期
///
/// slot 由 Isolate 拥有，随 Isolate 一起 drop。宿主函数回调**只在**
/// 本 Isolate 上执行（V8 保证），故 `get_slot` 返回的引用必然有效。
type SlotKey = ProbeObservations;

/// 在 Isolate 上登记共享观测。
pub fn install_slot(scope: &mut v8::PinScope<'_, '_>, obs: Arc<SlotKey>) {
    scope.set_slot(obs);
}

/// 注入 `__iv8_mark_page_task()`：页面任务被推进时调用。
///
/// D7 用它判断「暂停期间普通页面任务是否被推进」。它由一个
/// `Promise.resolve().then(...)` microtask 调用 —— 这是 `setTimeout`
/// 在 `v8` crate 里的**纯 JS 等价物**（`v8` 不提供定时器，
/// 那属于宿主事件循环能力）。
pub fn make_mark_page_task(
    _scope: &mut v8::PinScope<'_, '_>,
    _args: v8::FunctionCallbackArguments,
    _rv: v8::ReturnValue,
) {
    if let Some(obs) = _scope.get_slot::<Arc<SlotKey>>() {
        obs.page_task_ran.store(true, Ordering::SeqCst);
    }
}

/// 注入 `__iv8_reenter_eval()`：在**宿主函数回调内**对同一 Context 重入 eval（D5）。
///
/// 技术方案 §6 明文「**禁止**回调中再次同步 eval 同一 Context」。
/// 本函数的目的就是给出这条禁令的**实际后果**。
///
/// ## 为什么用宿主函数回调而不是 Inspector 泵
///
/// 见模块文档「实测约束」：`run_message_loop_on_pause` 的签名里没有
/// `PinScope`，而 `v8` 152.2.0 **未公开**任何从零构造 `Local<Context>`
/// 的公开 API ⇒ 在泵内重入 eval **不可构造**。
/// 宿主函数回调自带 `&mut PinScope`（这正是 T2.0 验证过的注入方式），
/// 被测性质不变：**回调内对同一 Context 同步求值**。
///
/// ## 为什么必须 catch_unwind
///
/// panic 若跨 FFI 边界传播会 **abort 进程**。而本函数的目的正是观察
/// 「重入 eval 会不会 panic」——不捕获的话，只能得到「进程死了」这个
/// 粗粒度结论，反而丢失「它是**可捕获异常**」这个更精确的答案。
pub fn make_reenter_eval(
    scope: &mut v8::PinScope<'_, '_>,
    _args: v8::FunctionCallbackArguments,
    _rv: v8::ReturnValue,
) {
    let Some(obs) = scope.get_slot::<Arc<SlotKey>>().cloned() else {
        return;
    };
    obs.reentrant_attempted.store(true, Ordering::SeqCst);

    let outcome =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| reenter_eval_inner(scope)));

    let final_outcome = match outcome {
        Ok(value) => value,
        Err(_) => ReentrantOutcome::Panicked,
    };
    obs.set_reentrant_outcome(final_outcome);
}

/// `make_reenter_eval` 的内部实现：同一 Context 上的同步 eval。
///
/// 用 `catch_unwind` 包裹，故内部的 `None` 返回不会传播成 panic。
fn reenter_eval_inner(scope: &mut v8::PinScope<'_, '_>) -> ReentrantOutcome {
    let Some(code) = v8::String::new(scope, crate::REENTER_SOURCE) else {
        return ReentrantOutcome::ScopeUnavailable;
    };
    let Some(script) = v8::Script::compile(scope, code, None) else {
        // 编译失败 = 有异常待取（如 SyntaxError）。这是**可捕获**的一类。
        return ReentrantOutcome::ReturnedNone;
    };
    match script.run(scope) {
        Some(_) => ReentrantOutcome::ReturnedValue,
        None => ReentrantOutcome::ReturnedNone,
    }
}
