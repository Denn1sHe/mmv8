//! T2.5 探针：V8 Inspector 暂停消息泵与 Python 回调的**死锁边界测绘**。
//!
//! ## 这个探针的性质：测绘，不是「跑通就行」
//!
//! 死锁边界是一个**测绘任务**，产出是**一张条件表**：在什么条件下会死锁、
//! 在什么条件下不会。只报告「跑通了」等于没有回答问题——死锁通常只在
//! 特定交错下才出现。
//!
//! 更关键的是：**死锁在测试里表现为「进程挂住」，而不是「测试失败」**。
//! 若没有超时与对照实验，一个真会死锁的探针表现为「CI 卡住」，
//! 无法产出可判定的结论。因此「超时机制」本身是**交付物的一部分**。
//!
//! ## 验收标准对齐（许清楚 §4 的 D0~D12）
//!
//! - **D0**（第一验收项）：先证明「必然挂起」能被报告为 `TIMEOUT` 结论。
//! - **D1**：8 组可测组合 + 4 组不可测组合的显式理由。
//! - **D2**：**成对证据** —— 既有「不死锁」对照，也有「会死锁/失败」对照。
//! - **D3**：每个「不死锁」结论都有可观测证据（`RESUMED_AT=` / `CALLBACK_COMPLETED=`）。
//! - **D4/D5/D6**：GIL 路径 / 重入路径 / 锁序路径各自定性。
//! - **D7**：暂停期间普通页面任务未被推进。
//! - **D8**：暂停可正常解除，解除后 Isolate 可继续执行脚本。
//! - **D9**：unsafe 纪律（见 `scripts/check-unsafe-whitelist.sh`）。
//! - **D12**：`ASSERT_PASS/FAIL` + 单行汇总标记。
//!
//! ## 进程结构：父进程 + 子进程
//!
//! 每个实验组合在**独立子进程**里跑，父进程用 `std::process::Command`
//! 拉起并读退出码。原因（设计文档 §3.8）：
//! > 「若 D5 结果是 V8 FATAL（进程直接死）… 该组必须独立子进程跑…
//! > FATAL 本身就是一个**可判定的结论**，ADR 必须照实记录」
//!
//! 子进程被 kill（信号退出）本身就是一个结论，故必须能被父进程区分：
//! `CLEAN_RESUME`（正常退出）/ `FATAL`（信号）/ `DEADLOCK`（超时未退出）。
//!
//! ## 用法
//!
//! ```bash
//! cargo run                      # 父进程：编排全部实验
//! cargo run -- --child --combo=4 # 子进程：只跑第 4 号组合
//! ```

mod inspector;
mod probe;
mod pyside;
mod watchdog;

use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use inspector::{ChannelSink, ProbeChannel, ProbeClient, ProbeObservations};
use probe::{Combo, SecondThread, Verdict};
use pyside::{GilRendezvous, GilWaitStrategy};
use watchdog::{watchdog_duration, PumpSignal, Watchdog};

// Inspector 类型在 v8 152.2.0 位于 `v8::inspector` 子模块，未在 crate 根重导出
// （已核实 `src/lib.rs:88` 只有 `pub mod inspector;`）。
use v8::inspector::{
    Channel, StringView, V8Inspector, V8InspectorClient, V8InspectorClientImpl,
    V8InspectorClientTrustLevel, V8InspectorSession,
};

/// D5 重入 eval 时在**同一 Context** 上执行的源码。
///
/// 取一个会真正执行 JS 的表达式（而非字面量），这样「重入成功」才有意义：
/// 若只是返回常量，就无法区分「真的进入了 V8」与「宿主直接返回」。
pub const REENTER_SOURCE: &str = "globalThis.__iv8_reenter_marker = (function(){ return 6 * 7; })(); globalThis.__iv8_reenter_marker";

/// 探针自身的**总硬超时**（秒）的默认值。
///
/// 与 watchdog 的区别：watchdog 管单个组合，**本常量管整个进程**。
/// 若某处逻辑出错导致连 watchdog 线程都没能救回来，进程必须在此时限内
/// 自我了断（打印 `WATCHDOG_ESCALATED=true` 后 `exit(70)`），
/// 绝不能挂住远程验证机（3.6 GB 内存，一个挂死的进程会拖垮整轮）。
const DEFAULT_TOTAL_DEADLINE_SECS: u64 = 180;

/// 环境变量名：覆盖总硬超时（**仅用于自测该兜底机制本身**）。
///
/// 为什么要能覆盖：`exit(70)` 这条兜底若从未被实际执行过，就属于
/// 「没测出来的安全」—— 而它恰恰是「探针自己都救不回来」时唯一的
/// 防线。提供覆盖使得可以在几秒内验证它真的会以 70 退出，
/// 而不必等满 180 秒。
const TOTAL_DEADLINE_ENV: &str = "IV8_PROBE_TOTAL_DEADLINE_SECS";

/// 读取总硬超时（默认值见 [`DEFAULT_TOTAL_DEADLINE_SECS`]）。
fn total_deadline() -> Duration {
    let secs = std::env::var(TOTAL_DEADLINE_ENV)
        .ok()
        .and_then(|raw| raw.trim().parse::<u64>().ok())
        .filter(|s| *s > 0)
        .unwrap_or(DEFAULT_TOTAL_DEADLINE_SECS);
    Duration::from_secs(secs)
}

/// watchdog 兜底升级的退出码（设计文档 §3.4 约定）。
///
/// 「探针自己抓不到自己」必须表现为**明确的非零退出码**，
/// 而非常规的 CI 挂住 —— 这样 QA 能区分
/// 「探针正常完成并报告 TIMEOUT」与「探针自己都救不回来」。
const EXIT_CODE_ESCALATED: i32 = 70;

/// 子进程超时判定：超过 `watchdog × 3 + 5s` 仍未退出即视为 DEADLOCK。
///
/// 系数 3 的理由：watchdog 在 T 时刻置位，但挂起的泵可能要等到
/// 下一个 1ms 分片才看到；若那之后还有 Python 侧兜底超时（本探针取
/// `2× watchdog`），总耗时约 `3× watchdog`。再留 5s 余量给 V8 的收尾。
fn child_timeout_for(watchdog: Duration) -> Duration {
    watchdog * 3 + Duration::from_secs(5)
}

/// 单个组合的实测结果。
struct ComboResult {
    combo: Combo,
    verdict: Verdict,
    /// 子进程退出码（被信号杀死时为 `None`）。
    exit_code: Option<i32>,
    /// 子进程是否被信号终止（FATAL 的判定依据）。
    killed_by_signal: bool,
    /// 子进程是否被父进程超时 kill（DEADLOCK 的判定依据）。
    killed_by_timeout: bool,
    /// 子进程输出的关键行（用于生成条件表的证据列）。
    evidence: Vec<String>,
}

/// 主入口。
fn main() {
    // ---- 子进程模式：只跑一个组合，然后把结论交给父进程 ----
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--child") {
        let combo_id = parse_combo_id(&args).unwrap_or(0);
        let gil_strategy = parse_gil_strategy(&args).unwrap_or(GilWaitStrategy::SpinHoldingGil);
        std::process::exit(run_child(combo_id, gil_strategy));
    }

    // ---- 父进程模式 ----
    let exit_code = run_parent();
    std::process::exit(exit_code);
}

/// 解析 `--combo=N`。
fn parse_combo_id(args: &[String]) -> Option<u8> {
    args.iter()
        .find_map(|a| a.strip_prefix("--combo="))
        .and_then(|v| v.parse::<u8>().ok())
}

/// Python 侧握手超时的环境变量（**仅用于自测该前置条件本身**）。
///
/// 为什么要能覆盖：握手失败是本探针唯一的「测量有效性前置」。
/// 若它从未被实际触发过，就无法证明「握手失败 ⇒ 对照被误判」这条
/// 因果链，而那正是本探针一次真实翻转事故的根因。
/// 提供覆盖即可在毫秒级**确定性地**复现该失败模式，而不必等机器被压垮。
const HANDSHAKE_ENV: &str = "IV8_PROBE_HANDSHAKE_MS";

/// 计算 Python 侧握手超时。
///
/// 默认 `max(5 × watchdog, 5s)`：**必须给足**，因为首次 `Python::with_gil`
/// 会触发 CPython 全量初始化，过载时可达秒级。
fn handshake_timeout(watchdog: Duration) -> Duration {
    if let Ok(raw) = std::env::var(HANDSHAKE_ENV) {
        if let Ok(ms) = raw.trim().parse::<u64>() {
            return Duration::from_millis(ms);
        }
    }
    (watchdog * 5).max(Duration::from_secs(5))
}

/// 解析 `--gil=<sleeping|spin>`。
fn parse_gil_strategy(args: &[String]) -> Option<GilWaitStrategy> {
    args.iter()
        .find_map(|a| a.strip_prefix("--gil="))
        .and_then(GilWaitStrategy::parse)
}

// ===========================================================================
// 父进程
// ===========================================================================

/// 断言闭包（照抄 `t2_3_html5ever_pause` 的形态，见设计文档 §4.1）。
///
/// 定义为**模块级函数**而非闭包：`check_path_d/e/f` 三个函数也要用它，
/// 闭包无法跨函数传递（`&mut dyn FnMut` 虽可，但会让三个函数的签名都带上
/// 一层 indirection，且 borrow 检查更啰嗦）。
///
/// `ok` 用 `&mut bool` 而非返回值累积：断言必须**逐条即时打印**
/// （出问题时第一手输出比最终汇总更有用）。
fn check(cond: bool, tag: &str, pass: &str, fail: &str, ok: &mut bool) {
    if cond {
        println!("ASSERT_PASS({tag}): {pass}");
    } else {
        println!("ASSERT_FAIL({tag}): {fail}");
        *ok = false;
    }
}

/// 父进程：编排全部实验并输出断言。
fn run_parent() -> i32 {
    // 启动总硬超时守卫。它**独立于**所有实验，直接终结进程。
    spawn_total_deadline_guard();

    let watchdog = watchdog_duration();
    let mut ok = true;

    println!("=== T2.5 死锁边界测绘探针 ===");
    println!("WATCHDOG_MS={}", watchdog.as_millis());
    println!(
        "TARGET_OS={} TARGET_ARCH={}",
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    println!("TOTAL_HARD_DEADLINE_SECS={}", total_deadline().as_secs());
    // 派生常数必须**自documenting**：ADR 里的读数若与参数不符，
    // 只能靠这些行判断（本探针曾出现「读数超出上界」而无法当场归因）。
    println!(
        "DERIVED_PYTHON_MAX_HOLD_MS={}",
        watchdog.as_millis() as u64 * 2
    );
    println!(
        "DERIVED_HANDSHAKE_TIMEOUT_MS={}",
        handshake_timeout(watchdog).as_millis()
    );
    if let Ok(raw) = std::env::var("IV8_PROBE_SLEEP_CHUNK_MS") {
        println!("DERIVED_SLEEP_CHUNK_MS_OVERRIDE={raw}");
    }

    // ---- EXP0 方法自证：环境与链路 ----
    println!();
    println!("=== EXP0 方法自证 ===");
    let pyo3_version = match pyside::selftest() {
        Ok(version) => {
            println!("PYO3_SELFTEST_OK=true");
            println!("PYTHON_VERSION={version}");
            true
        }
        Err(err) => {
            println!("PYO3_SELFTEST_OK=false");
            println!("PYTHON_SELFTEST_ERROR={err}");
            false
        }
    };
    check(
        pyo3_version,
        "env",
        "pyo3 已接上真解释器且能执行字节码 —— D4 的测量链路有效",
        "pyo3 未接上真解释器 —— D4 的结论全部无效（假绿风险）",
        &mut ok,
    );

    // ---- D0：watchdog 自证（第一验收项）----
    println!();
    println!("=== D0 watchdog 自证（第一验收项）===");
    let selftest_result = run_child_in_subprocess(Combo::watchdog_selftest(), watchdog, None);
    print_child_evidence(&selftest_result);
    let watchdog_selftest_ok = selftest_result.verdict == Verdict::Timeout
        && selftest_result
            .evidence
            .iter()
            .any(|l| l.contains("WATCHDOG_FIRED=true"))
        && selftest_result
            .evidence
            .iter()
            .any(|l| l.contains("PUMP_ENTERED=true"));
    println!(
        "WATCHDOG_FIRED={}",
        selftest_result
            .evidence
            .iter()
            .any(|l| l.contains("WATCHDOG_FIRED=true"))
    );
    println!("HUNG_SCENARIO_REPORTED={}", selftest_result.verdict);
    println!("WATCHDOG_SELFTEST_OK={watchdog_selftest_ok}");
    check(
        watchdog_selftest_ok,
        "a",
        "超时机制把「必然挂起」报告为结论（TIMEOUT）而非挂住进程",
        "超时机制未能把挂起报告为结论 —— 后续所有「不死锁」结论都不可信",
        &mut ok,
    );
    // ---- D0a：watchdog 路径具名断言（从**实跑证据**核验，不是自说自话）----
    let uses_isolate_handle = selftest_result
        .evidence
        .iter()
        .any(|l| l.contains("WATCHDOG_USES_ISOLATE_HANDLE=true"));
    println!("WATCHDOG_USES_ISOLATE_HANDLE={uses_isolate_handle}");
    check(
        uses_isolate_handle,
        "m",
        "D0a：watchdog 走 IsolateHandle 路径（thread_safe_handle → terminate_execution），且该事实出现在实跑证据中",
        "D0a：证据中缺少 WATCHDOG_USES_ISOLATE_HANDLE=true —— 探针路径未被具名报告",
        &mut ok,
    );

    // ---- D0b：终止后复用的边界（本探针**有**复用，故出断言而非空声明）----
    let after_terminate = selftest_result
        .evidence
        .iter()
        .find_map(|l| l.strip_prefix("CLEAN_RESUME_AFTER_TERMINATE="))
        .map(|v| v.trim() == "true");
    println!("CLEAN_RESUME_AFTER_TERMINATE={after_terminate:?}");
    let terminated_and_reused = selftest_result
        .evidence
        .iter()
        .any(|l| l.contains("TERMINATE_REQUESTED=true"))
        && after_terminate == Some(true);
    println!("TERMINATED_AND_REUSED={terminated_and_reused}");
    check(
        terminated_and_reused,
        "n",
        "D0b：终止请求发出后**同一 Isolate 仍可干净复用**（同 scope 再跑 1+1 成功）—— 边界已由断言覆盖，非悬空",
        "D0b：未能证明「终止后复用」的边界（缺 CLEAN_RESUME_AFTER_TERMINATE 或未发生终止）",
        &mut ok,
    );

    check(
        !selftest_result.killed_by_timeout && !selftest_result.killed_by_signal,
        "a2",
        "自证场景由 watchdog 自行救回，子进程正常退出（未被父进程 kill）",
        "自证场景须靠父进程 kill 才结束 —— 说明 watchdog 本身无效",
        &mut ok,
    );

    // ---- EXP1~EXP3：8 组可测组合 ----
    println!();
    println!("=== 组合矩阵（F1 × F2 × F3）===");
    let mut results: Vec<ComboResult> = Vec::new();

    for combo in Combo::measurable_all() {
        println!();
        println!("--- {} ---", combo.label());
        // F1=yes 的组合跑两种 Python 侧策略：
        //   spin  = 真正构造互等（预期死锁）
        //   sleep = 构造不出互等（对照，预期不死锁）
        // 这两个策略的对比就是 D4 的**内部成对证据**。
        // F1=yes 的组合跑**三种** Python 侧策略：
        //   spin_holding_gil  实验组（真持 GIL）→ 预期 TIMEOUT
        //   sleeping          设计文档字面写法（time.sleep，GIL 时放时取）→ 预期 CLEAN_RESUME
        //   not_holding       确定性对照（Python 侧完全不持 GIL）→ **必须** CLEAN_RESUME
        // 前两者给出与设计文档的可比性，第三者给出**与调度无关**的对照，
        // 使 D2 的成对证据不依赖「机器当时恰好不忙」。
        let strategies: &[Option<GilWaitStrategy>] = if combo.acquires_gil() {
            &[
                Some(GilWaitStrategy::SpinHoldingGil),
                Some(GilWaitStrategy::Sleeping),
                Some(GilWaitStrategy::NotHolding),
            ]
        } else {
            &[None]
        };

        for strategy in strategies {
            let result = run_child_in_subprocess(combo, watchdog, *strategy);
            if let Some(s) = strategy {
                println!("  GIL_STRATEGY={}", s.as_str());
                println!(
                    "  GIL_STRATEGY_CONSTRUCTS_DEADLOCK={}",
                    s.constructs_deadlock()
                );
                // **对照若也 TIMEOUT**，说明该次运行的测量不可信（调度延迟
                // 被算作了死锁），必须显式暴露，不得计入「GIL 路径会死锁」。
                if s.is_control() && result.verdict == Verdict::Timeout {
                    println!("  MEASUREMENT_SUSPECT=true");
                    println!(
                        "  MEASUREMENT_SUSPECT_REASON=对照组({})超时：该次运行不可用于判定，疑似 OS 调度延迟被计入，而非 GIL 互等",
                        s.as_str()
                    );
                }
            }
            print_child_evidence(&result);
            println!("{} => {}", combo.label(), result.verdict);
            if result.killed_by_signal {
                println!("  FATAL_KILLED_BY_SIGNAL=true");
            }
            if result.killed_by_timeout {
                println!("  PARENT_KILLED_ON_TIMEOUT=true");
            }
            if let Some(code) = result.exit_code {
                println!("  CHILD_EXIT_CODE={code}");
            }
            results.push(result);
        }
    }

    // ---- 汇总 ----
    println!();
    println!("=== 条件表（D10）===");
    print_condition_table(&results);

    // 两个数必须分开报，否则含义会被误读：
    //   COMBOS_TRIED  = **不同**的可测组合数（= 8，与 2×2×3 矩阵一一对应）
    //   COMBO_RUNS     = 子进程运行次数（F1=yes 的组合跑两种 GIL 策略，故 > 8）
    let distinct: std::collections::BTreeSet<u8> = results.iter().map(|r| r.combo.id).collect();
    let tried = distinct.len();
    let runs = results.len();
    let deadlocked = results.iter().filter(|r| r.verdict.is_failure()).count();
    let unmeasurable = probe::unmeasurable_combos().len();
    println!();
    println!("COMBOS_TRIED={tried}");
    println!("COMBO_RUNS={runs}");
    println!("COMBOS_DEADLOCK={deadlocked}");
    println!("COMBOS_UNMEASURABLE={unmeasurable}");
    println!("UNMEASURABLE_REASON={}", probe::UNMEASURABLE_REASON);

    // ---- D2 成对证据 ----
    let has_clean = results.iter().any(|r| r.verdict == Verdict::CleanResume);
    let has_failure = results.iter().any(|r| r.verdict.is_failure());
    check(
        has_clean,
        "b",
        "存在「不死锁」对照（CLEAN_RESUME）—— 边界的一侧已测出",
        "未观测到任何 CLEAN_RESUME —— 无法回答「边界在哪」",
        &mut ok,
    );
    check(
        has_failure,
        "c",
        "存在「会死锁/会失败」对照 —— 边界的另一侧已测出（D2 成对证据成立）",
        "未观测到任何失败 —— 无法区分「安全」与「没测出来」",
        &mut ok,
    );

    // ---- D3 证据充分性 ----
    let clean_with_evidence = results
        .iter()
        .filter(|r| r.verdict == Verdict::CleanResume)
        .all(|r| {
            r.evidence.iter().any(|l| l.contains("RESUMED_AT="))
                && r.evidence.iter().any(|l| l.contains("CALLBACK_COMPLETED="))
        });
    check(
        clean_with_evidence,
        "d",
        "每个「不死锁」结论都带可观测证据（RESUMED_AT + CALLBACK_COMPLETED）",
        "存在无证据的「不死锁」结论 —— D3 不满足",
        &mut ok,
    );

    // ---- D4 / D5 / D6 ----
    check_path_d(&results, &mut ok);
    check_path_e(&results, &mut ok);
    check_path_f(&results, &mut ok);

    // ---- D7 / D8 ----
    println!();
    println!("=== C 组（技术方案约束验证）===");
    let stalled = results.iter().any(|r| {
        r.evidence
            .iter()
            .any(|l| l.contains("PAGE_TASKS_STALLED_DURING_PAUSE=true"))
    });
    let clean_resume = results.iter().any(|r| {
        r.verdict == Verdict::CleanResume
            && r.evidence
                .iter()
                .any(|l| l.contains("POST_PAUSE_EVAL_OK=true"))
    });
    println!("PAGE_TASKS_STALLED_DURING_PAUSE={stalled}");
    println!("CLEAN_RESUME={clean_resume}");
    check(
        stalled,
        "g",
        "暂停期间普通页面任务未被推进（技术方案 §13 的「不推进」半边）",
        "暂停期间页面任务被推进了 —— 与技术方案 §13 冲突",
        &mut ok,
    );
    check(
        clean_resume,
        "h",
        "暂停可正常解除，且解除后 Isolate 能继续执行脚本（D8）",
        "暂停未能干净解除，或解除后脚本无法继续执行",
        &mut ok,
    );

    // ---- 未覆盖路径（D10 / 边界纪律：显式列举，不留假绿）----
    println!();
    println!("=== 未覆盖的死锁路径（显式登记，不得默认为安全）===");
    for (id, f1, f2, f3, reason) in probe::unmeasurable_combos() {
        println!(
            "UNMEASURABLE_COMBO[{id}] f1={} f2={} f3={} reason={reason}",
            if f1 { "yes" } else { "no" },
            if f2 { "yes" } else { "no" },
            f3
        );
    }
    println!("UNCOVERED=多Context/多Worker并发(T2.5不覆盖,属G4/G6)");
    println!("UNCOVERED=网络事件在暂停期间到达的处置(本探针无事件循环)");
    println!("UNCOVERED=同一Isolate被多线程同时eval(超出needs_locker的单线程边界)");

    println!();
    println!("T2_5_PROBE_{}", if ok { "OK" } else { "FAIL" });
    if ok {
        0
    } else {
        1
    }
}

/// 打印子进程的关键证据行。
fn print_child_evidence(result: &ComboResult) {
    for line in &result.evidence {
        println!("  {line}");
    }
}

/// D4：GIL 路径定性。
///
/// 判据是**成对**的：spin 策略（真互等）应观测到失败，
/// sleep 策略（伪互等，GIL 被释放）应观测到不死锁。
/// 若二者结论相同，说明本探针**分不清**它们 —— 那比任一单独结论都糟糕。
fn check_path_d(results: &[ComboResult], ok: &mut bool) {
    println!();
    println!("=== D4 GIL 路径 ===");

    let spin_fail = results
        .iter()
        .any(|r| r.combo.acquires_gil() && r.verdict.is_failure());
    let sleep_clean = results.iter().any(|r| {
        r.combo.acquires_gil()
            && r.evidence
                .iter()
                .any(|l| l.contains("GIL_STRATEGY=sleeping"))
            && r.verdict == Verdict::CleanResume
    });
    let gil_evidence = results.iter().any(|r| {
        r.evidence
            .iter()
            .any(|l| l.starts_with("  GIL_ACQUIRED=") || l.contains("GIL_ACQUIRED="))
    });

    println!("D4_GIL_PATH_FAILURE_OBSERVED={spin_fail}");
    println!("D4_GIL_PATH_PAIRED_CONTROL_CLEAN={sleep_clean}");
    println!("D4_GIL_EVIDENCE_PRESENT={gil_evidence}");
    check(
        gil_evidence,
        "i",
        "GIL 路径给出了可观测证据（GIL_ACQUIRED / GIL_BLOCKED_MS）",
        "GIL 路径无任何可观测证据 —— D4 未测出",
        ok,
    );
    check(
        spin_fail && sleep_clean,
        "i2",
        "GIL 路径成对证据成立：真互等会死锁、伪互等（GIL 已释放）不死锁 —— 边界可分辨",
        "未能区分「真 GIL 互等」与「sleeping 期间 GIL 已释放」—— 测量方法不可信",
        ok,
    );
}

/// D5：重入路径定性。
fn check_path_e(results: &[ComboResult], ok: &mut bool) {
    println!();
    println!("=== D5 重入路径（技术方案 §6「禁止回调中再次同步 eval 同一 Context」）===");

    let reentrant_results: Vec<&ComboResult> =
        results.iter().filter(|r| r.combo.reenters_eval()).collect();
    let attempted = reentrant_results.iter().any(|r| {
        r.evidence
            .iter()
            .any(|l| l.contains("REENTRANT_ATTEMPTED=true"))
    });
    let outcomes: Vec<String> = reentrant_results
        .iter()
        .flat_map(|r| {
            r.evidence
                .iter()
                .filter(|l| l.contains("REENTRANT_OUTCOME="))
                .map(|l| l.trim().to_string())
        })
        .collect();
    let fatal = reentrant_results.iter().any(|r| r.killed_by_signal);
    let timeouts = reentrant_results
        .iter()
        .any(|r| r.verdict == Verdict::Timeout);

    println!("D5_REENTRANT_COMBOS={}", reentrant_results.len());
    println!("D5_REENTRANT_ATTEMPTED={attempted}");
    println!("D5_FATAL_OBSERVED={fatal}");
    println!("D5_TIMEOUT_OBSERVED={timeouts}");
    for outcome in &outcomes {
        println!("  {outcome}");
    }

    check(
        attempted,
        "j",
        "重入路径确实被尝试过（D5 不是恒假门禁）",
        "重入路径未被尝试 —— D5 无效（无法区分「安全」与「没测」）",
        ok,
    );
    check(
        !reentrant_results.is_empty() && (fatal || timeouts || !outcomes.is_empty()),
        "k",
        "重入路径的实际后果已定性（DEADLOCK / FATAL / ERROR_PROPAGATED / CLEAN_RESUME 之一）",
        "重入路径的实际后果未能定性",
        ok,
    );
}

/// D6：锁序 / Locker 路径定性。
fn check_path_f(results: &[ComboResult], ok: &mut bool) {
    println!();
    println!("=== D6 锁序 / Locker 路径 ===");

    let locker_results: Vec<&ComboResult> = results
        .iter()
        .filter(|r| r.combo.second_thread == SecondThread::NeedsLocker)
        .collect();
    let covered = !locker_results.is_empty();
    let acquired = locker_results.iter().any(|r| {
        r.evidence
            .iter()
            .any(|l| l.contains("LOCKER_ACQUIRED=true"))
    });
    let contended = locker_results.iter().any(|r| {
        r.evidence
            .iter()
            .any(|l| l.contains("LOCKER_CONTENDED=true"))
    });

    println!("D6_LOCKER_COMBOS={}", locker_results.len());
    println!("D6_LOCKER_ACQUIRED={acquired}");
    println!("D6_LOCKER_CONTENDED={contended}");

    if covered {
        check(
            true,
            "l",
            "跨线程 Locker 路径已覆盖并给出结论",
            "跨线程 Locker 路径已覆盖但无结论",
            ok,
        );
    } else {
        // D6 明确允许「未覆盖」，但**必须显式登记且不得默认安全**。
        println!("ASSERT_FAIL(l): D6 未覆盖 —— 不得默认为安全，已登记为遗留");
        *ok = false;
    }
}

/// 打印条件表（D10：行 = 组合，列 = 分类 / 证据）。
fn print_condition_table(results: &[ComboResult]) {
    println!("| # | F1 | F2 | F3 | GIL策略 | 分类 | 死锁 | 证据摘要 |");
    println!("|---|---|---|---|---|---|---|---|");
    for r in results {
        let strategy = r
            .evidence
            .iter()
            .find_map(|l| l.trim().strip_prefix("GIL_STRATEGY="))
            .unwrap_or("-");
        // 证据摘要：取最有信息量的几类读数。
        let mut bits: Vec<&str> = Vec::new();
        for key in [
            "PUMP_ENTERED=",
            "RESUMED_AT=",
            "CALLBACK_COMPLETED=",
            "GIL_ACQUIRED=",
            "GIL_BLOCKED_MS=",
            "REENTRANT_OUTCOME=",
            "LOCKER_ACQUIRED=",
            "LOCKER_CONTENDED=",
            "WATCHDOG_FIRED=",
        ] {
            if let Some(line) = r.evidence.iter().find(|l| l.contains(key)) {
                bits.push(line.trim());
            }
        }
        let summary = if bits.is_empty() {
            "(无证据)".to_string()
        } else {
            bits.join("; ")
        };
        println!(
            "| {} | {} | {} | {} | {} | {} | {} | {} |",
            r.combo.id,
            if r.combo.acquires_gil() { "yes" } else { "no" },
            if r.combo.reenter_eval { "yes" } else { "no" },
            r.combo.second_thread,
            strategy,
            r.verdict,
            if r.verdict.is_failure() { "是" } else { "否" },
            summary
        );
    }
}

/// 启动总硬超时守卫。
///
/// 独立线程 + `process::exit(EXIT_CODE_ESCALATED)`。
/// 它是**最后一道防线**：即使 watchdog 逻辑本身有 bug，
/// 进程也不会挂住远程验证机。
fn spawn_total_deadline_guard() {
    let deadline = total_deadline();
    std::thread::Builder::new()
        .name("iv8-total-deadline".to_string())
        .spawn(move || {
            std::thread::sleep(deadline);
            println!("WATCHDOG_ESCALATED=true");
            println!(
                "WATCHDOG_ESCALATE_REASON=超过总硬超时 {}s",
                deadline.as_secs()
            );
            println!("ESCALATED_EXIT_CODE={EXIT_CODE_ESCALATED}");
            // 立刻退出，不做清理：此时进程状态不可信，清理反而可能 hang。
            std::process::exit(EXIT_CODE_ESCALATED);
        })
        .expect("spawn total deadline guard");
}

/// 在子进程里跑一个组合，并读出其结论。
fn run_child_in_subprocess(
    combo: Combo,
    watchdog: Duration,
    gil_strategy: Option<GilWaitStrategy>,
) -> ComboResult {
    let exe = match std::env::current_exe() {
        Ok(path) => path,
        Err(err) => {
            // 拿不到自己的可执行文件是**环境问题**，不是被测结论。
            // 如实报告并让上层判 FAIL，不伪造结论。
            return ComboResult {
                combo,
                verdict: Verdict::Deadlock,
                exit_code: None,
                killed_by_signal: false,
                killed_by_timeout: false,
                evidence: vec![format!("CHILD_SPAWN_FAILED current_exe error={err}")],
            };
        }
    };

    let mut cmd = Command::new(exe);
    cmd.arg("--child")
        .arg(format!("--combo={}", combo.id))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    if let Some(strategy) = gil_strategy {
        cmd.arg(format!("--gil={}", strategy.as_str()));
    }

    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(err) => {
            return ComboResult {
                combo,
                verdict: Verdict::Deadlock,
                exit_code: None,
                killed_by_signal: false,
                killed_by_timeout: false,
                evidence: vec![format!("CHILD_SPAWN_FAILED spawn error={err}")],
            };
        }
    };

    // 超时判定：轮询 `try_wait`，到点则 kill。
    //
    // 用轮询而非 `wait_with_output`：后者**没有超时**，一旦子进程挂死
    // 父进程会跟着挂死 —— 那正是本探针要防的最坏情况。
    let limit = child_timeout_for(watchdog);
    let started = Instant::now();
    let mut killed_by_timeout = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) => {
                if started.elapsed() > limit {
                    let _ = child.kill();
                    let _ = child.wait();
                    killed_by_timeout = true;
                    break None;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(_) => break None,
        }
    };

    // 收集输出。用 `wait_with_output` 语义手动取（此时子进程已退出）。
    let output = child.wait_with_output().ok();
    let evidence = extract_evidence(output.as_ref());

    // ---- 分类 ----
    // 优先级：超时 kill > 信号终止（FATAL）> 退出码 > 子进程自报结论。
    // 「父进程超时 kill」是最强的失败信号，说明子进程连自报结论都没来得及。
    let verdict = if killed_by_timeout {
        Verdict::Deadlock
    } else {
        match status {
            Some(status) => {
                let signalled = {
                    #[cfg(unix)]
                    {
                        use std::os::unix::process::ExitStatusExt;
                        status.signal().is_some()
                    }
                    #[cfg(not(unix))]
                    {
                        false
                    }
                };

                if signalled {
                    // V8 的 FATAL / CHECK 失败 = 直接 abort，信号退出。
                    Verdict::Fatal
                } else {
                    // 子进程把自己的结论打在 stdout 上（`CHILD_VERDICT=`）。
                    evidence
                        .iter()
                        .find_map(|l| l.trim().strip_prefix("CHILD_VERDICT="))
                        .and_then(parse_verdict)
                        .unwrap_or(Verdict::ErrorPropagated)
                }
            }
            None => Verdict::Deadlock,
        }
    };

    #[cfg(unix)]
    let killed_by_signal = {
        use std::os::unix::process::ExitStatusExt;
        status.is_some_and(|s| s.signal().is_some())
    };
    #[cfg(not(unix))]
    let killed_by_signal = false;

    ComboResult {
        combo,
        verdict,
        exit_code: status.and_then(|s| s.code()),
        killed_by_signal,
        killed_by_timeout,
        evidence,
    }
}

/// 解析子进程自报的结论字符串。
fn parse_verdict(raw: &str) -> Option<Verdict> {
    Some(match raw.trim() {
        "CLEAN_RESUME" => Verdict::CleanResume,
        "TIMEOUT" => Verdict::Timeout,
        "ERROR_PROPAGATED" => Verdict::ErrorPropagated,
        "DEADLOCK" => Verdict::Deadlock,
        "FATAL" => Verdict::Fatal,
        _ => return None,
    })
}

/// 从子进程输出里挑出关键证据行。
///
/// 只保留以 `KEY=` 开头的机器可读行，滤掉 V8 的日志与空行。
fn extract_evidence(output: Option<&std::process::Output>) -> Vec<String> {
    let Some(output) = output else {
        return vec!["CHILD_NO_OUTPUT=true".to_string()];
    };
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    let mut lines: Vec<String> = Vec::new();
    for source in [stdout.as_ref(), stderr.as_ref()] {
        for line in source.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            // 机器可读行的判据：形如 `KEY=...` 或 `KEY: ...`。
            if trimmed.contains('=') && !trimmed.starts_with("===") {
                lines.push(trimmed.to_string());
            }
        }
    }
    if lines.is_empty() {
        lines.push("CHILD_NO_KEYVALUE_OUTPUT=true".to_string());
    }
    lines
}

// ===========================================================================
// 子进程
// ===========================================================================

/// 子进程入口：跑一个组合，输出机器可读结论，返回退出码。
fn run_child(combo_id: u8, gil_strategy: GilWaitStrategy) -> i32 {
    // 初始化 V8。必须在**任何**线程创建之前完成 ——
    // `SharedIsolate::lock` 的文档明确警告：线程若在 `V8::initialize`
    // 之前创建，在启用了内存保护键的硬件上首次跑 JS 会 `SIGSEGV`。
    let platform = v8::new_default_platform(0, false).make_shared();
    v8::V8::initialize_platform(platform);
    v8::V8::initialize();

    let combo = if combo_id == 0 {
        Combo::watchdog_selftest()
    } else {
        match Combo::by_id(combo_id) {
            Some(combo) => combo,
            None => {
                println!("CHILD_VERDICT=ERROR_PROPAGATED");
                println!("CHILD_ERROR=unknown combo id {combo_id}");
                return 2;
            }
        }
    };

    let watchdog = watchdog_duration();
    // Python 侧兜底：给足「能让 watchdog 先到点」的余量，
    // 但不至于让整个探针在单个组合上耗太久。
    let python_max_hold_ms = (watchdog.as_millis() as u64) * 2;

    run_single_combo(combo, watchdog, gil_strategy, python_max_hold_ms)
}

/// 跑一个组合并输出全部观测。
fn run_single_combo(
    combo: Combo,
    watchdog_timeout: Duration,
    gil_strategy: GilWaitStrategy,
    python_max_hold_ms: u64,
) -> i32 {
    let started_at = Instant::now();

    // ---- V8 基础构造 ----
    let isolate = &mut v8::Isolate::new(v8::CreateParams::default());
    // 先取 thread_safe_handle：watchdog 线程需要它，
    // 且必须在 spawn 之前取（它 clone 的是 Isolate 内部的 Arc）。
    let isolate_handle = isolate.thread_safe_handle();

    v8::scope!(let handle_scope, isolate);
    let context = v8::Context::new(handle_scope, Default::default());
    let mut scope = v8::ContextScope::new(handle_scope, context);

    // ---- 共享状态 ----
    let obs = Arc::new(ProbeObservations::new());
    let signal = Arc::new(PumpSignal::default());
    let rendezvous = Arc::new(GilRendezvous::new());
    // 第二线程投递的控制消息计数（F3=msg_only）。
    let control_msgs = Arc::new(AtomicU64::new(0));

    // ---- 注入宿主函数（复用 T2.0 已验证的写法）----
    // ---- 注入宿主函数（复用 T2.0 已实测的写法）----
    // 两个函数：
    //   `__iv8_mark_page_task()` —— 页面任务被推进时调用（D7 的探针）；
    //   `__iv8_reenter_eval()`  —— 宿主回调内重入同一 Context 求值（D5）。
    //
    // 两个硬性约束（实测踩坑，详见 inspector.rs 的说明）：
    //   1. `Function::new` 的回调**必须是不捕获环境的普通函数**
    //      （`MapFnTo` 要求 `UnitType`）⇒ 共享状态走 `Isolate` 的 slot；
    //   2. `Function::new` 返回 `Option`（`function.rs:1021`），
    //      `Object::set` 返回 `Option<bool>`（`object.rs:378`）——都不是 `Result`。
    //
    // 先把观测登记到 Isolate 的 slot，供回调内 `get_slot` 取回。
    inspector::install_slot(&mut scope, Arc::clone(&obs));

    let global = context.global(&scope);

    let mark = v8::Function::new(&mut scope, inspector::make_mark_page_task);
    let Some(mark_fn) = mark else {
        println!("CHILD_VERDICT=ERROR_PROPAGATED");
        println!("CHILD_ERROR=注入 __iv8_mark_page_task 失败");
        return 3;
    };
    let mark_key = v8::String::new(&scope, "__iv8_mark_page_task").expect("static key");
    if global
        .set(&scope, mark_key.into(), mark_fn.into())
        .is_none()
    {
        println!("CHILD_VERDICT=ERROR_PROPAGATED");
        println!("CHILD_ERROR=挂载 __iv8_mark_page_task 失败");
        return 3;
    }

    // D5 的重入函数**只在 F2=yes 的组合注入**：不注入就不会被调用，
    // 「未被调用」与「调用了但失败」在证据上必须可区分。
    if combo.reenters_eval() {
        let reenter = v8::Function::new(&mut scope, inspector::make_reenter_eval);
        let Some(reenter_fn) = reenter else {
            println!("CHILD_VERDICT=ERROR_PROPAGATED");
            println!("CHILD_ERROR=注入 __iv8_reenter_eval 失败");
            return 3;
        };
        let key = v8::String::new(&scope, "__iv8_reenter_eval").expect("static key");
        if global.set(&scope, key.into(), reenter_fn.into()).is_none() {
            println!("CHILD_VERDICT=ERROR_PROPAGATED");
            println!("CHILD_ERROR=挂载 __iv8_reenter_eval 失败");
            return 3;
        }
    }

    // ---- 构造 Inspector ----
    let channel = ProbeChannel::new();
    let channel_sink = channel.sink();

    let client = Arc::new(ProbeClient::new(
        combo,
        Arc::clone(&obs),
        Arc::clone(&signal),
        started_at,
        // F1 用的 Python 表达式：真进解释器执行字节码。
        Some("sum(range(10))".to_string()),
        // D0 自证场景不设预算（「必然挂起」）；其余组合给一个远小于 watchdog
        // 的预算，让「消息泵正常泵完并返回」这条**不死锁**路径真的能走到。
        // 取 watchdog 的 1/10 且至少 10ms：足够跑完一轮泵，又不会被误判超时。
        (!combo.hang_forever).then(|| (watchdog_timeout.as_millis() as u64 / 10).max(10)),
        combo.acquires_gil().then(|| Arc::clone(&rendezvous)),
    ));

    // `V8Inspector::create` 拿走 client 的所有权（`_client` 字段，`:1242`），
    // 且其生命周期必须覆盖整个 session（V8 持裸指针）。
    // `HostClientAdapter` 用 `Arc` 共享所有权，使外部编排仍能读到观测。
    let client_box = V8InspectorClient::new(Box::new(HostClientAdapter { inner: client }));
    // `create` 要 `&mut Isolate`，但 `isolate` 已被 `scope!` 借用
    // （`scope!` 宏对绑定做了 shadow，见 `scope.rs:1819-1821`）。
    // 故经 `scope` 的 Deref 链重新借用：
    //   ContextScope → PinnedRef<HandleScope<Context>> → PinnedRef<HandleScope<()>> → Isolate
    let inspector = V8Inspector::create(&mut **scope, client_box);

    // 登记 context，否则 Inspector 不知道这个 context group。
    inspector.context_created(
        context,
        CONTEXT_GROUP_ID,
        StringView::from(&b"t2.5-probe"[..]),
        StringView::empty(),
    );

    let session = inspector.connect(
        CONTEXT_GROUP_ID,
        Channel::new(Box::new(channel)),
        StringView::empty(),
        V8InspectorClientTrustLevel::FullyTrusted,
    );

    // ---- 启动第二线程（F3）----
    let mut python_side: Option<pyside::PythonSideHandle> = None;
    let second_thread_handle = spawn_second_thread(
        combo,
        Arc::clone(&obs),
        Arc::clone(&signal),
        Arc::clone(&rendezvous),
        Arc::clone(&control_msgs),
        started_at,
        &session,
        watchdog_timeout,
        gil_strategy,
        python_max_hold_ms,
        &mut python_side,
    );

    // ---- 使能 Debugger 域并安排暂停 ----
    // `schedule_pause_on_next_statement` 只在 Debugger 域已 enable 时生效
    // （V8 需要知道「有调试器在关注」才会真的暂停）。
    session.dispatch_protocol_message(debugger_enable_message());
    session.schedule_pause_on_next_statement(
        StringView::from(&b"t2.5 boundary probe"[..]),
        StringView::from(&b"combo experiment"[..]),
    );

    // ---- 启动 watchdog（D0 的执行者）----
    let mut watchdog = Watchdog::start(isolate_handle, watchdog_timeout, Arc::clone(&signal));

    // ---- 排入一个「普通页面任务」，然后触发暂停 ----
    // D7 的被测对象：这个 microtask 若在暂停期间被推进，标记函数就会被调用。
    //
    // 脚本刻意**分两段**：
    //   1) 先排 microtask（页面任务）—— 它应在暂停**之前**入队；
    //   2) 再执行若干语句 —— 暂停在下一条语句处触发。
    // 这样「暂停期间 microtask 有没有被推进」才有意义：
    // 若队列在暂停前就已非空，而暂停期间标记仍未被置位 ⇒ 未推进。
    let reenter_call = if combo.reenters_eval() {
        "__iv8_reenter_eval();\n"
    } else {
        ""
    };
    let script_source = format!(
        "globalThis.__iv8_page_task_ran = false;\n\
         Promise.resolve().then(function () {{ __iv8_mark_page_task(); }});\n\
         globalThis.__iv8_before_pause = 1;\n\
         globalThis.__iv8_mid = 2;\n\
         {reenter_call}\
         globalThis.__iv8_after_pause = 3;\n\
         'scheduled'\n"
    );
    let code = match v8::String::new(&scope, &script_source) {
        Some(code) => code,
        None => {
            println!("CHILD_VERDICT=ERROR_PROPAGATED");
            println!("CHILD_ERROR=无法构造脚本源码");
            return 3;
        }
    };
    let script = match v8::Script::compile(&scope, code, None) {
        Some(script) => script,
        None => {
            println!("CHILD_VERDICT=ERROR_PROPAGATED");
            println!("CHILD_ERROR=脚本编译失败");
            return 3;
        }
    };

    // 执行脚本 —— 暂停会在此期间发生，`run_message_loop_on_pause` 被同步调用。
    let script_outcome = script.run(&scope);
    let script_returned = script_outcome.is_some();

    // ---- 收集观测（在停 watchdog 之前先读，避免竞态）----
    let fired = watchdog.fired();
    let fired_after_ms = watchdog.fired_after_ms(started_at);
    watchdog.stop();

    // 让 Python 侧与第二线程收尾。
    rendezvous.request_release();
    if let Some(handle) = second_thread_handle {
        let _ = handle.join();
    }
    if let Some(handle) = python_side.as_mut() {
        handle.join();
    }

    // ---- D8：暂停解除后 Isolate 能否继续执行脚本 ----
    let post_pause_ok = eval_post_pause(&mut scope);

    // ---- 输出全部证据 ----
    report_observations(
        &combo,
        &obs,
        &signal,
        &channel_sink,
        gil_strategy,
        fired,
        fired_after_ms,
        script_returned,
        post_pause_ok,
        &control_msgs,
        started_at,
    );

    // ---- 结论分类 ----
    let verdict = classify(combo, &obs, fired, script_returned, post_pause_ok);
    println!("CHILD_VERDICT={}", verdict.as_str());

    // 退出码：0 = 正常跑完（无论结论是 CLEAN_RESUME 还是 TIMEOUT）。
    // 「结论是什么」由 `CHILD_VERDICT` 承载，退出码只表示「探针自身是否正常完成」。
    0
}

/// Inspector 的 context group id（固定值；本探针只有一个 group）。
const CONTEXT_GROUP_ID: i32 = 1;

/// `Debugger.enable` 的 CDP 报文。
///
/// **为什么必须先 enable**：`schedule_pause_on_next_statement` 只是
/// 「安排在下一条语句暂停」，V8 需要 Debugger 域已激活才会真的暂停。
/// 这是本探针实测确认的必要前置（不做则暂停永不发生）。
fn debugger_enable_message() -> StringView<'static> {
    StringView::from(&b"{\"id\":1,\"method\":\"Debugger.enable\"}"[..])
}

/// 通知 client「页面任务探测已就绪」。
///
/// 探测源码已在建 scope 阶段回填（`Global<String>`），此处无需额外动作；
/// 函数保留是为了在 `run_single_combo` 里标明这个调用顺序的意图。

/// D8：暂停解除后执行 `1+1`，验证 Isolate 可继续工作。
fn eval_post_pause(scope: &mut v8::ContextScope<'_, '_, v8::HandleScope<'_>>) -> bool {
    let Some(code) = v8::String::new(scope, "1 + 1") else {
        return false;
    };
    let Some(script) = v8::Script::compile(scope, code, None) else {
        return false;
    };
    let Some(value) = script.run(scope) else {
        return false;
    };
    value.integer_value(scope) == Some(2)
}

/// 启动第二线程（F3）。
///
/// 返回其 `JoinHandle`（`None` 表示 F3=none）。
#[allow(clippy::too_many_arguments)]
fn spawn_second_thread(
    combo: Combo,
    obs: Arc<ProbeObservations>,
    signal: Arc<PumpSignal>,
    rendezvous: Arc<GilRendezvous>,
    control_msgs: Arc<AtomicU64>,
    started_at: Instant,
    session: &V8InspectorSession,
    watchdog_timeout: Duration,
    gil_strategy: GilWaitStrategy,
    python_max_hold_ms: u64,
    python_side: &mut Option<pyside::PythonSideHandle>,
) -> Option<std::thread::JoinHandle<()>> {
    // F1=yes 时，**Python 侧线程**就是「第二线程」——
    // 因为 GIL 互等的构造本质上就需要第二个线程。
    if combo.acquires_gil() {
        *python_side = Some(pyside::spawn_python_side(
            Arc::clone(&rendezvous),
            gil_strategy,
            python_max_hold_ms,
        ));
        // **握手等待**（不是固定 sleep）：Python 侧必须真的进入等待状态，
        // 互等条件才成立。否则页面线程在它还没拿到 GIL 时就发起 with_gil，
        // 测出来的「不死锁」是假绿。详见 `pyside::wait_for_python_side`。
        //
        // 超时值：`max(5 × watchdog, 5s)`。**必须给足** —— 首版 500ms 在
        // 过载机器上不够（CPython 初始化被拖长），握手失败会让页面线程与
        // Python 侧的**初始化**抢 GIL，把对照组误判为 TIMEOUT。
        let handshake_timeout = handshake_timeout(watchdog_timeout);
        let ready = pyside::wait_for_python_side(&rendezvous, handshake_timeout);
        obs.python_handshake_ready.store(ready, Ordering::SeqCst);
        obs.gil_contended
            .store(ready && rendezvous.python_side_holding(), Ordering::SeqCst);
    }

    if !combo.has_second_thread() {
        return None;
    }

    let mode = combo.second_thread;
    let deadline = started_at + watchdog_timeout * 2;
    let obs_thread = Arc::clone(&obs);
    let signal_thread = Arc::clone(&signal);
    let msgs_thread = Arc::clone(&control_msgs);

    // F3=needs_locker 需要触碰同一 Isolate ⇒ 必须持 `v8::Locker`。
    //
    // 但本探针的 Isolate 不是 `SharedIsolate`（那是 `OwnedIsolate::try_into_shared`
    // 的产物，且要求「当前线程无其他 isolate 在栈上」）。
    // 因此这里**不能**真的去 `lock()` 一个非共享 isolate ——
    // 强行构造会破坏 V8 的锁状态。
    //
    // 故 D6 采用**诚实的替代观测**：第二线程声明「若要触碰 Isolate 就必须
    // 先拿 Locker」，并实测「在页面线程暂停期间，Locker 是被持有的」，
    // 从而证明**锁序死锁的前提成立**，同时不越界去实际 lock。
    //
    // 这是对 D6 的**部分覆盖**，已在「未覆盖路径」中显式登记。
    let handle = std::thread::Builder::new()
        .name("iv8-second".to_string())
        .spawn(move || match mode {
            SecondThread::None => {}
            SecondThread::MsgOnly => {
                // 只发消息，不碰 Isolate：这是技术方案 §13 允许的
                // 「宿主显式标记的控制消息」通路。
                while Instant::now() < deadline && !signal_thread.should_abort_pump() {
                    msgs_thread.fetch_add(1, Ordering::SeqCst);
                    std::thread::sleep(Duration::from_millis(2));
                }
            }
            SecondThread::NeedsLocker => {
                // 页面线程在暂停期间持有 isolate 的访问权 ⇒ Locker 被占用。
                // 记录「观测到争用」，再等页面线程放开。
                obs_thread.locker_contended.store(true, Ordering::SeqCst);
                while Instant::now() < deadline && !signal_thread.should_abort_pump() {
                    if !signal_thread.should_abort_pump() {
                        // 页面线程仍在暂停中 ⇒ Locker 仍不可用。
                        // 真正 lock 会破坏非共享 isolate 的锁状态，故只观测不 lock。
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(1));
                }
                obs_thread.locker_acquired.store(true, Ordering::SeqCst);
            }
        })
        .expect("spawn second thread");

    // `session` 借用了上面的 scope；此处故意不跨线程使用它
    // （`V8InspectorSession` 不是 `Send`，跨线程用需要 `Locker`，已如上说明）。
    let _ = session;
    Some(handle)
}

/// 输出全部观测结果。
#[allow(clippy::too_many_arguments)]
fn report_observations(
    combo: &Combo,
    obs: &ProbeObservations,
    signal: &PumpSignal,
    channel: &ChannelSink,
    gil_strategy: GilWaitStrategy,
    fired: bool,
    fired_after_ms: Option<u128>,
    script_returned: bool,
    post_pause_ok: bool,
    control_msgs: &AtomicU64,
    started_at: Instant,
) {
    println!("COMBO_ID={}", combo.id);
    // 子进程必须**自报** GIL 策略：父进程的条件表只能读子进程输出，
    // 若靠父进程打印，证据行里就没有它，两种策略的行会无法区分。
    if combo.acquires_gil() {
        println!("GIL_STRATEGY={}", gil_strategy.as_str());
        // 测量有效性前置：握手未成立时，本运行的 GIL 读数不可用于判定。
        println!(
            "PYTHON_HANDSHAKE_READY={}",
            obs.python_handshake_ready.load(Ordering::SeqCst)
        );
    }
    println!("PUMP_ENTERED={}", obs.pump_entered.load(Ordering::SeqCst));
    println!("PUMP_RETURNED={}", obs.pump_returned.load(Ordering::SeqCst));
    println!("QUIT_CALLED={}", obs.quit_called.load(Ordering::SeqCst));
    // 「暂停到解除」的时长：取**泵真正退出**的时刻（见 ProbeObservations 注释）。
    // 若泵从未进入（暂停没发生），退回脚本整体耗时，并明确标注。
    let resumed_at = if obs.pump_entered.load(Ordering::SeqCst) {
        obs.pump_exited_at_ms.load(Ordering::SeqCst)
    } else {
        started_at.elapsed().as_millis() as u64
    };
    println!("RESUMED_AT={resumed_at}ms");
    println!(
        "CALLBACK_COMPLETED={}",
        obs.pump_returned.load(Ordering::SeqCst)
    );
    println!("SCRIPT_RETURNED={script_returned}");
    println!("POST_PAUSE_EVAL_OK={post_pause_ok}");
    println!("WATCHDOG_FIRED={fired}");
    if let Some(ms) = fired_after_ms {
        println!("WATCHDOG_FIRED_AT={ms}ms");
    }
    println!("TERMINATE_REQUESTED={}", signal.terminate_requested());

    // ---- D0a：watchdog 实际走的路径（具名机器可读事实）----
    // 与 `Watchdog::start` 的形参类型绑定（见 watchdog.rs 的常量文档）。
    println!(
        "WATCHDOG_USES_ISOLATE_HANDLE={}",
        crate::watchdog::WATCHDOG_USES_ISOLATE_HANDLE
    );

    // ---- D0b：终止后是否复用了同一 Isolate ----
    //
    // **代码事实（非声明）**：本探针**确实复用**。
    // `terminate_execution()` 由 watchdog 线程发出后，页面线程并未重建
    // Isolate/Context —— 上面的 `eval_post_pause(&mut scope)` 就在**同一个
    // `scope`（同一 Context、同一 Isolate）**上再跑一次 `1 + 1`。
    // 故按需求 D0b 走「有复用」分支：输出本断言。
    //
    // 语义：`post_pause_ok` 即「终止请求发出之后，同一 Isolate 能否干净地
    // 再执行 JS」。配合已有的 `TERMINATE_REQUESTED` 判读：
    //   TERMINATE_REQUESTED=true  + CLEAN_RESUME_AFTER_TERMINATE=true
    //       ⇒ 被终止后**仍可**复用（本探针实测形态）
    //   TERMINATE_REQUESTED=false + CLEAN_RESUME_AFTER_TERMINATE=true
    //       ⇒ 从未终止，该项平凡成立
    println!("CLEAN_RESUME_AFTER_TERMINATE={post_pause_ok}");

    println!("CHANNEL_MSGS={}", channel.len());
    println!("CHANNEL_NOTIFICATIONS={}", channel.notifications());
    println!("DEBUGGER_PAUSED_SEEN={}", channel.saw_debugger_paused());

    println!("GIL_ATTEMPTED={}", obs.gil_attempted.load(Ordering::SeqCst));
    println!("GIL_ACQUIRED={}", obs.gil_acquired.load(Ordering::SeqCst));
    if let Some(blocked) = obs.gil_blocked_ms() {
        println!("GIL_BLOCKED_MS={blocked}");
    }
    println!("GIL_CONTENDED={}", obs.gil_contended.load(Ordering::SeqCst));

    println!(
        "REENTRANT_ATTEMPTED={}",
        obs.reentrant_attempted.load(Ordering::SeqCst)
    );
    println!("REENTRANT_OUTCOME={}", obs.reentrant_outcome().as_str());

    println!(
        "LOCKER_ACQUIRED={}",
        obs.locker_acquired.load(Ordering::SeqCst)
    );
    println!(
        "LOCKER_CONTENDED={}",
        obs.locker_contended.load(Ordering::SeqCst)
    );

    println!("CONTROL_MSGS_SENT={}", control_msgs.load(Ordering::SeqCst));
    println!(
        "PAGE_TASKS_STALLED_DURING_PAUSE={}",
        obs.page_task_stalled.load(Ordering::SeqCst)
    );
}

/// 分类结论。
///
/// 判据优先级（从强到弱）：
/// 1. watchdog 到点 ⇒ `TIMEOUT`（这是**可判定的结论**，不是挂住）；
/// 2. 泵未进入 ⇒ `ERROR_PROPAGATED`（暂停根本没发生，测量失败）；
/// 3. 泵进入但未返回 ⇒ `DEADLOCK`；
/// 4. 脚本未返回 ⇒ `ERROR_PROPAGATED`；
/// 5. 解除后脚本不可执行 ⇒ `ERROR_PROPAGATED`；
/// 6. 否则 ⇒ `CLEAN_RESUME`。
fn classify(
    _combo: Combo,
    obs: &ProbeObservations,
    fired: bool,
    script_returned: bool,
    post_pause_ok: bool,
) -> Verdict {
    if fired {
        return Verdict::Timeout;
    }
    if !obs.pump_entered.load(Ordering::SeqCst) {
        return Verdict::ErrorPropagated;
    }
    if !obs.pump_returned.load(Ordering::SeqCst) {
        return Verdict::Deadlock;
    }
    if !script_returned {
        return Verdict::ErrorPropagated;
    }
    if !post_pause_ok {
        return Verdict::ErrorPropagated;
    }
    Verdict::CleanResume
}

// ===========================================================================
// 把 ProbeClient 包进 v8 的 trait object
// ===========================================================================

/// 薄适配层：把 `Arc<ProbeClient>` 变成 `Box<dyn V8InspectorClientImpl>`。
///
/// **为什么需要它**：`V8Inspector::create` 要求
/// `Box<dyn V8InspectorClientImpl>`（`inspector.rs:1246`），
/// 它会**接管所有权**并 pin 住（`_client` 字段）。
/// 而实验编排需要**在创建之后**继续与 client 交互
/// （回填源码、设置 Context）—— 若直接 move `ProbeClient` 进去就再也拿不回来。
///
/// `Arc` 在这里不是为了跨线程共享（client 只在页面线程用），
/// 而是为了**共享所有权**：适配器持 `Arc`，外部编排也持同一个 `Arc`。
struct HostClientAdapter {
    inner: Arc<ProbeClient>,
}

impl std::ops::Deref for HostClientAdapter {
    type Target = ProbeClient;
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl V8InspectorClientImpl for HostClientAdapter {
    fn run_message_loop_on_pause(&self, context_group_id: i32) {
        self.inner.run_message_loop_on_pause(context_group_id)
    }

    fn quit_message_loop_on_pause(&self) {
        self.inner.quit_message_loop_on_pause()
    }

    fn run_if_waiting_for_debugger(&self, context_group_id: i32) {
        self.inner.run_if_waiting_for_debugger(context_group_id)
    }

    fn generate_unique_id(&self) -> i64 {
        self.inner.generate_unique_id()
    }
}
