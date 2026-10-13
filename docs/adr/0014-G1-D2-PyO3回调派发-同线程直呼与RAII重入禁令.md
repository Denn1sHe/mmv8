# ADR-0014：G1 D2 —— Python 回调派发：**同线程直呼**（方案 A）+ 截止期限 + RAII 重入禁令

- 状态：**已接受**（2026-10-12 主理人汇总裁决；两位内部独立评审**背对背**辩论后落档）
- 日期：2026-10-12
- 关联：
  - **ADR-0012**（T2.5：GIL 路径死锁且 watchdog 打不断）—— 本 ADR 触发其**修正注记**（见 §决策 6）
  - **ADR-0009** 决策 2（`HostValue↔Python` 转换归 `sdk-python`；**白名单不变**）
  - **ADR-0002**（unsafe 白名单最终形态）
  - 施工图 `docs/design-g1-engine-2026-10-12.md` §2 / §3.6 / §7 U-1
  - **决策 brief**：`docs/decision-briefs/2026-10-12-G1-PyO3边界.md`
  - **评审意见（只读证据）**：`docs/decision-briefs/2026-10-12-G1-辩论意见-A.md`、`-B.md`
- 落地步骤：施工图 **S12**（`crates/sdk-python/**`）
- 前置实证：**U-1 已决**（`.workbuddy/refs/pyo3-lint-probe-evidence/`，`p1b`/`p1d` 通过、`p1c` 报错、`p4a/p4b` 修正 feature）

---

## 问题

「JS 调用 Python 回调」的派发设计，以及承载它的代码（PyO3 宏）该放哪个 crate。约束来自两条硬边界：
**ADR-0012**（GIL 路径死锁不可被 watchdog 打断）与 `AGENTS.md` **铁律 4**（unsafe 白名单）。

## 讨论要点（两位独立评审，D2 **高度收敛**）

| 维度 | 评审 A | 评审 B | 裁决 |
|---|---|---|---|
| 主问题（派发） | 方案 A（同线程直呼）；**要求修订 ADR-0012 字面**（D2.3） | 方案 A；**要求澄清「不 `with_gil`」精确含义**（§1 措辞修正） | **采纳 A**（决策 1） |
| 子问题（unsafe 归属） | **否决** brief 的方案 C（移边界）：U-1 已决使其前提消失，采纳即回归（D2.2-1） | 落「**不改**」分支（比 brief 少一层不确定性，§5） | **不改白名单、不移边界**（决策 1） |
| 回调截止期限 | A **未能抢占式兑现**；给三选一兜底：(a) 契约禁阻塞 (b) 运行时计量+事后报错 (c) 登记已知局限（D2.3） | 补强 1：入口/出口墙钟计量，超限报 `CallbackDeadline`；**不得**用 `terminate_execution`（§1 补强 1） | **采纳补强 1**（决策 2） |
| 重入禁令 | `in_callback: bool` **不够**，须覆盖**全部**入口 + 由 trampoline 置位（D2.3） | 补强 2：**RAII** + 覆盖全部重入入口，专测两条路径（§1 补强 2） | **采纳补强 2**（决策 3） |
| 运行时断言 | 反例驱动的强制补充项：回调线程 == eval 线程，违反报错而非 UB（§3） | 同左（§3 会翻转结论的证据） | **采纳**（决策 4） |
| 措辞 | ADR-0012 字面抵触须闭合（D2.3） | 「不 `with_gil`」精确化为「不发起跨线程阻塞式取锁」（§1 措辞修正） | **采纳**（决策 5、6） |
| brief 方案 C（首选） | **被攻破**：U-1 已决（F6）⇒ 采纳即回归（证伪职责总结表） | 条件未触发 ⇒ 落「不改」 | **记录评 A 证伪成果**（决策 8） |

> **两评审在 D2 无实质分歧**，均落「方案 A + 不改 unsafe 归属」，仅**补强项**由评 B 给出、
> **ADR-0012 修订要求**由评 A 给出。brief 的「若 U-1 命中则选方案 C」为**条件式**，U-1 实证**未命中** ⇒ 条件不触发。

## 决策

### 决策 1 —— 主形态：同线程直呼（方案 A）；unsafe 归属 = 不改

G1 只做「**Python 驱动 `eval` ⇒ 回调在该线程内直接派发**」。**关键不变量**：

> **回调总在「已持 GIL / 有活动 attach 状态」的线程上执行；派发路径不发起跨线程阻塞式取锁。**

- **ADR-0012 的死锁路径在结构上不存在**：该路径要求「另一线程持锁忙等」，而方案 A 下调用线程本身已持 GIL、
  无第二个 Python 线程、无跨线程 acquire ⇒ 「互等」前提不存在（不是「没测出来」，是「**没有这条路**」）。
- **精简前置**：`eval` **全程持有 GIL**（不得用 `allow_threads` 把 V8 运行包住）。
  **如实登记代价**：`eval` 全程持 GIL ⇒ **阻塞所有其他 Python 线程**（评 A §D2.3「A 的真实代价」）。

**unsafe 归属 = 不动作**：

- U-1 已实证（`unsafe_code="forbid"` 下 PyO3 宏编译通过；注入显式 `unsafe{}` 报错）⇒
  `sdk-python` **直接持有 `pyo3`**，**不改白名单、不移动 PyO3 边界**。
- **ADR-0009 决策 2 维持原样**（`HostValue↔Python` 转换留 `sdk-python`）；brief 的条件式方案 C **未触发**。
- 方案 B（异步 attach/detach）与方案 C（移边界）**登记备查、本轮不做**（G4）。

### 决策 2 —— 补强 1：回调截止期限（`JsErrorKind::CallbackDeadline`）

技术方案 §6 硬要求「同步宿主回调必须有截止期限」，施工图初稿**缺失**该机制。
**同线程下无法抢占正在运行的 Python 回调**（GIL 不可中断，ADR-0012 主结论）⇒ 期限只能**检测 + 上报**：

- 宿主在回调**进入前 / 返回后**做**墙钟计量**；超限 ⇒ 返回明确 `JsErrorKind::CallbackDeadline`。
- **不得**用 `terminate_execution` 作 Python 回调的期限手段（它打不到 CPython，ADR-0012 已证）。
- **上限的行使来自 Python 侧自持**（对齐 ADR-0012「Python 侧自持锁上限」范式）。
- **如实登记局限**：期限是「**拒绝下一次 / 事后判定**」，**不是**「终止运行中的回调」；
  「阻塞式回调导致页面线程无界挂起」为 G1 **已知局限**，G4 用 attach/detach 异步通道根治。

```rust
#[derive(Clone, Copy, Debug)]
pub struct CallbackDeadline { pub max_ms: u64 }   // 入口/出口墙钟计量；超限 ⇒ CallbackDeadline
```

### 决策 3 —— 补强 2：重入禁令 RAII + 覆盖全部入口

`Realm.in_callback` **必须 RAII**（guard 对象，panic/早返回都复位），且**覆盖全部重入入口**
（`eval` / `eval_module` / `call` / `perform_checkpoint` / 微任务执行），由**回调 trampoline** 置位。
**专测两条路径**：①「native fn → 再次 native fn」；②「微任务期间隐式重入」。

```rust
impl Realm {
    /// RAII 置位：guard 存活期内任何重入入口 ⇒ JsErrorKind::ReentrantEval。
    /// 由 native_fn trampoline 创建，Drop 时无条件复位（含 panic 展开路径）。
    pub(crate) fn enter_callback(&mut self) -> Result<CallbackGuard<'_>, JsError>;
}
pub(crate) struct CallbackGuard<'a> { realm: &'a mut Realm }
impl Drop for CallbackGuard<'_> { fn drop(&mut self); }
```

### 决策 4 —— 不变量运行时断言：回调线程 == eval 线程

`py_callback` 执行线程必须 == 发起 `eval` 的线程。**违反 ⇒ 显式错误，而非 UB**（评 A §D2.3、评 B §3）。
`Realm` 记录 `eval_thread: ThreadId`；trampoline 入口比对，不一致 ⇒ `JsErrorKind::Internal`（附线程信息）。

### 决策 5 —— 措辞精确化：「回调内不 `with_gil`」的准确含义

- **准确表述**：**不发起跨线程阻塞式取锁**。
- 同线程已持锁时 `with_gil` 是**重入**的（ADR-0012 的死锁**只在跨线程互等**时发生）；
  且 `assume_gil_acquired` 是 **unsafe**，在 `sdk-python` 内被 `unsafe_code=forbid` **禁止**（U-1 已证 lint 武装）。
- ⇒ **关键不变量是「回调总在已持 GIL 的线程上执行」；取 token 的具体调用不是判据。**

### 决策 6 —— ADR-0012 追加「修正注记」（评 A 要求闭合之点）

把 ADR-0012「对架构的直接影响 1（**必须**使用 PyO3 的 `attach`/`detach` 语义）」**限定适用范围**为：
**「回调入口线程未持有 GIL 时（暂停泵 / 跨线程路径）」**；**G1 的同线程路径不适用**。
与 ADR-0012 既有遗留「`attach`/`detach` 异步化**需另起 PoC 验证**」一致。
（本 ADR 落档时已在 ADR-0012 追加对应「修正注记」节——**修正注记不改原正文语义**。）

### 决策 7 —— 施工图 §3.6 补两条测试

| 测试名 | 断言要点 |
|---|---|
| `callback_deadline_enforced` | 回调墙钟超限 ⇒ `CallbackDeadline`（事后可判定，不挂死） |
| `callback_thread_equals_eval_thread` | 非 eval 线程触发回调 ⇒ 显式错误（非 UB） |

（另：重入两条路径 + RAII 复位，见决策 3；`no_gil_wait_in_callback` 保留。）

### 决策 8 —— 被拒方案、评 A 证伪成果、G4 登记

| 项 | 处置 |
|---|---|
| **评 A 的证伪成果** | 其攻破的 **brief 子问题 3「首选方案 C」**（U-1 已决使其前提消失）**被本裁决吸收**：采纳「不改白名单、不移边界」 |
| **方案 B（异步 attach/detach）** | 登记 **G4**；ADR-0012 遗留明写「未实测」，且把「JS `f()` 同步取值」改成异步语义，G1 超范围 |
| **方案 C（PyO3 移进 `engine-v8`）** | 登记备查；扩大 unsafe 面、与 ADR-0009 决策 2 字面冲突，且**不必要**（U-1 已决） |
| **方案 A 残余风险** | ① 阻塞式 Python 回调 ⇒ 页面线程**无界挂起**（不可被 watchdog 打断）；② `eval` 全程持 GIL ⇒ 阻塞其他 Python 线程。二者均**如实登记** |

## 后果

### 正面

- 死锁路径**结构性不存在**（非「测不出来」）；与 ADR-0012 的实测结论**相容**（经修正注记闭合字面）。
- **白名单零变更**、**ADR-0009 决策 2 不变**；`cargo deny` 许可面不变。
- 「失效 / 超限 / 跨线程」全部**可判定报错**，不静默 UB（对齐铁律 10）。

### 代价（如实登记）

1. `eval` 全程持 GIL ⇒ 阻塞其他 Python 线程。
2. 回调截止期限**不可抢占**，只能事后判定；阻塞式回调导致的挂起是 G1 已知局限（根治在 G4）。
3. 运行时线程断言覆盖「G1 内可触发的回调点」；若 G1 之外（watchdog 定时器 / 宿主消息泵 / 未来 Worker）
   有 JS 在别的线程触发回调，需**扩展断言覆盖**（否则方案 A 前提被破坏）。

### 遗留（本 ADR 不做）

- **attach/detach 异步化**（G4）：附「回调顺序 / 异常回填 / close 时 in-flight 回调」三类时序问题。
- **跨线程可中断 CPython 的可行路径**（决定期限是抢占式还是协作式）——登记，待实测。

> **G1 收尾注（2026-10-12 · S12/S13 验收 · 非语义修改）**：
> - **期限形态已落定为「协作式（事后判定）」**——抢占式**无可行路径**（ADR-0012 已证 GIL 不可中断），
>   故上述遗留的「是否抢占式」问题在 G1 **已定论为：不可抢占**，根治在 G4。
> - 决策 2/3/4 **均已实现**：`CallbackDeadline`（`crates/sdk-python/src/callback.rs:77-123`）、
>   `in_callback` RAII 重入守卫（`crates/engine-v8/src/realm.rs:657-684`，两路径测试 `realm.rs:1127/1158`）、
>   线程断言（机制层单测 `callback.rs:156-170`，因 G1 单线程下集成路径结构不可达）。
> - 未新增语义：仅登记「已实现/已定论」状态。

> **G2 修正注记 A（2026-10-13 · ADR-0018 决策 5 · 「覆盖全部重入入口」的扩展）**：
> 决策 3 的「`in_callback` RAII 且**覆盖全部重入入口**」在 G2 **显式扩展**到新增的 PyO3 入口——
> `PyContext::run_until` / `page.load` / `network.complete` / `input.dispatch` **一律**沿用 G1 形态
> （`sdk-python/src/lib.rs:122` 六处 `try_borrow_mut().map_err(|_| reentrant_pyerr())?`），
> 把重入变成**可判定错误（`ReentrantEval`）而非 `RefCell` panic**（铁律 10）。
> `PyPage`/`PyNetwork`/`PyInput` 各持 `Rc<RefCell<BrowsingContext>>`（不用 `Py<PyContext>` 反向引用）。
> **专测** `py_control_reentry_is_error`；**S14 首步验收项**。（**不改本 ADR 正文语义**，仅限定/扩展适用范围。）

> **G2 修正注记 B（2026-10-13 · ADR-0019 决策 6 · 「重入禁令」精确化为「求值类入口」）**：
> 决策 3 的重入禁令在 G2 需**区分两种重入**——① 「宿主回调内再次**求值脚本**（`eval`/`eval_module`）」
> **维持禁令**；② 「**分派期调用监听器**（`call`）」为**语义必需**（`el.click()` / 脚本内同步 `dispatchEvent`），
> 由 `engine-v8` 的**分派专用调用入口**放开（带 SAFETY + 深度论证：重入仅限「同一次事件分派的监听器调用」这一有界形状）。
> ⇒ 决策 3「`in_callback` 拒绝一切重入」的**实现范围**精确化为「拒绝求值类重入」。
> （**不改本 ADR 正文语义**，仅精确化适用范围；形态由施工图 S3/S12 定稿。）

