//! T2.2 探针：逻辑 Date 与多 Profile 的时钟 / 时区隔离。
//!
//! # 这个探针证伪什么
//!
//! 证伪「在同一个进程里，给两个 `Isolate` 设两个不同的时区，它们就能各自看到自己的时区」。
//! 官方文档已经说了时区是进程级（`v8::icu::set_default_time_zone` 的注释原文：
//! 「This mutates process wide state and is not synchronized with isolates running on
//! other threads」），本探针把这个事实**实测复核**一遍，并给出可判定的二值结论。
//!
//! # 三条必须记住的坑（都写进了 ADR-0010）
//!
//! 1. **禁止**用 `v8::Date::new`（宿主喂一个 f64 造 `Date` 对象）来测「时钟隔离」。
//!    它的实现是 `v8__Date__New(context, value)` —— 宿主喂什么就是什么，
//!    **根本不读 V8 的时钟**。用它测出来「隔离成功」是**假绿**。
//!    本探针所有时间读数都来自**执行 JS 源码**（`new Date(T_REF).getTimezoneOffset()`、
//!    `Date.now()`），让 V8 自己去查它内部的时区缓存与时钟。
//!    机器可验证：`ASSERT_PASS(h1)` 会扫描本文件源码，确认没有出现那个调用
//!    （needle 在运行时拼装，避免这个检查自己匹配到自己）。
//!
//! 2. **`date_time_configuration_change_notification` 必须传 `TimeZoneDetection::Skip`**。
//!    传 `Redetect` 会让 V8 **重新探测并覆盖**宿主刚设的时区，
//!    于是测出来「隔离失败」其实是**通知参数用错**。
//!
//! 3. **`set_default_time_zone` 的 `bool` 返回值必须检查**。
//!    对无法识别的 id（例如 ICU 自己的 `Etc/Unknown`）它返回 `false`
//!    且**不改变当前值**。静默忽略这个返回值 ⇒ 后面所有偏移读数都不可信。
//!
//! # 固定参考时刻（消除「现在」引入的不确定性）
//!
//! `T_REF = 1_700_000_000_000` ms = 2023-11-14T22:13:20Z。
//! 该时刻两个时区的 UTC 偏移**必然不同**，且**都不在 DST 切换边界上**：
//!
//! | 时区 | 该时刻偏移 | 为什么选它 |
//! |---|---|---|
//! | `Asia/Tokyo` | UTC+09:00（偏移 = **-540**） | 全年无夏令时，偏移稳定 |
//! | `America/Bogota` | UTC-05:00（偏移 = **+300**） | 全年恒定 UTC-05:00，**无 DST** |
//!
//! 符号约定（容易搞反）：`getTimezoneOffset()` 返回的是 **UTC − local**，
//! 所以 UTC+09:00 的 Tokyo 是 **-540**，UTC-05:00 的 Bogota 是 **+300**。
//!
//! # 实验结构
//!
//! - EXP0 基线自证：同一时区、两个 Isolate 读数必须相等（否则整份结论作废）。
//! - EXP1 时区隔离：A 保持 Tokyo、B 切到 Bogota，看两个 Isolate 的读数。
//! - EXP1c **缓存 vs 隔离**：证明「两个读数不同」来自**各自的缓存**，而不是各持一份时区。
//! - EXP2 process-global 直接证据：**只改进程全局设置**，B 的读数也跟着变。
//! - EXP3 时间值本身：两个 Isolate 的 `Date.now()` 与宿主墙钟的差。
//! - EXP4 替代路线：JS 侧 `Date` 兼容层（最小可行性）。
//! - EXP5 设置失败分支：`Etc/Unknown` 必须被拒。
//!
//! # 实测到的 API 约束：两个 Isolate 不能同时「已进入」
//!
//! 这是设计文档**没有预见**、但实测撞上的一条硬约束（已落 ADR-0010）：
//!
//! - `v8::Isolate::new` 返回的 `OwnedIsolate` 在**构造时就被 `enter()`**
//!   （`isolate.rs:2768` `OwnedIsolate::new` 里直接 `isolate.enter()`），
//!   且 `Drop` 里断言「必须按构造的逆序析构」（`isolate.rs:2875`）。
//! - 因此在同一线程上同时持有**两个** `OwnedIsolate`，第二个会盖在第一个之上，
//!   V8 直接 FATAL：
//!   `Fatal error in v8::HandleScope::CreateHandle() / Cannot create a handle without a HandleScope`。
//!   已实测确认：单 Isolate 正常，双 Isolate 必崩（与时区设置、refresh 调用与否**无关**）。
//! - crate 给的正规多 Isolate 通道是 `SharedIsolate` + `Locker`：
//!   `OwnedIsolate::try_into_shared()` 把 Isolate **退出**（变成 unentered），
//!   之后每次访问都用 `SharedIsolate::lock()` 拿锁并重新进入。
//!   本探针全部走这条通道，因此只有 `try_into_shared` 一处 `unsafe`，逐块带 `SAFETY:`。
//!
//! # 不改进程 TZ 环境变量
//!
//! 本探针**全程不设置进程 `TZ` 环境变量**（技术方案 §6 明确「避免并发修改进程 TZ」）。
//! 唯一的时区写入口是 `v8::icu::set_default_time_zone`，它改的是 ICU 的进程级默认值。

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

/// 固定参考时刻（epoch 毫秒）= 2023-11-14T22:13:20Z。见文件头「固定参考时刻」。
const T_REF_MS: f64 = 1_700_000_000_000.0;

/// EXP1 里 A 的时区：全年 UTC+09:00，`getTimezoneOffset()` 恒为 -540。
const TZ_A: &str = "Asia/Tokyo";
/// EXP1 里 B 的时区：全年 UTC-05:00 无 DST，`getTimezoneOffset()` 恒为 +300。
const TZ_B: &str = "America/Bogota";
/// `TZ_A` 在 `T_REF_MS` 时刻的期望偏移（UTC − local，单位分钟）。
const OFF_A_EXPECT: f64 = -540.0;
/// `TZ_B` 在 `T_REF_MS` 时刻的期望偏移（UTC − local，单位分钟）。
const OFF_B_EXPECT: f64 = 300.0;

/// EXP4 用的两个**固定逻辑时刻**（epoch 毫秒）。刻意不用「现在」。
const LOGICAL_T0: f64 = 1_700_000_000_000.0;
/// EXP4 的第二个逻辑时刻，与 `LOGICAL_T0` 差约 3.16 年，足以证明时钟真被宿主驱动了。
const LOGICAL_T1: f64 = 1_800_000_000_000.0;

/// EXP4 里宿主 → JS 的喂值通道名（写进 JS 全局）。
const LOGICAL_NOW_KEY: &str = "__hostLogicalNow";

/// EXP3 判定「`Date.now()` 与宿主墙钟同步」的宽松上界（毫秒）。
/// 单机单线程下真实差值在个位数毫秒；给 10 秒余量以吸收调度抖动。
const CLOCK_SYNC_TOLERANCE_MS: f64 = 10_000.0;

/// 一个可反复读时区/时间的 Isolate 封装。
///
/// 内部用 `Arc<SharedIsolate>` 而不是 `OwnedIsolate`，原因见文件头
/// 「实测到的 API 约束」：两个同时 *entered* 的 `OwnedIsolate` 会让 V8 直接 FATAL。
/// 走 `SharedIsolate::lock()` 之后，任意时刻只有一个 Isolate 处于 entered 状态。
struct TzIsolate {
    shared: Arc<v8::SharedIsolate>,
}

impl TzIsolate {
    /// 新建一个 Isolate，并立刻转成 `SharedIsolate`（即 **unentered** 状态）。
    fn new(label: &str) -> Result<Self, String> {
        let owned = v8::Isolate::new(v8::CreateParams::default());
        // 长篇论证：`owned` 是本 Isolate 的唯一所有者，且刚创建、从未被使用——
        // 无活着的 Weak / 无 Globals / 无 cppgc heap / 非 snapshot creator，
        // 且此刻它是本线程唯一 entered 的 isolate。这逐条对应 `try_into_shared`
        // 文档的 # Errors 列表。
        //
        // 但守卫脚本的±2 行窗口只看得到紧邻的标记，所以下面必须再留一行 SAFETY:。
        // SAFETY: 上述四项前提逐条满足，转换必然成功；转换后本 isolate 一律经
        // `SharedIsolate::lock()` 访问，锁由 `Locker` 守卫，不再有裸 `&mut Isolate` 外泄。
        let shared = unsafe { owned.try_into_shared() }
            .map_err(|e| format!("{label}: try_into_shared 失败: {e}"))?;
        Ok(TzIsolate {
            shared: Arc::new(shared),
        })
    }

    /// 在一次锁持有期内，把 `f` 施加于一个新建的 HandleScope + Context 上。
    ///
    /// `Locker` 在返回时析构：解锁并退出该 Isolate ⇒ 同一时刻只有一个 Isolate entered。
    fn with_scope<R>(&self, f: impl FnOnce(&mut v8::PinScope<'_, '_>) -> R) -> R {
        let mut locker = self.shared.lock();
        v8::scope!(let handle_scope, &mut *locker);
        let context = v8::Context::new(handle_scope, Default::default());
        let mut scope = v8::ContextScope::new(handle_scope, context);
        f(&mut scope)
    }

    /// 刷新该 Isolate 的日期/时区缓存，**且不重新探测宿主时区**。
    ///
    /// 必须传 `Skip`：传 `Redetect` 会让 V8 重新探测并**覆盖**宿主刚通过
    /// `set_default_time_zone` 设好的时区。见文件头「坑 2」。
    fn refresh(&self) {
        self.refresh_with(v8::TimeZoneDetection::Skip);
    }

    /// 同 [`TzIsolate::refresh`]，但允许指定探测策略 —— **仅供EXP6 自证使用**。
    ///
    /// 存在的唯一理由：把「传 `Redetect` 会覆盖刚设的时区」从口头断言变成
    /// 机器可复现的证据（见 EXP6）。生产路径一律走 [`TzIsolate::refresh`]（即 `Skip`）。
    fn refresh_with(&self, detection: v8::TimeZoneDetection) {
        let mut locker = self.shared.lock();
        locker.date_time_configuration_change_notification(detection);
    }

    /// 在该 Isolate 的一个新 Context 里执行一段 JS 源码，把结果读成 `f64`。
    ///
    /// **所有时间/时区读数都必须走这里**（执行 JS 源码），不得用「宿主喂值」的
    /// `Date` 构造 API —— 见文件头「坑 1」。
    fn eval_number(&self, src: &str) -> Result<f64, String> {
        self.with_scope(|scope| {
            let code = v8::String::new(scope, src).ok_or("EVAL: 无法构造源码字符串")?;
            let script = v8::Script::compile(scope, code, None)
                .ok_or_else(|| format!("EVAL: 编译失败（JS 语法错误）: {src}"))?;
            let value = script
                .run(scope)
                .ok_or_else(|| format!("EVAL: 执行返回 None（JS 抛异常或被终止）: {src}"))?;
            value
                .number_value(scope)
                .ok_or_else(|| format!("EVAL: 结果不是数值: {src}"))
        })
    }
}

/// 读时区偏移的 JS 表达式（宿主必须用它，不能自己算偏移）。
///
/// 关键点：`new Date(T_REF)` 里的 `T_REF` 是**固定参考时刻**而不是 `"now"`，
/// 这样断言结果只取决于「V8 查到的时区」，不取决于探针跑在什么时刻。
fn tz_offset_js(t_ref_ms: f64) -> String {
    format!("(function () {{ var d = new Date({t_ref_ms}); return d.getTimezoneOffset(); }})()")
}

/// 宿主当前的 epoch 毫秒（用于 EXP3 与 `Date.now()` 对比）。
fn host_now_ms() -> f64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(d) => d.as_millis() as f64,
        Err(_) => 0.0,
    }
}

/// 改进程全局时区，并**强制检查** `bool` 返回值。
///
/// `false` ⇒ 该 id 不可识别，当前值未被改变。这不是「可以继续跑」的软警告，
/// 而是**测量前提被破坏**：后面所有偏移读数都不可信。见文件头「坑 3」。
fn set_global_tz(tz: &str) -> Result<(), String> {
    if v8::icu::set_default_time_zone(tz) {
        Ok(())
    } else {
        Err(format!("TZ_SET_REJECTED tz={tz}"))
    }
}

/// EXP4 的 JS 侧 `Date` 兼容层（**最小可行**实现，只接管逻辑时钟）。
///
/// 覆盖：`new Date()`（零参数）与 `Date.now()`。
/// 不覆盖：描述符、参数解析差分、`toLocaleString`、`Intl.DateTimeFormat`
/// —— 技术方案 §6 说那些是 G1/G2 的差分任务，不在本 PoC 范围内。
const DATE_SHIM_JS: &str = r#"
(function () {
  var RealDate = Date;
  function LogicalDate() {
    var args = Array.prototype.slice.call(arguments);
    // 零参数构造 =「现在」⇒ 用宿主喂进来的逻辑时刻
    if (args.length === 0) { args = [globalThis.__hostLogicalNow]; }
    return new (Function.prototype.bind.apply(RealDate, [null].concat(args)))();
  }
  LogicalDate.prototype = RealDate.prototype;
  LogicalDate.now = function () { return globalThis.__hostLogicalNow; };
  LogicalDate.parse = RealDate.parse;
  LogicalDate.UTC = RealDate.UTC;
  globalThis.Date = LogicalDate;
  return true;
})()
"#;

/// EXP4 的读数表达式：一次读回 `Date.now()` 与 `new Date().getTime()`。
const SHIM_READ_JS: &str = "String(Date.now()) + ',' + String(new Date().getTime())";

/// EXP4 的完整实验。
///
/// **必须在同一个 Context 内跑完** —— 因为 `Date` 兼容层是「替换全局 `Date`」，
/// 而 [`TzIsolate::with_scope`] 每次都新建 Context，跨调用装不上。
/// 所以这里在一次 scope 内手工走完整个时序：喂 t0 → 装兼容层 → 读 → 喂 t1 → 读。
///
/// 返回 `(喂 t0 的读数, 喂 t1 的读数)`。任一步失败则该侧为 `Err`。
fn date_shim_experiment(iso: &TzIsolate) -> (Result<String, String>, Result<String, String>) {
    /// 在已给定的 scope 内，把 `logical_ms` 写进 JS 全局 [`LOGICAL_NOW_KEY`]。
    fn feed(scope: &mut v8::PinScope<'_, '_>, logical_ms: f64) -> Result<(), String> {
        let global = scope.get_current_context().global(scope);
        let key = v8::String::new(scope, LOGICAL_NOW_KEY).ok_or("SHIM: 无法构造属性名字符串")?;
        let val = v8::Number::new(scope, logical_ms);
        global
            .set(scope, key.into(), val.into())
            .ok_or_else(|| format!("SHIM: 写入全局属性 {LOGICAL_NOW_KEY} 失败"))?;
        Ok(())
    }

    iso.with_scope(|scope| {
        // 1) 宿主把逻辑时刻 t0 写进 JS 全局（喂 t0）。
        if let Err(e) = feed(scope, LOGICAL_T0) {
            return (Err(e), Err("SHIM: 未执行到 t1".to_string()));
        }

        // 2) 在 JS 侧替换全局 Date。
        if let Err(e) = install_shim(scope) {
            return (Err(e), Err("SHIM: 未执行到 t1".to_string()));
        }

        // 3) 喂 t0 的读数；4) 喂 t1，再读一次。
        let read0 = eval_pair(scope);
        if let Err(e) = feed(scope, LOGICAL_T1) {
            return (read0, Err(e));
        }
        let read1 = eval_pair(scope);
        (read0, read1)
    })
}

/// 在已给定的 scope 内编译并执行 [`DATE_SHIM_JS`]，把 JS 全局 `Date` 换掉。
fn install_shim(scope: &mut v8::PinScope<'_, '_>) -> Result<(), String> {
    let code = v8::String::new(scope, DATE_SHIM_JS).ok_or("SHIM: 无法构造兼容层源码")?;
    let script =
        v8::Script::compile(scope, code, None).ok_or("SHIM: 兼容层编译失败（JS 语法错误）")?;
    script
        .run(scope)
        .ok_or_else(|| "SHIM: 兼容层执行返回 None".to_string())?;
    Ok(())
}

/// 在已给定的 scope 内跑 [`SHIM_READ_JS`] 并读回字符串。
fn eval_pair(scope: &mut v8::PinScope<'_, '_>) -> Result<String, String> {
    let code = v8::String::new(scope, SHIM_READ_JS).ok_or("SHIM: 无法构造读数源码")?;
    let script = v8::Script::compile(scope, code, None).ok_or("SHIM: 读数脚本编译失败")?;
    let value = script.run(scope).ok_or("SHIM: 读数脚本执行返回 None")?;
    let text = value.to_string(scope).ok_or("SHIM: 读数结果无法转字符串")?;
    Ok(text.to_rust_string_lossy(scope))
}

fn main() {
    let mut ok = true;
    let mut check = |cond: bool, tag: &str, pass: &str, fail: &str| {
        if cond {
            println!("ASSERT_PASS({tag}): {pass}");
        } else {
            println!("ASSERT_FAIL({tag}): {fail}");
            ok = false;
        }
    };

    // ---------------- V8 初始化 ----------------
    let platform = v8::new_default_platform(0, false).make_shared();
    v8::V8::initialize_platform(platform);
    v8::V8::initialize();
    println!("V8_VERSION={}", v8::V8::get_version());
    println!("T_REF_MS={T_REF_MS}");
    println!("TZ_A={TZ_A} TZ_B={TZ_B}");

    // =========================================================================
    // EXP0 基线自证：同一时区，两个 Isolate 读相同偏移
    // =========================================================================
    println!();
    println!("=== EXP0 基线自证：同一时区两个 Isolate ===");
    if let Err(e) = set_global_tz(TZ_A) {
        println!("{e}");
        println!("TZ_SET_REJECTED —— 本机 ICU 时区数据不可用，后续全部断言无意义");
        println!("T2_2_PROBE_FAIL");
        return;
    }
    println!("TZ_DEFAULT_GLOBAL={}", v8::icu::get_default_time_zone());

    let iso_a = match TzIsolate::new("isolate=A") {
        Ok(v) => v,
        Err(e) => {
            println!("{e}");
            println!("BASELINE_VALID=false");
            println!("T2_2_PROBE_FAIL");
            return;
        }
    };
    let iso_b = match TzIsolate::new("isolate=B") {
        Ok(v) => v,
        Err(e) => {
            println!("{e}");
            println!("BASELINE_VALID=false");
            println!("T2_2_PROBE_FAIL");
            return;
        }
    };
    println!("TWO_ISOLATES_ALIVE=true （经 SharedIsolate 通道，两个 Isolate 同时存活）");

    iso_a.refresh();
    iso_b.refresh();

    let off_a0 = match iso_a.eval_number(&tz_offset_js(T_REF_MS)) {
        Ok(v) => v,
        Err(e) => {
            println!("{e}");
            println!("BASELINE_VALID=false");
            println!("T2_2_PROBE_FAIL");
            return;
        }
    };
    let off_b0 = match iso_b.eval_number(&tz_offset_js(T_REF_MS)) {
        Ok(v) => v,
        Err(e) => {
            println!("{e}");
            println!("BASELINE_VALID=false");
            println!("T2_2_PROBE_FAIL");
            return;
        }
    };
    println!("  isolate=A tz={TZ_A} offset={off_a0}");
    println!("  isolate=B tz={TZ_A} offset={off_b0}");
    // 基线不仅要「相等」，还必须等于该时区的**期望偏移**——
    // 否则「两个都返回 0」这种坏掉的测量也会让基线看起来成立。
    let baseline_valid = off_a0 == off_b0 && off_a0 == OFF_A_EXPECT;
    println!("BASELINE_VALID={baseline_valid}");

    // =========================================================================
    // EXP1 时区隔离：A 保持 Tokyo，B 切到 Bogota
    // =========================================================================
    println!();
    println!("=== EXP1 时区隔离 ===");
    println!("TZ_ISO_A={TZ_A}");
    println!("TZ_ISO_B={TZ_B}");

    if let Err(e) = set_global_tz(TZ_B) {
        println!("{e}");
        println!("T2_2_PROBE_FAIL");
        return;
    }
    // 只通知 B。A **不**通知 —— 这是观察「per-isolate 缓存」的窗口。
    iso_b.refresh();

    let off_a1 = iso_a
        .eval_number(&tz_offset_js(T_REF_MS))
        .unwrap_or(f64::NAN);
    let off_b1 = iso_b
        .eval_number(&tz_offset_js(T_REF_MS))
        .unwrap_or(f64::NAN);
    println!("TZ_READ_A={off_a1}");
    println!("TZ_READ_B={off_b1}");
    println!("TZ_DEFAULT_GLOBAL={}", v8::icu::get_default_time_zone());

    let tz_per_profile_isolated = off_a1 == OFF_A_EXPECT && off_b1 == OFF_B_EXPECT;
    println!("TZ_PER_PROFILE_ISOLATED={tz_per_profile_isolated}");

    // =========================================================================
    // EXP1c 「两个读数不同」到底是隔离还是缓存？
    // =========================================================================
    // 上一步只刷新了 B。现在**只刷新 A**（不给它做任何 per-isolate 配置），
    // 若 A 的读数也跟着变成 Bogota ⇒ A 并不「各自持有一份时区」，
    // 它只是**缓存**了上一次被告知的全局值。
    println!();
    println!("=== EXP1c 隔离 vs 缓存：只刷新 A，不做任何 per-isolate 配置 ===");
    iso_a.refresh();
    let off_a2 = iso_a
        .eval_number(&tz_offset_js(T_REF_MS))
        .unwrap_or(f64::NAN);
    println!("TZ_CACHE_PROBE isolate=A after_refresh offset={off_a2}");
    let cache_not_isolation_proven = off_a2 == OFF_B_EXPECT;
    println!("TZ_CACHE_NOT_ISOLATION={cache_not_isolation_proven}");
    if cache_not_isolation_proven {
        println!("  ⇒ A 没有任何「自己的时区」：一被通知就跟随了进程全局值");
        println!("  ⇒ EXP1 里「两读数不同」来自 per-isolate 缓存，不是隔离");
    }

    // =========================================================================
    // EXP2 process-global 直接证据：只改进程全局设置，B 的读数跟着变
    // =========================================================================
    println!();
    println!("=== EXP2 process-global 直接证据 ===");
    // 「改进程全局时区 → 只通知某一个 Isolate（不给它做任何 per-isolate 配置）
    //→ 看它的读数是否跟着变」。
    //
    // **踩过的坑（已修）**：第一版把全局设成 A/B **当前已经缓存着的**那个时区，
    // 于是 before == after，这条断言变成恒真的「反向复核」。
    // 现在两个方向用**相反的目标时区**，且每个目标都**必然不同于该 Isolate
    // 当前缓存的值**，并额外断言 `after` 等于目标时区的期望偏移。
    //
    // 进入 EXP2 时的状态：A缓存 Bogota(300)，B 缓存 Bogota(300)，全局 = Bogota。
    //
    // 方向一：动B。全局改回 Tokyo，只通知 B。
    let leak_b_before = off_b1;
    if let Err(e) = set_global_tz(TZ_A) {
        println!("{e}");
        println!("T2_2_PROBE_FAIL");
        return;
    }
    iso_b.refresh();
    let leak_b_after = iso_b
        .eval_number(&tz_offset_js(T_REF_MS))
        .unwrap_or(f64::NAN);
    println!("GLOBAL_LEAK_OBSERVED isolate=B tz_before={leak_b_before} tz_after={leak_b_after}");
    let global_leak_observed = leak_b_before != leak_b_after && leak_b_after == OFF_A_EXPECT;

    // 方向二：动 A。A 此刻缓存的是 Bogota(300)，目标时区必须**不同于**它，
    // 否则 before == after，断言恒真（这正是第一版的 bug）。
    // 这里用Tokyo：全局此刻已是 Tokyo，refresh A 后A 应变成 -540。
    let leak_a_before = off_a2;
    if let Err(e) = set_global_tz(TZ_A) {
        println!("{e}");
        println!("T2_2_PROBE_FAIL");
        return;
    }
    iso_a.refresh();
    let leak_a_after = iso_a
        .eval_number(&tz_offset_js(T_REF_MS))
        .unwrap_or(f64::NAN);
    println!("GLOBAL_LEAK_OBSERVED isolate=A tz_before={leak_a_before} tz_after={leak_a_after}");
    let global_leak_reverse_observed =
        leak_a_before != leak_a_after && leak_a_after == OFF_A_EXPECT;

    // =========================================================================
    // EXP3 时间值本身：Date.now() 与宿主墙钟
    // =========================================================================
    println!();
    println!("=== EXP3 时间值 ===");
    // 说明：这里用 SystemTime（epoch 毫秒）而不是 Instant ——
    // Instant 是单调时钟、**没有 epoch 原点**，无法与 Date.now() 的 epoch 毫秒
    // 直接相减。Instant 只适合测「读数耗时」，不能当参考时刻。
    let host_before = host_now_ms();
    let js_now_a = iso_a.eval_number("Date.now()").unwrap_or(f64::NAN);
    let js_now_b = iso_b.eval_number("Date.now()").unwrap_or(f64::NAN);
    let host_after = host_now_ms();

    let delta_a = (js_now_a - host_before)
        .abs()
        .min((js_now_a - host_after).abs());
    let delta_b = (js_now_b - host_before)
        .abs()
        .min((js_now_b - host_after).abs());
    let delta_ab = (js_now_a - js_now_b).abs();
    println!("HOST_EPOCH_MS_BEFORE={host_before}");
    println!("HOST_EPOCH_MS_AFTER={host_after}");
    println!("JS_NOW_A={js_now_a}");
    println!("JS_NOW_B={js_now_b}");
    println!("DATE_NOW_DELTA_A_MS={delta_a}");
    println!("DATE_NOW_DELTA_B_MS={delta_b}");
    println!("DATE_NOW_DELTA_AB_MS={delta_ab}");

    // 「now 是否可按 Isolate 隔离」= 宿主能否让两个 Isolate 的 now 读出不同的值。
    // v8 crate 152.2.0 没有任何时钟注入入口（src/date.rs 全文只有构造与 value_of，
    // 无 now / set_now / Clock / mock / fake_time），所以两个 Isolate 只能一起读同一个进程墙钟。
    // 判据：两个 Isolate 的读数都贴着宿主墙钟，且彼此几乎相等 ⇒ 不可隔离。
    let clock_evidence_consistent = delta_a < CLOCK_SYNC_TOLERANCE_MS
        && delta_b < CLOCK_SYNC_TOLERANCE_MS
        && delta_ab < CLOCK_SYNC_TOLERANCE_MS;
    let date_now_isolated = !clock_evidence_consistent;
    println!("CLOCK_INJECTION_API_EXPOSED=none (v8 crate 152.2.0 无时钟注入入口)");
    println!("DATE_NOW_ISOLATED={date_now_isolated}");
    if !date_now_isolated {
        println!("  证据：两个 Isolate 的 now 都等于同一个进程墙钟，且彼此几乎相等");
        println!("  ⇒ 时间值同样是 process-global，不可按 Isolate 隔离");
    }

    // =========================================================================
    // EXP6 自证：把两条「口头警告」变成机器可复现的证据
    // =========================================================================
    // 背景：坑 2（必须传 Skip）与「缓存是懒的」这两条此前只是**口头断言**，
    // 谁也没实测过。若它们不成立，整个探针的时序就建立在一个未验证的前提上。
    // 这里用两个受控对照把它们跑成证据。
    println!();
    println!("=== EXP6 自证：Skip vs Redetect、缓存懒加载 ===");
    let iso_v = match TzIsolate::new("isolate=verify") {
        Ok(v) => v,
        Err(e) => {
            println!("{e}");
            println!("VERIFY_SKIP_VS_REDETECT=false");
            println!("T2_2_PROBE_FAIL");
            return;
        }
    };

    // 对照组1：设 Tokyo → refresh(Skip) → 读。正确路径，应当读到 -540。
    if let Err(e) = set_global_tz(TZ_A) {
        println!("{e}");
        println!("T2_2_PROBE_FAIL");
        return;
    }
    iso_v.refresh();
    let v_skip = iso_v
        .eval_number(&tz_offset_js(T_REF_MS))
        .unwrap_or(f64::NAN);
    println!("VERIFY_SKIP_OFFSET={v_skip}");

    // 对照组 2：设 Tokyo → refresh(**Redetect**) → 读。
    // 若 Redetect 真的会「重新探测并覆盖」，这里应读回**宿主系统时区**的偏移，
    // 而**不是** Tokyo 的 -540 —— 这就是坑 2 所说的假阴性来源。
    if let Err(e) = set_global_tz(TZ_A) {
        println!("{e}");
        println!("T2_2_PROBE_FAIL");
        return;
    }
    iso_v.refresh_with(v8::TimeZoneDetection::Redetect);
    let v_redetect = iso_v
        .eval_number(&tz_offset_js(T_REF_MS))
        .unwrap_or(f64::NAN);
    let tz_global_now = v8::icu::get_default_time_zone();
    println!("VERIFY_REDETECT_OFFSET={v_redetect}");
    println!("VERIFY_TZ_GLOBAL_AFTER_REDETECT={tz_global_now}");

    // 判据：Redetect 路径要么读不到 Tokyo 的 -540，要么把 ICU 全局值也改掉了。
    // 两者都说明「传 Redetect 会破坏刚设好的时区」，即坑 2 是真坑而非传闻。
    let redetect_breaks_setup = v_redetect != OFF_A_EXPECT || tz_global_now != TZ_A;
    println!("VERIFY_REDETECT_BREAKS_SETUP={redetect_breaks_setup}");
    if redetect_breaks_setup {
        println!("  ⇒ 实测确认：传 Redetect 会让刚设的时区失效（读到别的偏移或全局值被改）");
        println!("  ⇒ 本探针全部 refresh 一律传 Skip，这是必需项而非风格偏好");
    }

    // 对照组 3：缓存陈旧（stale）的真实机制。
    //
    // ⚠️ 先重新设一次全局：对照组 2 的 Redetect 把 ICU 全局值**改回了**
    // Asia/Shanghai（见 VERIFY_TZ_GLOBAL_AFTER_REDETECT）。不重设就测，
    // 读数会被「全局本来就是 Shanghai」污染 —— 那是**混淆变量（confound）**。
    //
    // 第一版我曾假设「新建 Isolate 首次求值会读到宿主时区（故必须预先 notify）」——
    // **实测证伪**：全新 Isolate 的首次求值直接读到当前全局值（Tokyo -540），
    // 因为缓存是**在首次求值时**才从全局填充的（这才是「懒」的准确含义）。
    //
    // 真正需要 notify 的是**已经求值过、缓存已填充**的 Isolate：
    // 此后改全局，它仍读陈旧值，直到收到通知。这才是「必须通知」的适用场景。
    if let Err(e) = set_global_tz(TZ_A) {
        println!("{e}");
        println!("T2_2_PROBE_FAIL");
        return;
    }
    println!(
        "VERIFY_TZ_GLOBAL_BEFORE_STALE={}",
        v8::icu::get_default_time_zone()
    );
    let iso_lazy = match TzIsolate::new("isolate=lazy") {
        Ok(v) => v,
        Err(e) => {
            println!("{e}");
            println!("VERIFY_STALE_CACHE=false");
            println!("T2_2_PROBE_FAIL");
            return;
        }
    };
    // (a) 全新 Isolate 的首次求值：此刻全局已是 Tokyo。
    let v_first = iso_lazy
        .eval_number(&tz_offset_js(T_REF_MS))
        .unwrap_or(f64::NAN);
    println!("VERIFY_FRESH_FIRST_READ_OFFSET={v_first}");
    let fresh_picks_up_global = v_first == OFF_A_EXPECT;
    println!("VERIFY_FRESH_READS_GLOBAL={fresh_picks_up_global}");

    // (b) 缓存已被 (a) 填充。现在把全局改成 Bogota，但**不通知**。
    if let Err(e) = set_global_tz(TZ_B) {
        println!("{e}");
        println!("T2_2_PROBE_FAIL");
        return;
    }
    let v_stale = iso_lazy
        .eval_number(&tz_offset_js(T_REF_MS))
        .unwrap_or(f64::NAN);
    println!("VERIFY_STALE_READ_NO_NOTIFY_OFFSET={v_stale}");
    let stale_cache_confirmed = v_stale == OFF_A_EXPECT;
    println!("VERIFY_STALE_CACHE_WITHOUT_NOTIFY={stale_cache_confirmed}");
    if stale_cache_confirmed {
        println!("  ⇒ 实测确认陈旧缓存：全局已改成 Bogota，但未收到通知的 Isolate 仍读 Tokyo");
        println!("    ⇒ 「必须 notify」的正确适用场景是**已求值过**的 Isolate");
    }

    // (c) 补发通知（Skip）后应立刻跟上全局值。
    iso_lazy.refresh();
    let v_after_notify = iso_lazy
        .eval_number(&tz_offset_js(T_REF_MS))
        .unwrap_or(f64::NAN);
    println!("VERIFY_AFTER_NOTIFY_OFFSET={v_after_notify}");
    let notify_fixes_cache = v_after_notify == OFF_B_EXPECT;
    println!("VERIFY_NOTIFY_FIXES_CACHE={notify_fixes_cache}");

    // =========================================================================
    // EXP4 替代路线：JS 侧 Date 兼容层（最小可行性）
    // =========================================================================
    println!();
    println!("=== EXP4 替代路线：JS 侧 Date 兼容层 ===");
    let iso_s = match TzIsolate::new("isolate=shim") {
        Ok(v) => v,
        Err(e) => {
            println!("{e}");
            println!("DATE_SHIM_WORKS=false");
            println!("T2_2_PROBE_FAIL");
            return;
        }
    };
    iso_s.refresh();

    let (read_t0, read_t1) = date_shim_experiment(&iso_s);
    let shim_t0_ok = matches!(&read_t0, Ok(r) if *r == format!("{LOGICAL_T0},{LOGICAL_T0}"));
    let shim_t1_ok = matches!(&read_t1, Ok(r) if *r == format!("{LOGICAL_T1},{LOGICAL_T1}"));
    match &read_t0 {
        Ok(r) => println!("SHIM_READ_T0={r}"),
        Err(e) => println!("SHIM_T0_ERROR={e}"),
    }
    match &read_t1 {
        Ok(r) => println!("SHIM_READ_T1={r}"),
        Err(e) => println!("SHIM_T1_ERROR={e}"),
    }
    println!("SHIM_T0_OK={shim_t0_ok}");
    println!("SHIM_T1_OK={shim_t1_ok}");
    let date_shim_works = shim_t0_ok && shim_t1_ok;
    println!("DATE_SHIM_WORKS={date_shim_works}");

    // =========================================================================
    // EXP5 设置失败分支：Etc/Unknown 必须被拒，且当前值不变
    // =========================================================================
    println!();
    println!("=== TZ 设置失败分支 ===");
    let tz_before_reject = v8::icu::get_default_time_zone();
    let etc_unknown_rejected = !v8::icu::set_default_time_zone("Etc/Unknown");
    let tz_after_reject = v8::icu::get_default_time_zone();
    println!("TZ_SET_ETC_UNKNOWN_REJECTED={etc_unknown_rejected}");
    println!(
        "TZ_DEFAULT_UNCHANGED_AFTER_REJECT={}",
        tz_before_reject == tz_after_reject
    );
    println!(
        "TZ_REJECT_BRANCH_WORKS={}",
        etc_unknown_rejected && tz_before_reject == tz_after_reject
    );

    // =========================================================================
    // 断言
    // =========================================================================
    println!();
    check(
        baseline_valid,
        "a",
        "基线自证成立 —— 同一时区下两个 Isolate 读数相等且等于期望偏移，测量方法非恒真",
        "基线不成立：测量方法坏了，整份结论作废（禁止在此状态下写 ADR）",
    );
    check(
        off_a1 != off_b1 && off_a1 == OFF_A_EXPECT && off_b1 == OFF_B_EXPECT,
        "b",
        "两时区偏移不同且符合参考时刻的预期（Tokyo -540 vs Bogota +300）",
        "两时区偏移读数不符合预期（可能选错了时区或参考时刻落在 DST 边界）",
    );
    check(
        cache_not_isolation_proven,
        "c",
        "「两读数不同」被证明来自 per-isolate 缓存：只通知 A（不给它任何 per-isolate 配置）它的读数就跟随全局值",
        "无法证明「两读数不同」只是缓存 —— TZ_PER_PROFILE_ISOLATED=true 的解读不成立",
    );
    check(
        global_leak_observed,
        "d",
        "只改进程全局设置（未对 B 做任何 per-isolate 配置）后，B 的读数从Bogota 变成 Tokyo 的期望偏移 —— 时区是进程级",
        "改全局设置后 B 的读数没变，或没落到目标时区的期望偏移：无法证明 process-global",
    );
    check(
        global_leak_reverse_observed,
        "e",
        "反向复核成立：A 的读数也从 Bogota 变成 Tokyo 的期望偏移，且目标时区不同于它原缓存值（非恒真）",
        "反向复核不成立：目标时区与该 Isolate 原缓存值相同（断言恒真），或读数未跟随全局值",
    );
    check(
        clock_evidence_consistent,
        "f",
        "now 的二值结论有证据支撑：两个 Isolate 的读数都贴着同一个宿主墙钟",
        "now 的证据自相矛盾：读数既不贴墙钟也不彼此接近，测量不可信",
    );
    check(
        date_shim_works,
        "g",
        "JS 侧 Date 兼容层最小可行：宿主喂 t0/t1，now 与 getTime 同步跟随",
        "JS 侧 Date 兼容层最小可行性不成立（替代路线被证伪）",
    );
    check(
        etc_unknown_rejected && tz_before_reject == tz_after_reject,
        "h0",
        "set_default_time_zone 对 Etc/Unknown 返回 false 且不改变当前值 —— 探针检查了返回值",
        "set_default_time_zone 的返回值未被正确检查，或失败时改变了当前值",
    );

    // EXP6 的两条自证断言：把「必须传 Skip」「缓存是懒的」从口头警告变成证据。
    check(
        v_skip == OFF_A_EXPECT,
        "h2",
        "对照组自证：refresh(Skip) 路径读到 Tokyo 的期望偏移 -540 —— 探针自身的时序前提成立",
        "对照组失败：连 refresh(Skip) 都读不到 -540，探针的时序前提不成立，全部时区结论作废",
    );
    check(
        redetect_breaks_setup
            && fresh_picks_up_global
            && stale_cache_confirmed
            && notify_fixes_cache,
        "h3",
        "实测确认三条口头警告：Redetect 会破坏刚设的时区（故必须传 Skip）；全新 Isolate 首读即取当前全局值（缓存懒填充）；已求值过的 Isolate 不发通知就读陈旧值、补发 Skip 通知后即跟上",
        "口头警告未能复现（Redetect 未破坏设置 / 首读未取全局 / 未通知仍读到新值 / 通知后未跟上）—— ADR 引用的前提需重新核实",
    );

    // B7 的机器可验证形式：扫描本文件源码，确认没有那个「宿主喂值」的 Date 构造调用。
    // needle 在运行时拼装，避免这个检查自己匹配到自己。
    let forbidden_call = ["v8", "::Date::", "new("].concat();
    let this_source = include_str!("main.rs");
    let uses_host_fed_date = this_source.contains(&forbidden_call);
    check(
        !uses_host_fed_date,
        "h1",
        "已避开宿主喂值的 Date 构造 API 作为时钟证据（它不经 V8 时钟，用它测必然假绿）",
        "源码里出现了该调用：时钟证据是宿主喂的，属假绿",
    );

    println!("T2_2_PROBE_{}", if ok { "OK" } else { "FAIL" });
}
