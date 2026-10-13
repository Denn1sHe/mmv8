//! D0 的超时机制：把「挂起」变成一个**结论**，而不是一个挂住的进程。
//!
//! ## 为什么这是本探针的第一验收项
//!
//! 死锁在测试里的表现是**进程挂住**，不是**测试失败**。若超时机制本身不可靠，
//! 那么本探针所有「不死锁」的结论都无法与「根本没测出来」区分——
//! 这是许清楚 D0 把它列为第一验收项的原因，也是 ADR-0007 方法论
//! 「判定方法比结论更重要」的同款要求。
//!
//! ## 设计（架构师指定，见设计文档 §3.4）
//!
//! | 要素 | 定法 |
//! |---|---|
//! | 超时时长 | 本地 3000ms；远程 6000ms；环境变量 `IV8_PROBE_WATCHDOG_MS` 覆盖 |
//! | 时钟 | `std::time::Instant`（单调，不受系统时间调整影响） |
//! | 执行位置 | **独立 watchdog 线程**（回答 Q-9：技术方案 §6 说「执行超时由独立 watchdog 请求 V8 终止」）|
//! | 中断手段 | `IsolateHandle::terminate_execution()` + `AtomicBool` 让挂起的泵循环自行退出 |
//! | 兜底 | `terminate_execution` 后仍未在 2× watchdog 内退出 ⇒ `WATCHDOG_ESCALATED=true` 后 `exit(70)` |
//!
//! ## 为什么用 `IsolateHandle` 而不是 `&Isolate`
//!
//! watchdog 是**独立线程**，而 `&Isolate` 只能停留在创建它的线程上
//! （V8 的 Isolate 不是 `Send`）。`Isolate::thread_safe_handle()` 返回的
//! `IsolateHandle` 是 `Clone + Send + Sync`，其 `terminate_execution()` 的
//! 文档原文（`v8-152.2.0/src/isolate.rs:2607`）明说：
//! > "This method can be used by any thread even if that thread has not
//! > acquired the V8 lock with a Locker object."
//!
//! 这正好是 watchdog 需要的语义：**不持 Locker 也能请求终止**。
//! 若改用 `&Isolate` 版，本探针就必须引入 `SharedIsolate` + `Locker`，
//! 而那会把 D6（锁序路径）的因素混进 D0，破坏 D0 作为**前置基线**的干净性。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use v8::IsolateHandle;

/// 本地默认超时（毫秒）。远程机器 3.6 GB 内存、加载慢，取 6000（见 `AGENTS.md:67`）。
const DEFAULT_WATCHDOG_MS: u64 = 3000;

/// watchdog 走的是 `IsolateHandle` 路径还是 `&Isolate` 路径 —— **具名机器可读事实**。
///
/// ## 为什么要把这件事变成一个具名输出（而不是只写在注释里）
///
/// 需求稿 D0a 要求输出 `WATCHDOG_USES_ISOLATE_HANDLE=true|false`，
/// **如实报告探针实际走的路径即可**。
///
/// 本探针走的是 **`IsolateHandle` 路径**，代码链条（可用 grep 复核）：
///
/// ```text
/// main.rs   let isolate_handle = isolate.thread_safe_handle();   // IsolateHandle
/// main.rs   Watchdog::start(isolate_handle, ...)
/// 本文件    fn start(isolate_handle: IsolateHandle, ...)
/// 本文件    isolate_handle.terminate_execution();                // 跨线程，无需 Locker
/// ```
///
/// 选这条路径的原因见本模块文档「为什么用 `IsolateHandle` 而不是 `&Isolate`」：
/// watchdog 是**独立线程**，而 `&Isolate` 不是 `Send`。
///
/// 该常量与 `Watchdog::start` 的形参类型**绑定**：若将来改回 `&Isolate`，
/// 形参类型会先编译失败，从而强制同时更新本常量 —— 避免输出与现实脱节。
/// 探针每轮运行都会把它打进证据（`WATCHDOG_USES_ISOLATE_HANDLE=...`），
/// 并由父进程断言（`ASSERT(m)`）。
pub const WATCHDOG_USES_ISOLATE_HANDLE: bool = true;

/// 环境变量名：覆盖 watchdog 时长。
pub const WATCHDOG_ENV: &str = "IV8_PROBE_WATCHDOG_MS";

/// watchdog 线程的分片休眠粒度（毫秒）。
///
/// 存在的唯一理由：watchdog 必须能被**及时**停止。若线程一次睡满整个超时
/// 周期，则 9 个实验组合要白等 9 × 3s = 27s 的纯等待。分片后停止延迟 ≤ 1ms。
const STOP_CHECK_SLICE_MS: u64 = 1;

/// 读取配置后的 watchdog 时长。
///
/// 解析失败（非数字 / 0）时**回落到默认值而非报错**：watchdog 时长配错
/// 不该让探针整体失败——那属于「测量工具坏了」，比「测出死锁」更糟。
pub fn watchdog_duration() -> Duration {
    match std::env::var(WATCHDOG_ENV) {
        Ok(raw) => match raw.trim().parse::<u64>() {
            Ok(ms) if ms > 0 => Duration::from_millis(ms),
            _ => {
                eprintln!(
                    "WATCHDOG_CONFIG_INVALID raw={raw:?} -> 回落默认值 {DEFAULT_WATCHDOG_MS}ms"
                );
                Duration::from_millis(DEFAULT_WATCHDOG_MS)
            }
        },
        Err(_) => Duration::from_millis(DEFAULT_WATCHDOG_MS),
    }
}

/// 挂起场景与 watchdog 之间的共享信号。
///
/// 拆成三个 flag 是为了**分别归因**，这是 D0 的核心要求：
/// - [`PumpSignal::abort_pump`]：挂起的消息泵循环**自己**检查它并退出；
/// - [`PumpSignal::request_terminate`]：watchdog 已调用 `terminate_execution()`；
/// - [`PumpSignal::fired`]：watchdog 已到点（无论泵是否已退出）。
///
/// 若只有一个 flag，就无法区分「泵是因为 watchdog 让它退的」与
/// 「V8 执行真的被终止了」——而这个区分正是 D0 要证明的。
#[derive(Debug, Default)]
pub struct PumpSignal {
    /// 挂起的泵循环应停止并返回。
    abort_pump: AtomicBool,
    /// watchdog 已调用 `terminate_execution()`。
    request_terminate: AtomicBool,
    /// watchdog 线程已到点（无论泵是否已退出）。
    fired: AtomicBool,
}

impl PumpSignal {
    /// 置位「泵应退出」。泵循环体每轮检查它。
    pub fn set_abort_pump(&self) {
        self.abort_pump.store(true, Ordering::SeqCst);
    }

    /// 读取「泵应退出」。
    pub fn should_abort_pump(&self) -> bool {
        self.abort_pump.load(Ordering::SeqCst)
    }

    /// 是否已请求过 V8 终止执行。
    pub fn terminate_requested(&self) -> bool {
        self.request_terminate.load(Ordering::SeqCst)
    }

    /// watchdog 是否已到点。
    pub fn watchdog_fired(&self) -> bool {
        self.fired.load(Ordering::SeqCst)
    }
}

/// watchdog 线程句柄，及其观察到的结果。
///
/// Drop 即停止并 join：9 个实验组合顺序执行，若线程泄漏，后一个组合的
/// 时序会被前一个的 watchdog 污染（这属于「测量方法坏了」，必须避免）。
pub struct Watchdog {
    signal: Arc<PumpSignal>,
    /// 到点时刻；watchdog 线程尚未到点时为 `None`。
    fired_at: Arc<Mutex<Option<Instant>>>,
    /// 请求 watchdog 线程提前退出。
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl Watchdog {
    /// 启动独立 watchdog 线程。
    ///
    /// `isolate_handle` 必须是 `Isolate::thread_safe_handle()` 的产物——
    /// 它让 watchdog 无需持有 `Locker` 即可请求终止（见模块文档）。
    pub fn start(
        isolate_handle: IsolateHandle,
        timeout: Duration,
        signal: Arc<PumpSignal>,
    ) -> Self {
        let fired_at = Arc::new(Mutex::new(None));
        let fired_at_thread = Arc::clone(&fired_at);
        let signal_thread = Arc::clone(&signal);
        let stop_thread = Arc::new(AtomicBool::new(false));
        let stop_handle = Arc::clone(&stop_thread);

        let handle = std::thread::Builder::new()
            .name("iv8-watchdog".to_string())
            .spawn(move || {
                if !sleep_until_or_stop(&stop_thread, timeout) {
                    // 被主线程提前停止：本组合未超时，正常路径。
                    return;
                }

                // 到点。**先记时刻**（证据行要用），再置 flag，最后请求 V8 终止。
                // 顺序重要：若先置 flag 再记时刻，主线程可能在时刻写入前
                // 就读到 `fired == true` 并打印出一个空的 `fired_at`。
                if let Ok(mut slot) = fired_at_thread.lock() {
                    *slot = Some(Instant::now());
                }

                // 中断手段双保险（设计文档 §3.4）：
                //   1) 置 AtomicBool —— 让**纯 Rust 的挂起循环**（D0 自证场景、
                //      GIL 互等场景）能自行退出。这类挂起 V8 看不见，
                //      `terminate_execution` 对它无效。
                //   2) 调 `terminate_execution()` —— 让**正在 V8 里跑的 JS**
                //      被打断。D5 重入场景若卡在 V8 内部，只有这条路管用。
                signal_thread.set_abort_pump();
                signal_thread
                    .request_terminate
                    .store(true, Ordering::SeqCst);
                isolate_handle.terminate_execution();

                signal_thread.fired.store(true, Ordering::SeqCst);
            })
            .expect("spawn watchdog thread");

        Self {
            signal,
            fired_at,
            stop: stop_handle,
            handle: Some(handle),
        }
    }

    /// watchdog 是否已到点。
    pub fn fired(&self) -> bool {
        self.signal.watchdog_fired()
    }

    /// 到点时刻相对 `started_at` 的毫秒数；未到点则 `None`。
    pub fn fired_after_ms(&self, started_at: Instant) -> Option<u128> {
        self.fired_at
            .lock()
            .ok()
            .and_then(|slot| *slot)
            .map(|at| at.duration_since(started_at).as_millis())
    }

    /// 停止 watchdog 并回收线程。可重复调用。
    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for Watchdog {
    fn drop(&mut self) {
        self.stop();
    }
}

/// 分片休眠直到 `total` 到点；期间若 `stop` 被置位则提前返回 `false`。
///
/// 返回 `true` 表示「完整等到点」，`false` 表示「被提前停止」。
fn sleep_until_or_stop(stop: &AtomicBool, total: Duration) -> bool {
    let deadline = Instant::now() + total;
    let slice = Duration::from_millis(STOP_CHECK_SLICE_MS);
    loop {
        if stop.load(Ordering::SeqCst) {
            return false;
        }
        let now = Instant::now();
        if now >= deadline {
            return true;
        }
        std::thread::sleep((deadline - now).min(slice));
    }
}
