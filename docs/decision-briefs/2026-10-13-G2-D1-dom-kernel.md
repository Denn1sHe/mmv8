# 决策上报：G2-D1 —— DOM 内核的数据结构与所有权模型

> 状态：**待主理人组织内部独立子 agent 背对背辩论 → 落 ADR**（依 `docs/TASKS.md` 授权变更：
> 结论不阻塞等待，落 ADR 后即可执行）
> 提出人：AI 代理（架构师 高见远）｜ 日期：2026-10-13 ｜ 模板：`docs/decision-brief-template.md`
> 关联：施工图 `docs/design-g2-page-2026-10-13.md` §1.2 / §2.2 / §5（S1/S2/S7/S11）
> 关联 ADR：**ADR-0009**（类型面 / C-1~C-4）、**ADR-0007**（跨语言环泄漏）、**ADR-0013**（弱槽+租借 / node_map）
> 本代理的推荐：**明确标注「仅供参考」**（见 §6）

---

## 0. 一句话问题

> 我们要在 Rust 里造**规范 DOM 节点树**（网页文档的内存表示）。要拍板：**这棵树用什么数据结构存
> （每个节点怎么被引用、删除后节点身份怎么不被误认），以及当一个 JS 对象要「包装」一个 DOM 节点时，
> 两边怎么互相记住对方而不会像我们实测过的那样造出永远回收不掉的内存环？**

---

## 1. 背景与上下文

- **这个仓库是什么**（面向外行）：我们用 Rust 写一个离线「浏览器环境模拟器」——把 Google 的
  JavaScript 引擎 V8 嵌入进来，让 Python 能跑依赖网页环境的 JS（解析 HTML、操作 DOM、发网络请求等）。
  目前刚完成「引擎纵向验证」（G1：能 eval、能调宿主函数、能跑微任务）。**下一步 G2 是让一个真实网页
  能被解析、执行脚本、改写 DOM、被点击**。
- **涉及的模块 / 文件路径**：
  - `/Users/dennis/WorkBuddy/mmv8/crates/web-dom/`（**新实现，本决策的主题**；目前是空骨架）
  - `/Users/dennis/WorkBuddy/mmv8/crates/engine-v8/src/node_map.rs`（`NodeId ↔ 弱句柄` 映射，已验收）
  - `/Users/dennis/WorkBuddy/mmv8/crates/engine-v8/src/handles.rs`（弱槽句柄表，已验收）
  - `/Users/dennis/WorkBuddy/mmv8/crates/engine-v8/src/abi.rs`（`NodeId/JsHandle/WeakJsHandle` 打包约定）
  - `/Users/dennis/WorkBuddy/mmv8/docs/adr/0013-G1-D1-句柄表与GC纪律-弱引用槽表加显式租借.md`（C-1~C-4）
  - `/Users/dennis/WorkBuddy/mmv8/docs/adr/0007-T2.1-cppgc跨语言环无法回收.md`（泄漏实测）
- **当前状态**：G1 已完成（`engine-v8` 的句柄表 = 弱引用槽 + 显式租借；`node_map` = `NodeId↔弱句柄` 双向映射，
  持**弱句柄**）。但 **DOM 节点树本身尚不存在**——`web-dom` 是空骨架。
- **为什么现在必须决策**：DOM 内核是整个 G2/G3/G4 的**基点**：它的数据结构决定了「节点身份、
  增删改、查找、与 JS 包装的绑定」全部写法。**一旦写进代码，改动会波及所有调用点**（解析器、
  事件、绑定、序列化）。ADR-0009 决策 5 已就此警告「句柄表形态错了要到 G2/G3 才暴露，返工成本极高」。

---

## 2. 需要拍板的问题

- **主问题**：DOM 节点树用什么数据结构？节点之间如何互相引用？节点「身份」如何在一个节点被删除、
  槽位被复用后仍不被误认？
- **子问题**：
  1. 树用**扁平 arena（连续数组 + 下标）**、**独立对象分配 + 引用计数（`Rc<RefCell>`）**、
     还是**第三方 `slotmap` crate**？
  2. 一个 JS 对象「包装」一个 DOM 节点时：**节点侧存不存指向 JS 包装的引用**？存**强引用**还是**弱引用**？
  3. 节点删除后其下标可能被复用 —— 怎么保证「旧句柄不会静默指到新节点」？
  4. 「同一节点在同一 JS Realm 中必须复用同一包装」这条要求（技术方案 §5）由谁实现、怎么实现？
  5. 这样设计能否逐条兑现 **C-1~C-4**（ADR-0009/0013 硬约束）？

---

## 3. 备选方案（至少两个）

### 方案 A：**扁平 arena（数组 + 下标 + generation）**（本代理推荐，仅供参考）

- **做法**：所有节点存在一个连续数组里；节点之间不是「指针」而是「数组下标」（外加一个「第几代」
  计数防复用）。删除只把该槽标记为可复用并让「代」加一。JS 包装与节点的对应关系放在**引擎侧**的
  一张**弱引用**表里（`node_map`，已实现），DOM 侧**不存任何 JS 引用**。
- **关键代码（设计契约原文）**：

  ```rust
  /// 节点身份。高 32 位 = generation（复用防呆），低 32 位 = arena 下标。
  #[repr(transparent)]
  #[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
  pub struct NodeId(u64);
  impl NodeId {
      pub const fn new(index: u32, generation: u32) -> Self;
      pub const fn index(self) -> u32;
      pub const fn generation(self) -> u32;
      pub const fn raw(self) -> u64;     // 供 engine_v8::node_map 的 u64 key
  }

  /// 节点树：单线程 arena。零 unsafe（全部按下标索引，无自引用指针）。
  pub struct NodeTree {
      nodes: Vec<Option<NodeData>>,      // 槽位（None = 已删除/空）
      generations: Vec<u32>,             // 每槽的「代」
      free: Vec<u32>,                    // 可复用槽下标
      root: NodeId,
      len: usize,
  }
  pub struct NodeData {
      pub kind: NodeKind,                // Document/Element/Text/Comment/DocumentType
      pub parent: Option<NodeId>,
      pub first_child: Option<NodeId>, pub last_child: Option<NodeId>,
      pub prev_sibling: Option<NodeId>, pub next_sibling: Option<NodeId>,
      pub element: Option<ElementData>, pub text: Option<String>,
  }
  ```

  与 JS 包装的对应关系（**已实现**，`engine-v8/src/node_map.rs`）：

  ```rust
  /// `NodeId ↔ 弱句柄` 双向映射。持 WeakJsHandle（不阻止 GC）。
  pub struct NodeMap {
      by_node: HashMap<u64, WeakJsHandle>,
      by_handle: HashMap<WeakJsHandle, u64>,
  }
  ```

- **优点**：① **零 unsafe**、可序列化、缓存友好；② 节点身份用「下标 + 代」⇒ 删除后复用槽，
  旧 `NodeId` 因「代」不同而**可判定失效**（与 G1 的 `JsHandle` 同款思路，见 §5 约束）；
  ③ DOM 侧**不持 JS 引用** ⇒ 结构上不可能造出 ADR-0007 的跨语言环；
  ④ 「同一节点同一包装」由已验收的 `node_map` 承担（**弱**引用，不钉住 wrapper）。
- **缺点与风险**：① 需要自己维护 free list 与「代」（多写一点代码，但有 G1 的 `HandleTable` 为范本）；
  ② 「节点内部可变」要经 `Rc<RefCell<NodeTree>>` 共享给解析器 sink 与 JS 绑定 ⇒ **借用纪律不能重入**
  （解析 sink 与 JS 回调不得同时借）。
- **长期影响**：与 G1 的「弱槽 + generation」纪律同构，心智负担最低；G3 接 Stylo/布局时只需投影，不需改内核。
- **预估工作量**：中（arena 与代管理 ~ 参考 G1 `handles.rs`）。

### 方案 B：**每节点 `Rc<RefCell<Node>>`，父子用强引用**

- **做法**：每个节点是一个独立的堆对象，父持子**强引用**、子持父**弱引用**（`Weak`）避免纯环。
  JS 包装与节点的对应仍可放 DOM 侧。
- **关键代码（示意）**：

  ```rust
  pub struct Node {
      kind: NodeKind,
      parent: Weak<RefCell<Node>>,
      children: Vec<Rc<RefCell<Node>>>,
      attrs: AttributeMap,
  }
  pub type NodeRef = Rc<RefCell<Node>>;
  ```

- **优点**：贴近「DOM 是对象图」的直觉；局部改动方便。
- **缺点与风险**：① **无节点身份 generation** ⇒ 删除后释放、新节点恰好复用同一内存时，
  残留 JS 包装可能指向**错误的新节点**（ABA）；② 遍历/序列化需到处 `borrow()`，**运行期借用 panic 面大**；
  ③ 若 DOM 侧再存「节点 → JS 包装」的强引用，就会重演 ADR-0007 的持久强引用环（**已实测泄漏**）。
- **长期影响**：与「句柄带代」的 G1 纪律不一致，两套心智。
- **预估工作量**：中。

### 方案 C：**引入 `slotmap` crate（成熟的 arena + 代键）**

- **做法**：用现成 crate `slotmap` 的 `SlotMap`（键自带 generation）替代手写 arena。
- **关键代码**：

  ```rust
  use slotmap::{SlotMap, new_key_type};
  new_key_type! { pub struct NodeKey; }
  pub struct NodeTree { nodes: SlotMap<NodeKey, NodeData>, }
  ```

- **优点**：代键机制成熟、少写代码。
- **缺点与风险**：① 新增一个**第三方依赖**（需过 `cargo deny` 许可/来源检查、跨平台确认）；
  ② `NodeKey` 是 `slotmap` 自有类型，需与 `engine-v8::node_map` 的 `u64` key **再适配一层**
  （`slotmap` 文档明确不保证 `NodeKey` 可作为稳定 `u64` 序列化）；
  ③ 与「类型面只用本仓定义类型」的倾向不符。
- **长期影响**：多一个关键依赖；未来若需自定义布局（G3 投影）可能反受束缚。
- **预估工作量**：小（但引入依赖的合规成本另计）。

---

## 4. 证据（原文粘贴）

**证据 1 —— ADR-0007 实测：跨语言环无法回收（`CYCLE_LEAK=true`）**：

```
=== EXP3 真循环：DOM → Global(闭包) → 闭包捕获 DOM ===
  WeakRef 判定：exp3 存活 = true（**true = 被跨语言环保活，即泄漏**）
BASELINE_VALID=true
CYCLE_LEAK=true
```

> 根因原文：「cppgc 堆与 V8 堆是**两个独立堆**，V8 GC 只认识自己的根，而 `v8::Global` 正是这样一个根。」

**证据 2 —— ADR-0009 决策 5 的 C-1~C-4 原文（硬约束）**：

> **C-1** `JsHandle` 不是 GC 根（实现只能是 `engine-v8` 内部 slot table 的下标）。
> **C-2** `web-dom / web-events` 类型面不得出现 `JsHandle` / `HandleLease`；事件监听器槽只允许
> `NodeId`（整数）或 `WeakJsHandle`。
> **C-3** `NodeId↔JsHandle` 表归 `engine-v8`。
> **C-4** 禁止强引用环（跨堆追踪本轮不做）。

**证据 3 —— 已验收的 `node_map`（`crates/engine-v8/src/node_map.rs`）**：

```rust
/// 持 WeakJsHandle 而非 JsHandle（ADR-0013 决策 6 / C-2）：node_map 若持强句柄会
/// 永久钉住 wrapper，重演 ADR-0007 的泄漏（且泄漏点在白名单 crate 内部、门禁不可见）。
pub struct NodeMap { by_node: HashMap<u64, WeakJsHandle>, by_handle: HashMap<WeakJsHandle, u64> }
pub fn bind(&mut self, node: u64, handle: WeakJsHandle) -> Option<WeakJsHandle>; // 同一 node 复用同一 wrapper
```

**证据 4 —— 已验收的句柄 generation 复用防呆（`handles.rs`）**：

```rust
// free：清空 weak + generation += 1；generation 回绕到 0 ⇒ 退役该槽（彻底消除 ABA）
// resolve：比对完整 (index, generation)；弱空 ⇒ Collected；generation 不匹配 ⇒ StaleHandle
```

**证据 5 —— 技术方案 §5 原文（节点身份要求）**：

> 「DOM 用带 generation 的 NodeId 标识节点……同一 Realm 中同一节点必须复用同一 wrapper；
> 跨 Realm 的包装、原型和访问检查单独处理。断开的节点仍可能被 JS 持有，不能在 removeChild 时释放；
> 节点池复用也不能让旧句柄指向新节点。」

**证据 6 —— 现有仓库事实（已核实）**：

```
crates/web-dom/Cargo.toml   → [dependencies] 为空（骨架）
crates/web-dom/src/lib.rs   → 仅 crate 文档（占位）
crates/engine-v8/src/node_map.rs  → NodeMap 已实现并验收（持 WeakJsHandle，key 为 u64）
engine-v8/src/abi.rs 约定：JsHandle/WeakJsHandle 打包 = 高 32 位 generation，低 32 位 下标
```

---

## 5. 本仓库的既定约束（外部/内部评审必须在此边界内给建议）

- **铁律 4**（`AGENTS.md`）原文：「**unsafe 全仓禁止**（workspace lints 强制）。仅白名单 crate
  （`engine-v8` 及显式声明的 FFI 薄桥）例外……」。**`web-dom` 不在白名单** ⇒ **零 `unsafe`**。
- **铁律 5**原文：「**V8 / rusty_v8 类型不得越出 `engine-v8` crate**。」（守卫脚本**规则 5** `v8-type-escape` 机器强制）
- **ADR-0009 决策 5 C-1~C-4**（原文见 §4 证据 2）。
- **ADR-0013**：句柄 = **弱槽索引**（不 root）；存活由 `HandleLease` 显式承载；失效分
  `Collected`（弱空）/ `StaleHandle`（generation 不匹配）；`node_map` 持 **`WeakJsHandle`**。
- **ADR-0009「不新增 workspace 成员」**：本决策若要新增 crate（如 `slotmap` 是依赖不是成员）应另行说明。
- **跨平台铁律**：`web-dom` 代码须 mac + linux x86_64 双端可编译；文件名全小写下划线。
- **铁律 2**：本决策若产生新的 golden/快照，须独立 PR。

---

## 6. 本代理的推荐（**仅供参考，等待内部辩论**）

- **推荐方案 A（扁平 arena + 下标 + generation）**，理由：
  1. **零 `unsafe`**，完全落在白名单之外也能实现；
  2. 节点身份用「下标 + 代」⇒ 复用槽后旧 `NodeId` **可判定失效**（与 G1 `HandleTable` 同款纪律，
     直接兑现技术方案 §5「节点池复用不能让旧句柄指向新节点」）；
  3. DOM 侧**不持任何 JS 引用** ⇒ 与 ADR-0007/0013「单向 + 弱引用」一致，**结构上不可能**造出跨语言环；
  4. 「同一节点同一 wrapper」由**已验收的 `node_map`（弱引用）**承担，不引入第二真相源（C-3 归 `engine-v8`）；
  5. 不引入新依赖，避开 `cargo deny` 与跨平台确认成本。
- **我不确定的点（希望外部 AI 重点看）**：
  1. 「节点 → JS 包装」的弱引用，**回收后再次访问同一节点**时要「新建一个包装」——
     这是否会产生「同一节点两个包装」的身份裂缝？规范要求「同一 Realm 同一节点同一包装」，
     弱引用被回收后重建是否可接受？（G1 已明确：`node_map` 持弱句柄，回收即失效；这是本仓既定代价。）
  2. 删除节点后**是否立刻回收其 arena 槽**？若立刻回收，仍被 JS 持有的断开节点其 `NodeId` 会失效——
     规范要求「断开的节点仍可被 JS 持有」。**建议**：删除只**脱离树**（断开父子），
     **不**回收 arena 槽（槽回收另设显式 `dispose`，本轮可不做）⇒ 避免断开节点身份失效。
  3. 「代」用 G1 同款 32 位够不够？（G1 决策 4 认为够。）
  4. 连续 arena 在大文档（数万节点）下的扩容/内存是否可接受（相对 `Rc<RefCell>`）。

---

## 7. 请重点回答的问题

1. 方案 A 是否是兑现 C-1~C-4 与「节点身份 generation」的正确形态？有没有**同样零 unsafe、
   但更简单**的形态？
2. 「同一节点同一包装」在**弱引用被回收后**的语义裂缝（问题 1/2）应如何处理才既符合规范、
   又不重演 ADR-0007？
3. 节点**删除**与 **arena 槽回收**是否应解耦（断开 ≠ 回收）？给出建议与理由。
4. 若选 `slotmap`（方案 C），如何满足「key 与 `engine-v8::node_map` 的 `u64` 稳定对接」与
   「类型面只用本仓定义类型」两条约束？还是应当拒绝该依赖？
