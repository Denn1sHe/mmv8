# 决策上报：G2-D2 —— 解析器宿主与执行协议（+ 调度器核心）

> 状态：**待主理人组织内部独立子 agent 背对背辩论 → 落 ADR**（依 `docs/TASKS.md` 授权变更）
> 提出人：AI 代理（架构师 高见远）｜ 日期：2026-10-13 ｜ 模板：`docs/decision-brief-template.md`
> 关联：施工图 `docs/design-g2-page-2026-10-13.md` §2.1 / §2.4 / §5（S5/S8/S9/S10）
> 关联 ADR：**ADR-0006**（T2.3 html5ever 暂停/恢复 + push_front）、**ADR-0014**（PyO3 回调边界）、
> **ADR-0012**（T2.5 暂停泵死锁边界）、**ADR-0010**（双时钟与时区正交）
> **⚠️ 本决策触及 `AGENTS.md` 铁律 6 高风险路径：scheduler（事件循环 / 消息泵 / 暂停恢复 / 时序）。**
> 本代理的推荐：**明确标注「仅供参考」**（见 §6）

---

## 0. 一句话问题

> 网页里的 `<script>` 执行**必须打断 HTML 解析**（解析到 `</script>` 停下、跑完脚本、脚本还可能
> 往文档里写东西，然后解析从原地继续）。要拍板：**这个「暂停-执行-写回-恢复」的循环由谁驱动、
> 脚本是「就地同步执行」还是「排进事件循环的任务」、以及承载事件循环的调度器核心长什么样？**

---

## 1. 背景与上下文

- **这个仓库是什么**（面向外行）：我们用 Rust 写离线「浏览器环境模拟器」，嵌入 V8。G1 已完成
  「能 eval、能跑微任务、能调宿主函数」。G2 要让**真实网页**能被解析并执行其中的脚本。
- **涉及的模块 / 文件路径**：
  - `/Users/dennis/WorkBuddy/mmv8/crates/navigation/src/parser.rs`（**新文件：解析驱动**）
  - `/Users/dennis/WorkBuddy/mmv8/crates/navigation/src/sink.rs`（**新文件：TreeSink 落 DOM**）
  - `/Users/dennis/WorkBuddy/mmv8/crates/navigation/src/document_write.rs`（**新文件：插入点**）
  - `/Users/dennis/WorkBuddy/mmv8/crates/scheduler/`（**新实现：事件循环/消息泵/双时钟**，目前空骨架）
  - `/Users/dennis/WorkBuddy/mmv8/crates/engine-v8/src/realm.rs`（`eval` / `perform_checkpoint`，已验收）
  - `/Users/dennis/WorkBuddy/mmv8/poc/t2_3_html5ever_pause/src/main.rs`（T2.3 探针，**已通过 5/5 断言**）
  - `/Users/dennis/WorkBuddy/mmv8/docs/adr/0006-T2.3-html5ever-脚本暂停与document-write插入点.md`
- **当前状态**：T2.3 探针已证「低层 `Tokenizer` + 自定义 sink」可暂停/恢复、`push_front` 插入点语义成立。
  但 **html5ever 自带 `driver.rs` 直接丢弃暂停信号**（见 §4 证据 3），**必须自建驱动**。
  `scheduler` crate 目前是空骨架。
- **为什么现在必须决策**：① 解析驱动是 G2 的主体（技术方案 §7）；② 它**天然触发铁律 6**
  （暂停/恢复 + 事件循环 + 时序）；③ 若「就地同步执行」与「任务化执行」选错，
  `run_until` 的有界性、可中断性、与 Python 控制面的交互全都要返工；④ 调度器核心形态
  一旦定下，后续 G3~G6 全部复用。

---

## 2. 需要拍板的问题

- **主问题**：阻塞脚本的**执行**与**恢复解析**如何组织？
- **子问题**：
  1. 脚本执行是**在解析驱动帧内就地同步 `Realm::eval`**，还是**抛给调度器成为一个任务**（任务化）？
  2. `document.write` 的产出「何时、以什么路径」插回输入流？（ADR-0006 要求用 `push_front` 表达，
     且**上层不得直接操纵 `BufferQueue`**。）
  3. 解析器的**状态机归哪个 crate**？（`navigation` 自持 html5ever 的 `Tokenizer<TreeBuilder<…>>`？）
  4. 承载「事件循环 / 消息泵 / 时序」的 `scheduler` **核心形态**是什么（任务源模型 / 有界推进 /
     重入纪律 / 双时钟）？
  5. 脚本执行期与「控制入口隔离」（G1 的 `__iv8__` 撤销）如何衔接？

---

## 3. 备选方案（至少两个）

### 方案 A：**任务化执行 + 单线程消息泵**（本代理推荐，仅供参考）

- **做法**：解析驱动只负责「喂 chunk、观察暂停点」；一旦在 `</script>` 暂停，**把「执行该脚本」
  作为一个任务放进调度器的任务源队列**；调度器的 `run_until` 跑到它时才真正 `Realm::eval`；
  脚本内的 `document.write` 先落进**页面的写缓冲**，任务结束后由解析驱动「以 push_front 语义」注入
  再 `resume`。调度器是**单线程、不重入**的消息泵，按任务源分队列，`run_until` 有界（任务数 + 墙钟）。
- **关键代码（设计契约原文）**：

  ```rust
  // navigation::parser —— 自建可暂停/恢复驱动（不用一次性 parse_document）
  pub enum ParseOutcome { Done, BlockedOnScript(NodeId) }
  pub struct ParserHost {
      tokenizer: html5ever::tokenizer::Tokenizer<
          html5ever::tree_builder::TreeBuilder<NodeId, DomSink>>,
      input: html5ever::tokenizer::BufferQueue,
      pending_writes: std::collections::VecDeque<tendril::StrTendril>,
  }
  impl ParserHost {
      pub fn feed(&mut self, chunk: &str) -> ParseOutcome;          // 内部 push_back + 循环 feed
      pub fn enqueue_document_write(&mut self, text: &str);         // push_front 语义（ADR-0006）
      pub fn resume(&mut self) -> ParseOutcome;                     // 先注入 pending_writes 再继续
      pub fn finish(self);
  }

  // scheduler —— 单线程消息泵 + 双时钟（铁律 6 高风险路径）
  pub enum TaskSource { Timer, Network, UserInput, Message, Microtask, ScriptBlocking }
  pub struct RunBudget { pub max_tasks: u64, pub deadline: Option<Duration> }
  pub enum RunOutcome { Idle, BudgetExhausted, DeadlineExceeded, WaitingIo, Closed }
  pub trait TaskExecutor {
      fn run_task(&mut self, source: TaskSource, token: TaskToken) -> Result<TaskVerdict, PageError>;
      fn has_pending_io(&self) -> bool;
  }
  impl RunLoop {
      pub fn run_until(&mut self, budget: RunBudget, exec: &mut dyn TaskExecutor) -> Result<RunOutcome, PageError>;
  }
  ```

  执行脚本任务时的编排（页面 scope，控制入口撤销）——复用 G1 的 `ControlPlane::with_page_script`：

  ```rust
  // BrowsingContext 内（示意）
  let token = self.runloop.schedule(TaskSource::ScriptBlocking, token_for(node));
  // run_until 跑到该任务：
  let text = self.shared.borrow().tree.borrow().text_content(node);
  self.control.with_page_script(|plane| plane.realm_mut().eval(&text))?;   // 页面 scope
  // 任务结束：把写缓冲交回解析驱动
  let writes = self.shared.borrow_mut().take_writes();
  for w in writes { self.parser.enqueue_document_write(&w); }
  let next = self.parser.resume();
  ```

- **优点**：① `run_until` 天然**有界**（任务数 + 墙钟）⇒ 兑现 §16「禁止 eval 悄悄无限等待」；
  ② 脚本执行与解析是**两个可独立观测的阶段**，与 ADR-0006「两个独立关注点」一致；
  ③ 网络/定时器/输入与脚本用**同一任务模型** ⇒ G3~G6 复用；④ 可被 watchdog 掐（若将来）。
- **缺点与风险**：① 需要一套完整的调度器（更多代码）；② 若实现不当，解析驱动与调度器可能**互相重入**
  ⇒ 必须显式「不重入」纪律（见约束）。
- **长期影响**：调度器成为全项目骨干；页面行为时序可控、可测。
- **预估工作量**：中偏大（解析驱动 中 + 调度器 中）。

### 方案 B：**就地同步执行**（解析驱动帧内直接 `eval`）

- **做法**：解析驱动在 `</script>` 暂停点**直接**调用 `Realm::eval(script_text)`，脚本 `document.write`
  立即 `push_front`，然后继续 `feed`。不引入任务源概念。
- **关键代码（示意）**：

  ```rust
  loop {
      match self.tokenizer.feed(&mut self.input) {
          TokenizerResult::Done => return ParseOutcome::Done,
          TokenizerResult::Script(node) => {
              let text = tree.borrow().text_content(node);
              realm.eval(&text)?;                       // 就地同步执行
              while let Some(w) = writes.pop_front() {  // 立即注入
                  self.input.push_front(StrTendril::from(w));
              }
              // 继续 loop 恢复
          }
      }
  }
  ```

- **优点**：代码最少；与 T2.3 探针形状最接近；无需调度器即可先跑通解析。
- **缺点与风险**：① 脚本执行**不可被预算打断**（`run_until(max_tasks/deadline)` 无法约束解析中的脚本）；
  ② 与网络/定时器/输入**不能用同一模型** ⇒ 页面时序分叉；③ 与 Python 控制面交互时，
  「解析中直接跑 JS」会让 `run_until` 的语义含糊（到底推进了哪些「任务」？）。
- **长期影响**：G3/G4 大概率要**重写**成任务化。
- **预估工作量**：小（但返工风险高）。

### 方案 C：**混合**（解析中同步、其余任务化）

- **做法**：解析帧内同步执行（方案 B），但网络/定时器/输入走任务队列。
- **优点**：折中，解析先跑通。
- **缺点与风险**：**两套执行心智**并存；「预算/可中断性」在解析路径上无效 ⇒ 与 §6「预算」要求冲突；
  正是本仓想避免的「靠纪律」形态。
- **预估工作量**：中。

---

## 4. 证据（原文粘贴）

**证据 1 —— ADR-0006 结论原文（驱动必须自建；两关注点分离；push_front）**：

> 1. sink 对 `<script>` 起始标签返回 `TokenSinkResult::RawData(RawKind::ScriptData)`；
> 2. sink 对 `</script>` 返回 `TokenSinkResult::Script(handle)` 产生暂停点；
> 3. 驱动层在暂停点**执行脚本**，并把 `document.write` 的产出用 **`push_front`** 插入输入队列；
> 4. 脚本执行完毕后**再次 `feed`** 恢复解析。
>
> 「脚本执行」与「输入注入」是**两个独立关注点**：前者属 `scheduler`，后者属 `navigation`；
> 插入点以 `push_front` 表达，**不得由上层直接操纵 `BufferQueue`**。

**证据 2 —— T2.3 探针实测输出（原文）**：

```
ASSERT_PASS(a): 前缀 feed 后即产出 token —— 增量/可暂停成立
ASSERT_PASS(b): script 体内 <b> 未被解析为标签 —— RawData(ScriptData) 生效
ASSERT_PASS(c): </script> 处产生 1 个暂停点 —— TokenSinkResult::Script 生效
ASSERT_PASS(d): document.write 内容落在 </script> 之后 —— 插入点语义成立
ASSERT_PASS(e): 恢复后 </script> 后的 <i> 正常解析 —— tokenizer 回到 Data 状态
H5PROBE_OK
```

**证据 3 —— html5ever 0.27 自带 driver 丢弃暂停信号（原文）**：

```rust
// html5ever-0.27.0/src/driver.rs
fn process(&mut self, t: StrTendril) {
    self.input_buffer.push_back(t);
    // FIXME: Properly support </script> somehow.
    while let TokenizerResult::Script(_) = self.tokenizer.feed(&mut self.input_buffer) {}
}
```

**证据 4 —— html5ever 0.27 源码核实（本代理已复核）**：

- `TreeBuilderOpts::default()` 的 `scripting_enabled = true`（`tree_builder/mod.rs`）；
- `TreeBuilder` 即 `TokenSink`（`tree_builder/mod.rs:450` 实现 `process_token`）；
- `<script>` 起始 ⇒ `RawData(ScriptData)`（`:245`）；`</script>` ⇒ `ProcessResult::Script(node)`
  ⇒ `return tokenizer::TokenSinkResult::Script(node)`（`:371`）；
- `Tokenizer::feed(&mut BufferQueue) -> TokenizerResult<Sink::Handle>`，`TokenizerResult = { Done, Script(Handle) }`（`tokenizer/mod.rs:49`）。

**证据 5 —— ADR-0012 实测结论（暂停泵死锁边界，与调度器/时序相关）**：

```
D4_GIL_PATH_FAILURE_OBSERVED=true
PAGE_TASKS_STALLED_DURING_PAUSE=true
CLEAN_RESUME=true
```

> 结论原文：「**三条路径（GIL / 重入 / 锁序）里只有 GIL 一条会死锁，且 watchdog 打不断它**。」
> 宿主回调禁同步持 GIL 等待页面线程。

**证据 6 —— 现有仓库事实（已核实）**：

```
crates/navigation/Cargo.toml → [dependencies] 为空（骨架）
crates/scheduler/Cargo.toml  → [dependencies] 为空（骨架）
crates/engine-v8/src/realm.rs → eval / perform_checkpoint / set_microtasks_policy 已验收
docs/adr/0006 已批准；poc/t2_3_html5ever_pause 已通过
本地 registry：html5ever-0.27.0 + markup5ever-0.12.1（无更高 html5ever）
```

---

## 5. 本仓库的既定约束（外部/内部评审必须在此边界内给建议）

- **铁律 6**（`AGENTS.md`）原文：「**高风险路径改动必须走「决策上报协议」获批**……：……
  **scheduler（事件循环 / 消息泵 / 暂停恢复 / 时序）**；PyO3 回调边界……」。**本决策正命中该条。**
- **铁律 3** 原文：「**一事一 PR**，diff ≤ 400 行；功能与重构严格分离」。
- **ADR-0006** 对 `navigation` 的约束（原文见 §4 证据 1）：必须可暂停/恢复循环；两关注点分离；`push_front`；
  插入点顺序必须写成**回归测试**。
- **技术方案 §6** 原文（调度规则）：「定时器、网络、用户输入、消息等分队列」；「微任务……在约定的
  脚本清理和 task 边界检查」；「预算：task 数、总 CPU/墙钟……支持中断」；「I/O 线程仅投递纯数据；
  页面线程创建 JS 值并 resolve」。
- **技术方案 §4** 原文：「**同一个 JSContext 的并发 eval 排队处理**；……不要把一个正在执行的 Isolate
  随 Tokio future 在线程池间迁移。Tokio 负责 I/O，页面线程负责 JS 与 DOM。」
- **ADR-0010 结论 4**：逻辑时钟归 `scheduler`，**与时区正交**，不受进程边界约束。
- **ADR-0014**：PyO3 回调同线程直呼、RAII 重入禁令、截止期限（G2 的 `run_until` 与之衔接）。
- **unsafe**：`scheduler`/`navigation` **不在白名单 ⇒ 零 `unsafe`**。
- **跨平台铁律**：mac + linux x86_64 双端可编译；新增依赖须支持 linux x86_64。

---

## 6. 本代理的推荐（**仅供参考，等待内部辩论**）

- **推荐方案 A（任务化执行 + 单线程消息泵）**，理由：
  1. 兑现 §16「`run_until` 有界」与 §6「预算支持中断」——**只有任务化**才能对「脚本执行」施加
     任务数/墙钟预算；
  2. 与 ADR-0006「脚本执行属 `scheduler`、输入注入属 `navigation`」的**明文分工**一致；
  3. 网络/定时器/输入/脚本**同一任务模型** ⇒ G3~G6 复用，避免方案 C 的两套心智；
  4. 页面单线程 + 消息泵的形态与 §4「并发 eval 排队处理」一致。
- **调度器核心的最小裁决建议**（供辩论）：
  - **任务源分队列**（§6）：`Timer / Network / UserInput / Message / Microtask / ScriptBlocking`，
    同源内 FIFO；
  - **`run_until` 有界**：`max_tasks` + 墙钟 `deadline`；队列空但 I/O 未完 ⇒ `WaitingIo`（**不凭空完成**）；
  - **不重入**：`run_until` 运行期再次进入 ⇒ 显式错误（`Reentrancy` 类），不 panic、不死锁；
  - **双时钟**：`Clock` trait（`monotonic_now` / `logical_now_ms` / `advance` / `mode`）；
    逻辑模式**不实际等待**（§6），系统模式用墙钟；**时钟与时区正交**（ADR-0010）。
- **我不确定的点（希望外部 AI 重点看）**：
  1. **就地执行 vs 任务化**：STOP-the-world 的阻塞脚本语义，是否真的需要「任务化」？
     有没有「就地执行但可被预算打断」的第三种形态（例如在 eval 前后插预算检查 + watchdog）？
  2. **`document.write` 在 parser 结束后的语义**：G2 应「清空并重开文档」还是「明确报错」？
     （这决定 `document_write_after_load_defined` 的期望值。）
  3. **调度器是否应持有 `TaskExecutor` trait 还是普通函数表**：trait 对象简单但耦合页面；
     是否应让 `scheduler` 完全不含页面类型（本设计已用不透明 `TaskToken`）？
  4. **微任务与 MutationObserver 的交付边界**：`MutationObserver` 回调是**宿主微任务**还是
     V8 微任务？（G2 计划：编排层在 checkpoint 前先跑观察器回调，再 `perform_checkpoint`。）

---

## 7. 请重点回答的问题

1. 「阻塞脚本执行」应**任务化**（方案 A）还是**就地同步**（方案 B）？给出对 `run_until` 有界性、
   可中断性、与 Python 控制面交互的具体影响分析。
2. `document.write` 在「parser 已结束（post-load）」时的正确/可接受语义是什么？G2 应选哪种口径？
3. 调度器核心应如何设计，才能**同时**满足：① 任务源分队列；② `run_until` 有界；③ 不重入；
   ④ 微任务在规范边界 checkpoint；⑤ 与 `engine-v8` 的 `Realm`（单线程、`!Send`）协作？
   给出 `RunLoop` 的最简可行签名与不变量清单。
4. 本设计是否遗漏了任何**铁律 6 要求上报的时序/暂停-恢复**风险点？
