//! PyO3 侧：构造 GIL 互等的最小交错（D4）。
//!
//! ## 为什么必须用真 Python 解释器（决策 Q-4 的理由）
//!
//! Rust 宿主函数回调（T2.0 已验证的 `Function::new`）**不持 GIL**，
//! 因此用它构造不出 GIL 互等。GIL 是 CPython 解释器级的锁，
//! 只有真的把解释器拉起来、在两个线程上分别持锁/等锁，才构造得出互等。
//!
//! ## 互等的构造（设计文档 §3.5 D4 行）与一个必须诚实处理的细节
//!
//! 设计文档要求：「Python 侧函数内 `time.sleep()` 等页面线程」。
//! 但 **`time.sleep()` 期间 CPython 会释放 GIL** —— 这是 CPython 的既定行为。
//! 于是照字面实现会得到：Python 线程「在等页面线程」但**没持 GIL**，
//! 页面线程的 `Python::with_gil` 会**成功** ⇒ 观测到 `CLEAN_RESUME`。
//!
//! **这不是「GIL 路径不死锁」的证据，而是「这个构造没构造出互等」的证据。**
//! 若照字面实现并报 `CLEAN_RESUME`，就是一个**假绿**。
//!
//! 故本模块提供两种 Python 侧策略，并在输出里明确区分：
//!
//! | 策略 | Python 侧行为 | 是否构造出互等 | 作用 |
//! |---|---|---|---|
//! | [`GilWaitStrategy::Sleeping`] | `time.sleep()` | ❌ 否（GIL 被释放） | **对照**：证明「看起来像死锁但实际不死锁」可被区分 |
//! | [`GilWaitStrategy::SpinHoldingGil`] | 纯 Python 忙等（不调任何释放 GIL 的 API） | ✅ 是 | **正题**：构造真正的 GIL 互等 |
//!
//! 两个策略都跑，才能给出 D2 要求的**成对证据**：不是「全部不死锁」，
//! 也不是「全部死锁」，而是「边界在哪」。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use pyo3::ffi::c_str;
use pyo3::prelude::*;

use crate::inspector::ProbeObservations;

/// Python 侧等待页面线程的策略。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GilWaitStrategy {
    /// `time.sleep()`：释放 GIL，**构造不出**互等（对照组）。
    Sleeping,
    /// 纯 Python 忙等：持有 GIL，**构造出**互等（实验组）。
    SpinHoldingGil,
    /// Python 侧只完成初始化，随后**完全不持 GIL** 地等待（**确定性对照**）。
    ///
    /// ## 为什么需要第三个策略（一次真实翻转事故的产物）
    ///
    /// `Sleeping` 虽然是「不持锁」的语义，但其实现靠 `time.sleep()` 反复
    /// 「释放→重新获取」GIL。在过载机器上，页面线程的 `take_gil` 可能连续
    /// 多轮抢不到，使对照组的 `GIL_BLOCKED_MS` 从 0 被抬到秒级、
    /// **被误判为 `TIMEOUT`** —— 对照因此不是「干净的」。
    ///
    /// `NotHolding` 把 Python 侧的持锁时间压到**零**（准备完就把 GIL 还掉，
    /// 改在 Rust 侧等待），使对照的成立**与调度无关**。
    /// 这样 D2 的对照才是结构上可靠的，而不是「在空载时恰好成立」。
    NotHolding,
}

impl GilWaitStrategy {
    /// 稳定字符串标签。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Sleeping => "sleeping",
            Self::SpinHoldingGil => "spin_holding_gil",
            Self::NotHolding => "not_holding",
        }
    }

    /// 该策略是否真的构造出 GIL 互等（用于报告，避免误读）。
    pub fn constructs_deadlock(self) -> bool {
        matches!(self, Self::SpinHoldingGil)
    }

    /// 该策略是否为**对照**（本应干净地不死锁）。
    ///
    /// 对照若输出 `TIMEOUT`，说明该次运行的测量不可信
    /// （见 `MEASUREMENT_SUSPECT`），**不得**当成「GIL 路径也会死锁」的证据。
    pub fn is_control(self) -> bool {
        !self.constructs_deadlock()
    }

    /// 解析自命令行参数。
    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "sleeping" => Some(Self::Sleeping),
            "spin" | "spin_holding_gil" => Some(Self::SpinHoldingGil),
            "nohold" | "not_holding" => Some(Self::NotHolding),
            _ => None,
        }
    }
}

/// 页面线程与 Python 线程之间的共享同步点。
///
/// 用 `AtomicBool` 而非 channel：页面线程在泵内只**轮询**标志，
/// 不能阻塞在 channel 上——阻塞会引入第二个等待点，使归因复杂化。
#[derive(Debug)]
pub struct GilRendezvous {
    /// Python 侧线程**正持有 GIL** 并忙等（只有 `spin` 策略会置位）。
    pub python_side_holding: AtomicBool,
    /// Python 侧线程已进入等待，但**当前不持 GIL**（`sleeping` 策略）。
    pub python_side_waiting: AtomicBool,
    /// Python 侧线程可以退出了。
    pub release: AtomicBool,
}

impl GilRendezvous {
    /// 新建一组全零同步点。
    pub fn new() -> Self {
        Self {
            python_side_holding: AtomicBool::new(false),
            python_side_waiting: AtomicBool::new(false),
            release: AtomicBool::new(false),
        }
    }

    /// 请求 Python 侧退出等待（页面线程调用）。
    pub fn request_release(&self) {
        self.release.store(true, Ordering::SeqCst);
    }

    /// Python 侧是否已被要求退出。
    pub fn should_release(&self) -> bool {
        self.release.load(Ordering::SeqCst)
    }

    /// Python 侧线程当前是否正持有 GIL 并处于等待中。
    ///
    /// 页面线程在**发起 `with_gil` 之前**读它：D4 的「真互等」与
    /// 「伪互等（`time.sleep` 期间 GIL 已释放）」的唯一区分点。
    pub fn python_side_holding(&self) -> bool {
        self.python_side_holding.load(Ordering::SeqCst)
    }

    /// Python 侧线程是否已进入等待（不论是否持 GIL）。
    pub fn python_side_waiting(&self) -> bool {
        self.python_side_waiting.load(Ordering::SeqCst)
    }
}

impl Default for GilRendezvous {
    fn default() -> Self {
        Self::new()
    }
}

/// Python 侧线程句柄。
pub struct PythonSideHandle {
    handle: Option<std::thread::JoinHandle<()>>,
}

impl PythonSideHandle {
    /// join 该线程（保证解释器 finalize 前它已退出）。
    pub fn join(&mut self) {
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for PythonSideHandle {
    fn drop(&mut self) {
        self.join();
    }
}

/// 在一个独立线程里构造 GIL 侧的等待。
///
/// `max_hold_ms` 是**兜底上限**：无论策略如何，Python 侧最多持锁这么久。
/// 没有它，一旦互等成立且双方都等对方，进程会真的挂死
/// （远程机器只有 3.6 GB 内存，一个挂死的探针会拖垮整轮验证）。
pub fn spawn_python_side(
    rendezvous: Arc<GilRendezvous>,
    strategy: GilWaitStrategy,
    max_hold_ms: u64,
) -> PythonSideHandle {
    let handle = std::thread::Builder::new()
        .name("iv8-python-gil".to_string())
        .spawn(move || {
            // 关键：`with_gil` 在整个闭包执行期间**持有 GIL**。
            Python::with_gil(|py| {
                // ------------------------------------------------------------
                // 第 1 步：**先完成全部 Python 侧准备**（import / getattr）。
                // ------------------------------------------------------------
                //
                // 这一段的顺序是本探针的一个**实测修正**，务必保留。
                //
                // 首版实现把 `import("time")` / `getattr("sleep")` 放在
                // 「置就绪标志」**之后**。后果：标志已置 true，但 Python 线程
                // 还在持着 GIL 做 import。页面线程看到 ready 就开始 `with_gil`，
                // 于是它实际在等的是**对方的 import**，而不是设计中的「等待条件」。
                // 在机器过载（8 核 / load ~10-16）时，import 与随后的调度延迟
                // 叠加，可把 `GIL_BLOCKED_MS` 从 0 抬到 10 秒量级，
                // 使**对照组（sleeping）被误判为 TIMEOUT**。
                //
                // 修正：准备做在标志**之前**，使标志的语义严格等于
                // 「我已进入等待循环」—— 这与 `wait_for_python_side` 的
                // 语义契约一致。
                let time = py.import("time").expect("import time");
                if strategy == GilWaitStrategy::NotHolding {
                    // 确定性对照：准备完就**退出 with_gil（= 交还 GIL）**，
                    // 等待放到闭包之外。
                    rendezvous.python_side_waiting.store(true, Ordering::SeqCst);
                    return;
                }
                match strategy {
                    GilWaitStrategy::NotHolding => unreachable!("上方已提前 return"),
                    GilWaitStrategy::SpinHoldingGil => {
                        let monotonic = time.getattr("monotonic").expect("time.monotonic");

                        // ----------------------------------------------------
                        // 第 2 步：**准备完成后**才宣布就绪。
                        // ----------------------------------------------------
                        // 只有本策略才宣布「我**正持 GIL** 并忙等」。
                        // `Sleeping` 策略**不能**置这个标志 —— 它在
                        // `time.sleep()` 期间已把 GIL 交还 CPython，
                        // 置了就是**假绿**（把「没持锁」说成「持锁」）。
                        rendezvous.python_side_holding.store(true, Ordering::SeqCst);
                        spin_holding_gil_in_python(&monotonic, &rendezvous, max_hold_ms);
                    }
                    GilWaitStrategy::Sleeping => {
                        let sleep = time.getattr("sleep").expect("time.sleep");

                        // 宣布「我已进入等待，但**当前不持 GIL**」。
                        // 页面线程据此能区分真/伪互等。
                        rendezvous.python_side_waiting.store(true, Ordering::SeqCst);
                        sleep_in_python(&sleep, &rendezvous, max_hold_ms);
                    }
                }
            });

            // 确定性对照的等待放在 `with_gil` **之外** ⇒ Python 侧此刻
            // 完全不持 GIL ⇒ 页面线程的 `with_gil` 不可能被它挡住。
            if strategy == GilWaitStrategy::NotHolding {
                let deadline = Instant::now() + Duration::from_millis(max_hold_ms);
                while Instant::now() < deadline && !rendezvous.should_release() {
                    std::thread::sleep(Duration::from_millis(2));
                }
            }
        })
        .expect("spawn python-side thread");

    PythonSideHandle {
        handle: Some(handle),
    }
}

/// 等待 Python 侧线程进入其等待状态（**有界**）。
///
/// ## 为什么必须等，不能「sleep 一下就走」
///
/// 首次 `Python::with_gil` 会触发 **CPython 全量初始化**（本机实测
/// 几十毫秒；机器过载时可达秒级）。若不等它，页面线程会在 Python 线程
/// **还没进入等待循环**时就发起 `with_gil` ⇒ 争用条件不成立 ⇒ 结论不可信。
///
/// ## 超时值为什么必须给足（实测修正）
///
/// 首版超时是 500ms。在 8 核 / load ~10-16 的机器上，CPython 初始化
/// 会被拖长，500ms 不够 ⇒ 握手失败 ⇒ 页面线程与 Python 侧的**初始化**
/// 抢 GIL（而非与设计中的「等待条件」竞争）⇒ 对照组（`sleeping`）的
/// `GIL_BLOCKED_MS` 被抬高到秒级，**被误判为 `TIMEOUT`**。
///
/// 现由 main 按 watchdog 计算（`max(5×watchdog, 5s)`），并把结果作为
/// `PYTHON_HANDSHAKE_READY` 打进证据 —— 握手失败的运行**必须可见**，
/// 不能悄悄按一个未成立的争用条件出结论。
///
/// 有界而非无限：Python 初始化若真的失败，探针必须能继续并如实报告，
/// 而不是自己把自己挂死。
pub fn wait_for_python_side(rendezvous: &GilRendezvous, timeout: std::time::Duration) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        if rendezvous.python_side_holding() || rendezvous.python_side_waiting() {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    false
}

/// 策略一：`time.sleep()`（**释放 GIL**，构造不出互等）。
///
/// ## 分片长度为什么是 50ms 而不是 5ms（实测修正）
///
/// `time.sleep()` 期间 CPython 释放 GIL，但它**返回时会重新获取 GIL**。
/// 分片越短，「持 GIL 的时间占比」越高，页面线程抢到 GIL 的窗口越少。
/// 首版用 5ms 分片，在过载机器上观察到对照组被拖到秒级；
/// 改为 50ms 分片后，Python 侧约 99.9% 的时间不持 GIL，对照组恢复稳定。
///
/// 分片只影响 `request_release()` 的响应延迟（≤50ms），不影响任何结论。
fn sleep_in_python(sleep: &Bound<'_, PyAny>, rendezvous: &GilRendezvous, max_hold_ms: u64) {
    let chunk_secs = sleep_chunk_secs();
    let deadline = Instant::now() + Duration::from_millis(max_hold_ms);

    while Instant::now() < deadline && !rendezvous.should_release() {
        sleep.call1((chunk_secs,)).expect("time.sleep()");
    }
}

/// `time.sleep()` 的分片长度（秒）。
///
/// ## 为什么这个常数是一个**可配置的诊断旋钮**，而不是硬编码
///
/// 它是本探针一次真实「翻转事故」的根因：
/// 首版取 5ms，`time.sleep()` 每 5ms 就返回并**重新获取 GIL**，
/// 于是 Python 侧「持 GIL 的时间占比」偏高；在 8 核 / load 30+ 的机器上，
/// 页面线程的 `take_gil` 可能连续多轮抢不到，`GIL_BLOCKED_MS` 从 0
/// 被抬到**秒级**，使**对照组（sleeping）被误判为 `TIMEOUT`**。
/// 改到 50ms 后 Python 侧约 99.9% 的时间不持 GIL，对照恢复稳定。
///
/// 保留环境变量是为了让这个失效模式**可确定性复现**
/// （`IV8_PROBE_SLEEP_CHUNK_MS=5` 即可回到旧行为），
/// 而不是只能靠「把机器压垮」去碰运气 —— 这与 D0「让失败可判定」同一原则。
///
/// 默认 50ms：只影响 `request_release()` 的响应延迟（≤50ms），不影响结论。
fn sleep_chunk_secs() -> f64 {
    const DEFAULT_CHUNK_MS: u64 = 50;
    std::env::var("IV8_PROBE_SLEEP_CHUNK_MS")
        .ok()
        .and_then(|raw| raw.trim().parse::<u64>().ok())
        .filter(|ms| *ms > 0)
        .unwrap_or(DEFAULT_CHUNK_MS) as f64
        / 1000.0
}

/// 策略二：纯 Python 忙等，**全程持 GIL**（构造真正的互等）。
///
/// `time.monotonic()` 是纯查询（不释放 GIL），故整个循环都在持锁状态下进行。
/// 页面线程的 `Python::with_gil` 因此会一直阻塞到 `max_hold_ms` 兜底或被释放。
fn spin_holding_gil_in_python(
    monotonic: &Bound<'_, PyAny>,
    rendezvous: &GilRendezvous,
    max_hold_ms: u64,
) {
    let start: f64 = monotonic
        .call0()
        .expect("monotonic()")
        .extract()
        .unwrap_or(0.0);
    let deadline = start + (max_hold_ms as f64) / 1000.0;

    while !rendezvous.should_release() {
        let now: f64 = monotonic
            .call0()
            .expect("monotonic()")
            .extract()
            .unwrap_or(0.0);
        if now >= deadline {
            break;
        }
        // 纯计算：不下任何会释放 GIL 的调用。
        // 目的是「持有 GIL 且不释放」，故只做浮点运算。
        std::hint::black_box((now * 1.000_000_1).floor());
    }
}

/// 在泵内同步获取 GIL 并执行一个 Python 表达式（D4 的被测点）。
///
/// 这是**整个 GIL 死锁路径的核心测量**。若 Python 侧此刻持 GIL 不放，
/// 本次 `with_gil` 就会阻塞到 watchdog 到点。
///
/// 记录**两次**时刻，缺一不可：
/// - `gil_attempt_at_ms`：泵内**发起** `with_gil` 的时刻；
/// - `gil_acquired_at_ms`：真正**拿到** GIL 的时刻。
/// 两者之差就是「GIL 被阻塞了多久」—— 这是 D4 的定量证据。
/// 只记后者会在「一直没拿到」时丢失全部信息（因为永远不执行到那行）。
pub fn run_python_in_pump(expression: &str, obs: &Arc<ProbeObservations>, started_at: Instant) {
    let attempt_at = started_at.elapsed().as_millis() as u64;
    obs.gil_attempt_at_ms.store(attempt_at, Ordering::SeqCst);

    let outcome = Python::with_gil(|py| {
        // 真正进解释器执行字节码（而非 Rust 侧空转），
        // 这样「拿到 GIL 并干活」才是一次真实的跨语言往返。
        let c_expression = std::ffi::CString::new(expression)
            .unwrap_or_else(|_| std::ffi::CString::new("1 + 1").expect("static expr"));
        match py.eval(c_expression.as_c_str(), None, None) {
            Ok(value) => Ok(value.to_string()),
            Err(err) => Err(err.to_string()),
        }
    });

    let acquired_at = started_at.elapsed().as_millis() as u64;
    obs.gil_acquired_at_ms.store(acquired_at, Ordering::SeqCst);
    obs.gil_acquired.store(true, Ordering::SeqCst);

    match outcome {
        Ok(value) => println!("  PUMP_PYTHON_RESULT={value}"),
        Err(err) => println!("  PUMP_PYTHON_ERROR={err}"),
    }
}

/// 一次性自检：确认 Python 解释器真的被拉起来了，且能执行字节码。
///
/// **这是 D4 的方法自证**（对齐 ADR-0007 的 `BASELINE_VALID` 范式）：
/// 若 `pyo3` 没接上真解释器，`Python::with_gil` 会 panic，
/// 而 D4 的所有「不死锁」结论都会变成假绿。必须先证明这条链路是通的。
///
/// 返回 Python 版本串；失败返回 `Err` 并给出可诊断的原因。
pub fn selftest() -> Result<String, String> {
    Python::with_gil(|py| {
        let sys = py.import("sys").map_err(|e| e.to_string())?;
        let version: String = sys
            .getattr("version")
            .map_err(|e| e.to_string())?
            .extract()
            .map_err(|e| e.to_string())?;

        // 实算一次，确认解释器能执行字节码（而不只是返回元数据）。
        let value = py
            .eval(c_str!("6 * 7"), None, None)
            .map_err(|e| e.to_string())?
            .extract::<i32>()
            .map_err(|e| e.to_string())?;
        if value != 42 {
            return Err(format!("PYO3_SELFTEST_BAD_ARITH expected=42 got={value}"));
        }
        Ok(version)
    })
}
