# T2.2 探针 —— 逻辑 Date 与多 Profile 的时钟 / 时区隔离

**这个探针证伪什么**：证伪「在同一进程里，给两个 `Isolate` 设两个不同的时区，
它们就能各自看到自己的时区」。

**结论（本地 macOS arm64 实测）**：

| 二值结论 | 值 | 含义 |
|---|---|---|
| `BASELINE_VALID` | `true` | 测量方法非恒真（同一时区下两个 Isolate 读数相等且等于期望偏移） |
| `TZ_PER_PROFILE_ISOLATED` | `true` | 两个 Isolate 的**缓存**里各自是对应时区的偏移 |
| `TZ_CACHE_NOT_ISOLATION` | `true` | **但这不是隔离**：只通知A（不给它任何 per-isolate 配置），它的读数就跟随了进程全局值 |
| `DATE_NOW_ISOLATED` | `false` | `Date.now()` 与宿主墙钟差0~1 ms，两个 Isolate 读同一个进程墙钟；v8 crate 无时钟注入入口 |
| `DATE_SHIM_WORKS` | `true` | JS 侧 `Date` 兼容层最小可行：宿主喂 t0/t1，`Date.now()` 与 `new Date().getTime()` 同步跟随 |
| `TZ_REJECT_BRANCH_WORKS` | `true` | `set_default_time_zone("Etc/Unknown")` 返回 `false` 且不改变当前值 |
| 汇总 | `T2_2_PROBE_OK` | 11 条断言全通过（tag: a b c d e f g h0 h1 h2 h3） |

**架构含义**：时区隔离靠**进程边界**，不靠 Isolate 边界（同技术方案 §19
「按 Profile/时区划分进程池」）。

## 三条必须记住的坑

1. **禁止**用「宿主喂一个 f64 造 `Date` 对象」的 API 测时钟隔离 —— 它**不经V8 时钟**，
   用它测必然**假绿**。探针所有读数都来自执行 JS 源码，且 `ASSERT_PASS(h1)` 会扫描
   本探针源码确认没有那个调用。
2. `date_time_configuration_change_notification` **必须传 `TimeZoneDetection::Skip`**。
   传 `Redetect` 会让 V8 重新探测并**覆盖**宿主刚设的时区 ⇒ 测出来的「隔离失败」
   其实是通知参数用错。
3. `set_default_time_zone` 的 `bool` 返回值**必须检查**：不可识别的 id 返回 `false`
   且不改变当前值；静默忽略会让后面所有偏移读数不可信。

## 实测到的 API 约束：两个 `Isolate` 不能同时「已进入」

这是设计文档未预见、但实测撞上的硬约束：

- `v8::Isolate::new` 返回的 `OwnedIsolate` 在**构造时就被 `enter()`**
  （`isolate.rs:2768`），且 `Drop` 断言必须按构造逆序析构（`isolate.rs:2875`）。
- 同线程同时持有**两个** `OwnedIsolate` ⇒ V8 直接 FATAL：
  `Fatal error in v8::HandleScope::CreateHandle() / Cannot create a handle without a HandleScope`。
  **实测确认：与时区设置、refresh 调用与否无关。**
- 正规通道是 `SharedIsolate` + `Locker`：`try_into_shared()` 把 Isolate 退出
  （unentered），之后每次访问用 `SharedIsolate::lock()` 拿锁并重新进入。
  本探针全部走这条通道，**只有 `try_into_shared` 一处 `unsafe`**，带 `SAFETY:` 注释。

## EXP6：把两条「口头警告」变成机器可复现的证据

此前「必须传 `Skip`」与「缓存是懒的」只是**口头断言**，没人实测过。
探针用受控对照把它们跑成证据（断言 `h2` / `h3`）：

| 观察 | 值 | 结论 |
|---|---|---|
| `VERIFY_SKIP_OFFSET` | `-540` | `Skip` 路径正确读到 Tokyo |
| `VERIFY_REDETECT_OFFSET` | `-480` | **`Redetect` 让刚设的时区失效**（读到宿主时区） |
| `VERIFY_TZ_GLOBAL_AFTER_REDETECT` | `Asia/Shanghai` | **`Redetect` 把 ICU 全局值本身改回了宿主时区** |
| `VERIFY_FRESH_FIRST_READ_OFFSET` | `-540` | **全新 Isolate 首读即取当前全局值**（不需要预先 notify） |
| `VERIFY_STALE_READ_NO_NOTIFY_OFFSET` | `-540` | 全局已改 Bogota，但**已求值过**的 Isolate 不发通知仍读 Tokyo（陈旧缓存） |
| `VERIFY_AFTER_NOTIFY_OFFSET` | `300` | 补发 `Skip` 通知后立刻跟上 |

**修正了一条被广泛误传的说法**：「新建 Isolate 必须先 notify 才能读到新时区」
—— **实测证伪**。全新 Isolate 首次求值就会取当前全局值。
「必须 notify」的正确适用场景是**已经求值过、缓存已填充**的 Isolate。

（此处踩过一次坑并已修：对照组 2 的 `Redetect` 会把全局改回 `Asia/Shanghai`，
后续对照组若不重新设全局，读数会被「全局本来就是 Shanghai」**污染**——
那是混淆变量。已在源码里留下注释。）

## 独立 workspace

本目录是**独立 Cargo workspace**（`Cargo.toml` 顶部第一行是空的`[workspace]`），
只依赖 `v8 = "152.2.0"`，不进主 workspace（ADR-0008 决策 1）。

## 运行

```bash
cd poc/t2_2_clock_tz
cargo run -j2       # 末尾输出 T2_2_PROBE_OK 或 T2_2_PROBE_FAIL
```