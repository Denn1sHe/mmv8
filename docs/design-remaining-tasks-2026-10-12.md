# 剩余三项阻断性 PoC 的实现方案（T2.2 / T2.4 / T2.5）

> **本文档是什么**：把 `docs/remaining-scope-2026-10-12.md`（许清楚）的二值验收标准
> （B0~B9 / C1~C17 / D0~D12）转成**工程师可直接执行**的探针设计。
>
> **前置**：`docs/adr/0009-bindings-webidl与engine-v8的V8类型边界.md`（G1-PRE-1，已落档）。
>
> **本文档已做的核实**（不需工程师重做）：所有版本号、源码行号、编译可行性均在
> 2026-10-12 由架构师在本机 cargo registry 源码与**实际编译**核实，标注为「已核实」；
> 无法核实的写「**待实测**」，由工程师跑出来。
>
> **纪律**：`AGENTS.md` 铁律十条；每个探针是**独立 Cargo workspace**（ADR-0008 决策 1）；
> `poc/` 内每个 `unsafe` 须在前 2 行内有 `SAFETY:`（ADR-0008 决策 4）。

---

## 0. 两个必须现在定的决策（会改变工程师的代码结构）

### 决策 Q-4：T2.5 **引入 PyO3** —— 结论：**引入**

许清楚 §4 待确认 1 把这列为「本 PoC 最重要的待确认项」。

**结论：引入 `pyo3`，版本 `0.23`（待实测是否存在更新可兼容版本，但 `0.23` 是与 edition 2024
兼容的已知线；若 `cargo add pyo3` 解析到别的版本，以解析结果为准并在 ADR 记录）。**

**理由**：

1. **不引入就测不到 D4。** 许清楚 §4 Q3 明确列出三条死锁路径，GIL 路径是其中**唯一**
   需要真 Python 解释器才能构造的。Rust 宿主函数回调（T2.0 已验证的
   `Function::new`）**不持 GIL**，因此用它构造不出 GIL 互等。
2. **技术方案 §16 已经给了对策**（「宿主回调使用 PyO3 attach/detach 机制，避免持 GIL 等待
   页面线程」）—— 对策本身需要被验证，而**验证它的前提就是有 PyO3**。
3. **本机条件已核实**：`python3` 为 3.13.12，`Python.h` 存在于
   `/Users/dennis/.workbuddy/binaries/python/versions/3.13.12/include/python3.13/Python.h`
   （已核实），`uv` 在 `/opt/homebrew/bin/uv`（已核实）。
4. **`poc/` 是独立 workspace**，引入 pyo3 **不影响主门禁**的依赖与内存预算
   （ADR-0008 决策 1 的全部理由仍然成立）。

**已核实的风险与对策**：

| 风险 | 对策 |
|---|---|
| 本地 cargo 缓存**没有 pyo3**（已核实 `~/.cargo/registry/cache/*/` 下无 `pyo3-*`） | 首次 `cargo build` **需要联网**（走 rsproxy 镜像）。工程师第一次跑要联网，之后离线。**这不是阻塞**——`AGENTS.md:76` 已授权 AI 自主安装工具链与依赖 |
| 远程 `~/mmv8` 是否有 Python 开发头文件 | **待实测**。远程已装 `uv 0.13.0`（`AGENTS.md:63`），`uv venv` 建出的 venv **自带头文件**，无需系统 `python3-dev`。若仍失败，走决策上报，**不许改远程系统配置**（`AGENTS.md:67`） |
| pyo3 与 `v8` 都链接大二进制，远程 3.6 GB 内存可能 OOM | 按既有对策顺序：`cargo build -j2` → 仍 OOM 则走决策上报 |

**降级方案（若 PyO3 在远程确实装不上）**：T2.5 拆成**两个探针**——
`t2_5_inspector_pause`（纯 Rust，验重入路径 D5 与锁序路径 D6）与
`t2_5_gil_deadlock`（PyO3，验 GIL 路径 D4）。D4 标注为「**未覆盖**」，
**不得默认安全**（许清楚 D6 的同款纪律）。**不允许**用「Rust 侧模拟 GIL」替代真 GIL。

---

### 决策 Q-3：`html5ever` 采用 **0.27**，`markup5ever` 分叉**不复用** —— 结论：**先不升级，T2.4 不与 html5ever 联调**

**这一条我做了源码级核实（不是推断）。核实方法与原文如下。**

#### 已核实：markup5ever 0.12.1 与 0.39.0 的 ADT **源码级不兼容**

对比对象：本机 cargo registry 中 `markup5ever-0.12.1/`（配 html5ever 0.27）与
`markup5ever-0.39.0/`（配 blitz），路径
`~/.cargo/registry/src/rsproxy.cn-e3de039b2554c837/`。

**(a) 底层原子类型换了 crate。** `markup5ever-0.12.1/Cargo.toml` 有
`[dependencies.string_cache]`；`markup5ever-0.39.0/Cargo.toml` **没有** `string_cache`，
改为 `[dependencies.web_atoms] version = "0.2.3"`（已核实原文）。
0.12.1 的 `lib.rs` 有 `include!(concat!(env!("OUT_DIR"), "/generated.rs"))` 与
`pub mod data;`（build.rs 从 `local_names.txt` 生成）；0.39.0 **删掉了 `build.rs`、
`data/`、`local_names.txt`、`entities.rs`**，改为：

```rust
// markup5ever-0.39.0/lib.rs 原文
pub use web_atoms::{
    local_name, namespace_prefix, namespace_url, ns, LocalName, LocalNameStaticSet, Namespace,
    NamespaceStaticSet, Prefix, PrefixStaticSet,
};
```

⇒ **`LocalName` / `Namespace` / `Prefix` 的底层类型从 `string_cache` 的 `Atom`
变成了 `web_atoms` 的类型。** 这意味着 html5ever 0.27 造出的 `QualName`
与 markup5ever 0.39 造出的 `QualName` 是**两个互不相容的类型**，不可能在同一个数据结构里混用。

**(b) `TokenizerResult` 的归属与变体都变了（这条直接冲击 ADR-0006）。**

```rust
// html5ever-0.27.0/src/tokenizer/mod.rs:49-52 原文
pub enum TokenizerResult<Handle> {
    Done,
    Script(Handle),
}
```

```rust
// markup5ever-0.39.0/interface/mod.rs 原文（新位置 + 新变体）
#[must_use]
#[derive(Debug, PartialEq)]
pub enum TokenizerResult<Handle> {
    Done,
    Script(Handle),
    /// The document indicated that the given encoding should be used to parse it.
    EncodingIndicator(StrTendril),
}
```

⇒ **两处变化**：(i) 类型从 `html5ever::tokenizer` 移到 `markup5ever::interface`；
(ii) **多了一个变体 `EncodingIndicator`**。ADR-0006 的探针代码写的是
`match tok.feed(&mut input) { TokenizerResult::Done => …, TokenizerResult::Script(_) => … }`
——**在 0.39 上这是 `E0004` non-exhaustive 编译错误**（ADR-0006 已记录该类坑：「`Token` 需覆盖
`EOFToken`，少了会 `E0004` non-exhaustive」）。而 `feed()` 返回 `TokenizerResult`，
新增的 `EncodingIndicator` 意味着**编码嗅探这条新路径必须处理**，否则一旦 HTML 声明
非 UTF-8 编码就会走到未处理分支。

**(c) `TreeSink` trait 全面从 `&mut self` 改为 `&self`（源码级 diff 已确认）。**
0.12.1 → 0.39.0 的 `tree_builder.rs` 中至少 17 个方法签名从 `&mut self` 变为 `&self`，
`elem_name` 返回类型从 `ExpandedName<'a>` 变为 `Self::ElemName<'a>`（**关联类型**），
`NextParserState` 枚举**被删除**，`complete_script` 的返回类型随之改变，
`create_element` 的第一参数从 `&mut Sink` 变为 `&Sink`。
另新增 `ElemName` trait（`fn ns(&self) -> &Namespace; fn local_name(&self) -> &LocalName;`）与
`allow_declarative_shadow_roots` / `attach_declarative_shadow` / `maybe_clone_an_option_into_selectedcontent`。

**(d) `BufferQueue::push_front` 的接收者从 `&mut self` 变成 `&self` —— 这是唯一一处
「ADR-0006 的核心结论看起来还活着，但代码仍然编译不过」的情况，迷惑性最强。**
已核实两版原文：

```rust
// markup5ever-0.12.1/util/buffer_queue.rs:78 原文
pub fn push_front(&mut self, buf: StrTendril) {
// markup5ever-0.39.0/util/buffer_queue.rs:82 原文
pub fn push_front(&self, buf: StrTendril) {
```

⇒ 方法**存在**（所以 ADR-0006 的 `push_front` 插入点语义**在原理上很可能仍成立**），
但因 `Tokenizer::feed(&mut self, input: &mut BufferQueue)` 仍要 `&mut BufferQueue`
（已核实 `html5ever-0.27.0/src/tokenizer/mod.rs:209`；0.39 的 `feed` 签名**待实测**），
若调用方变量不是 `mut`，迁移时代码会因不可变绑定而编译失败。
**教训**：「API 还在」≠「代码能编译」。这类问题只有真跑一遍才能发现，
这正是把 T2.3-bis 单列为独立任务的根本理由。

**结论（Q-3）**：`HTML5EVER_VERSION_FORK_IMPACT = **BREAKING**`。
ADR-0006 的五条结论（a~e）在 0.39 上**不能假定继承**，其中**三条**有源码级确证的破坏：
`TokenizerResult` 的位置与变体（b）、`TreeSink` 的全套签名（c）、`push_front` 接收者变异（d）；
`QualName`/`LocalName` 底层类型换 crate（a）使新旧树**无法混用**。
唯一「看起来还活着」的是 `push_front` 的插入点语义，但它被 (a)(c) 的连带破坏掩盖，
**必须实测才能确认**。

#### 因此 T2.4 的探针形态定为：**先只验 blitz-dom 自身能力，不与 html5ever 联调**

**理由**：把「blitz-dom 能不能用」与「html5ever 0.39 好不好用」两个问题**解耦**。
后者需要重跑 T2.3 的五条断言并逐条对照 ADR-0006，是一个**独立的回归任务**；
混进 T2.4 会让「blitz 不通过」与「html5ever 分叉难处理」两个结论纠缠不清
——而 T2.4 的产出是**路线裁决**（走 Blitz 还是切 Stylo），必须单一变量。

**后续独立任务（不属于 T2.4）**：若 T2.4 通过、决定采用 Blitz，
则必须另起一个 T2.3-bis 回归任务，在 html5ever 0.39 上重跑 a~e 并出 ADR-0006 的修订版
（铁律 2：结论只能独立 PR 改）。

#### ⚠️ C4 的可执行性：本地**没有** `html5ever 0.39`，须先解决获取问题

许清楚 C4 的判据原文是「在 `html5ever 0.39` 上重跑 T2.3 的 5 条断言（a~e）」。
**架构师已核实本机 cargo registry 的实际状态**：

```
~/.cargo/registry/src/rsproxy.cn-e3de039b2554c837/
  → 只有 html5ever-0.27.0            （没有 0.39 / 0.40）
~/.cargo/registry/cache/rsproxy.cn-e3de039b2554c837/
  → 只有 html5ever-0.27.0.crate
  → blitz-html-*.crate               （不存在；blitz-html 才是依赖 html5ever ^0.39 的那个）
```

⇒ **C4 无法离线执行**，需要联网 fetch `html5ever 0.39`（走 rsproxy 镜像，
`AGENTS.md:89` 已配置）。这是**环境事实，不是设计障碍**，但必须显式登记，
否则 QA 看到 C4 未完成会误判为「T2.4 未交付」。

**处置建议（本 ADR 已按此定案）**：
- **本轮 T2.4 的交付物不含 C4 的执行结果**，只含 **C5 的二值结论**（`BREAKING`，已由源码级对比得出）。
- C4 作为 **T2.3-bis 的验收标准**，在该任务里连同 fetch 动作一起排。
- 这与许清楚 §依赖顺序表中「T2.4 的**裁决**需等 C4/C5 的分叉结论」**不冲突**：
  C5 已定（`BREAKING`），而 C4 只影响「若选 Blitz 才需复核」这条分支；
  由于 C9 已确证为 `false` ⇒ 裁决是 `SWITCH_TO_STYLO` ⇒ **该分支不会被走到**。
- **若 T2.4 结论意外变成 `KEEP_BLITZ`**（例如 blitz 出了新版修好了 shadow root），
  则 C4 **立即变成阻塞项**，必须先 fetch 再跑，不能默认「沿用 ADR-0006 的结论」。

**一句话给 QA**：C4 未执行的**唯一原因是环境缺 crate**，且当前裁决使其**不影响** T2.4 有效性；
T2.4 的验收依据是 C1/C2/C6/C7/C8/C9/C10 + C11 裁决，**不含 C4**。

---

## 1. T2.2 —— 逻辑 Date 与多 Profile 时钟/时区隔离

### 1.1 已核实的事实（工程师不必重做）

| 事实 | 原文/位置（`v8-152.2.0`，本机已下载） |
|---|---|
| 时区是**进程级**，官方文档明说 | `src/icu.rs:100-107`：「**This mutates process wide state and is not synchronized with isolates running on other threads**, so it should be called before those isolates evaluate any code observing the time zone.」 |
| `set_default_time_zone` 对无法识别的 id **返回 `false` 且不改变当前值**；`Etc/Unknown` 自身也被拒 | `src/icu.rs:96-101` 文档原文（已核实全文） |
| 唯一可写时区入口 | `pub fn set_default_time_zone(time_zone_id: &str) -> bool`（`src/icu.rs:108`，`#[must_use]`） |
| 缓存刷新入口在 **Isolate** 上，只收 `Skip\|Redetect` 枚举，**无「指定时区」参数** | `src/isolate.rs:2022` `date_time_configuration_change_notification(&mut self, time_zone_detection: TimeZoneDetection)`；枚举定义 `src/isolate.rs:116` |
| `Date` 类型的暴露面**极窄** | `src/date.rs` 全文 **31 行**，只有 `Date::new(scope, value: f64) -> Option<Local<Date>>` 与 `value_of() -> f64` |
| **无时钟注入 API** | `src/date.rs` 全文无 `now` / `set_now` / `Clock` / `mock` / `fake_time` |
| 可用于「现在」的唯一 JS 入口 | `Date::now()` / `new Date()`（JS 侧，走 V8 内部时钟，**不经 `Date::new`**） |

> 术语（给不懂 Rust 的维护者）：`Isolate` ≈ 「一个独立的 JS 引擎实例」，相当于
> 「一个进程里开的第二个 Node 进程」（同进程，但引擎状态隔离）。

### 1.2 依赖选型

```toml
[workspace]          # 独立 workspace（ADR-0008 决策 1）

[package]
name = "tzprobe"
version = "0.0.0"
edition = "2021"     # 与 t2_3 探针一致（v8 crate 本身是 edition 2021 语境）

[dependencies]
v8 = "152.2.0"       # 与 t2_0 / t2_1 探针完全一致，避免版本漂移
```

**不引入任何新依赖。** 不引入 pyo3（T2.5 才有）、不引入 chrono/time（逻辑时钟在 JS 侧做）。

> ⚠️ **本条已于 2026-10-12 经实测确认并作废一条早期结论**：
> 早期版本曾写「T2.2 依赖 `pyo3`（abi3）」。**该结论错误，已作废。**
> T2.2 探针 `poc/t2_2_clock_tz/Cargo.toml` 的 `[dependencies]` **只有 `v8 = "152.2.0"`**
> （已核实），实测在**纯 `v8`、无 PyO3**条件下成功。
> **理由**：时区读取只需执行 JS 表达式并读回数字，Python 侧无需参与。
> `pyo3` 是 **T2.5**（GIL 死锁路径）才需要的依赖。
> **后续 agent 若看到任何「T2.2 需要 pyo3 / abi3」的说法，一律以本条与探针 manifest 为准。**

### 1.3 探针目录与文件

```
poc/t2_2_tz_isolation/
├── Cargo.toml          # 独立 workspace；仅依赖 v8 = "152.2.0"
├── README.md           # 一句话说明本探针证伪什么
└── src/
    └── main.rs         # 单文件；三段实验 + 断言
```

### 1.4 逐条断言设计（对齐 B0~B9）

**固定参考时刻（消除「现在」引入的不确定性）**：
`T_REF = 1_700_000_000_000` ms（= 2023-11-14T22:13:20Z）。
两个时区在该时刻的 UTC 偏移**必须不同**：

| 时区 | 该时刻偏移 | 说明 |
|---|---|---|
| `Asia/Tokyo` | **UTC+09:00** | 全年不夏令时，偏移稳定 |
| `America/New_York` | **UTC-05:00**（EST） | 11 月是 EST（夏令时 3 月–11 月初）；若担心 DST 边界，改用 `America/Bogota`（全年 UTC-05:00，无 DST） |

> **建议用 `America/Bogota`** —— 偏移全年恒定，跨机器跨年份都不会因 DST 切换而偶发失败。
> 这一条是对许清楚 §2 风险表第 1 行的加固（原文：「断言必须选参考时刻偏移不同的两个时区」）。

**实验结构（三段，对应 B0 的自证要求）**：

```
=== EXP0 基线自证：同一时区，两个 Isolate 读相同偏移 ===
    设 Asia/Tokyo → refresh(A) → refresh(B) → 两者偏移必须相等
    ⇒ BASELINE_VALID=true    ← 若这条不成立，后面全部结论不可信

=== EXP1 时区隔离：两个 Isolate，两个时区 ===
    Isolate A: set Asia/Tokyo     → refresh(A) → 读 A 的偏移
    Isolate B: set America/Bogota → refresh(B) → 读 B 的偏移
    ⇒ TZ_ISO_A / TZ_ISO_B / TZ_READ_A / TZ_READ_B
    ⇒ TZ_PER_PROFILE_ISOLATED=true|false

=== EXP2 process-global 直接证据 ===
    设 Asia/Tokyo → 读 B → 改全局为 America/Bogota → refresh(B) → 再读 B
    ⇒ GLOBAL_LEAK_OBSERVED isolate=B tz_before=X tz_after=Y
      （B 的读数变了 ⇒ 时区是进程级，两个 Isolate 无法各持一份）

=== EXP3 时间值本身 ===
    在 EXP1 的两个 Isolate 里各跑 `Date.now()`，与宿主 `Instant` 对比
    ⇒ DATE_NOW_ISOLATED=true|false
    （预期：true|false 都可能，但必须给二值 + 证据）

=== EXP4 替代路线：JS 侧 Date 兼容层（最小可行性） ===
    宿主喂两个固定逻辑时刻，JS 侧替换全局 Date
    ⇒ DATE_SHIM_WORKS=true|false
```

**读时区的 JS 表达式（宿主必须用它，不能自己算）**：

```javascript
// 在 V8 里执行，读的是 V8 自己的时区数据库与时钟
(function () {
  var d = new Date(T_REF);           // 固定参考时刻，非 "now"
  return d.getTimezoneOffset();      // 返回 UTC - local 的分钟数（注意符号）
})()
```

**关键设计点（对齐 B7 的禁令）**：
**绝对禁止**用 `v8::Date::new(scope, f64)` 来测时钟隔离。理由已核实：
`Date::new` 的实现是 `v8__Date__New(context, value)` —— **宿主喂什么值就是什么值**，
**根本不读 V8 时钟**。用它测出来的一定是「隔离成功」，是**假绿**。
正确做法是执行 JS 源码 `new Date(T_REF).getTimezoneOffset()`，
让 **V8 自己**去查它内部的时区缓存。

**探针源码结构骨架**（工程师照此写）**：

```rust
// 关键：一个函数跑一个 Isolate，读回时区偏移分钟数
fn tz_offset_in_isolate(tz: &str, ref_ms: f64) -> Result<i32, String> {
    // 1. v8::icu::set_default_time_zone(tz) → 必须检查 bool 返回值！
    //    false ⇒ 时区不可识别 ⇒ 返回 Err("TZ_SET_REJECTED")，不得静默继续
    //    （已核实：Etc/Unknown 会被拒；远程 Linux 若 tzdata 缺失也会失败）
    // 2. let mut isolate = v8::Isolate::new(...);
    // 3. let scope = &mut v8::HandleScope::new(&mut isolate);
    // 4. let context = v8::Context::new(scope, Default::default());
    // 5. let scope = &mut v8::PinScope::new(scope, context);
    // 6. run_script(scope, "new Date(T_REF).getTimezoneOffset()") → 读 Number
    // 7. **必须在 isolate 销毁前** 调
    //    isolate.date_time_configuration_change_notification(v8::TimeZoneDetection::Skip);
    //    （icu.rs 文档原文要求：设完时区后通知每个 isolate 用 Skip 刷新缓存
    //      而不重新探测，否则会把刚设的时区覆盖掉）
}
```

> **这是一个极易踩的坑，必须写进 ADR**：`set_default_time_zone` 的文档原文说
> 「Afterwards, notify each isolate via `date_time_configuration_change_notification` with
> `TimeZoneDetection::Skip` so cached values are refreshed **without re-detecting
> (and overwriting) the zone just set here**」。
> 即：通知时如果传 `Redetect`，V8 会**重新探测并覆盖**宿主刚设的时区 ⇒ 测出来「隔离失败」
> 其实是**通知参数用错**。**必须传 `Skip`。**

### 1.5 断言清单（逐条二值）

| 编号 | 断言 | 期望输出 | 判据 |
|---|---|---|---|
| **B0** | 基线自证 | `BASELINE_VALID=true` | 同一时区下两个 Isolate 读数**相等**。若不等 ⇒ 测量方法本身坏了，**整份结论作废**（对齐 ADR-0007 的 `BASELINE_VALID` 范式） |
| **B1** | 输出两时区与偏移 | `TZ_ISO_A=Asia/Tokyo` / `TZ_ISO_B=America/Bogota` / `OFF_A=-540` / `OFF_B=300` | 两偏移**不相等**（符号约定：`getTimezoneOffset()` 返回 UTC−local，Tokyo 是 −540，Bogota 是 +300） |
| **B2** | 二值隔离结论 | `TZ_PER_PROFILE_ISOLATED=true\|false` | 由 EXP1 直接得出 |
| **B3** | process-global 证据 | `GLOBAL_LEAK_OBSERVED isolate=B tz_before=300 tz_after=-540` | 若 `false`，必须出现此行**且两个值不同**。**这一条是结论可信度的核心**（许清楚原文） |
| **B4** | 时间值隔离 | `DATE_NOW_ISOLATED=true\|false` + 证据行 | 若 false，附 process-global 证据 |
| **B5** | 替代路线 | `DATE_SHIM_WORKS=true\|false` | 替换全局 `Date` 后，宿主喂 `t0` 时 `Date.now()===t0` 且喂 `t1` 时变为 `t1` |
| **B6** | 下一条候选路线 | 由 ADR 落档 | 若 B5 也 false，ADR 必须写技术方案 §6 的另一半「薄 V8 时间桥作为独立构建配置」 |
| **B7** | **禁用 `Date::new(scope,f64)` 测时钟** | 探针源码中无此调用 + ADR 原话记录此坑 | **代码评审 + ADR 落档**。探针注释里必须写明「`Date::new` 是宿主喂值，不经 V8 时钟，用它测必然假绿」 |
| **B8** | 落 ADR | `docs/adr/0010-*.md` | 结构对齐 ADR-0006/0007 |
| **B9** | 汇总标记 | `T2_2_PROBE_OK` / `T2_2_PROBE_FAIL` | 任一断言失败 ⇒ `FAIL` |

**额外必须覆盖的分支（许清楚 P1-4）**：
`TZ_SET_REJECTED` 分支 —— 显式试一次 `set_default_time_zone("Etc/Unknown")`，
断言它**返回 `false`**（已核实文档：ICU 自己的 unknown id 会被拒）。
这条断言输出 `TZ_REJECT_BRANCH_WORKS=true`，用于证明探针**检查了返回值**。

### 1.6 预期输出格式（完整示例，含假想结论）

```
=== EXP0 基线自证：同一时区两个 Isolate ===
  isolate=A tz=Asia/Tokyo offset=-540
  isolate=B tz=Asia/Tokyo offset=-540
BASELINE_VALID=true

=== EXP1 时区隔离 ===
TZ_ISO_A=Asia/Tokyo
TZ_ISO_B=America/Bogota
TZ_READ_A=-540
TZ_READ_B=300
ASSERT_FAIL(e): 两 Isolate 读到不同偏移
TZ_PER_PROFILE_ISOLATED=true

=== EXP2 process-global 直接证据 ===
GLOBAL_LEAK_OBSERVED isolate=B tz_before=300 tz_after=-540
ASSERT_PASS(f): 改变全局设置后，另一个 Isolate 的读数也变了 —— 时区是进程级

=== EXP3 时间值 ===
DATE_NOW_DELTA_A_MS=3
DATE_NOW_DELTA_B_MS=4
DATE_NOW_ISOLATED=true

=== EXP4 替代路线：JS 侧 Date 兼容层 ===
SHIM_T0_OK=true
SHIM_T1_OK=true
DATE_SHIM_WORKS=true

=== TZ 设置失败分支 ===
TZ_SET_ETc_UNKNOWN_REJECTED=true
TZ_REJECT_BRANCH_WORKS=true

ASSERT_PASS(a): 基线自证成立 —— 测量方法非恒真
ASSERT_PASS(b): 两时区偏移不同（-540 vs +300）
ASSERT_PASS(d): set_default_time_zone 对 Etc/Unknown 返回 false —— 探针检查了返回值
ASSERT_PASS(g): 已避开 Date::new(scope,f64) 作为时钟证据
T2_2_PROBE_OK
```

> 注意 `T2_2_PROBE_OK` 与 `TZ_PER_PROFILE_ISOLATED=true` **不矛盾**：
> 前者是「探针全部断言通过」，后者是「两个 Isolate 各自看到了自己的时区」。
> 而 EXP2 证明「改全局会影响另一个」——**这两件事同时为真，正是 process-global 的正确表述**。
> **ADR 里必须把这个区分讲清楚**，否则维护者会以为「隔离成功就不用管全局了」。

### 1.7 通过 / 不通过判据

| 结论 | 条件 |
|---|---|
| **探针有效** | `BASELINE_VALID=true` 且 `T2_2_PROBE_OK` |
| **隔离成立** | `TZ_PER_PROFILE_ISOLATED=true` |
| **隔离不成立**（**这是本 PoC 预期的结论，不是失败**） | `TZ_PER_PROFILE_ISOLATED=false` 且 `GLOBAL_LEAK_OBSERVED` 行的前后值**不同** |

**重要**：许清楚 §2 待确认 2 已指出——「若技术方案 §19 的『按 Profile/时区划分进程池』是
最终形态，那么 T2.2 的正确结论可能是『**同进程内不保证隔离，隔离靠进程边界**`，
即 PoC 应当**测出并接受 process-global**，而不是把它当失败」。
**因此 `TZ_PER_PROFILE_ISOLATED=false` 是可接受的、有价值的结论，只要证据链完整。**

### 1.8 不通过时的替代路线

| 若 | 则 |
|---|---|
| `TZ_PER_PROFILE_ISOLATED=false`（预期） | 架构级答案 = **一个 Profile 一个进程**，或**按时区分进程池**（技术方案 §19）。ADR 落档为架构结论 |
| `DATE_SHIM_WORKS=false` | 走技术方案 §6 的另一半：「以**薄 V8 时间桥**作为独立构建配置」。ADR 必须写明该候选 |
| `TZ_SET_REJECTED` 在远程 Linux 出现 | **这是环境问题不是结论**：走 `AGENTS.md` 的决策上报协议评估是否装 tzdata，**不许自行改远程系统配置**（`AGENTS.md:67`）。ADR 必须记录该分支在本地通过、在远程因环境缺失而失败，并标注「非机制问题」 |
| `BASELINE_VALID=false` | **测量方法坏了**，整份结论作废。必须先修方法再重跑，**禁止**在此状态下写 ADR |

### 1.9 边界（不做）

- 不实现 Profile 的 serde schema / `get_defaults` / 增量覆盖。
- 不做完整 `Date` 兼容层的描述符/构造/解析差分（那是 G1/G2）。
- 不碰 `Intl` 生态、不调 `set_common_data_78`。
- **不改进程 `TZ` 环境变量作为解决方案**（技术方案 §6 明确「避免并发修改进程 TZ」）。
  若探针为实验临时设 `TZ`，ADR 必须标注为「**实验手段，非候选方案**」。
- 不做跨进程/多线程压测（单进程多 Isolate 的机制验证）。

---

## 2. T2.4 —— Blitz 动态 DOM / 样式失效 / Shadow DOM

### 2.1 已核实的事实（**含一次真实编译**，工程师不必重做可行性判断）

**这是本轮最重要的核实结果，直接改变了 T2.4 的成本结构与结论预期。**

**(a) `blitz-dom 0.3.0-beta.2` 在本机可离线编译通过。**
架构师实测（`cargo run --offline`，`default-features = false`）：

```
   Compiling blitz-dom v0.3.0-beta.2
   Compiling blitzcheck v0.0.0 (/private/tmp/blitzcheck.sEyL0W)
error[E0782]: expected a type, found a trait
 --> src/main.rs:3:36
  |   let _t = std::any::type_name::<blitz_dom::Document>();
  |                      ^^^^^^^^^^^^^^^^^^^^
help: you can add the `dyn` keyword if you want a trait object
```

⇒ **`blitz-dom` 及其 191 个传递依赖全部编译通过**（唯一错误是**我的探针代码**把
`Document` 当成了 struct，而它其实是 **trait**）。
这证明：**依赖可获取性（C1 在 macOS 侧）已成立**，且 `Document` 的真实形态是
`pub trait Document`（`src/document.rs:130`）。

**(b) `default-features = false` 是必须的（否则违反技术方案 §3）。** 已核实
`blitz-dom-0.3.0-beta.2/Cargo.toml` 的 `[features]`：`default = ["svg", "woff",
"accessibility", "system-fonts", "file-input", "custom-widget"]`，而
`custom-widget = ["dep:anyrender", "accessibility"]`。实测对比：

| 配置 | 锁定的包数 | 是否含 GUI 栈 |
|---|---|---|
| 默认 features | 243 | **含 `anyrender`** ✗ |
| `default-features = false` | **191** | **无** ✓ |

⇒ **探针的 `Cargo.toml` 必须写 `default-features = false`**，否则 C2
（不依赖 `blitz`/`blitz-shell`/`blitz-paint`/`anyrender`）无法通过。

**(c) Shadow DOM 在 blitz-dom 中明确未实现 —— 这是 C9 的确定性答案。**
已核实 `blitz-dom-0.3.0-beta.2/src/stylo.rs` 原文：

```rust
impl<'a> TShadowRoot for BlitzNode<'a> {
    type ConcreteNode = BlitzNode<'a>;
    fn as_node(&self) -> Self::ConcreteNode { self }
    fn host(&self) -> <Self::ConcreteNode as TNode>::ConcreteElement {
        todo!("Shadow roots not implemented")        // ← 第 231 行
    }
    fn style_data<'b>(&self) -> Option<&'b style::stylist::CascadeData> {
        todo!("Shadow roots not implemented")        // ← 第 238 行
    }
}
```
```rust
fn as_shadow_root(&self) -> Option<Self::ConcreteShadowRoot> {
    // TODO: implement shadow DOM
    None                                               // ← 第 311-313 行
}
```

另已核实 `selectors::Element for BlitzNode` 中：

```rust
fn parent_node_is_shadow_root(&self) -> bool { false }      // 第 337 行
fn containing_shadow_host(&self) -> Option<Self> { None }    // 第 341 行
```

以及一条**决定性的上游限制**（`src/stylo.rs:319-327` 注释原文）：

> "This correctly uses a unique id for the `OpaqueElement` … However, it makes it
> impossible for us to 'rehydrate' the `OpaqueElement` back into an actual Element
> which is **required to implement the `implicit_scope_for_sheet_in_shadow_root`
> method** … This is required to enable and use the Shadow DOM functionality in Stylo."

**⇒ C9（Shadow DOM）在 `blitz-dom 0.3.0-beta.2` 上必然为 `false`。**
而许清楚 C11 明确：`SHADOW_DOM_WORKS=false` ⇒ **T2.4 不通过** ⇒ 触发切换到
Stylo + Taffy + Parley。

**这不是探针的失败，而是 T2.4 的裁决结论。** 按技术方案 §8 原文：
「若动态 DOM、Shadow DOM、样式失效或 GC 对接不能通过 PoC，则切换为直接
Stylo + Taffy + Parley 适配；业务接口保持不变。**该替代会增加集成工作，应视为技术闸门
结果，不以「已有轮子』掩盖成本。**」

**(d) 动态 DOM 与样式失效的入口已定位（探针要用的 API）。**

| 需要什么 | 用哪个 API（位置已核实） |
|---|---|
| 构造文档 | `BaseDocument::new(DocumentConfig)`（`src/document.rs:385`），`DocumentConfig` 字段见 `src/config.rs:33` |
| **串行样式遍历（避开 rayon）** | `DocumentConfig { style_threading: StyleThreading::Sequential, .. }`（`src/config.rs:20-29`）—— **这回答了许清楚 §3 待确认 5（`rayon` 与单一页面线程是否冲突）**：用 `Sequential` 即绕过全局 rayon 线程池；且 `Parallel` 的文档明说「panics if another `Parallel` resolve is in flight on a different thread」 |
| 变更 DOM | `doc.mutate()` → `DocumentMutator`（`src/document.rs:627`；`src/mutator.rs:79`），有 `create_element` / `append_children` / `set_attribute` / `set_style_property` / `remove_node` / `insert_nodes_before` 等 20+ 方法 |
| 触发样式+布局重算 | `BaseDocument::resolve(&mut self, current_time_for_animations: f64)`（`src/resolve.rs:39`，注释原文「Restyle the tree and then relayout it」） |
| 读布局 | `BaseDocument` 内部走 `taffy::compute_root_layout`（`src/resolve.rs:368`），`hit(x,y)` 可作间接读数（`src/document.rs:1549`） |

**(e) `markup5ever` 分叉见 §0 的 Q-3 决策：`BREAKING`，T2.4 不与 html5ever 联调。**

### 2.2 依赖选型

```toml
[workspace]

[package]
name = "blitzprobe"
version = "0.0.0"
edition = "2021"

[dependencies]
blitz-dom     = { version = "=0.3.0-beta.2", default-features = false }
blitz-traits  = "=0.3.0-beta.2"     # 见下方说明：blitz-traits 无 [features] 段
markup5ever   = "0.39"               # 与 blitz-dom 版本对齐；仅用于构造 QualName
```

> **为什么 `blitz-traits` 不写 `default-features = false`**：已核实
> `blitz-traits-0.3.0-beta.2/Cargo.toml` **没有 `[features]` 段**。
> 写 `default-features = false` 在此**无害但也无意义**（架构师实测：`cargo
> generate-lockfile --offline` 仍成功锁定 191 个包，与不写时完全一致）。
> 为了不误导工程师以为它做了实事，**统一不写**，并在 ADR 记录「blitz-traits 无特性可关」。

- **版本必须 `=` 精确锁定** `0.3.0-beta.2`：beta 的 semver 不保证 API 稳定，
  且 `blitz-dom` 自身用 `=0.3.0-beta.2` 锁 `blitz-traits` 与 `stylo_taffy`（已核实）。
- **`edition = "2021"`** 而非仓库的 2024：`blitz-dom` 的 `rust-version = "1.89.0"`（已核实），
  且探针独立 workspace，不受主 workspace 的 `edition 2024` 约束。
- **不加 `blitz` 顶层 / `blitz-html`**（技术方案 §3 硬要求；`blitz-html` 会拖入 html5ever 0.39）。

### 2.3 探针目录与文件

```
poc/t2_4_blitz_probe/
├── Cargo.toml
├── README.md
└── src/
    └── main.rs      # 单文件；三组实验 + 断言
```

### 2.4 逐条断言设计（对齐 C1~C17）

**实验结构**：

```
=== EXP0 依赖形态 ===
    输出锁定版本：blitz-dom / stylo / stylo_dom / taffy / parley / markup5ever
    读 Cargo.lock 断言：无 blitz / blitz-shell / blitz-paint / anyrender
⇒ TARGET_LINUX_OK（本地侧记 TARGET_MAC_OK）与 NO_GUI_DEP

=== EXP1 动态 DOM（连续变更 → 读数变化）===
    1. 建文档，插入 <div id=box style="width:100px;height:50px">
    2. resolve → 读 box 布局宽度            ⇒ W0
    3. mutate: set_style_property("width", "200px")
    4. resolve → 再读宽度                    ⇒ W1
    5. 断言 W1 == 200 且 W1 != W0             ⇒ DYNAMIC_DOM_WORKS
    6. mutate: set_attribute("class", "big")
    7. resolve → 再读                          ⇒ W2
    8. 断言变化反映第 6 步                     ⇒ STYLE_INVALIDATION_WORKS

=== EXP2 NodeId 稳定性 ===
    在 EXP1 的变更序列中，每步前后都读同一批 NodeId
    断言：NodeId 未被回收后复用（blitz 的 NodeId 是 slot+version 打包，
           已核实 blitz-traits/src/node_id.rs:5-8）
⇒ NODE_ID_MAP_CONSISTENT

=== EXP3 后端树只是缓存 ===
    用 DocumentMutator::remove_node 删掉全部后端节点
    再从规范 DOM 重建（重新 create_element + append）
    断言布局读数与删除前一致                   ⇒ BACKEND_IS_CACHE

=== EXP4 Shadow DOM（预期 false，但仍必须跑）===
    尝试挂 shadow root（在 blitz-dom 上预期无法完成）
    ⇒ SHADOW_DOM_WORKS=false
```

**关键设计点**：

1. **C9 的断言必须真去尝试，不能因为「源码写了 todo!()」就直接跳过。**
   理由：ADR-0007 的方法论是「结论必须实测，不能推断」。若工程师直接写
   `SHADOW_DOM_WORKS=false`，这条断言就是**恒假**的门禁——**无法区分
   「确实不支持」与「没测出来」**。必须真去调 API，若 panic 则捕获并记 `false` +
   记 panic 消息。**这是本探针最容易做成假绿的地方。**
2. **C1 的 Linux 侧必须真在远程跑**（`AGENTS.md:72` 跨平台纪律第 4 条）。
   `blitz-dom` 的唯一平台相关依赖 `objc2` 的 target 是 `cfg(any(target_vendor = "apple"))`
   （已核实），**从依赖图看 Linux 应可编译，但未实测**（许清楚 §8.4 也标为待核实）。
3. **不与 html5ever 联调**（Q-3 决策）。若工程师想用 HTML 解析器建初始 DOM，
   **改用 `DocumentMutator` 的 `create_element` / `append_children`**，
   不设 `html_parser_provider`（`DocumentConfig` 该字段为 `Option`，已核实）。

### 2.5 断言清单

| 编号 | 断言 | 期望输出 | 判据 |
|---|---|---|---|
| **C1** | Linux 可用性 | `TARGET_LINUX_OK=true\|false` | 本地侧记 `TARGET_MAC_OK`；**远程 `~/mmv8` 侧才是 C1 的真答案** |
| **C2** | 不依赖 GUI | `NO_GUI_DEP=true` | 断言 `Cargo.lock` 中无 `blitz`/`blitz-shell`/`blitz-paint`/`anyrender`/`anyrender_vello`/`winit`/`accesskit`/`usvg`/`wuff`/`tracing`（已核实：`default-features = false` 时这些全部不入树） |
| **C3** | 双机跑通 | 两侧输出均入台账 | QA 回写 `docs/TASKS.md` |
| **C4/C5** | html5ever 分叉 | `HTML5EVER_VERSION_FORK_IMPACT=BREAKING` | **由架构师已定**（§0 Q-3，含 (a)(b)(c) 三处源码级证据）。探针**不重跑**，此项由 ADR 承担 |
| **C6** | 动态 DOM | `DYNAMIC_DOM_WORKS=true\|false` | EXP1 的 W1==200 且 W1!=W0 |
| **C7** | 样式失效 | `STYLE_INVALIDATION_WORKS=…` / `LAYOUT_INVALIDATION_WORKS=…` | style 级（class 变更→重算样式）与 layout 级（width 变更→重算布局）两级分别判定 |
| **C8** | NodeId 映射 | `NODE_ID_MAP_CONSISTENT=…` | 变更序列中 NodeId 不漂移 |
| **C9** | Shadow DOM | `SHADOW_DOM_WORKS=…` | **预期 `false`**（已核实 `todo!("Shadow roots not implemented")`） |
| **C10** | 后端树只是缓存 | `BACKEND_IS_CACHE=…` | 删后端可重建且读数一致 |
| **C11** | **不通过裁决** | `T2_4_VERDICT=KEEP_BLITZ\|SWITCH_TO_STYLO` | 任一 of {C1, C6, C7 两项, C8, C9} 为 false ⇒ `SWITCH_TO_STYLO` |
| **C12** | 成本判据 | ADR 给出量化依据 | 「需要改动的 crate 数 + 需要重跑的 PoC 数」，不接受「感觉更麻烦」（许清楚原文） |
| **C13** | 差异清单 | ADR 列出需自建能力 | 技术方案 §8 原文列举：DOM→样式树→box/fragment tree 转换、匿名盒、inline formatting、字体回退、滚动、绝对/固定定位、transform、hit testing、table/float/writing-mode/复杂分页、SVG 几何、替换元素、表单控件 |
| **C14** | 接口不变 | ADR 给出验证方式 | `LayoutBackend` 的对外接口不随路线变化 |
| **C15** | 不掩盖成本 | ADR 记录成本依据 | 原文引用技术方案 §8 的「不以已有轮子掩盖成本」 |
| **C16** | 落 ADR | `docs/adr/0011-*.md` | 结构对齐 ADR-0006/0007 |
| **C17** | 汇总标记 | `T2_4_PROBE_OK` / `T2_4_PROBE_FAIL` | 注意：**`PROBE_OK` 指「探针自身全部断言跑完且判定方法有效」**，与 `T2_4_VERDICT` 是两件事——`PROBE_OK` + `VERDICT=SWITCH_TO_STYLO` 是**合法且预期**的组合 |

### 2.6 预期输出格式

```
=== EXP0 依赖形态 ===
LOCKED blitz-dom=0.3.0-beta.2 stylo=0.20.0 stylo_dom=0.20.0 taffy=0.14.0 parley=0.11.1 markup5ever=0.39.0
NO_GUI_DEP=true
TARGET_MAC_OK=true
TARGET_LINUX_OK=false          ← 待远程实测填入

=== EXP1 动态 DOM ===
LAYOUT_W0=100.0
STYLE_CLASS_BEFORE=false
STYLE_CLASS_AFTER=true
LAYOUT_W1=200.0
ASSERT_PASS(a): set_style_property 后重算反映前一次修改 —— 动态 DOM 成立
ASSERT_PASS(c): class 变更后样式重算生效 —— style 失效成立

=== EXP2 NodeId 稳定性 ===
NODE_ID_STABLE=true

=== EXP3 后端树只是缓存 ===
BACKEND_IS_CACHE=true

=== EXP4 Shadow DOM（源码已知 todo!()）===
SHADOW_ATTACH_PANIC="not implemented: Shadow roots not implemented"
ASSERT_FAIL(e): blitz-dom 0.3.0-beta.2 未实现 shadow root（todo!() at stylo.rs:231/238）
SHADOW_DOM_WORKS=false

=== 裁决 ===
T2_4_VERDICT=SWITCH_TO_STYLO
T2_4_PROBE_OK
```

### 2.7 通过 / 不通过判据

| 结论 | 条件 |
|---|---|
| **走 Blitz**（`KEEP_BLITZ`） | C1 ∧ C6 ∧ C7(style) ∧ C7(layout) ∧ C8 ∧ C9 **全部为 true** |
| **切 Stylo + Taffy + Parley**（`SWITCH_TO_STYLO`） | 上列任一为 false。**按已核实事实，C9 必为 false ⇒ 此为预期结果** |
| **探针无效** | `T2_4_PROBE_FAIL`（方法本身坏了，如远程 OOM）——此时**不得出裁决** |

### 2.8 不通过时的替代路线（本项目前已可写）

因 C9 已在源码层确证为 false，替代路线的 ADR 要点**可以现在就写**：

- **`LayoutBackend` 的对外接口保持不变**（C14）——这是技术方案 §8 的硬要求，
  所以「切路线」不动上层 crate 的边界，只换 `layout-backend` 的内部实现。
- **需自建能力清单**（C13）直接引用技术方案 §8 原文列举的 12 项。
- **成本估计的依据**（C15）：可量化项包括
  「新增 crate 数」「需自建的布局环节数」「需重跑的 PoC 数」
  （T2.3-bis 无论如何都要跑，见 §0 Q-3 后续任务）。
- **一个重要的架构影响**：`blitz-dom` 已经**自带** Stylo + Taffy + Parley
  （已核实依赖树含 `stylo 0.20` / `stylo_taffy` / `taffy 0.14` / `parley 0.11`）。
  切「直连 Stylo 路线」意味着**这些依赖仍需引入，只是不再经由 blitz-dom 的 DOM 抽象**。
  ⇒ **切路线省掉的是「blitz 的 DOM 抽象层」，不是「样式与布局依赖」**。
  这一条必须写进 ADR，否则维护者会误以为切换能省掉整个依赖树。

---

## 3. T2.5 —— V8 Inspector 暂停消息泵与 Python 回调死锁边界

### 3.1 已核实的事实（原文）

| 事实 | 原文/位置 |
|---|---|
| **V8 暂停时同步回调宿主** —— 这是死锁入口 | `src/inspector.rs:583-586`：`pub trait V8InspectorClientImpl { fn run_message_loop_on_pause(&self, context_group_id: i32) {} ... }` —— **默认空函数体** |
| 解除暂停 | `src/inspector.rs:537` `quit_message_loop_on_pause(&self)` |
| 协议通道 | `src/inspector.rs:449-453` `pub trait ChannelImpl { fn send_response(&self, call_id: i32, message: UniquePtr<StringBuffer>); fn send_notification(&self, message: UniquePtr<StringBuffer>); fn flush_protocol_notifications(&self); }` |
| 程序化暂停（不需外部调试器） | `src/inspector.rs:908` `schedule_pause_on_next_statement(&self, reason: StringView, detail: StringView)`；`:924` `cancel_pause_on_next_statement()` |
| 构造链 | `V8Inspector::create(isolate: &mut Isolate, client: V8InspectorClient)`（`:1246`）→ `V8Inspector::connect(context_group_id, channel, state, client_trust_level)`（`:1270`）→ `V8InspectorSession` |
| **pinned 生命周期（UB 来源）** | `src/inspector.rs:656-657` 注释原文：「V8 will hold onto a raw pointer to the `RawV8InspectorClient`, so we need to make sure it stays **pinned**」；`V8InspectorClientHeap` 与 `ChannelHeap` 均含 `PhantomPinned`（`:660`、`:395`） |
| 构造本身已在 crate 内用 `Box::into_pin` 处理了 pinned | `V8InspectorClient::new`（`:680`）、`Channel::new`（`:407`）——**探针侧无需手工 unsafe 处理 pin** |
| 跨线程访问 Isolate | `src/locker.rs` 模块文档：`v8::Locker` 支持「one thread at a time」 |
| 终止执行（watchdog 用） | `src/isolate.rs:1066` `terminate_execution(&self) -> bool`；`:1073` `cancel_terminate_execution` |
| 微任务 | `src/isolate.rs:1970` `perform_microtask_checkpoint(&mut self)` |

> 术语：`StringBuffer` ≈ V8 侧用引用计数持有的字符串容器，`UniquePtr<StringBuffer>`
> ≈ 「移交所有权的智能指针」。`PhantomPinned` ≈ 「类型系统层面禁止移动这个值」。

### 3.2 依赖选型

```toml
[workspace]

[package]
name = "deadlockprobe"
version = "0.0.0"
edition = "2021"

[dependencies]
v8 = "152.2.0"
pyo3 = { version = "0.23", features = ["extension-module"] }
```

> `extension-module` 是 PyO3 的标准 feature；**若链接报错，去掉它**并在 ADR 记录
> （PyO3 在 `extension-module` 与非 `extension-module` 下的链接行为不同，属已知坑）。
> **具体版本以 `cargo` 解析结果为准**，ADR 记录实际解析值。

**明确不引入**：任何 async 运行时（tokio 等）。T2.5 是「单页面线程 + 必要第二线程」的
最小边界，用 `std::thread` 即可。引入 tokio 会让「单线程模型」这个前提失效。

### 3.3 探针目录与文件

```
poc/t2_5_inspector_deadlock/
├── Cargo.toml
├── README.md
└── src/
    ├── main.rs        # 实验编排 + 断言
    ├── inspector.rs   # ChannelImpl / V8InspectorClientImpl 的探针实现
    ├── watchdog.rs    # D0 的超时机制
    └── pyside.rs      # PyO3 侧：构造 GIL 互等的最小交错
```

### 3.4 **D0 前置：超时机制必须先被证明有效**（这是本 PoC 的第一验收项）

**为什么这条不可省**（许清楚 D0 原文）：死锁在测试里表现为「**测试挂住**」而不是「测试失败」。
若超时机制不可靠，本 PoC 的所有「不死锁」结论都无法与「没测出来」区分。

**D0 的设计**：

```
=== D0 自证：故意挂起 → watchdog 必须把它报告为结论 ===
    1. 造一个「必然挂起」的场景：
       run_message_loop_on_pause 的实现体 = 无限循环 { std::thread::sleep(1ms) }
       （不进 quit 分支 ⇒ 永远不返回）
    2. 主线程设 watchdog：T0_WATCHDOG_MS = 3000（远程 6000，见下）
    3. 到时后：
       - 打印 WATCHDOG_FIRED=true
       - 调用 isolate.terminate_execution()（已核实存在）
       - 从挂起线程发出退出信号
       - 打印 HUNG_SCENARIO_REPORTED=DIMAGINATION（或该场景的真实分类）
    4. 进程以 T2_5_PROBE_OK 正常退出（**不是被 kill**）
```

**超时机制的实现方式（架构师指定，避免工程师各写一套）**：

| 要素 | 定法 |
|---|---|
| 超时时长 | 本地 `3000ms`；远程 `6000ms`（远程 3.6 GB 机器慢，`AGENTS.md:67`）。用环境变量 `IV8_PROBE_WATCHDOG_MS` 覆盖 |
| 时钟 | `std::time::Instant`（单调时钟，不受系统时间调整影响） |
| 执行位置 | **独立 watchdog 线程**，不是宿主线程（回答许清楚 §7 **Q-9**：技术方案 §6 说「执行超时由**独立** watchdog 请求 V8 终止」——故 watchdog 是独立线程，**本 ADR 定此**） |
| 中断手段 | `Isolate::terminate_execution()`（`src/isolate.rs:1066`）+ 置一个 `AtomicBool` 让挂起的泵循环自行退出 |
| 兜底 | 若 `terminate_execution` 后仍未在 `2× watchdog` 内退出，**打印 `WATCHDOG_ESCALATED=true` 后 `std::process::exit(70)`** —— 让「探针自己抓不到自己」表现为**明确的非零退出码**，而不是 CI 挂住 |

> **这一点很重要**：CI 挂住与「探针报告超时」在证据上是两件事。
> 兜底 `exit(70)` 让 QA 能区分「探针正常完成并报告 TIMEOUT」与「探针自己都救不回来」。

#### ⚠️ `terminate_execution` 的线程语义 —— 已核实的**权威原文**（勿凭直觉改）

本节纠正一处流传中的**相反说法**：「`terminate_execution` 只能在执行线程调用、
同线程调用无效」是**错的**，与 V8 源码原文直接矛盾。

`v8-152.2.0/src/isolate.rs:2608-2621`（`IsolateHandle::terminate_execution`，
`Isolate::terminate_execution` 的文档明确指向它）：

```rust
/// Forcefully terminate the current thread of JavaScript execution
/// in the given isolate.
///
/// This method can be used by any thread even if that thread has not
/// acquired the V8 lock with a Locker object.
///
/// Returns false if Isolate was already destroyed.
pub fn terminate_execution(&self) -> bool
```

`v8/include/v8-isolate.h:1270-1277`（C++ 侧同一段注释）：

> "Forcefully terminate the current thread of JavaScript execution in the given
> isolate. **This method can be used by any thread even if that thread has not
> acquired the V8 lock with a Locker object.**"

**正确的三条结论**：

| # | 结论 | 依据 |
|---|---|---|
| 1 | **可以从任意线程调用**，**包括未持有 V8 Locker 的线程** | 上两处原文均明写「can be used by **any thread** even if that thread has not acquired the V8 lock with a Locker object」 |
| 2 | 因此 **watchdog 线程正应当**直接调它，且**不得**先去抢 Locker —— 抢 Locker 会与执行线程互等，反而制造第二重死锁 | `src/locker.rs:1-4` 模块文档：「Support for sharing an isolate between threads, **one thread at a time**, via the `v8::Locker` API」 |
| 3 | 终止后若要继续调用引擎，需 `cancel_terminate_execution()` 恢复能力 | `src/scope.rs:1233-1235` 原文：「If `CanContinue` returns false and `HasTerminated` returns true, it is possible to call `CancelTerminateExecution` in order to continue calling into the engine.」 |

**对工程师的直接影响**：watchdog 线程持有一个 `IsolateHandle`（`isolate.thread_safe_handle()`，
`src/isolate.rs:1055-1058`）或 `Arc<Isolate>` 即可调用，**不需要 `v8::Locker`**。
若按相反说法实现（在 watchdog 里抢 Locker），会得到「watchdog 自己也挂住」的假超时，
且会把 D0 的自证结果污染成 `WATCHDOG_SELFTEST_OK=false`——**这是必须避免的假绿/假红**。

**D0 自证必须验证这一点**：故意挂起场景里，watchdog 应当**报告超时并让进程正常退出**；
若报告 `WATCHDOG_ESCALATED=true`（即 `terminate_execution` 没生效），
**先查是不是误加了 Locker**，而不是直接判定「终止 API 不够用」。

### 3.5 逐条断言设计（对齐 D0~D12）

**三个实验因子（许清楚 §4 Q2）**：

- **F1** 暂停期间宿主是否尝试获取 GIL：`no` / `yes`
- **F2** 宿主回调是否重入同一 Context 的 eval：`no` / `yes`
- **F3** 是否有第二线程参与：`none` / `msg_only` / `needs_locker`

**组合矩阵（2×2×3 = 12 组，但其中 4 组在本 PoC 边界内不可测，须显式说明）**：

| # | F1 | F2 | F3 | 可测 | 预期 |
|---|---|---|---|---|---|
| 1 | no | no | none | ✅ | `CLEAN_RESUME`（基准） |
| 2 | no | no | msg_only | ✅ | `CLEAN_RESUME` |
| 3 | no | no | needs_locker | ✅ | 待测（D6） |
| 4 | **yes** | no | none | ✅ | **本 PoC 的核心**：GIL 路径 |
| 5 | yes | no | msg_only | ✅ | 待测 |
| 6 | yes | no | needs_locker | ✅ | 待测（D4+D6 交叉） |
| 7 | no | **yes** | none | ✅ | 待测（D5）——技术方案 §6 明文「禁止」 |
| 8 | no | yes | msg_only | ✅ | 待测 |
| 9 | no | yes | needs_locker | ❌ | **不可测**：同一线程上 eval 会与 V8 的锁序死锁且无法归因（见下） |
| 10 | **yes** | **yes** | none | ❌ | **不可测**：两个因子同时为真时，无法归因是 GIL 还是重入导致的 |
| 11 | yes | yes | msg_only | ❌ | 同上 |
| 12 | yes | yes | needs_locker | ❌ | 同上 |

⇒ **`COMBOS_TRIED=9`**（12 − 3 组不可测）。D1 要求「**或明确说明为何某些组合不可测**」，
本表即为说明：**归因不可分离**（two-factor confound）。

**每组必须输出**（D2/D3 的成对证据与积极证据）：

```
COMBO[4] f1=yes f2=no f3=none => TIMEOUT
  RESUMED_AT=<never>
  CALLBACK_COMPLETED=false
  GIL_ACQUIRED=true
  GIL_HELD_BY=<tid>
```

**「成对证据」的具体保证**（D2）：上表组合 1（`CLEAN_RESUME`）与组合 4（`TIMEOUT`）
构成本 PoC 的**成对证据**。若实测下来 9 组**全部**是 `CLEAN_RESUME`，
**不能接受** —— 必须回到 D0 的挂起场景证明「本环境确实能观察到 TIMEOUT」，
否则输出 `COMBOS_DEADLOCK=0` + `WATCHDOG_SELFTEST_OK=true`，并在 ADR 写明
「在 9 种组合下未观测到死锁，**但不主张架构安全**」（许清楚 §4 风险表第 6 行原文）。

**三条死锁路径各自的定性**：

| 断言 | 对应 | 关键设计 |
|---|---|---|
| **D4（GIL 路径）** | F1=yes | PyO3 侧起一个线程 `py.allow_threads` 持锁，主线程在 `run_message_loop_on_pause` 里 `Python::with_gil(...)` 调一个 Python 函数。**构造互等**：Python 侧函数内 `time.sleep()` 等页面线程 |
| **D5（重入路径）** | F2=yes | 在 `run_message_loop_on_pause` 里对**同一 Context** 调 `scope.eval(...)`。技术方案 §6 明文「**禁止**回调中再次同步 eval 同一 Context」。**本 PoC 要给出实际后果**：死锁 / V8 FATAL / 可捕获异常 / 静默错误。**必须捕获 panic 并记录**（`std::panic::catch_unwind`），V8 FATAL 会直接终止进程 ⇒ 用独立子进程跑这一组 |
| **D6（锁序/Locker）** | F3=needs_locker | 第二线程用 `v8::Locker` 拿同一 Isolate。若不覆盖跨线程，D6 记「未覆盖」且**不得默认安全** |

**C 组（技术方案约束验证）**：

| 断言 | 设计 |
|---|---|
| **D7** 暂停期间普通页面任务未被推进 | 暂停前排一个 `setTimeout(f, 0)`（或 microtask）；暂停期间观察它**是否被触发**。⇒ `PAGE_TASKS_STALLED_DURING_PAUSE=true`。⚠️ **注意**：`v8` crate **不提供** `setTimeout`（那是宿主的事件循环能力），故须用 `setTimeout` 的**纯 JS 等价物**：`Promise.resolve().then(...)`（microtask）+ 一个宿主提供的标记函数。技术方案 §13 要求的是「不推进**普通页面任务**」，而 microtask 属于「V8 内部队列」。**这一条的具体形态待实测**，ADR 必须记录实测到的是 microtask 还是宿主任务 |
| **D8** 暂停可正常解除 | `quit_message_loop_on_pause()` 后 Isolate 可继续执行脚本 ⇒ `CLEAN_RESUME=true` + 解除后 `eval("1+1")==2` |
| **D9** unsafe 约束 | 每个 `unsafe` 紧邻 `SAFETY:`；`bash scripts/check-unsafe-whitelist.sh` 输出 PASS。**注意**：`V8InspectorClient::new` / `Channel::new` 的 pinned 处理**已在 v8 crate 内完成**（已核实 `Box::into_pin`），探针侧**应能零 unsafe**。若实测确实需要 unsafe，逐块 `SAFETY:` |

### 3.6 断言清单与预期输出

```
=== D0 watchdog 自证 ===
WATCHDOG_FIRED=true
HUNG_SCENARIO_REPORTED=DIMAGINATION
WATCHDOG_SELFTEST_OK=true
ASSERT_PASS(a): 超时机制把「必然挂起」报告为结论而非挂住进程

=== COMBO[1] f1=no f2=no f3=none ===
COMBO[1] => CLEAN_RESUME
  RESUMED_AT=12ms
  CALLBACK_COMPLETED=true
ASSERT_PASS(b): 组合 1 不死锁（正向证据）

=== COMBO[4] f1=yes f2=no f3=none ===
COMBO[4] => TIMEOUT
  GIL_ACQUIRED=true
  CALLBACK_COMPLETED=false
ASSERT_PASS(c): GIL 路径在 watchdog 内被报告为 TIMEOUT（与 b 成对）

=== COMBO[7] f1=no f2=yes f3=none ===
COMBO[7] => <实测：DEADLOCK|FATAL|ERROR_PROPAGATED|TIMEOUT>
  REENTRANT_RESULT=<实测>
ASSERT_PASS(d): 重入路径的实际后果已定性 —— 技术方案 §6「禁止」的代价已实测

COMBOS_TRIED=9
COMBOS_DEADLOCK=<n>
COMBOS_UNMEASURABLE=3
UNMEASURABLE_REASON=two-factor-confound(F1&F2 或 F2&F3 同时为真时无法归因)

=== C 组 ===
PAGE_TASKS_STALLED_DURING_PAUSE=<实测>
CLEAN_RESUME=true

T2_5_PROBE_OK
```

### 3.7 通过 / 不通过判据

| 结论 | 条件 |
|---|---|
| **探针有效** | `WATCHDOG_SELFTEST_OK=true` 且 `T2_5_PROBE_OK` |
| **边界测绘完成** | `COMBOS_TRIED=9` 且每组有一行分类结论 + 证据行 |
| **探针无效** | `T2_5_PROBE_FAIL` 或进程以 `70` 退出（watchdog 兜底）—— 此时**不得出任何架构结论** |

**关键**：`T2_5_PROBE_OK` **不代表「无死锁」**。它只代表「9 种组合的分类都测出来了」。

### 3.8 不通过时的替代路线

| 若 | 则 |
|---|---|
| PyO3 在远程装不上 | 拆两个探针（见 §0 决策的降级方案）；D4 记「未覆盖」，**不得默认安全** |
| D5 结果是 V8 FATAL（进程直接死） | 该组必须**独立子进程**跑（父进程用 `std::process::Command` 拉起并读退出码）；FATAL 本身就是一个**可判定的结论**，ADR 必须照实记录 |
| watchdog 兜底被触发（`exit(70)`） | 说明挂起场景连 `terminate_execution` 都救不回来。**这是关于「终止 API 是否够用」的重要结论**，必须写进 ADR 并上报（影响技术方案 §6 的 watchdog 设计） |
| 9 组全部 `CLEAN_RESUME` | **不接受**。必须回到 D0 证明环境能观察到 TIMEOUT，否则输出 `COMBOS_DEADLOCK=0` + 显式声明「不主张架构安全」 |

### 3.9 边界（不做）

- 不实现任何 CDP 域（技术方案 §13：「不能因为 WebSocket 连上就宣称完整 CDP」）。
- 不接 WebSocket / 外部调试器（用 `schedule_pause_on_next_statement`）。
- 不做 Worker / 多 Isolate 并发死锁测绘（属 G4/G6）。
- 不改 `sdk-python` / `debug` crate（均 T0 占位）。
- 不实现 watchdog 的生产形态（探针里的超时是**验证工具**；但 ADR 必须写明**如何迁移到生产**）。

---

## 4. 三项 PoC 的共同规范

### 4.1 输出风格（对齐 `t2_3_html5ever_pause`）

```rust
// 断言闭包（照抄既有探针的形态）
let mut ok = true;
let mut check = |cond: bool, tag: &str, pass: &str, fail: &str| {
    if cond { println!("ASSERT_PASS({tag}): {pass}"); }
    else    { println!("ASSERT_FAIL({tag}): {fail}"); ok = false; }
};
// ... 断言 ...
println!("T2_2_PROBE_{}", if ok { "OK" } else { "FAIL" });
```

### 4.2 独立 workspace（ADR-0008 决策 1）

每个 `poc/*/Cargo.toml` **顶部第一行**是空 `[workspace]`。
**理由原文**：「主 workspace 的 19 个成员名单由 `workspace_policy.rs` 精确锁定，
探针进去会破坏它；引入 `v8` 依赖会让每次门禁都要链接 V8（远程 3.6 GB 内存下有 OOM 风险）」。

### 4.3 unsafe 纪律（ADR-0008 决策 4）

- 每个 `unsafe` 代码行的**前 2 行内**必须有 `SAFETY:` 标记（守卫脚本规则 3 的 ±2 行窗口）。
- **禁止** crate 级 `#![allow(unsafe_code)]`（规则 2）。
- **禁止** `#[allow(unsafe_code)]` 修饰 `mod` / `impl`（规则 4）。
- **注意**：ADR-0009 决策 6 已指出，`v8` crate 内部已用 `Box::into_pin` 处理了
  Inspector 的 pinned 生命周期 ⇒ **T2.5 探针应能零 unsafe 实现**。若实测需要，
  逐块 `SAFETY:`。

### 4.4 `SAFETY:` 注释的写法（照抄 ADR-0008 决策 4 的原例）

```rust
// 长篇论证可以写在上面，但**必须在紧邻处保留一行 SAFETY: 标记**，
// 否则它落在 lineno-3、超出守卫的 ±2 行窗口，仍会被判违规。
// SAFETY: 上述论证成立 —— 本块无跨堆指针、无可变别名。
unsafe { /* … */ }
```

### 4.5 落 ADR 的三份

| 探针 | ADR 编号 | 核心章节（对齐 ADR-0006/0007） |
|---|---|---|
| T2.2 | `0010-*.md` | 问题 / 实测方法 / **判定方法（比结论更重要）** / 关键发现原文 / 结论 / 对架构的直接影响 / 附带的实测 API 约束 / 机器可验证 |
| T2.4 | `0011-*.md` | 同上 + **切换路线的差异清单 + 成本依据** |
| T2.5 | `0012-*.md` | 同上 + **条件表（行=组合，列=分类/证据）** + **未覆盖路径的显式列举** |

---

## 5. 对许清楚 §7 全部 12 个待确认问题的逐条回答

| # | 问题 | 归属 | **结论** | 落点 |
|---|---|---|---|---|
| **Q-1** | `bindings-webidl ↔ engine-v8` 依赖方向 | 架构师 | ✅ **已定**：`v8` 只在 `engine-v8/Cargo.toml`；`bindings-webidl` 与 `engine-v8` **都依赖同一个类型面 crate**（默认 `engine-v8/src/abi.rs`；若维护者批准新增成员则为 `crates/v8-abi`）。**两者的依赖方向结论不随 crate 位置改变** | ADR-0009 决策 1 + 方案 E |
| **Q-2** | `JsHandle` 是否重新引入 ADR-0007 的强引用环 | 架构师 | ✅ **已定：不会**。`JsHandle` 是槽位下标不是 GC 根；`web-dom` 类型面上不得出现 `JsHandle`；强引用环禁令四条 | ADR-0009 决策 5（C-1~C-4） |
| **Q-3** | html5ever 0.27 vs 0.39 采用哪个 | 架构师 | ✅ **已定：T2.2/T2.4 阶段继续用 0.27**，`HTML5EVER_VERSION_FORK_IMPACT=**BREAKING**`（三处源码级证据）。T2.4 不与 html5ever 联调。**若 T2.4 通过并决定采用 Blitz**，则必须另起 T2.3-bis 在 0.39 上重跑 a~e 并出 ADR-0006 修订版（铁律 2） | 本文 §0 Q-3 |
| **Q-4** | T2.5 是否引入 PyO3 | 架构师 | ✅ **已定：引入** `pyo3`。不引入则 D4（GIL 路径，最关键的死锁路径）无法测。已给降级方案（拆两个探针） | 本文 §0 Q-4 |
| **Q-5** | 「逻辑时钟」归属 crate；是否允许同进程混用多时区 | 架构师 | ✅ **已定**：① 逻辑时钟（`Clock`：`monotonic_now`/`epoch_origin`/`frame_epoch`）归 **`scheduler` crate**；② **同进程内禁止混用多时区**——时区隔离靠**进程边界**（技术方案 §19 的「按 Profile/时区划分进程池」是架构级答案）。T2.2 的正确产出是**测出并接受 process-global** | 本文 §1.8 |
| **Q-6** | `NodeId ↔ JsHandle` 映射表归属 | 架构师 | ✅ **已定：归 `engine-v8/src/node_map.rs`**。合并回答 §1 待确认 3 与 §3 待确认 4 | ADR-0009 决策 5（C-3） |
| **Q-7** | 采用 Blitz 是否需把 MSRV 1.85 → 1.89 | 架构师 | ⚠️ **待实测，且当前不需要**。`blitz-dom` 的 `rust-version = "1.89.0"`（已核实），但它只出现在 `poc/` 的**独立 workspace**（ADR-0008），**不进主 workspace** ⇒ 不触发主 workspace 的 MSRV 声明。若将来 Blitz 进 `layout-backend`，才需登记为 D-03 延续 | 本文 §2.2 |
| **Q-8** | 「受控的 Inspector 消息泵」的具体语义 | 架构师 | ✅ **已定最小语义**（本 PoC 范围内）：暂停期间**只泵 Inspector 协议消息与宿主显式标记的控制消息**；**不推进**普通页面任务、**不跑** microtask checkpoint、**不派发**网络事件。理由：技术方案 §13 明文「暂停期间页面线程运行受控的 Inspector 消息泵，**不推进普通页面任务，避免网络事件在暂停中任意改变页面**」。D7 验证其中的「不推进」半边 | 本文 §3.5（D7） |
| **Q-9** | 超时/截止期限由谁设 | 架构师 | ✅ **已定：独立 watchdog 线程**（技术方案 §6 原文「执行超时由**独立** watchdog 请求 V8 终止」），用 `Isolate::terminate_execution()` + `AtomicBool` 双保险，兜底 `exit(70)` | 本文 §3.4 |
| **Q-10** | iv8 兼容模式的「读取自动递增时间」是否属 T2.2 范围 | 架构师 | ✅ **已定：排除**。它与「Profile 间隔离」是**正交的两个语义轴**（技术方案 §13：logical/system 时钟是独立维度）。留在 T2.2 会让 PoC 膨胀 | 本文 §1.9 |
| **Q-11** | T2.4 是否需要 feature flag 使 Blitz 可选 | 架构师 | ⏸ **延后到 G3 打包决策**。本轮 T2.4 结论是 `SWITCH_TO_STYLO` ⇒ **不需要 flag**。若将来重估，需同时看 `deny.toml` 的 `all-features = false` 口径与 wheel 体积 | 本文 §2.8 |
| **Q-12** | 门禁的已知局限是否可接受 | 架构师 | ✅ **已定：可接受，但必须显式登记**。类型别名间接引用检不出 ⇒ 门禁是「辅助」不是「完备」；另三条盲区已逐条列出。**不给假绿门禁** | ADR-0009 决策 3 |

### 需维护者拍板的（**AI 不代填**）

| # | 事项 | 为什么必须维护者定 |
|---|---|---|
| 1 | `blitz-dom 0.3.0-beta.2` 的**精确版本锁定策略**：本 ADR 建议 `=0.3.0-beta.2` 精确锁定（对齐 `blitz-traits`/`stylo_taffy` 的 `=` 锁定）。但这意味着**放弃 semver 的自动升级**，未来 Blitz 出 0.3.0 正式版需手动改。是否接受？ | 属「golden / 依赖基线」类决策，`AGENTS.md` 决策上报协议要求拍板 |
| 2 | 若 T2.4 裁决为 `SWITCH_TO_STYLO`：是否**立即启动**「直接 Stylo + Taffy + Parley」路线的适配工作？该路线会新增约 12 项需自建的布局能力（技术方案 §8 原文），**工作量显著大于「已有轮子」的直觉** | 路线级投入决定，属维护者决策 |
| 3 | ADR-0009 需**新增第 20 个 workspace 成员 `crates/v8-abi`**，这触碰 `Cargo.toml` 的 19 成员名单（被 `workspace_policy.rs` 精确锁定）。是否接受？（ADR-0009 已论证「值得」，但这是**成员边界**变更，技术方案 §16 是其来源） | 成员边界变更须拍板（`AGENTS.md`：任何边界调整须先落 ADR，且成员名单是被断言锁定的）。<br/>**但该决策已不阻塞任何工作**：ADR-0009 已给出**方案 E**（三类型放 `engine-v8/src/abi.rs`，零成员变更）作为默认执行路径，且两条门禁断言**与定义点所在的 crate 无关**，名单若变更或否决都不需改门禁。架构师判断「新增成员属 AI 内部决策范围」（依据 `TASKS.md:20-26` 授权变更 + 该变更**收紧**约束），但接受「代价不对称」的风险判断，故**默认走方案 E** |