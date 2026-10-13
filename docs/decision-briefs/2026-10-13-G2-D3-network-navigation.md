# 决策上报：G2-D3 —— 资源等待与导航语义（+ PyO3 控制面）

> 状态：**待主理人组织内部独立子 agent 背对背辩论 → 落 ADR**（依 `docs/TASKS.md` 授权变更）
> 提出人：AI 代理（架构师 高见远）｜ 日期：2026-10-13 ｜ 模板：`docs/decision-brief-template.md`
> 关联：施工图 `docs/design-g2-page-2026-10-13.md` §2.4 / §2.5 / §2.7 / §5（S6/S10/S13/S14）
> 关联 ADR：**ADR-0014**（PyO3 回调边界）、**ADR-0012**（暂停泵死锁边界）、**ADR-0006**（解析/插入点）
> **⚠️ 本决策触及 `AGENTS.md` 铁律 6 两处高风险路径：scheduler（时序/暂停恢复）与 PyO3 回调边界。**
> 本代理的推荐：**明确标注「仅供参考」**（见 §6）

---

## 0. 一句话问题

> 网页脚本会发网络请求（`fetch`），响应还没到、页面就得**停下来等**（不凭空成功）。要拍板：
> **「等」在接口上怎么表达（`run_until` 返回什么）、「响应到了」怎么安全地回填进同一个 JS 线程、
> 以及新导航怎么把**旧页面的迟到响应**丢掉、不让它写进新页面？**

---

## 1. 背景与上下文

- **这个仓库是什么**（面向外行）：我们用 Rust 写离线「浏览器环境模拟器」，嵌入 V8，让 Python 跑依赖
  网页环境的 JS。G1 已完成「引擎纵向」（eval / 微任务 / Python 回调）。G2 要让**真实网页**能解析、
  执行脚本、发请求、等待响应、被点击。
- **涉及的模块 / 文件路径**：
  - `/Users/dennis/WorkBuddy/mmv8/crates/network/`（**新实现：Fetch 语义 + OfflineBundle**，空骨架）
  - `/Users/dennis/WorkBuddy/mmv8/crates/navigation/src/controller.rs`（**新文件：导航协调器 + epoch**）
  - `/Users/dennis/WorkBuddy/mmv8/crates/navigation/src/page.rs`（**新文件：BrowsingContext + run_until**）
  - `/Users/dennis/WorkBuddy/mmv8/crates/scheduler/src/runloop.rs`（**新文件：run_until 有界语义**）
  - `/Users/dennis/WorkBuddy/mmv8/crates/sdk-python/src/page.rs`（**新文件：PyPage/PyNetwork/PyInput**）
  - `/Users/dennis/WorkBuddy/mmv8/crates/sdk-python/src/callback.rs`（PyO3 回调派发，已验收）
- **当前状态**：网络层与调度器是空骨架；`engine-v8` 的 `eval`/`call`/`root`/`HandleLease` 已验收。
  技术方案 §16 给出了控制接口的**拟定示例**（见 §4 证据 1），并明说「具体类型与异常约定由 SDK
  契约测试固定」。
- **为什么现在必须决策**：① 「等待-回填-作废」是 G2 六项必产出之一（技术方案 §18）；
  ② 它同时触发**铁律 6 的两条高风险路径**（scheduler 时序 + PyO3 回调边界）；
  ③ 若「等待语义」定错，`run_until` 会变成「无限等待」或「凭空完成」，两者都违反 §6/§16；
  ④ 导航代次（epoch）是「关闭与导航不能只清空全局变量」的落地（技术方案 §4）。

---

## 2. 需要拍板的问题

- **主问题**：页面「等待网络」的**接口语义**与「响应回填」的**线程/重入安全**如何定义？
- **子问题**：
  1. `run_until(max_tasks, deadline_ms)` 在**队列空但请求未完成**时返回什么？
     （`WaitingIo`？还是别的？）
  2. 宿主 `network.complete(id, resp)` 是**立即 resolve** 还是**入队后由 `run_until` resolve**？
  3. 新导航后，**旧页面的迟到响应**如何作废（`navigation_epoch`）？
  4. JS `fetch` 如何拿到一个「以后才 resolve 的 Promise」，且**避开 G1 遗留 K-11 的
     `PromiseResolver` FATAL 风险面**？
  5. `run_until` / `network.complete` 作为 **PyO3 边界**，如何满足 ADR-0014「不跨线程阻塞取锁」？

---

## 3. 备选方案（至少两个）

### 方案 A：**有界推进 + 入队回填 + epoch 作废**（本代理推荐，仅供参考）

- **做法**：
  - `run_until` **有界**（`max_tasks` + 墙钟 `deadline`）；队列空但仍有未完成请求 ⇒ 返回 **`WaitingIo`**
    （**不凭空完成**，§6）。
  - `network.complete(id, resp)` **只入队**一个 `TaskSource::Network` 任务，**不在调用内同步跑 JS**；
    真正的 resolve 发生在下一次 `run_until` 跑到该任务时（保证线程一致 + 有界）。
  - 每条请求携带 `navigation_epoch`；`begin_navigation` 使 epoch +1，`NetworkService::abort_epoch`
    把旧 pending 标 `Aborted`，**旧响应/旧任务落地前校验 epoch**，不符则丢弃。
  - JS `fetch` 用**普通闭包**（非 `PromiseResolver`）拿 Promise：
    `function fetch(u){ return new Promise((res,rej)=>{ __iv8_fetch(u,res,rej); }); }`，
    `__iv8_fetch` 把 `res/rej` 经 `Realm::root` 存为 `HandleLease`（按 `RequestId` 索引）。
- **关键代码（设计契约原文）**：

  ```rust
  // scheduler
  pub enum RunOutcome { Idle, BudgetExhausted, DeadlineExceeded, WaitingIo, Closed }
  pub struct RunBudget { pub max_tasks: u64, pub deadline: Option<Duration> }
  pub trait TaskExecutor {
      fn run_task(&mut self, source: TaskSource, token: TaskToken) -> Result<TaskVerdict, PageError>;
      fn has_pending_io(&self) -> bool;    // true ⇒ 队列空时返回 WaitingIo
  }
  impl RunLoop { pub fn run_until(&mut self, budget: RunBudget, exec: &mut dyn TaskExecutor) -> Result<RunOutcome, PageError>; }

  // network
  #[repr(transparent)] pub struct RequestId(u64);
  pub struct RequestSpec { pub id: RequestId, pub url: String, pub method: String,
      pub headers: Vec<(String,String)>, pub body: Option<Vec<u8>>, pub epoch: NavigationEpoch, /* ... */ }
  pub enum FetchOutcome { Ready(RequestId), Pending(RequestId) }   // Ready = OfflineBundle 命中
  impl NetworkService {
      pub fn fetch(&mut self, spec: RequestSpec) -> Result<FetchOutcome, NetworkError>;
      pub fn pending(&self) -> Vec<RequestId>;
      pub fn complete(&mut self, id: RequestId, resp: Response) -> Result<(), NetworkError>; // 只入队
      pub fn abort_epoch(&mut self, epoch: NavigationEpoch);
      pub fn export_log(&self) -> Vec<NetLogEntry>;
  }

  // navigation
  #[repr(transparent)] pub struct NavigationEpoch(u64);
  impl NavigationController {
      pub fn begin_navigation(&mut self, url: &str, snapshot: &PageSnapshot) -> Result<NavigationEpoch, PageError>; // epoch += 1
      pub fn is_current(&self, epoch: NavigationEpoch) -> bool;
  }
  ```

  Python 控制面（§16 示例的最小兑现）：

  ```python
  with runtime.JSContext(profile=profile, time_mode="logical") as ctx:
      ctx.page.load(snapshot)
      result = ctx.run_until(max_tasks=10000, deadline_ms=2000)   # -> "WaitingIo" / "Idle" / ...
      for request in ctx.network.pending():
          ctx.network.complete(request.id, host_fetch(request))
      ctx.run_until(max_tasks=10000, deadline_ms=2000)
      entries = ctx.network.export_log()
  ```

- **优点**：① `run_until` **有界且语义明确**（`WaitingIo` 表达「在等 I/O」）；
  ② resolve 与 JS 在同一线程、不在 `complete` 调用栈内同步跑（避免重入 + 满足 ADR-0014）；
  ③ epoch 作废是「可判定的丢弃」而非「靠清空全局变量」；④ 避开 K-11 的 `PromiseResolver` 风险面。
- **缺点与风险**：① 需要维护「res/rej 句柄 + lease」的生命周期（随 abort/complete 释放）；
  ② 宿主必须理解 `run_until` 可能要**循环调用**（模型是「推进直到无事可做或等 I/O」）。
- **长期影响**：网络/定时器/输入共用任务模型；G3~G6 的在线传输只换 `Transport` 实现。
- **预估工作量**：中。

### 方案 B：**`network.complete` 立即同步 resolve**

- **做法**：`complete(id, resp)` 直接调用存好的 `res`（同步跑 JS 与后续微任务）。
- **优点**：宿主接口「一步到位」。
- **缺点与风险**：① 在 Python 调用栈内**同步跑任意 JS** ⇒ 触发 ADR-0014 的**重入**问题
  （`complete` 可能在 `run_until` 之外被调，甚至与正在跑的 JS 交错）；② 可能**越过 `run_until` 的预算**
  （§6 预算失效）；③ 若 `complete` 从别的线程调用 ⇒ 触碰 V8 `!Send` 与 ADR-0012 的 GIL 死锁面。
- **长期影响**：破坏「任务化」一致性，回调时序难测。
- **预估工作量**：小（但风险高）。

### 方案 C：**`run_until` 无限等待直到 I/O 完成**

- **做法**：`run_until` 在有待完成 I/O 时**阻塞等待**，直到宿主提供响应。
- **优点**：宿主调用更简单（一次 `run_until` 跑完）。
- **缺点与风险**：**直接违反** §16「禁止 eval 悄悄无限等待」与 §6「缺少在线响应时返回 waiting_io」；
  且同步等待会触碰 ADR-0012 的 GIL 死锁面。
- **预估工作量**：小（但**违反明文要求**）。

---

## 4. 证据（原文粘贴）

**证据 1 —— 技术方案 §16 控制接口示例（原文）**：

```python
with runtime.JSContext(profile=profile, time_mode="logical") as ctx:
    ctx.page.load(snapshot)
    result = ctx.run_until(max_tasks=10000, deadline_ms=2000)
    for request in ctx.network.pending():
        ctx.network.complete(request.id, host_fetch(request))
    ctx.run_until(max_tasks=10000, deadline_ms=2000)
    entries = ctx.network.export_log()
```

> 原文补充：「以上为拟定接口示例，**具体类型和异常约定由 SDK 契约测试固定**。宿主回调使用
> PyO3 attach/detach 机制，避免持 GIL 等待页面线程；大量请求采用批量消息接口。」

**证据 2 —— 技术方案 §6 原文（等待语义）**：

> 「**`drain` 只处理当前到期任务，缺少在线响应时返回 waiting_io，不凭空完成请求；`advance` 也不替
> 网络后端假造成功。**」；「异步完成：**I/O 线程仅投递纯数据；页面线程创建 JS 值并 resolve**」；
> 「预算：task 数、总 CPU/墙钟、虚拟时间上限；支持中断」。

**证据 3 —— 技术方案 §4 原文（导航代次与关闭）**：

> 「每个异步请求、句柄、Worker 和日志条目携带 ContextId、RealmId、**navigation_epoch**。
> **新导航撤销旧文档任务和资源消费者，延迟返回的旧响应不能写入新页面。**……同一个 JSContext 的
> 并发 eval 排队处理；跨 Context 才是并行单元。」

**证据 4 —— 技术方案 §9 原文（网络分层）**：

> 「网络分成两个层次：`NetworkService` 实现页面可观测的 Fetch/XHR……；`Transport` 只负责发送字节
> 和返回响应。……**OfflineBundle**……资源未命中进入**等待或明确失败**；允许**宿主稍后注入**。」

**证据 5 —— ADR-0012（T2.5）实测结论（相关）**：

```
D4_GIL_PATH_FAILURE_OBSERVED=true
CLEAN_RESUME=true
```

> 原文结论：「**三条路径（GIL / 重入 / 锁序）里只有 GIL 一条会死锁，且 watchdog 打不断它**」；
> 「宿主回调禁同步持 GIL 等待页面线程」。

**证据 6 —— ADR-0014 决策 1/2（PyO3 回调边界，已接受）**：

> 关键不变量：「**回调总在「已持 GIL / 有活动 attach 状态」的线程上执行；派发路径不发起跨线程阻塞式取锁。**」
> 「回调截止期限……同线程下无法抢占……只能检测 + 上报。」

**证据 7 —— G1 遗留 K-11（`PromiseResolver` 风险面，原文）**：

> 「`set_host_import_module_dynamically_callback` + `PromiseResolver` 触发 **V8 FATAL**
> （`V8_Fatal → … → v8::Script::Run`，SIGTRAP），已整体回退；专项排查进行中。」

**证据 8 —— 现有仓库事实（已核实）**：

```
crates/network/Cargo.toml      → [dependencies] 为空（骨架）
crates/navigation/src/         → 暂无 controller.rs / page.rs（G2 新建）
crates/sdk-python/src/callback.rs → py_callback（同线程直呼 + 截止期限 + 线程断言）已验收
crates/engine-v8/src/handles.rs   → HandleLease（自持 Global）已验收 → 用于保存 res/rej
```

---

## 5. 本仓库的既定约束（外部/内部评审必须在此边界内给建议）

- **铁律 6** 原文：高风险路径含「**scheduler（事件循环 / 消息泵 / 暂停恢复 / 时序）**」与
  「**PyO3 回调边界（GIL 获取与释放）**」——**本决策同时命中两条**。
- **技术方案 §6** 原文：「缺少在线响应时**返回 waiting_io，不凭空完成请求**」；「I/O 线程仅投递纯数据；
  页面线程创建 JS 值并 resolve」；「预算……支持中断」。
- **技术方案 §4** 原文：「新导航撤销旧文档任务……**延迟返回的旧响应不能写入新页面**」。
- **技术方案 §16** 原文：「**禁止 eval 悄悄无限等待**」；「宿主回调使用 PyO3 attach/detach 机制，
  **避免持 GIL 等待页面线程**」。
- **ADR-0014**：同线程直呼、RAII 重入禁令、截止期限；`sdk-python` 继承 `unsafe_code = "forbid"`（零手工 unsafe）。
- **ADR-0012**：GIL 路径死锁不可被 watchdog 打断 ⇒ 不得跨线程阻塞等待。
- **ADR-0009 / C-2**：`network`/`navigation` 类型面不得出现 `v8::`；`v8::` 只在 `engine-v8`。
- **未决：K-11**（`PromiseResolver` FATAL）——本设计**刻意绕开**该 API。
- **跨平台铁律**：`network` 新增依赖（若 `reqwest` 等）须支持 linux x86_64 且过 `cargo deny`；
  本决策的 **G2 主路径不引入在线传输**（OfflineBundle 先行）。

---

## 6. 本代理的推荐（**仅供参考，等待内部辩论**）

- **推荐方案 A（有界推进 + 入队回填 + epoch 作废）**，理由：
  1. `run_until` 返回 `WaitingIo` **逐字兑现** §6「不凭空完成请求」与 §16「禁止无限等待」；
  2. `complete` **只入队**⇒ resolve 与 JS 同线程、不越预算、可在 `run_until` 观测 ⇒ 满足 ADR-0014/§4；
  3. epoch 作废把「旧响应不写新页面」变成**可判定校验**（而非清空全局变量）；
  4. 用普通闭包替代 `PromiseResolver`，**绕开 K-11 的 FATAL 风险面**。
- **我不确定的点（希望外部 AI 重点看）**：
  1. `run_until` 的返回值集合是否应包含 `WaitingIo`，还是把「等待」作为**异常**抛出更贴合 §16 的口径？
  2. `res/rej` 用 `HandleLease` 保存的**生命周期**：abort/complete 后何时 drop lease？若宿主
     `complete` 两次（重复）应如何处理（幂等还是报错）？
  3. `network.complete` 的**批量接口**（§16「大量请求采用批量消息接口」）G2 是否要现在就定形，
     还是留到在线传输（G6）？
  4. 若将来 `complete` 从**非页面线程**调用（在线传输），本方案的「只入队纯数据」是否足够？
     （需与 ADR-0014 的 attach/detach 异步化衔接。）

---

## 7. 请重点回答的问题

1. 方案 A 是否是正确的「等待-回填」语义？`run_until` 用返回值 `WaitingIo` 与用异常，哪个更贴合
   §16 的契约风格？给出理由。
2. 「宿主 `complete` 只入队、由 `run_until` resolve」是否满足铁律 6 对 PyO3 回调边界的要求
   （ADR-0014 不跨线程阻塞取锁）？有没有遗漏的死锁或重入路径？
3. epoch 作废的实现要点有哪些（校验时机、res/rej 释放、重复 complete、abort 与 close 的交叠）？
4. 用「普通闭包存 res/rej」替代 `PromiseResolver` 是否是可接受的临时形态？其局限是什么？
