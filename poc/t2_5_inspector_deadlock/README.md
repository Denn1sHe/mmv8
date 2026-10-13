# T2.5 探针：V8 Inspector 暂停消息泵与 Python 回调的**死锁边界**

> **这个探针证伪什么**：不是「跑通就行」，而是**测绘死锁边界**——
> 在什么条件下会死锁、在什么条件下不会，产出一张**条件表**。
>
> 对应验收标准：许清楚 `docs/remaining-scope-2026-10-12.md` §4 的 **D0~D12**。
> 施工图：`docs/design-remaining-tasks-2026-10-12.md` §0（决策 Q-4）+ §3。

## 为什么「超时机制」是交付物的一部分

死锁在测试里的表现是**进程挂住**，不是**测试失败**。若超时机制本身不可靠，
本探针所有「不死锁」的结论都无法与「根本没测出来」区分——两件事在证据上
长得一模一样。故 **D0 是第一验收项**：先用一个「必然挂起」的场景证明
watchdog 能把它变成 `TIMEOUT` 结论，再开始测绘。

## 运行

```bash
cd poc/t2_5_inspector_deadlock && cargo run
```

**完整原始输出留档在 [`run-output.txt`](./run-output.txt)（572 行，`T2_5_PROBE_OK` / exit 0）**，
供 ADR-0012 与 QA 证据台账直接引用。

> **留档来源说明**：该文件由**当前冻结源码**（已 `cargo fmt`，含 D0a/D0b 两项断言）
> 在**隔离环境**（无并发 cargo）实跑生成，`T2_5_PROBE_OK` / exit 0。
> 每轮子进程证据均含 `WATCHDOG_USES_ISOLATE_HANDLE=true` 与
> `CLEAN_RESUME_AFTER_TERMINATE=true`；父进程另有 `ASSERT_PASS(m)` / `ASSERT_PASS(n)`
> 从**实跑证据**核验这两项（见 ADR-0012 的「D0a / D0b」节）。
> 计时字段（`RESUMED_AT` 等）逐次浮动，属正常。

> ⚠️ **必须在无并发负载下运行**（不要与其他 `cargo` / 编译任务并行）。
> 本探针的判据是「watchdog 是否到点」，而 watchdog 只读墙钟 ——
> 并发负载会把**对照组**的调度延迟也算成 `TIMEOUT`。
> 判读规则与证据见 ADR-0012 的「运行环境要求与负载敏感性」一节。
> 探针自身有多重护底（含进程总硬超时 180s → `exit(70)`），**不会挂住 CI**，
> 受影响的只是分类的准确性。

环境变量：

| 变量 | 作用 | 默认 |
|---|---|---|
| `IV8_PROBE_WATCHDOG_MS` | watchdog 超时时长（ms） | 本地 3000 / 远程建议 6000 |
| `IV8_PROBE_TOTAL_DEADLINE_SECS` | 进程总硬超时（秒）。**仅用于自测 `exit(70)` 兜底本身** | 180 |
| `IV8_PROBE_SLEEP_CHUNK_MS` | `sleeping` 策略的 `time.sleep` 分片（ms）。**诊断旋钮**，见「测量局限」 | 50 |
| `IV8_PROBE_HANDSHAKE_MS` | Python 侧握手超时（ms）。**诊断旋钮** | `max(5×watchdog, 5s)` |
| `PYO3_PYTHON` | 指定 Python 解释器 | `python3` |
| `IV8_PROBE_PYTHON` | 同上（探针自有的别名） | 未设 |

自测兜底机制（应在数秒内以 70 退出）：

```bash
IV8_PROBE_TOTAL_DEADLINE_SECS=2 cargo run   # → EXIT=70，打印 WATCHDOG_ESCALATED=true
```

## 偏离设计文档之处（**必须读**，否则会把假绿当结论）

| 设计文档原文 | 本探针做法 | 理由 |
|---|---|---|
| §3.2：`pyo3` 用 `features = ["extension-module"]` | 改为 `"auto-initialize"` | 本探针是 `cargo run` 的独立二进制，`extension-module` 不链接 libpython ⇒ 运行期 `dyld: Library not loaded: @rpath/libpython3.13.dylib`。二者互斥 |
| §3.5 D4：Python 侧用 `time.sleep()` 等页面线程 | **两种策略都跑**：`spin`（真持 GIL）+ `sleeping`（GIL 已释放，对照） | `time.sleep()` **释放 GIL**，照字面实现构造不出互等，会得到「不死锁」的**假绿**。详见下节 |
| §3.5 D5：在 `run_message_loop_on_pause` 里对同一 Context 重入 eval | 改由**宿主函数回调**（`Function::new`）构造 | 泵回调签名里**没有** V8 scope，且 152.2.0 未公开任何从零构造 `Local<Context>` 的 API ⇒ **泵内重入不可构造**（不是「危险」，是「做不到」）。被测性质不变 |
| §3.5 表格把 #9~#12 标 ❌（4 行），汇总却写 `COMBOS_TRIED=9` | 按**表格**执行 ⇒ `COMBOS_TRIED=8`、`COMBOS_UNMEASURABLE=4` | 二者矛盾；逐行的 ❌ 标记才是可核对依据，`9` 疑为算术笔误（12−4=8） |
| §3.4 兜底「`terminate_execution` 后 2× watchdog 内未退出则 `exit(70)`」 | 保留，并追加**进程总硬超时**独立线程 | `exit(70)` 只覆盖「单组合被救回但很慢」；若 watchdog 线程本身失效，须有第二道兜底 |

## 进程结构

```
父进程（编排 + 断言）
  ├─ 子进程 --combo=0   D0 watchdog 自证（「必然挂起」）
  ├─ 子进程 --combo=1   f1=no  f2=no  f3=none
  ├─ 子进程 --combo=2   f1=no  f2=no  f3=msg_only
  ├─ 子进程 --combo=3   f1=no  f2=no  f3=needs_locker
  ├─ 子进程 --combo=4 --gil=spin    ← 真 GIL 互等
  ├─ 子进程 --combo=4 --gil=sleep   ← 伪 GIL 互等（GIL 已释放，对照）
  ├─ 子进程 --combo=5   （同上两种策略）
  ├─ 子进程 --combo=6   （同上两种策略）
  ├─ 子进程 --combo=7   f1=no  f2=yes f3=none      ← 重入 eval
  └─ 子进程 --combo=8   f1=no  f2=yes f3=msg_only
```

**为什么每个组合独立子进程**（设计文档 §3.8）：
D5（重入 eval）很可能让 V8 **FATAL 直接 abort 进程**。
FATAL 本身就是一个**可判定的结论**，但它只能表现为「子进程被信号杀死」，
所以必须由父进程读退出状态来区分：

| 父进程观测到 | 分类 |
|---|---|
| 子进程正常退出且自报 `CLEAN_RESUME` | `CLEAN_RESUME` |
| 子进程正常退出且自报 `TIMEOUT` | `TIMEOUT` |
| 子进程被**信号**杀死 | `FATAL` |
| 子进程超时仍未退出，被父进程 kill | `DEADLOCK` |

## 三重超时（任一失效都不会挂住 CI）

| 层 | 位置 | 兜底 |
|---|---|---|
| 单组合 watchdog | 独立线程，`IsolateHandle::terminate_execution()` + `AtomicBool` | 见下 |
| 子进程超时 | 父进程轮询 `try_wait`，超时即 `kill` | 判定为 `DEADLOCK` |
| **进程总硬超时** | 独立线程，180s | 打印 `WATCHDOG_ESCALATED=true` 后 **`exit(70)`** |

`exit(70)` 的意义（设计文档 §3.4）：让 QA 能区分
「探针正常完成并报告 `TIMEOUT`」与「**探针自己都救不回来**」。
这两件事在 CI 上的表现完全不同（前者正常退出，后者卡住）。

## 三个实验因子

| 因子 | 取值 | 含义 |
|---|---|---|
| **F1** | `no` / `yes` | 暂停期间宿主是否尝试获取 GIL |
| **F2** | `no` / `yes` | 宿主回调是否重入同一 Context 的 eval |
| **F3** | `none` / `msg_only` / `needs_locker` | 第二线程的参与方式 |

## 关键实测约束（写代码时会踩的坑）

### 1. `Debugger.enable` 是 `schedule_pause_on_next_statement` 的前置

`schedule_pause_on_next_statement` 只「安排在下一条语句暂停」。
若 Debugger 域未激活，V8 **不会真的暂停**，`run_message_loop_on_pause`
永远不被调用，探针会「全部 `CLEAN_RESUME`」——一个**全是绿灯的假结果**。
故本探针先 `dispatch_protocol_message("{\"id\":1,\"method\":\"Debugger.enable\"}")`。

### 2. PyO3 的 feature 是 `auto-initialize`，不是 `extension-module`

设计文档 §3.2 写的是 `features = ["extension-module"]`。**实测该 feature 不可用**：

- `extension-module` 的语义是「链接时**不**链接 libpython，符号留给宿主进程解析」；
- 本探针是 `cargo run` 起的**独立二进制**，没有宿主 Python 进程 ⇒
  运行期 `dyld[...]: Library not loaded: @rpath/libpython3.13.dylib`。
- 二者互斥，必须取 `auto-initialize`（探针自己把解释器拉起来）。

### 3. 本机 Python 的 `LIBDIR` 被烘死为 `/install/lib`

`sysconfig` 的 `LIBDIR` 是 python-build-standalone 的打包瑕疵
（构建前缀被写死进 `Makefile`，安装到别处后不重写）。
PyO3 忠实读取它，于是产出 `-L /install/lib`，**链接直接失败**：

```text
ld: warning: search path '/install/lib' not found
ld: library 'python3.13' not found
```

**本探针的处理**：`build.rs` 自动探测真实库目录并 emit
`rustc-link-search` + `rpath`。不这样做就得手工设
`RUSTFLAGS="-L <lib> -C link-arg=-Wl,-rpath,<lib>"`，
而**每台机器都要记得带**（本地 macOS arm64 / 远程 Linux x86_64），
漏掉的报错还分「链接期」和「运行期」两种形态，很难联想到同一件事。
探测失败时 `build.rs` **静默跳过**，把判断权交回 pyo3 与链接器（正常的
Python 走原路径即可，本脚本对它们零影响）。

### 4. `time.sleep()` **释放 GIL** —— 照字面实现会得到假绿

设计文档 §3.5 D4 行要求「Python 侧函数内 `time.sleep()` 等页面线程」。
但 **`time.sleep()` 期间 CPython 会释放 GIL**，于是：

- Python 线程「在等页面线程」但**没持 GIL**；
- 页面线程的 `Python::with_gil` 会**成功** ⇒ 观测到 `CLEAN_RESUME`。

**这不是「GIL 路径不死锁」的证据，而是「这个构造没构造出互等」的证据。**
照字面实现并报 `CLEAN_RESUME` 就是**假绿**。

本探针因此对 F1=yes 的组合跑**两种** Python 侧策略：

| 策略 | Python 侧行为 | Python 侧持 GIL？ | 实测 | 作用 |
|---|---|---|---|---|
| `--gil=spin` | 纯 Python 忙等（只调 `time.monotonic()`，不释放 GIL） | 全程持锁 | `TIMEOUT`，`GIL_BLOCKED_MS≈6000` | **正题**：真 GIL 互等 |
| `--gil=sleeping` | `time.sleep(50ms)` 循环 | ~99.9% 不持锁 | `CLEAN_RESUME`，`GIL_BLOCKED_MS≈0` | 与设计文档字面可比 |
| `--gil=nohold` | 仅初始化，随后**完全不持 GIL** | **零** | `CLEAN_RESUME`，`GIL_BLOCKED_MS=0` | **确定性对照**（与调度无关） |

若三者结论相同，或**对照也超时**，说明测量方法不可信 —— 断言 `(i2)` 会判 FAIL，
且对照超时会自动打出 `MEASUREMENT_SUSPECT=true`。

### 4.1 测量局限（一次真实翻转事故的产物）

架构师独立重跑时，组合 5 + `sleeping` 曾从 `CLEAN_RESUME` 翻转为
`TIMEOUT`（`GIL_BLOCKED_MS=10183`）。查因结论与已做加固：

1. **就绪标志语义不严**（已修）：首版把 `import("time")` 放在置标志**之后**，
   页面线程实际等的是「对方的 import」。⇒ 准备全部完成后才置标志。
2. **对照组 GIL 重获取过频**（已修）：`time.sleep` 分片首版 5ms，
   Python 侧每 5ms 就重新抢一次 GIL，过载时页面线程可能连续抢不到。
   ⇒ 改 50ms 分片，并新增 `not_holding`（持锁时间为零的确定性对照）。

**残余风险（未消除，已登记）**：`TIMEOUT` 的判据是「watchdog 是否到点」，
而 watchdog 只看墙钟 ⇒ **任何**超过 watchdog 的调度延迟都会被计为 `TIMEOUT`。
缓解手段：`not_holding` 严格对照 + `MEASUREMENT_SUSPECT` 自动标红 +
输出顶部 `DERIVED_*` 派生常数（使「读数是否越界」当场可判）。

**复现记录**：在 8 核机器上用旧参数、合成负载至 load≈90，各跑 8~10 次，
**未能复现**该翻转（`GIL_BLOCKED_MS` 恒为 0）。故按「已定位两个可修脆弱点 +
残余风险已登记」如实记录，**不宣称已完全消除**。

### 5. `GIL_CONTENDED` 由「Python 侧是否真持锁」决定，不由「是否在等」决定

`spin` 策略才置 `python_side_holding`；`sleeping` 策略只置 `python_side_waiting`。
若两者都置同一个标志，`GIL_CONTENDED` 就分不出真伪互等 —— 那又是一个假绿。

### 6. 握手等待，不是固定 `sleep`

Python 线程首次 `Python::with_gil` 会触发 **CPython 全量初始化**（本机实测几十毫秒）。
固定 `sleep(20ms)` 可能来不及 ⇒ 页面线程在 Python 侧还没拿到 GIL 时就发起
`with_gil` ⇒ 争用条件不成立 ⇒ 假绿。故用**有界握手**（`wait_for_python_side`）。

### 5. `v8::Locker` 需要 `SharedIsolate`，不能对普通 `Isolate` 用

`v8-152.2.0` 的 `Locker` 只能从 `SharedIsolate::lock()` 获得，
而 `SharedIsolate` 来自 `OwnedIsolate::try_into_shared()`（`unsafe`，
且要求「当前线程无其他 isolate 在栈上」）。
强行对普通 `Isolate` 构造 Locker 会**破坏 V8 的锁状态**。

故 F3=`needs_locker` 采用**诚实的部分覆盖**：观测并记录
「页面线程暂停期间 Locker 被占用」（锁序死锁的**前提**成立），
但**不实际 `lock()`**。已在「未覆盖路径」中显式登记。

## unsafe 纪律（D9）

`poc/` 在 `scripts/check-unsafe-whitelist.sh` 白名单内，但**约束不放宽**：
每个 `unsafe` 须在**前 2 行内**有 `// SAFETY:`，禁止 crate 级与 mod/impl 级开口。

**本探针实测为「零 unsafe」** —— `src/` 下没有任何 `unsafe {}` 块
（`grep -rn "unsafe" src/` 只命中两处注释文本）。

```
$ bash scripts/check-unsafe-whitelist.sh
== unsafe 白名单守卫（AGENTS.md 铁律 4） ==
== PASS：白名单外无 unsafe；无 crate/mod 级开口；白名单内 unsafe 均带 SAFETY 注释 ==
$ echo $?
0
```

### 为什么能做到零 unsafe（这是有意义的结论，不是运气）

两条曾经看起来**必须** unsafe 的路径，在 `v8` 152.2.0 上都有安全的公开替代：

| 看似必须 unsafe 的路径 | 实际的公开安全做法 |
|---|---|
| 泵回调内造 scope 才能重入 eval | **放弃在泵内重入**，改由宿主函数回调（它自带 `&mut PinScope`）⇒ 见「偏离设计之处」 |
| `V8InspectorClient` / `Channel` 含 `PhantomPinned`、V8 持裸指针 | `V8InspectorClient::new`（`inspector.rs:680`）与 `Channel::new`（`:406`）**已在 crate 内**用 `Box::into_pin` 完成 pin，且 `V8Inspector` / `V8InspectorSession` **持有**其所有权（`_client` / `_channel` 字段）⇒ 探针侧只需传 `Box`，不需要 pin 相关的 unsafe |
| 宿主函数回调要访问共享状态 | `Isolate::set_slot` / `get_slot`（`scope.rs:2123-2150`）⇒ 无需捕获闭包、无需碰裸数据指针 |

**这也解释了为什么「禁止回调中重入 eval」这条禁令在实践中容易被违反**：
在 `v8` crate 的公开 API 下，从泵回调里重入**根本做不到**（拿不到 scope），
所以真实的违反只可能发生在**宿主函数回调**里 —— 而那正是本探针 D5 构造的位置。

## 汇总标记

`T2_5_PROBE_OK` / `T2_5_PROBE_FAIL`。

**`T2_5_PROBE_OK` 不代表「无死锁」**，它只代表
「各组合的分类都测出来了，且方法自证有效」。这一点必须在 ADR 里写清楚，
否则维护者会把「在已覆盖的 N 种组合下未观测到死锁」误读成「无死锁风险」。
