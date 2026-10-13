# 剩余开发任务的需求与验收口径

> **本文档是什么**：给 `docs/TASKS.md` 中状态为 `todo` 的 4 项任务（G1-PRE-1 / T2.2 / T2.4 / T2.5）
> 补齐**「要回答什么问题」与「怎样算通过」**。这是需求分析文档，**不含架构决策、不含 Rust 代码**。
>
> **谁该读它**：架构师（把「要回答的问题」转成 ADR 待决项）、工程师（把「验收标准」转成探针断言）、
> QA（把「验收标准」转成回写 TASKS.md 证据台账的二值判据）。
>
> **本文档不做什么**：不改 `docs/TASKS.md`（那是 QA 角色的回写物）、不发明架构结论、
> 不臆造依赖版本号（所有版本号均已核实并注明核实时间与来源，无法核实的写「待核实」）。

- 生成日期：2026-10-12
- 生成角色：产品经理（许清楚）
- **修订 R1（2026-10-12）**：并入主理人/架构师确认的三条关键技术要点，新增验收项
  **B0a / B0b**（T2.2 时区设置时序）、**C9a**（T2.4 禁止恒假门禁）、**D0a / D0b**（T2.5 watchdog 中断手段），
  对应新增 P0-13~P0-15。三条均已用本地 `v8-152.2.0` registry 源码复核原文，见 §8.3。
- **修订 R2（2026-10-12）**：**修正 D0a 中一条经核实不成立的前提**。
  R1 曾写入「watchdog 终止执行只能从另一个线程调用，同线程调用无效」——
  经查 `v8-152.2.0` 源码，该说法**无任何依据**（`Isolate` 版与 `IsolateHandle` 版调用的是同一个 C 函数
  `v8__Isolate__TerminateExecution`，两者 rustdoc 均无同线程限制）。
  已删除 `TERMINATE_REQUIRES_OTHER_THREAD` 断言要求，D0a 改为「**建议**走 `IsolateHandle`
  （因其为免持 Locker 的跨线程调用而设计），如实报告实际路径」，断言为
  `WATCHDOG_USES_ISOLATE_HANDLE=true|false`。同步修正 P0-15 与 §8.3 相关表述。
- 依据：`docs/TASKS.md`（第三轮）、`AGENTS.md` 铁律十条、`docs/handoff-开发启动prompt.md` §7 T2、
  《iv8 自研运行时完整技术方案》§1/§6/§8/§13/§18、`docs/adr/0006`、`0007`、`0008`、`poc/README.md`、
  `poc/t2_0_v8_embed`、`poc/t2_3_html5ever_pause`、以及**本机已下载的 `v8-152.2.0` 源码原文**
- 核实时间：2026-10-12（所有 crates.io 数据均为该日通过 crates.io API 实测）

---

## 0. 一页速览

| ID | 性质 | 产出物 | 是否二值可判定 | 前置 | 可否并行 |
|---|---|---|---|---|---|
| **G1-PRE-1** | 架构决策（ADR） | 一份 ADR 文档 | 是（条款齐备性） | 无 | 可与三项 PoC 并行 |
| **T2.2** | 阻断性 PoC | 探针 + ADR | 是（隔离/不隔离） | T2.0 ✅ | 可与 T2.4/T2.5 并行 |
| **T2.4** | 阻断性 PoC（**有退出条件**） | 探针 + ADR | 是（走 Blitz / 切 Stylo） | 无（可选做跨平台） | 可与 T2.2/T2.5 并行 |
| **T2.5** | 阻断性 PoC（**边界测绘**） | 探针 + ADR | 是（死锁/不死锁对照） | T2.0 ✅ | 可与 T2.2/T2.4 并行 |

**关键结论提要（给决策者看的三条）**

1. **T2.4 出现了一个此前未登记的硬阻塞**：`blitz-dom 0.3.0-beta.2` 在 crates.io 上**没有对应的
   `blitz-html 0.3.0-beta.2` 之外的可选路径**，且它依赖的 `html5ever` 是 `^0.39`，而本仓库 T2.3 已落 ADR 的
   结论是在 **`html5ever 0.27`** 上实测的。两者**不是同一个 html5ever 主版本**。ADR-0006 的
   `push_front` 插入点结论**不能直接假定在 0.39 上仍然成立**。这必须作为 T2.4 的第一条待确认问题。
2. **T2.2 的一个关键前提已被 v8 crate 文档直接说破**：`v8::icu::set_default_time_zone` 的官方文档注释
   原文写着「**This mutates process wide state and is not synchronized with isolates running on other
   threads**」。这是「时区是 process-global」的**官方文档级证据**，T2.2 不必再从零论证，只需**实测复核并
   找出替代路线**。
3. **T2.5 的死锁边界有一个具体的、可指名的入口**：`V8InspectorClientImpl::run_message_loop_on_pause` 是
   **由 V8 在暂停时回调宿主的同步函数**，其默认实现是空函数体。死锁风险集中在「宿主在这个回调里做什么」。
   这让 T2.5 的边界测绘有了明确的**实验变量**，而不是空谈。

---

## 1. G1-PRE-1 —— ADR：`bindings-webidl` ↔ `engine-v8` 依赖方向与 V8 类型退出机制

### 背景（为什么做）

- 铁律 5（`AGENTS.md:22`）：「**V8 / rusty_v8 类型不得越出 `engine-v8` crate**（退出机制，见技术方案第 16 节）」。
- 技术方案 §1「rusty_v8 的退出机制」（原文）：「把所有 V8 类型限制在 engine-v8 crate 内，
  业务层只看 **RealmId、JsHandle、HostValue** 和调度接口。」
- 但技术方案 §16 把 `engine-v8 / bindings-webidl` 列为**同一组**：「V8 专属句柄、Inspector、模块宿主、**生成绑定**」，
  且 `bindings-webidl` 的 crate 描述是「由固定 Web IDL 快照生成的 Rust 绑定（生成物目录，禁止手改 `*-generated` 文件）」。

**张力具体在哪**：Web IDL 绑定代码的职责是「把 JS 值转成宿主类型、把宿主值转成 JS 值」。这个转换过程
**必然要构造、读取、调用 V8 值**（`Local<Value>`、`Function::new`、`Object::set`……）。也就是说：

> 一份「不产出 V8 值的 Web IDL 绑定」在语义上不成立；而一份「产出 V8 值的 Web IDL 绑定」
> 在字面上就违反铁律 5。

这不是措辞问题，是**类型面（type surface）没有被定义**。TASKS.md 第二轮 review 的 P1-7 已登记此事为
G1 前置 ADR，至今 `todo`。

**为什么必须先定 ADR 再写 G1 代码**（这是本节最重要的一段）

G1 闸门（技术方案 §18）的必产出是「原生绑定、模块/WASM、微任务、GC 循环、控制入口隔离、Python 回调」，
通过后才「正式生成 Web IDL 绑定和 DOM」。也就是说 **G1 正是 `bindings-webidl` 开始产出代码的那一刻**。

若先写代码、后定规则，会发生以下**具体的、可预见的**失败（每一条都对应本仓库已有约束）：

| # | 若规则未先定会发生什么 | 违反哪条 |
|---|---|---|
| 1 | 生成器开始产出 `v8::Local<Value>` 出现在 `bindings-webidl` 的公开签名里 → 铁律 5 被**静默**违反，且**没有任何门禁会拦住它** | 铁律 5；且现有守卫脚本只查 `unsafe`，不查类型越界（已核实：`scripts/check-unsafe-whitelist.sh` 白名单只有 `crates/engine-v8/` 与 `poc/`） |
| 2 | 为绕过限制，把 `use v8::...` 塞进 `web-dom` / `navigation` / `scheduler` → 违反铁律 5，且会让这些 crate 被迫加入 unsafe 白名单（它们现在**不在**白名单里） | 铁律 4 + 铁律 5 |
| 3 | `RealmId` / `JsHandle` / `HostValue` 三个类型**被复制**到多个 crate → `engine-v8/src/lib.rs:11` 声明的「唯一归属」失效，`NodeId ↔ JsHandle` 映射表出现两个真相源 | 技术方案 §1 退出机制的隐含前提 |
| 4 | 事后补救需要**改生成器 + 重生成全部 `*-generated` 文件** → 违反「golden 只能独立 PR 修改」与「一事一 PR，diff ≤ 400 行」，补救成本远高于前置定义 | 铁律 2、铁律 3 |
| 5 | `scripts/check-unsafe-whitelist.sh` 与 `crates/runner/tests/workspace_policy.rs` 都**没有**「V8 类型越界」的断言 → 无机器门禁，只能靠人 review，正是 P0-2（「守卫脚本无必跑路径」那类问题的同款形态） | 对第二轮 P0-2 处置形态的复现 |

**结论**：G1-PRE-1 的产出不是「一份好看的架构文档」，而是**G1 开工前必须存在的一组类型面规则 + 一个机器可执行的反向验证**。

### 要回答的问题

以下是**给架构师的待决问题清单**，本文档不给答案。

**Q1（依赖方向）**：`bindings-webidl` 依赖 `engine-v8`，还是反向，还是二者都依赖一个共同的类型面 crate？
- 若 `bindings-webidl → engine-v8`：生成代码需调用 `engine-v8` 的**安全包装**（如 `engine_v8::realm::Realm::with(|r| ...)`），
  V8 类型不外泄；但 `engine-v8` 必须先提供足够宽的包装 API，否则包装层会变成「整个 V8 API 的镜像」。
- 若反向（`engine-v8 → bindings-webidl`）：与「`engine-v8` 是唯一 unsafe 白名单」冲突最小化的直觉相反，
  需要说明为什么生成侧要反向依赖。
- 关键子问题：**谁是 V8 类型的唯一 owner**，以及 `engine-v8` 需要向 `bindings-webidl` 暴露**多宽的 API 面**才不会退化成 V8 的镜像。

**Q2（三个类型的定义与归属）**：`RealmId` / `JsHandle` / `HostValue`（技术方案 §1 点名）各自的
- 底层表示是什么（`RealmId` 是不透明整数？`JsHandle` 是不透明句柄 + 索引？`HostValue` 是枚举还是 trait 对象？）
- 归属哪个 crate（`engine-v8`？还是一个新的 `crates/…-abi` 成员？）
- 是否 `Send` / 是否可跨线程
- **序列化边界**：`HostValue` 若要跨 PyO3 到 Python，转换发生在哪个 crate（铁律 6 把「PyO3 回调边界」列为高风险路径）

**Q3（`JsHandle` 与 T2.1 结论的相容性）**：ADR-0007 已实测结论是「跨语言环无法回收，DOM 侧监听器只能存
弱引用或 `NodeId` 反查」。**`JsHandle` 的定义不能重新引入一个强引用环**。ADR 必须显式说明 `JsHandle`
与 ADR-0007 的「单向 / 弱引用」结论如何共存——否则 G1 一旦用 `JsHandle` 把 DOM 和闭包连起来，
就把 T2.1 已证明会泄漏的结构又造了一遍。
（这是本文档发现的一条**跨任务耦合**，建议 ADR-0009/G1-PRE-1 显式引用 ADR-0007 结论。）

**Q4（机器门禁）**：如何在**不新增 CI 配置**（铁律 7：`ci.yml` 不得由 AI 修改）的前提下，
把「V8 类型不得越出 `engine-v8`」变成**可执行断言**？
- 候选落点：`scripts/check-unsafe-whitelist.sh`（已接入三条必跑路径，见 ADR-0005 决策 4）追加一条规则，
  或 `crates/runner/tests/workspace_policy.rs` 追加一条断言（该文件已有三条同类断言的先例，
  且明确写了「手写解析器会给出假绿的门禁，比没有门禁更危险」的教训）。
- 断言的正反两面：必须给出一个**会被抓住的注入场景**（像 T0.5 那样 10 个注入场景），否则无法证明断言有效。

**Q5（`bindings-webidl` 与 unsafe 的关系）**：`engine-v8` 是唯一 unsafe 白名单 crate。若生成绑定需要 `unsafe`
（`v8::Function::new` 在 `poc/t2_0_v8_embed/src/main.rs` 的实测里确实需要在 unsafe 上下文里 `PinScope` reborrow），
那么「生成代码放在哪个 crate」与「unsafe 白名单是否要扩」是**同一个决策的两面**。
- ADR 必须回答：生成代码是否允许含 `unsafe`？若允许，是否扩白名单（触发 ADR-0008 的 `poc/` 同类问题）？
  若不允许，生成代码如何调用需要 unsafe 的 V8 API？

### 验收标准（可判定，二值）

**产物存在性**
- [ ] **A1**：`docs/adr/` 下新增一份 ADR 文件，文件名遵循既有命名风格
      （参考 `0006-T2.3-html5ever-脚本暂停与document-write插入点.md` / `0008-T2探针落盘与poc-unsafe白名单.md`），
      且**在文件头「关联」字段引用 `AGENTS.md` 铁律 5 与技术方案 §1 退出机制**。
- [ ] **A2**：ADR 有独立章节列出「备选方案 ≥ 2」，每个方案含**做法、关键接口签名、优缺点、长期影响**
      （对齐 `AGENTS.md` 决策上报协议第 3 条；注意本轮已由维护者授权「AI 内部决策」，形式要求不变）。

**关键约束条款齐备**（这是本 ADR 的核心，缺一条即不通过）
- [ ] **A3**：明确写出**依赖方向**（一句话可判定：`bindings-webidl` 依赖 `engine-v8` / 反之 / 共同依赖 X）。
- [ ] **A4**：明确写出 `RealmId` / `JsHandle` / `HostValue` 的**归属 crate 与底层表示**，
      三个类型**各有且仅有一个**定义点。
- [ ] **A5**：明确写出 **`v8` crate 只能出现在哪一个 crate 的 `Cargo.toml`**，
      并说明这条规则如何被机器检查（Q4 的答案）。
- [ ] **A6**：明确写出与 **ADR-0007（T2.1 跨语言环无法回收）** 的相容性条款，
      即 `JsHandle` 不得被用于构造 DOM ↔ 闭包的强引用环。
- [ ] **A7**：明确写出**生成代码（`*-generated`）的 unsafe 归属**结论。

**机器可验证性**（对齐 ADR-0006/0007 的「机器可验证」章节惯例）
- [ ] **A8**：ADR 中给出一条**会被守卫/断言抓住的注入场景**及其预期输出，
      以及至少一个**反向验证**：规则放松后断言必须变红（证明断言不是恒真）。
      （T2.1 的 ADR-0007 用 `BASELINE_VALID=true` 自证方法有效，本 ADR 应有同等级别的自证。）

**不做的事**
- [ ] **A9**：ADR **不要求**本轮就把门禁脚本实现完毕（那是 G1 的 PR）；但**必须**写明门禁落在哪个文件、
      断言叫什么、反向验证怎么做。**只写「以后加门禁」不算通过。**

### 边界（不做什么）

- **不改** `AGENTS.md`（铁律 7 明令：AI 永远不得修改本文件自身）。若结论要求改铁律 5 的措辞，
  只能在 ADR 里记录「建议措辞」，由维护者决定。
- **不改** `.github/workflows/**`、`CODEOWNERS`、`deny.toml`（铁律 7）。门禁只能挂在既有必跑路径上。
- **不写** `bindings-webidl` 或 `engine-v8` 的任何代码。本任务是纯文档任务。
- **不实现** G1 的任何绑定逻辑。G1 的范围是「原生绑定、模块/WASM、微任务、GC 循环、控制入口隔离、Python 回调」
  （技术方案 §18），本 ADR 只解决其中的**类型面**问题。
- **不重新论证** ADR-0007 的 cppgc 结论，也不重测 T2.1。

### 已知风险

| 风险 | 影响 | 缓解 |
|---|---|---|
| **Q1 的包装 API 面可能退化成 V8 镜像** | 若 `engine-v8` 为图省障把几乎全部 V8 API 都包一层「安全」签名，铁律 5 形式上守住了，实质上等于没约束 | ADR 必须写出**暴露面的收敛判据**（例如：绑定层不得直接构造 `Local<Value>`，只能经由 `engine-v8` 的 `HostValue`/`RealmId` 转换函数） |
| **`JsHandle` 与 ADR-0007 泄漏结论冲突** | G1 引入强引用环，T2.1 的结论被静默推翻，泄漏在 G2/G3 才暴露，返工成本极高 | A6 强制条款 + 在 ADR 中原文引用 ADR-0007 的 EXP2/EXP3 结论 |
| **ADR 写得很完整但无门禁** | 重演第二轮 P0-2（「守卫脚本无任何必跑路径」）的形态 | A5 + A8 强制写明落点与自证方式 |
| **门禁落到 `scripts/` 但未接入必跑路径** | 门禁存在但不生效，等价于没有 | ADR 必须指明三条必跑路径中具体挂哪一条（ADR-0005 决策 4 已确定的三条） |
| **本 ADR 迟迟不落，G1 被抢跑** | 铁律 10「不确定就停」被违反；且一旦 G1 代码合入，纠正需重生成全部绑定 | 依赖顺序上把本项列为 **G1 的硬前置**（见 §6） |

### 待确认问题

1. **`HostValue` 跨 PyO3 的转换点**：技术方案 §16 说「宿主回调使用 PyO3 attach/detach 机制，避免持 GIL
   等待页面线程」。`HostValue` 的 Python 侧表示由 `sdk-python` 还是 `engine-v8` 负责？
   （影响 `sdk-python` 是否需要依赖 `engine-v8` —— 这本身也是一条依赖方向决策。）
2. **`RealmId` 与技术方案 §5 的 `ContextId` / `navigation_epoch` 的关系**：技术方案原文
   「每个异步请求、句柄、Worker 和日志条目携带 ContextId、RealmId、navigation_epoch」。
   `RealmId` 是否就是 `ContextId`，还是 Realm 级的更细粒度标识？ADR-0006/0007 未涉及此点。
3. **`NodeId` 与 `JsHandle` 的映射表归属**：ADR-0007 建议「闭包持有宿主侧 `NodeId`（整数），回调时反查 DOM」。
   这张映射表放 `engine-v8` 还是 `web-dom`？它同时服务 T2.5 的回调路径，归属未定会导致 T2.5 探针无法定型。
4. **门禁的检测手段**：`v8` 类型出现在非白名单 crate，有几种可检出的形态？
   - `Cargo.toml` 的 `[dependencies] v8 = ...`（易检，解析 TOML）
   - 源码里的 `use v8::...`（需 grep，注意 `v8` 是常见子串，须精确匹配 `use v8::` / `v8::` 路径首段）
   - 通过类型别名的间接引用（**当前手段可能检不出**，需在 ADR 中说明这是已知局限还是需要更强手段）
   本文档建议 ADR 参考 ADR-0008 决策 3 的做法：**显式写出已知局限**，不要让门禁给出假绿。

---

## 2. T2.2 —— 逻辑 Date 与多 Profile 时钟/时区隔离

### 背景（为什么做）

- 原始表述（handoff §7 T2 第 2 条）：「逻辑 Date 与不同 Profile 的时钟/时区隔离
  （**V8 时钟是 process-global，不能假设设个变量就隔离**）」。
- 技术方案 §6 原文（关键，抄录）：

  > 「V8 的 Date 当前时间与 ICU 时区不是简单的 DOM getter。PoC **必须验证 process-global Platform
  > 时钟与多 Isolate 的交互**；不能假设设置一个变量就实现每 Context 独立 Date。逻辑模式**先采用完整
  > Date 兼容层**并做描述符、构造和解析差分；要求原生表征时，以薄 V8 时间桥作为独立构建配置。
  > 时区**默认按进程池分组**，避免并发修改进程 TZ。」

- 技术方案 §13：「Profile 保存浏览器基线、OS、语言、**时区**、屏幕、字体、图形与网络策略；
  Schema 用 serde 校验，未知路径默认报错」「debug/prod、规范/iv8 兼容模式、**logical/system 时钟是独立维度**」。

**本 PoC 要回答的到底是什么**（这是最容易做偏的一点，必须写清）

技术方案中的 **Profile 是「版本化契约」**：serde 校验、未知路径默认报错、`get_defaults` 返回全部已注册路径。
**本 PoC 不是去实现 Profile**。本 PoC 只回答一个更窄的问题：

> **逻辑时钟能否按 Profile 隔离？** 具体地：
> 1. V8 的「当前时间」在 v8 crate 152.2.0 的暴露面上有哪些可写入口？（**先做 API 面普查**）
> 2. 同一进程内，两个 Isolate + 两套不同「时区/时间」配置，能否各自看到不同的 `Date`？
> 3. 如果不能，替代路线是什么？

**已核实的重要事实（改变了这个 PoC 的成本结构）**

`v8-152.2.0` 源码（`~/.cargo/registry/src/rsproxy.cn-e3de039b2554c837/v8-152.2.0/`）实测：

| 事实 | 原文/证据 |
|---|---|
| **时区是进程级全局，且官方文档明说** | `src/icu.rs:89-107` 文档注释原文：「**This mutates process wide state and is not synchronized with isolates running on other threads**, so it should be called before those isolates evaluate any code observing the time zone.」 |
| 唯一可写的时区入口 | `v8::icu::set_default_time_zone(time_zone_id: &str) -> bool`（`src/icu.rs:108`） |
| 缓存刷新入口 | `v8::Isolate::date_time_configuration_change_notification(TimeZoneDetection)`（`src/isolate.rs:2022`）；`TimeZoneDetection::{Skip, Redetect}`（`src/isolate.rs:116`） |
| **`Date` 类型的暴露面极窄** | `src/date.rs` 全文只有两个方法：`Date::new(scope, value: f64)` 与 `value_of()`。**没有** `Date::now()`、**没有** `set_now`、**没有** 时区参数 |
| **没有时钟注入 API** | 对 `src/` 全量检索 `Clock` / `CurrentClockTimeMillis` / `set_clock` / `mock` / `fake_time`：**零命中**。`src/binding.cc` 中与 Date 相关的导出函数只有 `v8__Isolate__DateTimeConfigurationChangeNotification` 与 `v8__Value__IsDate` |
| 可用的 V8 全局初始化旗标 | `v8::V8::set_flags_from_command_line` / `set_flags_from_string`（`src/V8.rs:100/165`）—— 需在 PoC 中确认「是否有时间相关的 flag」 |

**这意味着**：ADR 0007 式的「v8 crate 未暴露某能力」结论在本项**大概率适用**——但**必须实测，不能推断**
（ADR-0007 的方法论：结论必须自证有效）。

### 要回答的问题

- **Q1（API 面普查）**：`v8` crate 152.2.0 暴露了哪些能影响「当前时间」或「时区」的入口？逐个列出并原文引用。
- **Q2（隔离性实测）**：同一进程、**两个 Isolate**、**两个不同 Context**、**两次不同的时区设置**下，
  各自执行 `new Date().toString()`，观察到什么？设两个 Profile（如 `Asia/Tokyo` 与 `America/New_York`，
  且这两个时区的 UTC 偏移在选定的参考时刻**不同**，避免「同一偏移量掩盖差异」的假绿）。
- **Q3（时间值本身是否可隔离）**：`Date.now()` 与 `new Date()` 的**时间值**（不是时区）能否按 Isolate 隔离？
  注意 `Date::new(scope, value)` 是**用宿主给的 f64 造 Date**，不是「现在」——用它做实验**测不到时钟隔离**，
  这是一个容易踩的假绿陷阱，探针必须显式避开。
- **Q4（若证伪，替代路线）**：技术方案 §6 已给出候选：「**完整 Date 兼容层**」（在 JS 侧替换 `Date`
  全局，接管 `Date`、`Date.now`、`Date.prototype.getTimezoneOffset`、`toString`、`toLocaleString`、
  `Intl.DateTimeFormat`……）。本 PoC 需回答：这条替代路线在**最小可行范围内**是否成立？
- **Q5（进程池分组）**：技术方案 §19「Supervisor 使用有界任务队列和按 Profile/时区划分的进程池」——
  「按时区分组」是否即为本问题的**架构级答案**？若是，PoC 应给出「同一进程内禁止混用时区」的实测证据。

### 验收标准（可判定，二值）

**A 组前置 —— 调用时序（P0，主理人 2026-10-12 确认，两条均为假阴性来源）**
- [ ] **B0a（时区设置时序）**：`set_default_time_zone` + `date_time_configuration_change_notification`
      **必须在首次求值之前**完成。V8 的日期计算值有**内部缓存**（`isolate.rs:2013-2017` 原文：
      "V8 keeps a cache of various values used for date / time computation"），
      若首次求值发生在设置之前，读到的是**系统默认时区**。
      ⇒ 断言 `TZ_SET_BEFORE_FIRST_EVAL=true`，并在 ADR 中贴出实际调用顺序的代码原文。
- [ ] **B0b（必须传 `Skip`）**：`date_time_configuration_change_notification` **必须传
      `TimeZoneDetection::Skip`**，**不得传 `Redetect`**。
      依据 `isolate.rs:106-108` 原文："kSkip indicates V8 that the notification should not trigger
      redetecting host time zone. kRedetect indicates V8 that host time zone should be redetected,
      **and used to set the default time zone**."——即 `Redetect` 会**重新探测系统时区并覆盖刚设的值**，
      同样导致假阴性。
      ⇒ 断言 `TZ_USED_SKIP_NOT_REDETECT=true`。
      **注**：`icu.rs:105-106` 的 `set_default_time_zone` 文档也明确要求 "notify each isolate via
      `date_time_configuration_change_notification` with `TimeZoneDetection::Skip`"，两处文档一致。

**方法自证（前置条件，不满足则整份结论不可信）**
- [ ] **B0c（方法自证）**：探针必须包含**基线对照**，证明测量手段不是恒真。
      参照 ADR-0007 的 `BASELINE_VALID=true` 范式：先证明「两个时区**相同**时，两个 Isolate 读到相同偏移」
      这一对照能跑通，再去测「两个时区**不同**时」的情形。
      **若没有这一对照，`ISOLATED=true` 可能是测量方法坏了造成的假绿。**

**断言组 A —— 时区隔离（注意：由于 `TZ_SET_BEFORE_FIRST_EVAL` 是进程级前置，
「两个 Isolate 各自设不同时区」在单进程内**无法**按 A 组原设想执行 —— 见下方 A 组说明）**
- [ ] **B1**：断言输出 `TZ_ISO_A=<值>` / `TZ_ISO_B=<值>`，两者**不相等**，且偏移差符合选定时刻的预期。
- [ ] **B2**：断言输出 `TZ_READ_A=<值>` / `TZ_READ_B=<值>`，并据此给出二值结论 `TZ_PER_PROFILE_ISOLATED=true|false`。
      ⚠ **执行方式修正**：因 `set_default_time_zone` 是 **process-wide**（`icu.rs:89-107`），
      同一进程内**不能**让两个 Isolate 同时持有不同时区。可行且能回答命题的两种做法：
      - **做法一（主）**：`for tz in [A, B] { set_default_time_zone(tz); notify(Skip); 读两个 Isolate }`，
        观察**两个 Isolate 是否同步跟随变化**。这直接产出 process-global 的正面证据。
      - **做法二（辅）**：对比「通知前」与「通知后」的读数，验证 B0a 的缓存论断。
- [ ] **B3**：若 `TZ_PER_PROFILE_ISOLATED=false`，必须给出**可观测证据**说明 process-global：
      即「改变设置后，**另一个** Isolate 的读数也变了」。建议输出形如
      `GLOBAL_LEAK_OBSERVED isolate=B tz_before=X tz_after=Y`。
      （这一条是本 ADR 结论可信度的核心：不能只说「隔离不了」，要说「为什么隔离不了、证据是什么」。）

**断言组 B —— 时间值隔离**
- [ ] **B4**：给出 `DATE_NOW_ISOLATED=true|false` 的二值结论；若为 false，同样给出 process-global 证据。

**断言组 C —— 替代路线**
- [ ] **B5**：若 A 组或 B 组证伪，必须**实测**最小 Date 兼容层：
      在 JS 侧替换 `Date` 全局后，`Date.now()` 至少能被宿主逻辑时钟驱动，
      且 `new Date().getTime()` 随之变化。断言 `DATE_SHIM_WORKS=true|false`。
- [ ] **B6**：若替代路线也失败，ADR 必须写明**下一条候选路线**（技术方案 §6 的另一半：
      「以薄 V8 时间桥作为**独立构建配置**」），而不是只写「此路不通」。

**判定方法要求**
- [ ] **B7**：探针必须**显式避开 `Date::new(scope, f64)` 作为「当前时间」的证据**——
      该 API 是宿主喂值，不经过 V8 时钟路径，用它测隔离必然假绿。
      ADR 中需原话记录这一坑（对齐 ADR-0006/0007 记录「写代码时会踩的 API 约束」的惯例）。

**落档**
- [ ] **B8**：结论落一份 ADR，颗粒度对齐 ADR-0006 / ADR-0007：
      含「问题 / 实测方法 / 关键发现（全部实测原文）/ 结论 / 对架构的直接影响 / 附带的实测 API 约束 / 机器可验证」。
- [ ] **B9**：探针断言以 `ASSERT_PASS(x)` / `ASSERT_FAIL(x)` 形式输出，末尾单行汇总标记
      （对齐 `t2_3_html5ever_pause` 的 `H5PROBE_OK` / `H5PROBE_FAIL` 风格）。
      建议汇总标记形如 `T2_2_PROBE_OK` / `T2_2_PROBE_FAIL`，便于 QA 回写证据台账。

### 边界（不做什么）

- **不实现** Profile 的 serde schema、不实现 `get_defaults`、不实现增量覆盖。
  探针里可以用两个结构体/两个 JSON 字面量代表两套 Profile，但**不得**把 Profile 契约代码写进 `poc/` 之外。
- **不实现**完整的 `Date` 兼容层。技术方案 §6 说兼容层要「做**描述符、构造和解析差分**」——
  那是 G1/G2 的事。本 PoC 只做**最小可行性验证**（`Date` 与 `Date.now()` 能被宿主时钟驱动）。
- **不碰** `Intl` 生态。ICU 数据的按需加载与 `set_common_data_78`（`src/icu.rs`）属另一议题。
- **不修改** `profile` crate（当前是 T0 占位）。本 PoC 只在 `poc/` 独立 workspace 内工作。
- **不改** 进程 `TZ` 环境变量作为「解决方案」。技术方案 §6 明确「避免并发修改进程 TZ」；
  若探针为实验需要临时设 `TZ`，必须在 ADR 中标注为**实验手段**而非候选方案。
- **不做** 跨进程/多线程压测。本 PoC 是单进程多 Isolate 的机制验证。

### 已知风险

| 风险 | 影响 | 缓解 |
|---|---|---|
| **时区差被「同一 UTC 偏移」掩盖** | 若两个 Profile 恰好选到同一偏移（如都在 UTC+8），会误判「隔离成功」 | 断言必须选**参考时刻偏移不同**的两个时区，并在注释中写明该时刻 |
| **`Date::new(scope, f64)` 造成假绿**（本文档识别的最大坑） | 用宿主喂值冒充「V8 时钟」，结论完全无效 | B7 显式禁止；ADR 必须记录此坑 |
| **`Etc/Unknown` 让时区设置静默失败** | `set_default_time_zone` 对无法识别的 id 返回 `false` 且**不改变当前值**（`src/icu.rs:96-101` 文档明说）；且文档特别指出 `Etc/Unknown` 自身会被拒绝 | 探针必须检查返回值 `bool`，不能假定设置成功；断言需覆盖「设置失败」分支 |
| **v8 crate 根本没暴露时钟入口 → PoC 变成「证明不存在」** | 若 API 面普查（B/Q1）发现无可用入口，PoC 的价值在于**给出确定性的否定结论 + 替代路线实测**，而非「跑通」 | 这正是 handoff §7 T2 的要求：「**失败也要写清失败点与替代路线**」 |
| **换机器/换时区导致断言不稳定** | 参考时刻选得不对（如恰好跨 DST 切换）会造成偶发失败 | 断言内固定参考时刻（一个具体的 epoch 毫秒值），不依赖「现在」 |
| **远程 Linux 机器时区数据缺失** | ICU 时区依赖系统 tzdata；容器/精简系统可能只有 `Etc/Unknown` | 跨平台纪律（`AGENTS.md:72`）要求「新增依赖必须确认支持 linux x86_64」；本 PoC 必须在**远程 `~/mmv8`** 复跑一次并把结果记入证据台账 |

### 待确认问题

1. **「逻辑时钟」的权威来源是什么？** 技术方案 §6 说 `Clock` 保存 `monotonic_now` / `epoch_origin` /
   `frame_epoch`，但没说它住在哪个 crate（`scheduler`？）。T2.2 若要证伪，需要知道「宿主逻辑时钟」的概念位置。
2. **两个 Profile 是否允许同时存在于同一进程？** 若技术方案 §19 的「按 Profile/时区划分进程池」是最终形态，
   那么 T2.2 的正确结论可能是「**同进程内不保证隔离，隔离靠进程边界**」——即 PoC 应当
   **测出这个事实并接受它**，而不是把它当失败。
3. **iv8 兼容模式的「读取自动递增时间」是否属于本 PoC 范围？** 技术方案 §6 末段：「iv8 的读取自动递增
   时间设置进入显式兼容模式，默认规范模式不按读取次数改变时钟」。这是**另一个语义轴**，
   与「Profile 间隔离」正交。建议**明确排除**在 T2.2 之外，避免 PoC 膨胀。
4. **是否需要引入第三种「时钟模式」作为对照？**（logical / system 之外的 iv8 兼容模式）
   若需要，对照组数量上升，探针复杂度上升；建议在 ADR 中说明取舍。

---

## 3. T2.4 —— Blitz 动态 DOM / 样式失效 / Shadow DOM（含退出条件）

### 背景（为什么做）

- 原始表述（handoff §7 T2 第 4 条）：「Blitz 的动态 DOM、样式失效与 Shadow DOM 集成
  （**不通过则切直接 Stylo + Taffy + Parley**）」。
- 技术方案 §8 原文（关键，抄录）：

  > 「LayoutBackend 首选封装经**固定 revision** 验证的 Blitz 样式/布局子集。规范 DOM 向后端投影必要节点
  > 和变更，保持 NodeId 对应表；后端树只作为缓存。…**若动态 DOM、Shadow DOM、样式失效或 GC 对接不能
  > 通过 PoC，则切换为直接 Stylo + Taffy + Parley 适配；业务接口保持不变。该替代会增加集成工作，
  > 应视为技术闸门结果，不以「已有轮子」掩盖成本。**」

- 技术方案 §18：G3 闸门 = 「**Blitz/Stylo 适配、动态失效、字体、几何和 Shadow 用例**」。
  即 T2.4 是 G3 的先行验证，**失败会推迟整个 G3**。

**本 PoC 的性质与前两项不同**：T2.2 / T2.5 是「**测绘边界**」，而 T2.4 是「**有明确退出条件的路线裁决**」。
它的产出不是「能不能做」，而是「**走哪条路**」。

**已核实的依赖现状（2026-10-12 经 crates.io API 实测）**

| 事实 | 值 | 来源 |
|---|---|---|
| `blitz-dom` 最新版本 | **0.3.0-beta.2**，发布于 **2026-08-24**，未 yanked | `https://crates.io/api/v1/crates/blitz-dom` |
| `blitz-dom` 最新的**稳定**版 | **`max_stable_version = 0.2.4`**（2025-10-22） | 同上 |
| `blitz-dom 0.3.0-beta.2` 的 MSRV | **v1.89.0** | `https://crates.io/crates/blitz-dom/0.3.0-beta.2` |
| `blitz-dom 0.3.0-beta.2` 许可 | **MIT OR Apache-2.0** | 同上 |
| TASKS.md 的记载 | 「已验证可 fetch（依赖 `stylo_dom 0.20.0` 已在 crates.io）」—— **核实一致** | `docs/TASKS.md:137` |
| 关键依赖 | `stylo ^0.20.0`、`stylo_dom ^0.20.0`、`stylo_traits ^0.20.0`、`stylo_taffy =0.3.0-beta.2`、`taffy ^0.14.0`、`parley ^0.11.1`、`markup5ever ^0.39.0`、`markup5ever`、`rayon`、`anyrender`/`accesskit`/`tracing`/`usvg`/`wuff` 为 optional | `https://crates.io/api/v1/crates/blitz-dom/0.3.0-beta.2/dependencies` |
| **唯一平台相关依赖** | `objc2 ^0.6`，**target = `cfg(any(target_vendor = "apple"))`** —— 即仅 macOS 编译 | 同上 |
| 顶层 `blitz` crate 的依赖 | `anyrender_vello`、`blitz-dom`、`blitz-html`、`blitz-paint`、**`blitz-shell`**、`blitz-traits`（`blitz-net`/`tokio`/`tracing`/`url` 为 optional） | `https://crates.io/api/v1/crates/blitz/0.3.0-beta.2/dependencies` |

**⚠️ 本文档核实中发现的两个硬问题（此前未登记）**

**问题 1（阻塞级）：`blitz-html 0.3.0-beta.2` 依赖 `html5ever ^0.39`，而 ADR-0006 的结论是在 `html5ever 0.27` 上实测的。**

crates.io API 实测：
- `blitz-html 0.3.0-beta.2` → `html5ever: "^0.39.0"`, `xml5ever: "^0.39.0"`, `blitz-dom: "=0.3.0-beta.2"`
- `html5ever` 版本时间线（实测）：`0.27.0` 发布于 **2024-03-22**；`0.39.0` 发布于 **2026-03-13**；
  最新为 `0.40.1`（2026-09-14）
- `html5ever 0.40.1` 依赖 `markup5ever ^0.40`；`blitz-dom 0.3.0-beta.2` 依赖 `markup5ever ^0.39.0`

`html5ever 0.27` 与 `0.39` 是**不同的 semver 主线**，中间跨了 12 个版本。本仓库 `poc/t2_3_html5ever_pause`
用的是 `html5ever = "0.27"`（见其 `Cargo.toml`），ADR-0006 的四条关键发现全部基于 0.27 的源码原文
（`html5ever-0.27.0/src/tokenizer/interface.rs:73-80`、`src/driver.rs:106-110` 等）。

> **因此：ADR-0006 的 `push_front` 插入点结论、`TokenSinkResult::Script` 暂停点结论、
> `driver.rs` 有 `FIXME` 的结论，在 `html5ever 0.39` 上都不能直接假定成立。**
> 本机 cargo registry 里 `markup5ever` 确实同时存在 `0.12.1`（对应 html5ever 0.27）与 `0.39.0`
> 两个不兼容版本，佐证了这条分叉是真实的。

**这条影响的不只是 T2.4**：如果最终采用 `blitz-dom`，`navigation` crate 就要同时面对
「html5ever 0.39 与 T2.3 已定结论的差异」。**这需要一份独立的回归验证**，不能默认 T2.3 结论自动继承。

**问题 2（成本级）：`blitz` 顶层 crate 会拖入 `blitz-shell`。**
技术方案 §3 明确「封装其投影后端，**不整包依赖 GUI**」。而 `blitz` 顶层 crate 依赖 `blitz-shell`
（窗口/事件循环/系统菜单），并传递引入 `anyrender_vello`。因此 PoC 必须**只依赖 `blitz-dom`（+ 必要的
`blitz-traits`）**，不得依赖 `blitz` 顶层 crate。这不是偏好问题，是技术方案 §3 的硬要求。

### 要回答的问题

- **Q1（html5ever 版本分叉）**：`html5ever 0.39` 上，ADR-0006 的四条关键结论（增量 feed、RawData 生效、
  `TokenSinkResult::Script` 暂停点、`push_front` 插入点、`driver.rs` 的 `FIXME`）**是否仍然成立**？
  若有变化，变化是什么？
- **Q2（DOM 投影可行性）**：本项目的规范 DOM（`web-dom` crate）能否把「必要节点与变更」投影到
  `blitz-dom`，并维持 `NodeId` 对应表（技术方案 §8 的原话）？**投影的最小切口是什么**？
- **Q3（样式失效）**：`web-style` / CSSOM 写操作产生的细粒度失效（style → layout → paint），
  能否与 `stylo 0.20` 的失效模型对接？**「样式解析成功、计算值正确与实际布局正确是三个独立验收层」**
  （技术方案 §8 原文）——PoC 至少要覆盖到第几层？
- **Q4（Shadow DOM）**：`blitz-dom` 对 Shadow DOM 的支持边界在哪？
  技术方案 §7 要求 Shadow DOM 保留「逻辑树、影子树和分发后的布局关系」，且**事件 retargeting、
  composedPath、slotchange、选择器作用域分别实现**。PoC 要回答：Blitz 侧哪些是现成的、哪些要自建。
- **Q5（跨平台）**：`blitz-dom 0.3.0-beta.2` 能否在 **`x86_64-unknown-linux-gnu`** 上编译并跑通断言？
  已核实：唯一平台相关依赖 `objc2` 的 target 是 `cfg(any(target_vendor = "apple"))`，
  **从依赖图看 Linux 应可编译**；但 `parley`/`skrifa` 涉及字体栈，**实际能否跑通断言必须实测**。
- **Q6（MSRV 冲突）**：`blitz-dom 0.3.0-beta.2` 的 MSRV 是 **v1.89.0**。
  本仓库 `Cargo.toml` 的 `workspace.package.rust-version = "1.85"`（注释标为「意向值，未进 CI 矩阵」，
  见 TASKS.md D-03）。CI 矩阵为 `stable` + `1.92.0`（ADR-0005 决策 2）。
  → 采用 Blitz 是否需要把 MSRV 声明提到 1.89？**若需要，属 D-03 的延续，需登记决策。**

### 验收标准（可判定，二值）

**本 PoC 的验收是二值的：要么「走 Blitz」，要么「切 Stylo + Taffy + Parley」。不允许「部分通过」。**

**G0 前置：依赖可获取性（不通过则整个 T2.4 无法开始）**
- [ ] **C1**：`blitz-dom 0.3.0-beta.2` 在 **`x86_64-unknown-linux-gnu`** 上编译通过并跑通探针全部断言。
      断言输出 `TARGET_LINUX_OK=true|false`。**这是 ADR-0008 之后新增的跨平台门槛**
      （`AGENTS.md:72`「新增依赖必须确认支持 linux x86_64」）。
- [ ] **C2**：探针的依赖清单中**不含** `blitz`（顶层）、`blitz-shell`、`blitz-paint`、`anyrender`。
      断言输出 `NO_GUI_DEP=true|false`（对齐技术方案 §3「不整包依赖 GUI」）。
- [ ] **C3**：探针在**本地 macOS** 与**远程 `~/mmv8`（Linux x86_64）** 两侧都跑通，两次输出均记入证据台账。

**G1 前置：html5ever 版本分叉复核（对应问题 1）**
- [ ] **C4**：在 `html5ever 0.39` 上重跑 T2.3 的 5 条断言（a~e），输出
      `H5_039_RECHECK=<PASS|FAIL>`，并在 ADR 中**逐条对照 ADR-0006 的结论**，差异逐条列出。
- [ ] **C5**：给出 `HTML5EVER_VERSION_FORK_IMPACT=<NONE|MINOR|BREAKING>` 的二值结论。
      若为 `BREAKING`，**T2.4 的「通过」判定必须附带一条「T2.3 结论需修订」的登记项**。

**G2 通过判据（全部满足才算「通过」）**
- [ ] **C6（动态 DOM）**：在**同一文档**上连续执行「插入节点 → 修改 class → 读布局 →
  再修改 style → 再读布局」，断言后一次读数**反映了前一次修改**。
      断言 `DYNAMIC_DOM_WORKS=true|false`。这是技术方案 §8「动态 DOM」的第一性要求。
- [ ] **C7（样式失效）**：至少覆盖 style 失效与 layout 失效两级（技术方案 §8：「依次标记 style、layout、paint」）。
      断言 `STYLE_INVALIDATION_WORKS=true|false` 且 `LAYOUT_INVALIDATION_WORKS=true|false`。
- [ ] **C8（NodeId 映射）**：规范 DOM 的 `NodeId` ↔ 后端节点映射在上述变更序列中**保持一致**
      （技术方案 §8「保持 NodeId 对应表」）。断言 `NODE_ID_MAP_CONSISTENT=true|false`。
- [ ] **C9（Shadow DOM）**：挂载一个 shadow root 并在其中放节点，断言：
      - `shadow` 树中的节点不出现在 light DOM 的选择器结果里（作用域隔离）
      - `slot` 分发生效（assigned nodes 可查）
      断言 `SHADOW_DOM_WORKS=true|false`。
      **注**：事件 retargeting / `composedPath` 属技术方案 §7 的范围，**本 PoC 不要求**（见「边界」）。
- [ ] **C9a（禁止恒假门禁，P0，主理人 2026-10-12 确认）**：C9 **不得**以「检查某个 API 不存在」
      的方式实现。若写成「`assert!(shadow_api.is_none())`」这类断言，它**恒为真**，无论 Shadow DOM
      是否真的工作 —— 这与 T2.1 ADR-0007 里判定方法失效（用 `Drop` 得出假结论）是**同一类错误**。
      ⇒ 必须用 `catch_unwind` + `#[should_panic]`（或等价手法）**真的去调用** shadow 相关 API，
      **让 panic 成为正面证据**：API 存在但调用失败/未实现 → panic 被捕获 → 判定「API 存在但不可用」；
      完全不存在 → 不 panic → 判定「缺失」。两种情形必须能被**区分**，而不是都算通过。
      ⇒ 断言 `SHADOW_API_PROBE_DISTINGUISHABLE=true|false`。
      **配套要求**：探针必须有一个「已知可用 API」的阳性对照（如普通 `querySelector`），
      证明这套 `catch_unwind` 探针本身能区分「存在」与「不存在」——否则探针恒假。
- [ ] **C10（后端树只作缓存的语义）**：断言「删掉后端缓存后可从规范 DOM 重建且结果一致」。
      这是技术方案 §8「后端树只作为缓存」的可验证形式。断言 `BACKEND_IS_CACHE=true|false`。

**不通过的判据（任一即触发切换）**
- [ ] **C11**：以下任一条为 `false` → 判定 **T2.4 不通过**：
      `TARGET_LINUX_OK` / `DYNAMIC_DOM_WORKS` / `STYLE_INVALIDATION_WORKS` /
      `LAYOUT_INVALIDATION_WORKS` / `NODE_ID_MAP_CONSISTENT` / `SHADOW_DOM_WORKS`
- [ ] **C12**：`HTML5EVER_VERSION_FORK_IMPACT=BREAKING` 且修订 T2.3 结论的成本**高于**直接切 Stylo 路线
      → 判定不通过。（**成本判据需在 ADR 中给出可复核的量化依据**，例如「需要改动的 crate 数 + 需要重跑的 PoC 数」，
      不接受「感觉更麻烦」。）

**切换路线的判据（T2.4 不通过时，同一份 ADR 内必须给出）**
- [ ] **C13**：ADR 必须写出切到 **Stylo + Taffy + Parley** 时的**具体差异清单**：
      哪些能力在直连路线下需要自建（技术方案 §8「真正要补的布局环节」原文列举：
      DOM → 样式树 → box/fragment tree 转换、匿名盒、inline formatting、字体回退、滚动、
      绝对/固定定位、transform、hit testing、table/float/writing-mode/复杂分页、SVG 几何、
      替换元素、表单控件）。
- [ ] **C14**：ADR 必须给出「业务接口保持不变」的**验证方式**（技术方案 §8 的原话要求）。
      即：两条路线下 `LayoutBackend` 的对外接口应一致——PoC 至少要证明**接口面不随路线变化**。
- [ ] **C15**：ADR 必须明确记录「该替代会增加集成工作，应视为技术闸门结果，**不以『已有轮子』掩盖成本**」
      （技术方案 §8 原话），并给出成本估计的**依据**而非结论。

**落档**
- [ ] **C16**：结论落一份 ADR，结构对齐 ADR-0006 / ADR-0007
      （问题 / 实测方法 / 关键发现（实测原文）/ 结论 / 对架构的直接影响 / 附带 API 约束 / 机器可验证）。
- [ ] **C17**：探针输出风格对齐既有探针：`ASSERT_PASS(x)` / `ASSERT_FAIL(x)` + 单行汇总
      （建议 `T2_4_PROBE_OK` / `T2_4_PROBE_FAIL`）。

### 边界（不做什么）

- **不依赖 `blitz` 顶层 crate**（技术方案 §3「不整包依赖 GUI」）。只用 `blitz-dom` + `blitz-traits`
  （+ 必要的 `stylo*`）。这是硬边界，见 C2。
- **不实现 Shadow DOM 的全部规范行为**。本 PoC 只验「作用域隔离 + slot 分发」。
  以下均**不在**本 PoC 范围（属技术方案 §7 / G2）：事件 retargeting、`composedPath`、`slotchange`、
  选择器作用域的完整实现、Custom Elements 与 reaction queue。
- **不做完整布局正确性验证**。技术方案 §8 明确「样式解析成功、计算值正确与实际布局正确是三个独立验收层」，
  且「『内部数字自洽』只能辅助排错，最终必须比较规定场景的浏览器结果」。
  本 PoC 只验「变更 → 失效 → 重算」链路成立，**不**做像素级 Chromium 差分（那是 T1.3 的差分 harness 与 G3 的事）。
- **不做 font fallback / 字体栈验证**。技术方案 §18 把「字体」列为 G3 的必产出。
  但需注意 `parley` 依赖 `skrifa` 做字体处理，**首次运行可能需要系统字体**，跨平台断言（C1）须记录字体依赖情况。
- **不引入** `deny.toml` 之外的新许可类别。已核实 `blitz-dom` 为 MIT OR Apache-2.0（在 `deny.toml:35-51` 白名单内）；
  但传递依赖中 **`stylo` 系列为 MPL-2.0**（在白名单内，`deny.toml:45`），
  **Taffy 为 MIT**、**parley 为 Apache-2.0 OR MIT**（均在白名单内）。仍需 `cargo deny check` 实测确认。
- **不改** `deny.toml`（铁律 7）。若传递依赖引入白名单外的许可，**上报**，不自行扩白名单。
- **不做** 真实渲染 / GPU / 窗口。PoC 只到「布局数值可读」为止。

### 已知风险

| 风险 | 严重度 | 影响 | 缓解 |
|---|---|---|---|
| **html5ever 0.27 vs 0.39 版本分叉**（本文档核实发现） | **高** | ADR-0006 的四条结论在 0.39 上可能不成立；若成立性未知就切到 `blitz-dom`，`navigation` crate 会按错误的假设实现 | C4/C5 强制复核；`BREAKING` 时登记 T2.3 修订项 |
| **beta 版依赖（`0.3.0-beta.2`，而稳定版停在 `0.2.4`）** | **高** | semver 不保证 API 稳定；`0.3.0` 正式版发布时可能改 API，`blitz-traits`/`stylo_taffy` 用 `=0.3.0-beta.2` **精确锁定**说明上游自己也还在动 | ADR 必须记录精确版本 + 发布日期；技术方案 §19 要求「保存 Cargo.lock、Stylo/Blitz revision」 |
| **Linux x86_64 可用性未实测** | **高** | `parley`/`skrifa` 涉及字体栈，`objc2` 虽仅 macOS，但字体/文本栈在 Linux 上的行为可能不同 | C1/C3 强制在远程 `~/mmv8` 跑通（`AGENTS.md:72`） |
| **MSRV 1.89 与仓库声明 1.85 冲突** | 中 | 若 CI 矩阵下探到 1.85–1.88 会失败；当前 CI 只跑 stable + 1.92.0，暂不触发 | 登记为 D-03 的延续决策（Q6） |
| **投影成本被低估** | 中 | 技术方案 §8 说「规范 DOM 向后端投影必要节点和变更」，但没说切口。若需要全量镜像，等于重写 DOM | C6/C8 要求证明**最小切口**成立；若必须全量镜像，这是「通过但成本极高」，ADR 必须显式记录 |
| **`blitz-traits` / `stylo_taffy` 精确版本锁定** | 中 | `=0.3.0-beta.2` 意味着**无法与其他版本共存**，未来升级 Stylo 时会被卡住 | ADR 必须记录该锁定事实，作为技术方案 §19「升级 Stylo 或 Blitz 时通过适配层消化变动」的输入 |
| **`rayon` 传递依赖** | 低 | `blitz-dom` 非 optional 依赖 `rayon ^1`，可能引入线程池，与「页面线程单一」的调度模型是否冲突待查 | 列入待确认问题 |
| **Bash 只依赖可获取性，不覆盖完整集成** | 中 | 只做「能 fetch + 能编译」不足以支撑 G3 决策 | 本 PoC 的 C4~C10 才是路线裁决依据；C1~C3 只是门槛 |

### 待确认问题

1. **是否直接采用 `html5ever 0.39`/`0.40`，把 T2.3 的结论在新版本上重新确认？**
   还是继续用 `0.27` 并放弃 `blitz-html`？这决定 `navigation` 与 `layout-backend` 的版本基线，
   **必须在写 G1/G2 代码前定**（建议：这条升级到与 G1-PRE-1 同批次决策）。
2. **若采用 Blitz，`stylo 0.20` 与仓库其他部分是否统一到 0.22？**
   已核实 `stylo` 最新为 **0.22.0（2026-09-30）**，许可 MPL-2.0、edition 2024（`AGENTS.md` 无 edition 限制）。
   但 `blitz-dom 0.3.0-beta.2` 锁 `stylo ^0.20.0`。跟随 Blitz 还是跟随最新，**是两难**，需决策。
3. **`blitz-dom` 归入哪个 crate？** 技术方案 §16 的分组是 `web-style` / `layout-backend` / `graphics`。
   按 §8「LayoutBackend 首选封装 Blitz」，似乎是 `layout-backend` 依赖 `blitz-dom`。
   但 §16 又说「可选重型后端」。是否需要 feature flag 使 Blitz 可选？（影响 wheel 体积与 `deny.toml` 的 `all-features = false` 口径）
4. **`NodeId` 映射表的归属**（与 G1-PRE-1 的 Q3 同一问题）。T2.4 需要它，G1 也需要它。**建议合并决策。**
5. **`rayon` 与单一页面线程模型是否冲突？**（技术方案 §6 的调度对象表未提 rayon）
6. **Windows 是否在支持范围内？** 本项目发布面向 manylinux x86_64 + mac；已核实 `objc2` 仅 apple，
   但 Linux 之外未评估。跨平台纪律目前只要求 mac + linux x86_64（`deny.toml:16-20` 的 targets 列表）。

---

## 4. T2.5 —— V8 Inspector 暂停消息泵与 Python 回调死锁边界

### 背景（为什么做）

- 原始表述（handoff §7 T2 第 5 条）：「V8 Inspector 暂停消息泵与 Python 回调的**死锁边界**」。
- 技术方案 §13 原文（关键，抄录）：

  > 「Runtime、Debugger、Profiler 和 HeapProfiler 优先接 V8 Inspector，保证局部变量、闭包、call frame 求值和
  > 单步行为来自真正的引擎调试器。**暂停期间页面线程运行受控的 Inspector 消息泵，不推进普通页面任务，
  > 避免网络事件在暂停中任意改变页面。**」

- 技术方案 §6 末段（关键，抄录）：

  > 「执行超时由独立 watchdog 请求 V8 终止；终止后的任务明确标记失败并销毁受影响执行单元。
  > **同步宿主回调必须有截止期限，禁止回调中再次同步 eval 同一 Context。**」

- 已知前置：T2.0 已验证「宿主回调注入」可用 —— `poc/t2_0_v8_embed` 实测
  `PROBE_CALLBACK host fn invoked, argc = 3`、`PROBE_CALLBACK_RESULT = callback-ok`、
  `PROBE_THROW propagated as None`。

**为什么这一项不是「跑通就行」**

「死锁边界」是一个**测绘任务**，产出是**一张条件表**：在什么条件下会死锁、在什么条件下不会。
只报告「跑通了」等于没有回答问题——因为死锁通常在特定交错下才出现。

更关键的是：**死锁在测试里表现为「测试挂住」，而不是「测试失败」**。
若没有超时与对照实验，一个真正会死锁的探针会表现为「CI 卡住」，无法产出可判定的结论。
**因此本 PoC 的验收标准里，「超时机制」本身是交付物的一部分，不是可选项。**

**已核实的关键 API 事实（`v8-152.2.0` 源码实测，让边界测绘有了明确的实验变量）**

| 事实 | 原文/位置 | 对死锁边界的意义 |
|---|---|---|
| **暂停时 V8 会回调宿主的同步函数** | `src/inspector.rs:583-586`：`pub trait V8InspectorClientImpl { fn run_message_loop_on_pause(&self, context_group_id: i32) {} ... }` | **这就是死锁入口**。V8 暂停时**同步调用**宿主的 `run_message_loop_on_pause`。默认实现是**空函数体**（不泵消息 → 暂停不被处理） |
| 退出消息泵的函数 | `src/inspector.rs:537` `quit_message_loop_on_pause(&self)` | 宿主用它解除暂停。**源码中无任何线程限制声明**（rustdoc 为空），因此**不得**写成「只能从另一线程调用」——见 D0a 的伪命题说明 |
| 启动消息泵 | `src/inspector.rs:528` `run_message_loop_on_pause`；`src/inspector.rs:543` `run_if_waiting_for_debugger` | 「泵」是宿主侧责任 |
| 触发暂停的手段 | `src/inspector.rs:908` `schedule_pause_on_next_statement`；`src/inspector.rs:924` `cancel_pause_on_next_statement` | 有**程序化**的暂停触发手段，不必依赖外部 debugger 客户端 |
| 协议通道 | `src/inspector.rs:449-453` `pub trait ChannelImpl { fn send_response(...); fn send_notification(...); fn flush_protocol_notifications(...); }` | 暂停事件通过 Channel 送到宿主；宿主在**同一线程**处理 |
| Inspector client 与 Channel 的生命周期约束 | `src/inspector.rs:656-664`：「V8 will hold onto a raw pointer to the RawV8InspectorClient, so we need to make sure it stays **pinned**」；`ChannelHeap` 同样含 `PhantomPinned` | 这两个对象**不能随意移动**，且 V8 持有裸指针 —— 生命周期设计不当会直接是 UB |
| 跨线程访问 Isolate 的手段 | `src/locker.rs` 文档：「Support for sharing an isolate between threads, one thread at a time, via the `v8::Locker` API」 | **死锁的第二个来源**：多线程 + Locker 的锁序 |
| 宿主回调注入已验证 | `poc/t2_0_v8_embed/src/main.rs` | 死锁的**第三个来源**：暂停期间若宿主回调进 Python，而 Python 侧又持有 GIL 并等待页面线程 |

**综合：死锁边界至少有三条独立的候选路径**
1. **GIL 路径**：暂停消息泵在页面线程上同步等待 Python 侧决策，而 Python 侧持 GIL 并在等页面线程 → 互等。
   （技术方案 §16 原文已给出对策：「宿主回调使用 PyO3 attach/detach 机制，**避免持 GIL 等待页面线程**」——
   但**对策本身需要验证**，这正是 T2.5 要做的。）
2. **重入路径**：宿主回调中再次同步 `eval` 同一 Context → 技术方案 §6 明文「**禁止**」。
   PoC 需验证这条禁令的**实际后果**（是死锁？是 V8 FATAL？是可捕获的错误？）。
3. **锁序/Locker 路径**：页面线程持 Locker 暂停中，另一线程要拿 Locker 才能发消息 → 自锁。

> **勘注**：本探针的实际构造与本节原始描述不同——`run_message_loop_on_pause` 的签名只有 `&self` + `i32`，
> 且 `Local::from_non_null` 是 `pub(crate)`，**泵内无法从零构造 `Local<Context>`，因此无法从该入口重入 eval**；
> 探针改由**宿主函数回调**构造重入。结论与偏离理由见 `docs/adr/0012`（第 342 行附近）。
> （本条为指针，不重述 ADR 内容；D5 判据语义不变，仍按「重入路径的实际后果已定性」验收。）

### 要回答的问题

- **Q1（泵的语义）**：`run_message_loop_on_pause` 的默认实现是**空函数体**。
  若宿主什么都不做，暂停会怎样（永久挂起？自动继续？）？**实测**给出行为，不要推断。
- **Q2（条件表）**：死锁是否发生，取决于哪些**可枚举**的条件？
  至少覆盖这三个因子：
  - **暂停期间宿主是否尝试获取 GIL**（是 / 否）
  - **宿主回调是否重入同一 Context 的 eval**（是 / 否）
  - **是否有第二个线程参与**（无 / 有，仅发消息 / 有，需 Locker）
- **Q3（每种组合的可观测结果）**：对每个组合，输出下列**之一**并给出证据：
  `DEADLOCK` / `FATAL` / `ERROR_PROPAGATED` / `CLEAN_RESUME` / `TIMEOUT`。
  （参考 T2.0 已实测 `PROBE_THROW propagated as None`，说明异常路径是可观测的。）
- **Q4（超时机制）**：如何在探针里实现**可靠的超时**，使得「死锁」被报告为一个**结论**而不是一个挂住的进程？
- **Q5（对策验证）**：技术方案 §16 给的「PyO3 attach/detach，避免持 GIL 等待页面线程」是否真的避免死锁？
  §6 给的「同步宿主回调必须有截止期限」如何落地（谁设截止期限？超时后如何中断？）？
- **Q6（暂停期间页面任务的正确行为）**：技术方案 §13 要求「暂停期间运行受控的消息泵，
  **不推进普通页面任务**」。PoC 需验证：暂停期间普通 task 是否真的没被执行
  （例如：暂停前排一个 `setTimeout`，暂停期间它不应触发）。
- **Q7（pinned 生命周期）**：`V8InspectorClient` / `Channel` 含 `PhantomPinned`，V8 持裸指针。
  在 PoC 的实验构造中，是否会引入 UB？（这是 `unsafe` 白名单问题，ADR-0008 已把 `poc/` 加入白名单，
  但每个 unsafe 仍须带 `SAFETY:`。）

### 验收标准（可判定，二值）

**D0 前置：超时机制本身必须先被证明有效（这是本 PoC 的第一验收项）**
- [ ] **D0**：探针必须包含一个**故意会挂起**的场景，并证明**超时机制能把它报告为结论而非挂住进程**。
      断言输出形如 `WATCHDOG_FIRED=true` 与 `HUNG_SCENARIO_REPORTED=DIMAGINATION`（或该场景的真实分类）。
      **理由**：若超时机制不可靠，本 PoC 的所有「不死锁」结论都无法与「没测出来」区分。
      （对齐 ADR-0007 的方法论：「判定方法（这一段比结论更重要）」。）
- [ ] **D0a（watchdog 的中断手段，P0）**：watchdog 终止执行**建议走 `IsolateHandle`**
      （经 `Isolate::thread_safe_handle()` 取得，见 `isolate.rs:1055`）——
      理由是它为「**免持 Locker 的跨线程调用**」而设计（`locker.rs:170-172`：
      "A handle usable from any thread without holding the lock, for
      `IsolateHandle::terminate_execution` and `IsolateHandle::request_interrupt`."）。
      断言 `WATCHDOG_USES_ISOLATE_HANDLE=true|false`，**如实报告探针实际走的路径即可**。
      **⚠ 明确排除一条伪命题（2026-10-12 修正）**：v8 crate 里**不存在**「同线程调用
      `terminate_execution` 无效」这一限制，**不得**把它写成断言或对照实验。
      核实依据（本地 `v8-152.2.0` 源码）：
      - `Isolate::terminate_execution`（`isolate.rs:1066`）与
        `IsolateHandle::terminate_execution`（`isolate.rs:2616`）**调用的是同一个 C 函数**
        `v8__Isolate__TerminateExecution`（`binding.cc:423-425`，实现就是
        `isolate->TerminateExecution();`）；
      - 两者 rustdoc 中**均无**「同线程无效 / 只能在另一线程调用」的表述；
      - `IsolateHandle` 的 rustdoc 原文是「**can be used by any thread** even if that thread has
        not acquired the V8 lock」——是**放宽**（任何线程都可用），不是**限制**。
      ⇒ 结论：「同线程无效」无任何源码依据，属早期误传，已废止。
- [ ] **D0b（终止后的恢复路径）**：`terminate_execution` 之后，若要继续用该 Isolate，
      需 `cancel_terminate_execution`（`isolate.rs:2639`，其文档指出被强制终止后
      "the isolate can not resume execution **until all JavaScript frames have propagated the
      uncatchable exception**"）。
      ⇒ 若探针在终止后复用 Isolate，必须断言 `CLEAN_RESUME_AFTER_TERMINATE=true|false`；
      否则应在边界中声明「终止后不复用」，不得留悬空状态。

**A 组：不死锁的对照实验（每个组合都要有）**
- [ ] **D1**：至少覆盖 §Q2 的 **3 个因子 × 全组合**或**明确说明为何某些组合不可测**，
      每个组合输出一行分类结论。断言汇总 `COMBOS_TRIED=<n>` 与 `COMBOS_DEADLOCK=<m>`。
- [ ] **D2**：必须存在**至少一个「不死锁」的对照**（`CLEAN_RESUME` 或 `ERROR_PROPAGATED`），
      与至少一个「会死锁/会失败」的对照形成**成对证据**。
      **只有「全部不死锁」或「全部死锁」的探针，无法回答「边界在哪」。**
- [ ] **D3**：每个「不死锁」结论必须有可观测证据（例如 `RESUMED_AT=<ms>`、
      `CALLBACK_COMPLETED=true`），不接受「没卡住」这种消极证据。

**B 组：三条死锁路径各自定性**
- [ ] **D4（GIL 路径）**：在暂停期间尝试获取 GIL 的场景，输出分类结论 + 证据。
- [ ] **D5（重入路径）**：宿主回调中重入同一 Context 的 `eval`，输出分类结论 + 证据。
      **明确回答**技术方案 §6「禁止回调中再次同步 eval 同一 Context」的**实际后果是什么**
      （死锁 / V8 FATAL / 可捕获异常 / 静默错误）。
- [ ] **D6（锁序/Locker 路径）**：若有第二线程参与，输出分类结论 + 证据；
      若 PoC 不覆盖跨线程，明确写「未覆盖」并登记为遗留（**不得默认安全**）。

**C 组：技术方案约束的验证**
- [ ] **D7**：暂停期间普通页面任务**未被推进** → `PAGE_TASKS_STALLED_DURING_PAUSE=true|false`
      （对应 §13「不推进普通页面任务」）。
- [ ] **D8**：暂停可被正常解除（不残留挂起状态） → `CLEAN_RESUME=true|false`，
      且解除后 Isolate 可继续执行脚本。
- [ ] **D9**：若实验构造含 `unsafe`，每个 `unsafe` 块带**紧邻的 `// SAFETY:` 注释**
      （ADR-0008 决策 4 记录了「`SAFETY:` 必须在 unsafe 行前 2 行内」的守卫窗口约定）；
      `bash scripts/check-unsafe-whitelist.sh` 输出 PASS。

**落档**
- [ ] **D10**：结论落一份 ADR，结构对齐 ADR-0006 / ADR-0007，**且必须包含一张条件表**
      （行 = 实验组合，列 = 是否死锁 / 分类 / 证据）。
- [ ] **D11**：ADR 必须给出**对架构的直接影响**，至少覆盖：
      - `scheduler` crate 的消息泵设计（技术方案 §6 属高风险路径，铁律 6 要求决策上报）
      - PyO3 回调边界的线程模型（§16 的 attach/detach 对策是否被采纳）
      - 「同步宿主回调必须有截止期限」的实现位置
- [ ] **D12**：探针输出风格对齐既有探针：`ASSERT_PASS(x)` / `ASSERT_FAIL(x)` + 单行汇总
      （建议 `T2_5_PROBE_OK` / `T2_5_PROBE_FAIL`）。

### 边界（不做什么）

- **不实现** 完整 CDP / DevTools 协议。技术方案 §13 明确「DOM、CSS、DOMDebugger、Network、Page、
  Storage、DOMStorage、IndexedDB 与 Target 等域**由宿主自行提供**」「不能因为 WebSocket 连上就宣称完整 CDP」。
  本 PoC 只验**暂停/消息泵/回调的边界**，不实现任何 CDP 域。
- **不实现** WebSocket / 调试器客户端。需要程序化暂停时用
  `schedule_pause_on_next_statement`（`src/inspector.rs:908`），不接外部调试器。
- **不做** GIL 之外的 Python 侧真实多线程压测。可以用 Python 线程构造最小交错，
  但不做压力测试或长跑。
- **不做** Worker / 多 Isolate 并发下的死锁测绘。T2.5 是**单页面线程 + 必要第二线程**的最小边界；
  完整并发死锁属 G4/G6。
- **不改** `sdk-python` / `debug` crate（均为 T0 占位）。本 PoC 只在 `poc/` 内工作。
- **不实现** watchdog 的生产形态。探针里的超时是**验证工具**，生产实现属 `scheduler`。
  但 ADR 必须写明「探针用的超时机制如何迁移到生产」这一衔接点。
- **不承诺** 「已找到全部死锁路径」。验收标准里的「组合」是**当前可枚举的组合**；
  ADR 必须显式列出**未覆盖的路径**（对齐 ADR-0008 决策 3「显式写出已知局限」的纪律）。

### 已知风险

| 风险 | 严重度 | 影响 | 缓解 |
|---|---|---|---|
| **死锁在测试中表现为「挂住」而非「失败」** | **高** | 探针/CI 卡死，无法产出结论；远程 3.6 GB 机器上还可能拖垮其他任务 | D0 把「超时机制有效」列为**第一验收项**；探针必须有硬超时 + 明确的 `TIMEOUT` 分类输出 |
| **实验构造引入 UB（pinned 生命周期）** | **高** | `V8InspectorClient`/`Channel` 含 `PhantomPinned` 且 V8 持裸指针；构造不当直接是 UB，且 UB 可能表现为「随机崩溃」而非稳定现象 | D9；每个 unsafe 带 `SAFETY:`；建议先跑最小构造再叠加实验 |
| **单次实验的交错不可复现** | **高** | 死锁是概率性的；一次「没死锁」可能只是运气好 | D1 要求**多组合**且 D2 要求**成对证据**；建议同一组合重复 N 次并报告次数（对齐 T2.1 EXP1/EXP2/EXP3 的分实验结构） |
| **只覆盖单 Isolate，结论被误推广到多 Context** | 中 | 技术方案 §5 提到多 Context 共享 Realm 的场景 | 边界中显式声明「未覆盖」；ADR 登记遗留 |
| **远程机器 3.6 GB 内存下多线程实验 OOM** | 中 | `AGENTS.md:67` 已记录「链接 V8 若 OOM」的历史问题 | 本 PoC 只需单 Isolate + 少量线程，内存压力小；若 OOM，**走决策上报**，不许自行改远程系统配置（`AGENTS.md:67`） |
| **「无死锁」被误读为「架构安全」** | 中 | 维护者不读 Rust，容易把 PoC 结论当成承诺 | ADR 必须用「**在已覆盖的 N 种组合下未观测到死锁**」的表述，**禁止**写成「无死锁风险」 |
| **铁律 6 触发** | 中 | scheduler（消息泵/暂停恢复/时序）与 PyO3 回调边界均在铁律 6 高风险路径清单内 | D11 要求 ADR 给出对架构的直接影响；若结论要改 `scheduler` 设计，须走决策上报协议（AGENTS.md 决策上报协议第 3 条：≥2 备选方案 + 证据原文） |

### 待确认问题

1. **「Python 回调」在本 PoC 中如何构造？** T2.0 已验证 Rust 侧的宿主函数注入
   （`Function::new`），但**真正的 Python 回调**需 PyO3。PoC 是否引入 `pyo3` 依赖？
   - 若引入：远程机器需能编译 PyO3（`uv` 已装，但 PyO3 需 `python3` 开发头文件）。
   - 若不引入：只能验「Rust 宿主函数」侧的边界，**Python GIL 这一条最关键的死锁路径就测不到**。
   **这是本 PoC 最重要的待确认项**，建议在开工前定。
2. **`run_message_loop_on_pause` 的正确默认行为是什么？** 默认空实现意味着「不泵消息」。
   技术方案要求「暂停期间运行**受控的**消息泵」——那么「受控」的具体语义（泵哪些消息？
   `Network` 事件在暂停中到达怎么办？）需要架构师给出，本 PoC 按给定的语义验证。
3. **超时/截止期限由谁设？** 技术方案 §6 说「同步宿主回调必须有截止期限」「执行超时由独立 watchdog 请求 V8 终止」。
   watchdog 是**宿主线程**还是**独立线程**？这影响 T2.5 的实验构造方式。
4. **是否需要在探针里验证 `v8::Locker`？** 若 Q1 的决策是不引入跨线程，则 D6 记为「未覆盖」。
   但若架构上确实需要多线程访问 Isolate，这个空白会成为 G4 的风险。**建议本 PoC 至少覆盖 D6 的最小形态。**
5. **与 T2.2 的耦合**：T2.2 若采用「JS 侧 Date 兼容层」，则暂停期间日期相关行为是否需要一并验证？
   建议**解耦**（T2.2 不含暂停语义，T2.5 不含日期语义）。

---

## 5. 需求池表（P0 / P1 / P2）

### P0 —— 必须有（缺失则「完成所有开发」不成立）

| # | 需求项 | 对应任务 | 二值判据 |
|---|---|---|---|
| P0-1 | G1-PRE-1 的 ADR 落档，且 A3~A8 七条约束条款齐备 | G1-PRE-1 | A1~A8 全为 ✅ |
| P0-2 | 依赖方向有唯一、无歧义的结论 | G1-PRE-1 | A3 |
| P0-3 | `RealmId` / `JsHandle` / `HostValue` 三个类型各有且仅有一个定义点 | G1-PRE-1 | A4 |
| P0-4 | 「V8 类型不得越出 `engine-v8`」有**机器可执行**的门禁落点 + 自证方式 | G1-PRE-1 | A5 + A8 |
| P0-5 | T2.2 的方法自证（基线对照）成立，否则隔离结论不可信 | T2.2 | B0c |
| P0-6 | T2.2 给出 `TZ_PER_PROFILE_ISOLATED` 与 `DATE_NOW_ISOLATED` 两个二值结论，且证伪时附 process-global 证据 | T2.2 | B2/B3/B4 |
| P0-7 | T2.2 给出替代路线的实测结论（「失败也要写清失败点与替代路线」） | T2.2 | B5/B6 |
| P0-8 | T2.4 的 Linux x86_64 可用性实测通过 | T2.4 | C1 + C3 |
| P0-9 | T2.4 的通过/不通过判据给出唯一结论（走 Blitz 或切 Stylo+Taffy+Parley），不接受「部分通过」 | T2.4 | C6~C11 全满足或 C11 触发 |
| P0-10 | **html5ever 0.27 / 0.39 版本分叉的影响已定性** | T2.4 | C4 + C5 |
| P0-11 | T2.5 的超时机制先被证明有效（死锁被报告为结论而非挂住） | T2.5 | D0 |
| P0-12 | T2.5 存在**成对证据**（至少一个不死锁 + 一个会死锁/失败的对照） | T2.5 | D2 |
| **P0-13** | **T2.2 的时区设置在首次求值前完成，且通知传 `Skip` 不传 `Redetect`**（否则假阴性） | T2.2 | **B0a + B0b** |
| **P0-14** | **T2.4 的 C9 不得写成恒假断言**，须用 `catch_unwind` 让 panic 成为正面证据，并有阳性对照 | T2.4 | **C9a** |
| **P0-15** | **T2.5 的 watchdog 走 `IsolateHandle` 并如实报告实际路径**（免持 Locker 的跨线程调用） | T2.5 | **D0a** |
| T2.x | **四项任务的结论各自落一份 ADR**，结构对齐 ADR-0006/0007/0008 | 全部 | B8 / C16 / D10 |
| T2.x-e | **证据台账有原文**（铁律 9：无证据不得标 `done`） | 全部 | QA 回写 `docs/TASKS.md` |

### P1 —— 应该有（缺失会留下已知盲区，但可登记遗留）

| # | 需求项 | 对应任务 |
|---|---|---|
| P1-1 | G1-PRE-1 的 `JsHandle` 与 ADR-0007 泄漏结论的相容性条款 | G1-PRE-1（A6） |
| P1-2 | `bindings-webidl` 的 unsafe 归属结论（生成代码能否含 unsafe） | G1-PRE-1（A7） |
| P1-3 | T2.2 记录「`Date::new(scope, f64)` 不可用于测时钟隔离」这一坑 | T2.2（B7） |
| P1-4 | T2.2 覆盖 `set_default_time_zone` 返回 `false` 的失败分支（`Etc/Unknown` 会被拒） | T2.2 |
| P1-5 | T2.2 在远程 Linux 复跑一次（tzdata 依赖） | T2.2 |
| P1-6 | T2.4 证明「后端树只是缓存」（删缓存可重建） | T2.4（C10） |
| P1-7 | T2.4 不依赖 `blitz` 顶层 / `blitz-shell`（对齐技术方案 §3） | T2.4（C2） |
| P1-8 | T2.4 的切换路线给出「业务接口保持不变」的验证方式 | T2.4（C14） |
| P1-9 | T2.4 记录 MSRV 1.89 与仓库 1.85 的冲突，登记为 D-03 延续 | T2.4（Q6） |
| P1-10 | T2.4 验证「暂停/失效链路」而非像素级正确（并说明为何不做差分） | T2.4（C7） |
| P1-11 | T2.5 三条死锁路径（GLI / 重入 / 锁序）各自定性 | T2.5（D4/D5/D6） |
| P1-12 | T2.5 验证「暂停期间普通页面任务不被推进」 | T2.5（D7） |
| P1-13 | T2.5 的条件表（行=组合，列=是否死锁/分类/证据） | T2.5（D10） |
| P1-14 | T2.5 显式列出**未覆盖**的死锁路径（不留假绿） | T2.5（边界） |
| P1-15 | 四项任务均给出「对架构的直接影响」章节 | 全部 |
| P1-16 | `poc/README.md` 与 TASKS.md 的状态表随结论同步更新 | 全部（QA） |

### P2 —— 可以有（锦上添花，或明确登记为遗留）

| # | 需求项 | 对应任务 |
|---|---|---|
| P2-1 | T2.2 顺带验证 iv8 兼容模式的「读取自动递增时间」（若架构师认为应在本轮） | T2.2 |
| P2-2 | T2.2 覆盖 `Intl.DateTimeFormat` 的时区表现 | T2.2 |
| P2-3 | T2.4 覆盖 font fallback 的最小形态 | T2.4 |
| P2-4 | T2.4 评估 Windows 可用性（本项目当前只需 mac + linux x86_64） | T2.4 |
| P2-5 | T2.5 覆盖 `v8::Locker` 跨线程访问 Isolate 的完整形态 | T2.5 |
| P2-6 | T2.5 覆盖多 Context / 多 Worker 的并发死锁 | T2.5 |
| P2-7 | 把三项 PoC 的断言收敛成 `runner` 的正式契约测试（而非仅 `poc/` 一次性） | 全部 |
| P2-8 | T2.4 评估 `blitz` 稳定版 `0.2.4`（而非 beta）是否更稳 | T2.4 |

---

## 6. 依赖顺序建议

### 6.1 图示

```
                    ┌─────────────────────────────────────────────┐
                    │  G1-PRE-1（ADR：类型面 + 依赖方向 + 门禁）    │
                    │  —— 纯文档任务，无代码前置                    │
                    └───────────────┬─────────────────────────────┘
                                    │
                    【G1 代码的硬前置】必须先落 ADR
                                    │
        ┌───────────────────────────┼───────────────────────────┐
        │                           │                           │
        ▼                           ▼                           ▼
┌───────────────┐        ┌───────────────────┐       ┌──────────────────┐
│ T2.2 时钟/时区 │        │ T2.4 Blitz 集成   │       │ T2.5 死锁边界    │
│ 前置：T2.0 ✅  │        │ 前置：无（可选做   │       │ 前置：T2.0 ✅    │
│               │        │  跨平台可行性预筛）│       │  ⚠ 待定：是否引入 │
│ 可独立进行    │        │ 可独立进行         │       │  PyO3（见待确认1）│
└───────┬───────┘        └─────────┬─────────┘       └────────┬─────────┘
        │                          │                          │
        │        ⚠ 发现新耦合：     │                          │
        │◀─────────────────────────┘                          │
        │   html5ever 0.27(T2.3已定) vs 0.39(blitz-html 所需)   │
        │   → 建议升级为「与 G1-PRE-1 同批次决策」              │
        │                                                          │
        └──────────► T2.4 的「切 Stylo」路线 ◄──────────────────┘
                     （T2.4 不通过时的备选，需独立验证）
```

### 6.2 谁先做

| 序 | 任务 | 理由 |
|---|---|---|
| **1** | **G1-PRE-1**（ADR） | 纯文档、零代码前置，且是 G1 的**硬前置**。技术方案 §18 的 G1 必产出含「原生绑定」，而 `bindings-webidl` 就是原生绑定。ADR 未落就写 G1 代码 = 直接违反铁律 5 且无门禁可拦。**成本最低、影响最大，必须第一。** |
| **2** | **T2.2** 与 **T2.5**（可并行） | 二者共用已就绪的 T2.0 前置；探针互不干扰（ADR-0008 已确立「独立 Cargo workspace」形态）；**互不依赖**。 |
| **3** | **T2.4**（与 T2.2/T2.5 并行启动，但**裁决**需等 C4/C5 的分叉结论） | 依赖可获取性验证（fetch + Linux 编译）可立即开始；但「走 Blitz 还是切 Stylo」的最终裁决，取决于 html5ever 版本分叉的定性，而那需要一轮决策。 |

### 6.3 谁可并行

- **T2.2 ∥ T2.5**：完全并行。无共享文件、无共享探针 workspace、无依赖关系结论。
- **G1-PRE-1 ∥ 任意 PoC**：ADR 是文档，PoC 是独立 workspace 探针，物理上无冲突。
  唯一耦合是**概念耦合**：G1-PRE-1 的 Q3（`NodeId` 映射表归属）与 T2.4 的 Q4 是同一问题，
  **建议合并决策**而不是分别回答（否则会出现两个真相源）。
- **T2.4 ∥ T2.2/T2.5**：并行。但**建议先做一个「依赖可获取性预筛」**（`cargo add blitz-dom` +
  Linux 目标编译），因为若 Linux 直接编不过，T2.4 的全部 PoC 工作都要改为「验证 Stylo 路线」，
  早点知道能省掉整轮工作。这个预筛成本很低（就是 C1/C2 两条断言）。

### 6.4 谁依赖谁的前置结论

| 依赖方 | 依赖的结论 | 来自 | 若结论不同则… |
|---|---|---|---|
| **G1 全部代码** | A3 依赖方向 + A4 三类型归属 + A5 门禁落点 | G1-PRE-1 | 无 ADR 则违反铁律 5，且无门禁（最坏情况：泄漏在 G2 才暴露） |
| **T2.4 的裁决** | C5 `HTML5EVER_VERSION_FORK_IMPACT` | T2.4 自身的 C4 复核 | 若 `BREAKING` 且修订成本高 → 直接切 Stylo 路线（技术方案 §8 已授权） |
| **`navigation` crate（G2）** | T2.3 结论是否需按 html5ever 0.39 修订 | T2.4 的 C4/C5 | 若需修订 → ADR-0006 要出「修订版」而非原地改（铁律 2：golden/结论只能独立 PR 改） |
| **`scheduler` crate（G1/G2）** | D11 的消息泵设计影响 | T2.5 | scheduler 属铁律 6 高风险路径，改设计须走决策上报协议 |
| **`sdk-python`（G1）** | `HostValue` 的 Python 侧转换点 | G1-PRE-1 的待确认 1 | 若 `sdk-python` 需依赖 `engine-v8`，则依赖方向要多一条边 |
| **T2.4 的 Shadow 结论（G2/G3）** | C9 的 Shadow 支持边界 | T2.4 | Shadow DOM 的 retargeting/`composedPath` 等属技术方案 §7，不在 T2.4 内，须另排 |
| **本需求文档本身** | T2.2 的「同一进程内是否应保证隔离」 | 待确认问题 2 | 若答案是「按进程池隔离」，T2.2 的正确产出是「**测出并接受 process-global**」，而非「修复它」 |

### 6.5 建议的推进节奏（无排期，仅表达依赖顺序）

1. **立即**：G1-PRE-1 写 ADR（架构师）。这是唯一「不做就会污染后续所有工作」的一项。
2. **同时**：T2.4 的依赖可获取性预筛（工程师，半天量级：只验 C1/C2）。
3. **并行**：T2.2、T2.5 两个探针（工程师，可分两个 PR 或一个 PR 两个 workspace）。
   T2.5 开工前须先定「是否引入 PyO3」（见 T2.5 待确认 1）——**这个决定会改变探针结构**，
   不宜边做边定。
4. **T2.4 裁决**：等 C4/C5 有结论后，出 ADR 定路线（Blitz 或 Stylo）。
5. **收口**：QA 按 P0 判据复核 + 回写 `docs/TASKS.md` 证据台账（铁律 9：无证据不得标 `done`）。

---

## 7. 汇总：待确认问题清单（需维护者或架构师拍板）

> 按 AGENTS.md 决策上报协议，这些属于「需要拍板的事」。
> 注意本轮已由维护者授权「AI 内部开多个子 agent 讨论 → 结论落 ADR → 依 ADR 执行」，
> 协议的**形式要求不变**（自包含、≥2 备选方案含代码原文、证据原文、本仓库约束条款、推荐标注"仅供参考"）。

| # | 待确认问题 | 归属 | 阻塞什么 | 本文档位置 |
|---|---|---|---|---|
| **Q-1** | **`bindings-webidl` ↔ `engine-v8` 的依赖方向** | 架构师 | G1 全部代码 | §1 Q1 / A3 |
| **Q-2** | **`JsHandle` 的定义是否会重新引入 ADR-0007 已证明会泄漏的强引用环** | 架构师 | G1 + T2.1 结论的有效性 | §1 Q3 / A6 |
| **Q-3** | **html5ever 0.27（ADR-0006 已定）vs 0.39（blitz-html 所需）—— 采用哪个** | 架构师 | T2.4 裁决、`navigation` crate 版本基线 | §3 问题 1 / C4、C5 |
| **Q-4** | **T2.5 的探针是否引入 PyO3**（不引入则测不到 GIL 这条最关键的死锁路径） | 架构师 + 工程师 | T2.5 探针结构 | §4 待确认 1 |
| **Q-5** | **「逻辑时钟」的权威归属 crate**（`scheduler`？）；**是否允许同进程内混用多时区** | 架构师 | T2.2 的正确结论形态 | §2 待确认 1、2 |
| **Q-6** | **`NodeId ↔ JsHandle` 映射表的归属**（G1-PRE-1 Q3 与 T2.4 Q4 是同一问题，**建议合并决策**） | 架构师 | G1 + T2.4 + T2.5 | §1 Q3 / §3 Q4 |
| **Q-7** | **采用 Blitz 是否需把 MSRV 从 1.85 提到 1.89**（D-03 延续） | 架构师 | CI 矩阵 / T2.4 | §3 Q6 / P1-9 |
| **Q-8** | **「受控的 Inspector 消息泵」的具体语义**（暂停中哪些消息被泵、`Network` 事件到达怎么办） | 架构师 | T2.5 的实验构造 | §4 待确认 2 |
| **Q-9** | **超时/截止期限由谁设**（宿主线程 or 独立 watchdog 线程） | 架构师 | T2.5 的实验构造 + `scheduler` 设计 | §4 待确认 3 |
| **Q-10** | **iv8 兼容模式的「读取自动递增时间」是否属 T2.2 范围**（建议排除） | 架构师 | T2.2 范围 | §2 待确认 3 |
| **Q-11** | **T2.4 是否需要 feature flag 使 Blitz 可选**（影响 wheel 体积与 `deny.toml` 的 `all-features = false` 口径） | 架构师 | G3 打包策略 | §3 待确认 3 |
| **Q-12** | **门禁「V8 类型越界」的已知局限是否可接受**（如经类型别名的间接引用当前手段检不出） | 架构师 | G1-PRE-1 的 A5/A8 | §1 待确认 4 |

---

## 8. 依赖核实附录（全部于 2026-10-12 核实）

### 8.1 crates.io 数据（经 crates.io API 实测）

| crate | 最新版本 | 发布日期 | 稳定版（max_stable） | 许可 | 备注 |
|---|---|---|---|---|---|
| `blitz-dom` | `0.3.0-beta.2` | 2026-08-24 | **`0.2.4`**（2025-10-22） | MIT OR Apache-2.0 | MSRV v1.89.0；共 26 个版本；未 yanked |
| `blitz-html` | `0.3.0-beta.2` | — | — | — | 依赖 `html5ever ^0.39.0`、`xml5ever ^0.39.0` |
| `blitz`（顶层） | `0.3.0-beta.2` | — | — | — | 依赖 `blitz-shell`、`anyrender_vello` 等 → **不采用** |
| `html5ever` | `0.40.1` | 2026-09-14 | `0.40.1` | — | `0.39.0` 发布于 2026-03-13；`0.27.0` 发布于 2024-03-22 |
| `markup5ever` | 见下 | — | — | — | 本机并存 `0.12.1`（配 html5ever 0.27）与 `0.39.0`（配 blitz） |
| `stylo` | `0.22.0` | 2026-09-30 | `0.22.0` | **MPL-2.0** | edition 2024；`blitz-dom 0.3.0-beta.2` 锁 `^0.20.0` |
| `taffy` | `0.14.0` | 2026-08-24 | `0.14.0` | **MIT** | MSRV 1.71；`blitz-dom` 要求 `^0.14.0`（**对齐**） |
| `parley` | `0.12.0` | 2026-10-09 | `0.12.0` | Apache-2.0 OR MIT | MSRV 1.88；`blitz-dom` 要求 `^0.11.1`（**落后一个次版本**） |

来源 URL：
- `https://crates.io/api/v1/crates/blitz-dom`
- `https://crates.io/api/v1/crates/blitz-dom/0.3.0-beta.2/dependencies`
- `https://crates.io/api/v1/crates/blitz-html/0.3.0-beta.2/dependencies`
- `https://crates.io/api/v1/crates/blitz/0.3.0-beta.2/dependencies`
- `https://crates.io/api/v1/crates/html5ever`
- `https://crates.io/api/v1/crates/html5ever/0.40.1/dependencies`
- `https://crates.io/api/v1/crates/stylo`
- `https://crates.io/api/v1/crates/taffy`
- `https://crates.io/api/v1/crates/parley`

### 8.2 本仓库既有事实（源码原文核对）

| 事实 | 出处 |
|---|---|
| 铁律 5：「V8 / rusty_v8 类型不得越出 `engine-v8` crate」 | `AGENTS.md:22` |
| 技术方案 §1 退出机制：「业务层只看 RealmId、JsHandle、HostValue 和调度接口」 | `docs/…技术方案.docx` §1；`engine-v8/src/lib.rs:11` 引用 |
| unsafe 白名单仅两个目录：`crates/engine-v8/`、`poc/` | `scripts/check-unsafe-whitelist.sh:43`；ADR-0008 决策 2 |
| **`workspace_policy.rs` 现有三条断言：成员集合 / engine-v8 lint 副本 / 白名单唯一性** —— **无一条检查 V8 类型越界** | `crates/runner/tests/workspace_policy.rs` |
| `bindings-webidl/Cargo.toml` 的 `[dependencies]` 为**空**（依赖方向尚未确立） | `crates/bindings-webidl/Cargo.toml` |
| `poc/t2_3_html5ever_pause/Cargo.toml` 依赖 `html5ever = "0.27"` | `poc/t2_3_html5ever_pause/Cargo.toml` |
| T2.0 前置实测输出 | `docs/TASKS.md:140-144` |
| 探针输出风格：`ASSERT_PASS(x)` / `ASSERT_FAIL(x)` + 单行汇总（`H5PROBE_OK`） | `poc/t2_3_html5ever_pause/src/main.rs:150-193` |

### 8.3 `v8-152.2.0` crate 源码事实（本机已下载，`~/.cargo/registry/src/rsproxy.cn-e3de039b2554c837/v8-152.2.0/`）

| 事实 | 出处 |
|---|---|
| `set_default_time_zone` 文档：「**This mutates process wide state and is not synchronized with isolates running on other threads**」 | `src/icu.rs:89-107` |
| `set_default_time_zone` 对无法识别的 id 返回 `false` 且不改变当前值；`Etc/Unknown` 自身也被拒 | `src/icu.rs:96-107` |
| `Date` 类型**只有** `Date::new(scope, f64)` 与 `value_of()` 两个方法 | `src/date.rs` 全文 |
| **无时钟注入 API**：对 `Clock`/`CurrentClockTimeMillis`/`set_clock`/`mock`/`fake_time` 全量检索零命中 | `src/` 全目录；`src/binding.cc` 中 Date 相关导出仅 `v8__Isolate__DateTimeConfigurationChangeNotification`、`v8__Value__IsDate` |
| `date_time_configuration_change_notification(TimeZoneDetection::{Skip, Redetect})` | `src/isolate.rs:2022`；枚举定义 `src/isolate.rs:116` |
| **V8 日期计算值有内部缓存**，通知用于重置缓存 | `src/isolate.rs:2013-2017` 原文："V8 keeps a cache of various values used for date / time computation. This notification will reset those cached values for the current context" |
| **`Skip` 与 `Redetect` 的差别（决定是否覆盖已设时区）** | `src/isolate.rs:106-108` 原文："kSkip indicates V8 that the notification should not trigger redetecting host time zone. kRedetect indicates V8 that host time zone should be redetected, **and used to set the default time zone**." |
| `set_default_time_zone` 文档明确要求配合 `Skip` 使用 | `src/icu.rs:104-106`："notify each isolate via `date_time_configuration_change_notification` with `TimeZoneDetection::Skip` so cached values are refreshed without re-detecting (and overwriting) the zone just set here" |
| **`terminate_execution` 有两个同名方法，但调用同一个 C 函数、无线程限制** | `Isolate::terminate_execution`（`isolate.rs:1066`）与 `IsolateHandle::terminate_execution`（`isolate.rs:2616`）**均调用** `v8__Isolate__TerminateExecution`（`binding.cc:423-425`：`isolate->TerminateExecution();`）。两者 rustdoc **均无**同线程限制；`IsolateHandle` 版的 rustdoc 是**放宽**表述：「can be used by **any thread** even if that thread has not acquired the V8 lock with a Locker object」。**「同线程调用无效」不成立。** |
| `IsolateHandle` 为「免持 Locker 的跨线程调用」而设计（**建议**用它，非唯一路径） | `src/locker.rs:170-172`："A handle usable from any thread without holding the lock, for `IsolateHandle::terminate_execution` and `IsolateHandle::request_interrupt`."；句柄由 `isolate.rs:1055` `thread_safe_handle()` 取得 |
| `cancel_terminate_execution`：被强制终止后须先让所有 JS 帧传播完不可捕获异常才能恢复 | `src/isolate.rs:2624-2639` |
| `V8InspectorClientImpl` 的 `run_message_loop_on_pause` / `quit_message_loop_on_pause` / `run_if_waiting_for_debugger` **默认均为空函数体** | `src/inspector.rs:583-586` |
| `ChannelImpl` 三方法：`send_response` / `send_notification` / `flush_protocol_notifications` | `src/inspector.rs:449-453` |
| `V8InspectorClient` 与 `Channel` 均 `Pin<Box<...>>` + `PhantomPinned`；注释：「V8 will hold onto a raw pointer … so we need to make sure it stays pinned」 | `src/inspector.rs:656-664`、`src/inspector.rs:394-400` |
| `schedule_pause_on_next_statement` / `cancel_pause_on_next_statement`（程序化暂停，无需外部调试器） | `src/inspector.rs:908` / `:924` |
| `v8::Locker`：「Support for sharing an isolate between threads, one thread at a time」 | `src/locker.rs` 模块文档 |

### 8.4 明确标「待核实」的项

| 项 | 状态 |
|---|---|
| `blitz-dom 0.3.0-beta.2` 在 `x86_64-unknown-linux-gnu` 上**能否编译并跑通断言** | **待核实**（依赖图看应可，因唯一平台依赖 `objc2` 的 target 是 `cfg(any(target_vendor = "apple"))`；但 `parley`/`skrifa` 的字体栈在 Linux 上的实际行为未实测）。→ 即 T2.4 的 **C1** |
| `html5ever 0.39` 上 ADR-0006 的四条结论是否仍成立 | **待核实**（本文档只核实了版本分叉**存在**，未核实结论是否变化）。→ 即 T2.4 的 **C4** |
| `blitz-dom` 传递依赖的实际许可集合是否全部在 `deny.toml` 白名单内 | **待核实**（已核实 `blitz-dom`/`taffy`/`parley`/`stylo` 本身在白名单内，但传递依赖需 `cargo deny check` 实测） |
| `blitz-dom` 的 `rayon ^1` 是否与「单一页面线程」调度模型冲突 | **待核实** |
| T2.5 是否引入 PyO3 / 远程 Linux 是否具备编译 PyO3 的条件 | **待核实**（依赖 Q-4 的拍板） |

---

## 9. 文档纪律声明

- 本文档**未修改** `docs/TASKS.md`（该文件由 QA 角色回写）。
- 本文档**未修改** `AGENTS.md`、`.github/workflows/**`、`CODEOWNERS`、`deny.toml`（铁律 7）。
- 本文档**未编写**任何 Rust 代码，未创建任何探针。
- 本文档**未发明**架构决策：§1 的「要回答的问题」是给架构师的输入清单，不是结论；
  §5 的 P0 判据是「怎样算通过」，不含「该怎么设计」。
- 所有依赖版本号均于 **2026-10-12** 经 crates.io API 或本机 cargo registry 源码核实，
  并在 §8 注明来源；无法核实的集中列于 §8.4 并标注「待核实」。
