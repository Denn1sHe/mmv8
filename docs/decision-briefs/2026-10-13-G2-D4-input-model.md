# 决策上报：G2-D4 —— 输入事件最小模型

> 状态：**待主理人组织内部独立子 agent 背对背辩论 → 落 ADR**（依 `docs/TASKS.md` 授权变更）
> 提出人：AI 代理（架构师 高见远）｜ 日期：2026-10-13 ｜ 模板：`docs/decision-brief-template.md`
> 关联：施工图 `docs/design-g2-page-2026-10-13.md` §2.3 / §2.6 / §5（S3/S12）
> 关联 ADR：**ADR-0007**（跨语言环泄漏）、**ADR-0013**（弱引用槽 / C-2）
> 说明：本决策**不触及**铁律 6 的 scheduler/PyO3 高风险路径（分派算法本身是纯函数）；
> 仅「编排层调用监听器」的部分落在 `navigation`（非白名单、零 unsafe）。
> 本代理的推荐：**明确标注「仅供参考」**（见 §6）。**优先级：本批最低（P2）。**

---

## 0. 一句话问题

> 宿主（Python）要**模拟用户点击**：「往某个页面节点上派发一个 click 事件」，让页面里的监听器
> 被调用、默认动作（如聚焦）也生效。要拍板：**这个「模拟点击」在 G2 做到哪一步——
> 要不要支持「按坐标自动找目标节点」（命中测试）？模拟出的「可信事件」与页面自己
> `new Event(...)` 有什么必须体现的区别？**

---

## 1. 背景与上下文

- **这个仓库是什么**（面向外行）：我们用 Rust 写离线「浏览器环境模拟器」，嵌入 V8，让 Python 跑网页 JS。
  G2 要让页面能被解析、跑脚本、改 DOM、**被点击**。
- **涉及的模块 / 文件路径**：
  - `/Users/dennis/WorkBuddy/mmv8/crates/web-events/src/dispatch.rs`（**新文件：分派算法，纯函数**）
  - `/Users/dennis/WorkBuddy/mmv8/crates/web-events/src/trusted.rs`（**新文件：可信事件 + 激活状态**）
  - `/Users/dennis/WorkBuddy/mmv8/crates/navigation/src/input.rs`（**新文件：输入编排**）
  - `/Users/dennis/WorkBuddy/mmv8/crates/navigation/src/binding.rs`（监听器注册经 JS 绑定面）
- **当前状态**：`web-events` 与 `navigation` 均为空骨架；G1 的 `Realm::call`（调用 JS 函数）已验收，
  可用于调用监听器句柄。
- **为什么现在必须决策**：① 「输入 click」是 G2 六项必产出之一（技术方案 §18）与纵向样例的收尾；
  ② 「命中测试」（坐标 → 目标节点）**依赖布局几何**，而 **G2 的几何是 stub（见 D1/施工图 R1）**，
  所以要明确 G2 输入到底承诺什么；③ 「可信事件 / user activation」的语义会影响 SDK 契约与 G3。

---

## 2. 需要拍板的问题

- **主问题**：G2 的「输入」应覆盖到哪一层？
- **子问题**：
  1. **命中测试**（坐标 → 目标节点）：G2 做还是推 G3？
  2. 可信事件（`isTrusted=true`）在 G2 要体现哪些可观测差异？
  3. **user activation**（「用户激活」状态）G2 要不要维护？维护到什么程度（sticky / transient）？
  4. **默认动作**（如 click 聚焦）G2 做到哪些？
  5. 监听器**回调**由谁调用、如何在 C-2（弱引用槽）约束下安全调用？

---

## 3. 备选方案（至少两个）

### 方案 A：**显式 target 可信事件（不做命中测试）**（本代理推荐，仅供参考）

- **做法**：`input.dispatch(kind, target)` **要求宿主显式给出目标节点**（NodeId），校验该节点属于
  当前 Document；合成 `isTrusted=true` 的事件，走 capture→target→bubble 分派；可信「激活类」事件
  （click/keydown）置 `UserActivation.sticky`；默认动作只做**最小**（click ⇒ 聚焦）。
  **命中测试（坐标→节点）明确推 G3**（因为 G2 几何是 stub，无可靠命中依据），
  `capabilities.input == "explicit_target"`。
- **关键代码（设计契约原文）**：

  ```rust
  // web-events：纯算法（不调 v8）
  pub enum EventPhase { None, Capturing, AtTarget, Bubbling }
  pub struct DispatchStep { pub target: NodeId, pub phase: EventPhase, pub listeners: Vec<ListenerRef> }
  pub fn plan_dispatch(tree: &NodeTree, registry: &ListenerRegistry, target: NodeId, event: &Event) -> Vec<DispatchStep>;

  #[derive(Clone, Copy, Debug, Default)]
  pub struct ListenerOptions { pub capture: bool, pub passive: bool, pub once: bool }
  #[derive(Clone, Copy, Debug)]
  pub struct ListenerRef { pub callback: engine_v8::WeakJsHandle, pub options: ListenerOptions } // 弱引用（C-2）

  #[derive(Clone, Copy, Debug, Default)]
  pub struct UserActivation { pub sticky: bool, pub transient: bool }

  // navigation：编排层调用监听器（经 engine-v8 安全 API）
  pub enum InputKind { Click, KeyDown, Focus }
  impl BrowsingContext {
      pub fn input_dispatch(&mut self, kind: InputKind, target: Option<NodeId>) -> Result<(), PageError>;
  }
  ```

- **优点**：① 不需要布局即可工作（避开 G2 几何 stub 的坑）；② 分派算法是**纯函数**，易测；
  ③ 监听器用**弱句柄**（C-2），回收即跳过，不重演 ADR-0007；④ 范围清晰、可交付。
- **缺点与风险**：① 宿主必须知道目标节点（对「模拟坐标点击」不友好——但那属 G3）；
  ② `UserActivation` 只做最小，复杂激活语义（transient 消耗、嵌套）留给 G3。
- **长期影响**：G3 接布局后，`input.dispatch` 增加「坐标 → 目标」分支，**接口不变**（`target: Option<..>` 留位）。
- **预估工作量**：小。

### 方案 B：**完整命中测试 + 坐标输入**

- **做法**：`input.dispatch(kind, coords)` 通过布局做命中测试得到目标节点；支持 pointer/button/focus 状态。
- **优点**：更接近真实浏览器输入。
- **缺点与风险**：**依赖 G2 不存在的布局几何**（stub）⇒ 命中结果不可信，等于**假绿**；
  会把「布局未实现」的问题掩盖成「输入已实现」。违反本仓「不假绿」纪律。
- **长期影响**：与 G3 强耦合，过早实现大概率返工。
- **预估工作量**：大（且被几何阻塞）。

### 方案 C：**只做 `dispatchEvent`（不支持宿主可信输入）**

- **做法**：只支持页面内 `el.dispatchEvent(new Event(...))`（非可信），宿主输入**不提供**。
- **优点**：最小。
- **缺点与风险**：**不满足 G2 必产出**（技术方案 §18 明列「输入」；§7 要求「宿主 input 接口生成可信
  模拟事件并更新 pointer、button、focus 和 user activation 状态」）。
- **预估工作量**：小（但不符合闸门要求）。

---

## 4. 证据（原文粘贴）

**证据 1 —— 技术方案 §7「事件与输入」原文**：

> 「事件模型覆盖 capture、target、bubble、once、passive、AbortSignal、默认动作、stopPropagation、
> preventDefault 与监听器变更。**普通 new Event/dispatchEvent 保持非可信状态；宿主 input 接口生成
> 可信模拟事件并更新 pointer、button、focus 和 user activation 状态。可信事件不是「真实硬件输入」的证明。**
> 坐标输入从命中测试确定目标，**显式 target 输入则验证节点仍属于当前 Document**。click、focus、
> 表单提交、键盘输入的默认动作采用统一状态机，防止只发事件而不更新页面状态。」

**证据 2 —— 技术方案 §18 G2 必产出原文**：「流式解析、阻塞脚本、`document.write`、导航、
**资源等待和输入**。」

**证据 3 —— ADR-0007 实测（跨语言环泄漏，约束监听器不得持强引用）**：

```
=== EXP3 真循环：DOM → Global(闭包) → 闭包捕获 DOM ===
  WeakRef 判定：exp3 存活 = true（**true = 被跨语言环保活，即泄漏**）
CYCLE_LEAK=true
```

**证据 4 —— ADR-0013 决策 6 / C-2 原文（监听器槽纪律）**：

> 「`web-dom` / `web-events` 的类型面**不得**出现 `JsHandle` / `HandleLease`；
> **事件监听器槽只允许 `NodeId`（整数）或 `WeakJsHandle`**。」

**证据 5 —— 施工图 §2.6 / §6 R1（几何 stub 口径）**：

> 「几何 API 在 G2 返回**确定 stub 值**（数值 0 / 空 rect），`capabilities.geometry == "stub"`。
> 完整几何 + 布局 flush 属 G3。」⇒ **命中测试在 G2 无可靠依据。**

**证据 6 —— 现有仓库事实（已核实）**：

```
crates/web-events/Cargo.toml → [dependencies] 为空（骨架）
crates/navigation/src/       → 暂无 input.rs / binding.rs（G2 新建）
crates/engine-v8/src/realm.rs → call(handle, this, args) 已验收 → 可用于调用监听器
crates/engine-v8/src/node_map.rs → 弱句柄映射已验收
```

---

## 5. 本仓库的既定约束（外部/内部评审必须在此边界内给建议）

- **ADR-0013 决策 6 / C-2** 原文（见 §4 证据 4）：监听器槽只允许 `NodeId` 或 `WeakJsHandle`；
  `web-events`/`web-dom` 类型面**不得**出现 `JsHandle`/`HandleLease`。
- **ADR-0007** 原文：DOM 侧只许弱引用或单向（`NodeId` 反查）；禁止「DOM →(强)→ 闭包 →(捕获)→ DOM」。
- **铁律 5 / 守卫规则 5**：`v8::` 只许出现在 `engine-v8` ⇒ `web-events`/`navigation` 零 `v8::`。
- **铁律 4**：`web-events`/`navigation` 不在白名单 ⇒ **零 `unsafe`**。
- **技术方案 §7** 原文（见 §4 证据 1）：可信事件 ≠ 真实输入；显式 target 须校验属当前 Document。
- **「不假绿」纪律**（`docs/HANDOFF-2026-10-12.md` §六.5）：不得用恒真/不可信的门禁冒充能力。
- **跨平台铁律**：mac + linux x86_64 双端可编译。

---

## 6. 本代理的推荐（**仅供参考，等待内部辩论**）

- **推荐方案 A（显式 target 可信事件，命中测试推 G3）**，理由：
  1. **几何是 stub ⇒ 命中测试无可靠依据**，现在做命中等于**假绿**（违反本仓纪律）；
  2. 显式 target 已能兑现 §7「显式 target 输入则验证节点仍属于当前 Document」与 §18「输入」；
  3. 分派算法为**纯函数** ⇒ 可独立测试（capture/target/bubble 顺序、once/passive）；
  4. 监听器走**弱句柄**（C-2）⇒ 结构上不重演 ADR-0007。
- **我不确定的点（希望外部 AI 重点看）**：
  1. `UserActivation` 在 G2 需要做到什么程度？（只 sticky？transient 消耗？）会不会影响 G3 的
     剪贴板/全屏等能力门控？
  2. 默认动作 G2 只做「click ⇒ 聚焦」是否足够？还是应当**全部登记为 G3**（连聚焦也不做）？
  3. `input.dispatch` 的签名中，`target: Option<NodeId>`（G3 用 `None` + 坐标）是否为好的扩展位？
  4. 「监听器回调」是否应由 `navigation::input` 直接经 `Realm::call` 调用，还是经一个更薄的
     「事件桥」以便 G3 复用？（本设计倾向前者，保持最小面。）

---

## 7. 请重点回答的问题

1. G2 把**命中测试推给 G3**、只做**显式 target 可信事件**，是否是正确取舍？如何避免「输入已实现」被
   误读为「坐标点击已可用」？（需要什么具名口径 / `capabilities` 标志？）
2. 可信事件（`isTrusted`）、`UserActivation`、默认动作三者在 G2 的**最小但可测**集合分别应是什么？
3. 监听器以 `WeakJsHandle` 存储、调用前 `resolve`（回收即跳过）是否是 C-2 下的正确形态？
   有没有比「回调时反查」更好的做法？
4. 本决策是否有被低估的**铁律 6** 风险（例如分派若在 scheduler 任务内进行，是否也需上报）？
