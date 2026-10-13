# ADR-0020：G3 D1 —— 布局后端路线与 **opaque/rehydrate 策略**（直连 Stylo + Taffy + Parley）

- 状态：**已接受**（2026-10-13 主理人汇总裁决；两位内部独立评审背对背辩论 ⊕ 主理人对全部事实指控的
  原文级亲验：**0 驳回**）
- 日期：2026-10-13
- 关联：
  - **ADR-0011**（T2.4 裁决 `SWITCH_TO_STYLO`）—— 本 ADR 为其**表述更正注记**（见「对 ADR-0011 的更正注记」）
  - **ADR-0016**（结构性借用规则 / arena + generation）、**ADR-0017**（调度 + as-built）、**ADR-0018**（导航重绑）
  - **ADR-0009**（`v8::` 类型边界 / 不新增 workspace 成员）
  - 施工图 `docs/design-g3-layout-2026-10-13.md` §1.2 / §1.5 / §2 / §3.5 / §7
  - **决策 brief**：`docs/decision-briefs/2026-10-13-G3-stylo-direct.md`
  - **评审意见（只读证据）**：`docs/decision-briefs/2026-10-13-G3-辩论意见-A.md`（①②D1、B1/B2/B6/B8）、
    `-B.md`（①D1/D2、③B1/B2）
  - **裁决书**：`docs/decision-briefs/2026-10-13-G3-裁决-主理人.md` §二·D1 / §三（G3-B1/B2/B5/B6/B13）/ §四（P-1/P-4）
- 决策者：主理人（两评 D1 方向一致；分歧全在机制层 ⇒「方向维持 + 机制修正」）
- 落地步骤：施工图 **S1 / S3（探针）/ S6（stylo_host）/ S11（shadow）**

---

## 问题

G3 要把「页面」从「能解析、能跑脚本」推进到「**能算样式、能算布局、能读几何尺寸**」。ADR-0011 已裁决
切「**直接 Stylo + Taffy + Parley**」。需拍板：
1. 该路线**是否立即投入**；
2. 该路线的**核心价值主张**如何表述（「自建 `Element` 是否真能绕开 blitz 的 Shadow 堵点」）；
3. **`opaque()` / rehydrate 机制**如何设计（这是全案**唯一的真难题**，施工图原稿把「零 unsafe 实现
   全套 trait」当作**已定事实**，而 P-1/P-4 判定**未覆盖**它）；
4. Shadow 支撑面的 G3 边界。

## 讨论要点（两评审的真实分歧与采纳）

| 维度 | 评审 A（证伪） | 评审 B（从零） | 主理人裁决 |
|---|---|---|---|
| 方向 | 支持 `SWITCH_TO_STYLO`（blitz 桩已逐行复核） | 同 | **维持方向**（一致） |
| 核心价值主张 | **攻破**：宣称「自建绕开 `OpaqueElement` 限制」未证；该限制载体是**无 self 关联函数**，自建同样只有 64-bit opaque | **同判**：opaque/rehydrate 是第一号难题，须先探 | **采纳：表述更正**（决策 1） |
| P-4 判据 | **打错靶**：判据不调用 `implicit_scope_for_sheet_in_shadow_root` | 判据偏窄且依赖不存在的 JS 查询面 | **采纳：P-4 重定义**（决策 3） |
| blitz 堵点定性 | 「结构性限制」**定性过强**；blitz 自述可修（「We should see if selectors will accept a PR…」） | — | **采纳：降级为「上游实现所限、修正不可控于本仓」+ 补备选 D**（决策 4） |
| crate trait 路径 | `stylo_dom` 无 `TNode/TElement`，trait 在 `stylo`（import 名 `style`）的 `dom` 模块 | 同 | **采纳**（决策 5，见 §三 G3-B5） |
| 「零 unsafe」 | 与「可实现全套 trait」不可兼得（二选一必错其一） | 同 | **采纳：以「线程局部侧表」实现 ⇒ 零 unsafe 为准；若必须 unsafe ⇒ 停上报**（决策 2） |

## 决策

### 决策 1 —— **维持 `SWITCH_TO_STYLO`**，核心理由**表述更正**

**更正后表述**：

> **我方与 blitz 面对同一个 opaque/rehydrate 问题**；差异在于**策略自主**：
> `OpaqueElement` 是 `Copy + Eq + Hash` 的 64-bit 句柄（**可作侧表键**），而
> `implicit_scope_for_sheet_in_shadow_root` 是一个**无 `self` 的关联函数** ⇒ **必须线程局部/全局侧表**
> 反查（锚点稳定策略 + 生命周期管理）；**路径可行性以待 P-1 实测**。blitz 因以 `NodeId` 作 opaque、
> 需 `Slab` 反查而受制，修形须改上游（**不在我方掌控**）。

原稿「自建 `Element` 可**绕开** blitz 的结构性限制」**不再作为理由**——它绕开的是**blitz 的 DOM 抽象层**，
不是 opaque 问题本身。

**源码证据（原文，主理人亲验）**：
- `selectors-0.40.0/tree.rs:17` `pub struct OpaqueElement(NonNull<()>);`；`:41` `pub unsafe fn as_const_ptr<T>(...)`（唯一反解，unsafe）；`:50` `fn opaque(&self) -> OpaqueElement;`（**无默认实现**）。
- `stylo-0.20.0/dom.rs:976` `fn implicit_scope_for_sheet_in_shadow_root(_opaque_host: OpaqueElement, _sheet_index: usize) -> Option<ImplicitScopeRoot> { None }`（**关联函数、无 `self`**）；调用点 `stylo-0.20.0/stylist.rs:3150` `match E::implicit_scope_for_sheet_in_shadow_root(host, index) {`。
- `blitz-dom-0.3.0-beta.2/src/stylo.rs:577` 注释原文：`// We cannot currently implement this as we are using the NodeId as the OpaqueElement, and need a reference to the Slab to convert it back into an Element`。

### 决策 2 —— **opaque/rehydrate = 线程局部侧表**（**零 unsafe 为准**；受限 unsafe 须停上报）

**机制草案**（详见施工图 §1.2.1）：

```rust
/// 线程局部「opaque 侧表」——把 OpaqueElement 当作不透明键，**从不反解指针**。
pub struct OpaqueSideTable {
    anchors:   HashMap<NodeId, Box<NodeAnchor>>,     // 每 (tree,node) 一个稳定地址锚点
    by_opaque: HashMap<OpaqueElement, NodeId>,        // 关联函数据此反查（纯查表）
    active_tree: Cell<Option<TreeId>>,
}
struct NodeAnchor { tree: TreeId, id: NodeId }        // Box ⇒ 地址不随 Vec 增长而变
thread_local! { static SIDE: RefCell<OpaqueSideTable> = RefCell::new(OpaqueSideTable::default()); }
```

- **零 unsafe 论证**：`OpaqueElement::new(&*anchor)` 为**安全**函数（取 `&T`）；我方**从不**调用
  `unsafe fn as_const_ptr`，只把 `OpaqueElement` 当 `Eq+Hash` **键**使用 ⇒ **无 unsafe 面**。
- **锚点稳定**：锚点为 `Box<NodeAnchor>`（堆分配地址稳定，规避 `NodeTree.nodes: Vec<Option<NodeData>>`
  在 append 时 realloc 的地址不稳问题）。
- **生命周期**：侧表在**每次 `resolve` 起清空/复用**；`reset()`（导航）时清空（对齐 ADR-0018·D2）。
- **执行纪律**：**P-1 首要判定**含此机制（建 ≥2 节点，走 `opaque()`→关联函数反查路径；
  判定 = 编译 + 反查命中 + **零 unsafe**）。**若探针发现必须 unsafe ⇒ 停下上报**，不得静默开白名单。

### 更正注记（S1/P-1 实测，2026-10-13）—— **`TElement` 必需 `unsafe fn` 阻断「零 unsafe」**

> **实测结果**（证据 `.workbuddy/refs/g3-probe-evidence/` 原文；探针 `poc/t3_g3_probe`）：
> - **P-1 的 opaque 侧表机制本身 = 零 unsafe，反查命中**：`OPAQUE_ADDRESS_STABLE=true`、
>   `IMPLICIT_SCOPE_REVERSE_LOOKUP=Some(100)`、`P1_MECHANISM_ZERO_RESTRICTED_CODE_AND_HITS=true`
>   （探针 crate 级 `#![forbid(unsafe_code)]` 下**编译通过**）。
> - **P-1 总判定 = BLOCK（`TELEMENT_REQUIRED_unsafe_FNS=5`）**：`style::dom::TElement` 有 **5 个必需
>   `unsafe fn`**（**无默认实现**）：`set_handled_snapshot`（`stylo-0.20.0/dom.rs:648`）、
>   `set_dirty_descendants`（`:675`）、`unset_dirty_descendants`（`:680`）、
>   `ensure_data`（`:739`）、`clear_data`（`:744`）。Rust 要求 **impl 亦声明为 `unsafe fn`**
>   ⇒ `forbid(unsafe_code)` 拒绝其**声明与实现**（最小复现 `forbid_unsafe_fn_repro.rs`）。
> - 本仓 `[workspace.lints.rust] unsafe_code = "forbid"`（`Cargo.toml:67`）；`forbid` **不可被 `#[allow]` 覆盖**
>   ⇒ `layout-backend`（白名单外 + 继承 forbid）**无法实现 `TElement`**。
> - 守卫脚本 `scripts/check-unsafe-whitelist.sh` 的 `WHITELIST = ("crates/engine-v8/","poc/")`
>   ⇒ **`layout-backend` 不在白名单**。
>
> **⇒ 决策 2 的「零 unsafe 为准」须精确化为**：**opaque 机制零 unsafe（已验证）；但 `TElement` 表面
> 强制 5 个具名 `unsafe fn` ⇒ 承载 `TElement` 实现的 crate 不可能零 unsafe。**
> **⚠️ 追加（工程师实测提示，架构师复核为真）**：`ensure_data`/`clear_data` 的**函数体****曾疑**需真 `unsafe`
> 腔体（blitz 参考实现 `stylo.rs:692/697` + `node/stylo_data.rs` 用 `UnsafeCell<Option<ElementDataWrapper>>`；
> `ensure_data(&self)` 的共享引用 + 返回 `ElementDataMut<'_>` 契约使 `RefCell` 不可行）。**P-1b 实测已定形
> ⇒ 实际 unsafe 面 = 「5 声明 + 0 腔体」（终值）**——「急切持有 `ElementDataWrapper` + 安全 `borrow_mut()`/
> `DerefMut` 写 default」成立，**无需 `UnsafeCell`**（commit `407c0aa`）。此点已并入白名单 brief §3·A / §6.4 / §7.4 / 证据 E。
> **这是「上游 trait 表面」的阻断，非机制缺陷**（工程师已按本 ADR 决策 2 的纪律**停下上报**：任务 #6）。
> **处置（新增决策点，走决策上报协议 —— 见 `docs/decision-briefs/2026-10-13-G3-unsafe-whitelist-layout-backend.md`）**：
> - **A（推荐）**：把承载 `TElement` 实现的 crate 加入 unsafe 白名单（`[lints.rust] unsafe_code = "deny"`
>   + 守卫脚本 `WHITELIST` + **ADR-0002 修正注记**），实现处**逐 `fn` `#[allow(unsafe_code)]` + `// SAFETY:`**；
> - **B**：新建**极薄**的、显式声明的「stylo FFI 薄桥」crate（白名单），只放 `TElement` 等外表实现，
>   `layout-backend` 保持零 unsafe（**代价**：`ADR-0009`「不新增 workspace 成员」须修正）。
> - **C（不成立）**：`deny` + 单个 `#[allow]` —— 仍须先入白名单（守卫规则 1/4），且违反「整模块开口」禁令；
>   **D（不成立）**：不实现 `TElement` ⇒ 无法跑 stylo 级联 ⇒ 与 ADR-0011 冲突。
> **S1 结论：`G3_S1_VERDICT=BLOCK`**（P-8/P-11/opaque 机制均 PASS；P-1 因 `TElement` 表面 BLOCK；P-2 PEND）。
> **本决策点未闭合前不得进入 S2+；`layout-backend` 的 `[lints]` 在批准前维持 `workspace = true`（零改动）。**

### 决策 3 —— **P-4 重定义**（在真实 `web-dom` 模型上测 opaque 机制）

P-4 判定 = 在**真实 `web-dom::NodeTree`** 上（允许前置「S6-数据模型最小面」：`NodeKind::ShadowRoot` +
`attach_shadow`，**不含 JS 面**）：
① `attach_shadow` + 影子根内作者表 `:host` / `::slotted` **经 `implicit_scope_for_sheet_in_shadow_root`
生效**（**含 G3-B1 的 opaque 机制步骤**）；② light 查询/遍历**不含**影子内容。
**P-4 失败 ⇒ 停止**，回到本 ADR 重评备选，**不静默换路**。

### 决策 4 —— 备选 D **入册**；「结构性限制」**降级**为「上游实现所限、修正不可控于本仓」

原 ADR-0011「上游结构性限制」措辞**过强**——blitz 自述其堵点为**实现选择**（用 id 而非指针），
且注释提出可向上游提 PR（「We should see if selectors will accept a PR…Or find some other solution that
will enable rehydration」，`blitz-dom/src/stylo.rs:316-325`）。故：
- 本 ADR 采纳**备选 D = 「复用 blitz-dom + 在上游 `opaque()` 侧修形」**入册（成本对比见 brief §3）；
- 默认仍走**备选 A（直连）**；备选 D 仅在 P-4 失败时重新评估。

### 决策 5 —— crate 归属与 trait 路径**更正**

- 实现的是 **`stylo`**（包名 `stylo`，import 名 `style`）的 `dom` 模块 trait：
  `style::dom::{TDocument, TNode, TShadowRoot, TElement}`（`stylo-0.20.0/dom.rs:119/153/364/409`）
  + `selectors::Element`（`selectors-0.40.0/tree.rs:46`）。
- **`stylo_dom` 仅**提供状态位（`ElementState`/`DocumentState`；`stylo_dom-0.20.0/lib.rs` 全长 196 行，
  **无任何 trait**）。
- `stylo` 需 **`servo` feature**（`stylo-0.20.0/Cargo.toml:29-30` `[features] default = ["servo"]`）
  + `build = "build.rs"` / `links = "servo_style_crate"`（同文件 `:17-18`）⇒ **prefs 初始化路径**
  是**新集成面**（见 §三 G3-B12 / P-11）。
- 门面 trait 加 **`: std::fmt::Debug`**（`BindingInputs`/`PageBinding` 均 `#[derive(Debug)]` 且要装
  `Box<dyn LayoutBackend>`；`binding.rs:47/66`）⇒ 否则编译失败（G3-B6）。

### 决策 6 —— Shadow 支撑面边界（G3）

- G3 只做：`attachShadow`（+`shadowRoot` 最小 JS 面）、作用域隔离、`:host`/`::slotted`、slot 分发、
  **布局参与**；
- **P-4 / Shadow 单测以 Rust 层为主**（直接调 stylo 匹配 API + `NodeTree` 作用域遍历）；
- **`querySelector` 不做**（登记 G4）；事件 retargeting / `composedPath` / `slotchange` **推 G4**。

## 后果

### 正面
- 「方向维持 + 机制修正」：opaque 难题**显式设计 + 探针前置**，不再是未证假设；
- crate 路径更正避免 S2 编译连败一轮；`: Debug` 一行防空指针式编译失败；
- P-4 重定义后，**测的就是宣称要绕开的东西**（判别力恢复）。

### 代价（如实登记）
- 侧表引入**线程局部状态**（生命周期须守）；若 P-1 实测必须 unsafe ⇒ **停下上报**（可能推翻路线）。
- stylo `servo` feature / build.rs / prefs 初始化为新集成面（P-11）。

### 待实测登记
| # | 待实测项 | 落点 | S1 结果 |
|---|---|---|---|
| T1 | 侧表 opaque 反查命中 + 零 unsafe | S1（P-1） | ✅ **机制零 unsafe + 反查命中**；**总判定 = 已裁（E′+R，ADR-0024 草案）——随维护者批准落档** |
| T2 | `:host`/`::slotted` 经 `implicit_scope...` 生效 | S3（P-4） | ✅ **P-4 PASS**（三判据全过含负对照，`e59e562`+`9f91a6e`）⇒ **R1 解除** |
| T3 | stylo→taffy 桥（`to_taffy_style`）样式→布局联动 | S1（P-2） | **PEND**（未跑） |
| T4 | prefs/初始化在无 GUI 环境可运行 | S1（P-11） | ✅ `P11_HEADLESS_OK=true` |
| T5 | **`TElement` 必需 `unsafe fn` 的落点**（E′ 子目录条目，ADR-0024）+ **实际 unsafe 面 = 「5 声明 + 0 腔体」（P-1b 终值）** | **已裁（E′+R，ADR-0024 草案）→ ADR-0002 修正**；**待维护者批准守卫数据行** | ⏳ 待批准 |

### 遗留（本 ADR 不做）
- 事件 retargeting / `composedPath` / `slotchange`（G4）；
- `querySelector`（G4）；
- opaque 若须 unsafe 的白名单（**停上报后**再评估）。

## 对 ADR-0011 的更正注记
1. 「上游有**一条结构性限制**」→ 降级为「**上游实现所限、修正不可控于本仓**」（决策 4）。
2. 「自建 `Element` 绕开该限制」这一**理由表述作废**，替换为「**策略自主（侧表）**」（决策 1）。
3. 「备选 = blitz」→ 具体化为「**备选 D = blitz + 上游 `opaque()` 修形**」并入册（决策 4）。
4. **翻盘触发条件**（原 ADR-0011 遗留第 2 条）保留并强化：若 `selectors` 未来把 `OpaqueElement`
   放宽到 128-bit / 提供安全 rehydrate ⇒ blitz 路线复活，本 ADR 前提翻转，须重评。

## 对施工图的更正注记
1. §1.2 全部 `stylo_dom::{TNode,Element}` → `style::dom::{TNode,TElement,TShadowRoot}` + `selectors::Element`（决策 5）。
2. §1.2 `LayoutBackend` 加 `: std::fmt::Debug`（G3-B6）。
3. §1.2 新增 **§1.2.1 opaque 侧表草案**（决策 2）。
4. §1.5 / §3.5 Shadow：**Rust 层为主**；JS 面最小 `attachShadow`/`shadowRoot`；`querySelector` 登记 G4（决策 6）。
5. §7 **P-1 强化**（含侧表机制）、**P-4 重定义**（决策 2/3）。
6. §2.4 注明 stylo `servo` feature / build.rs / prefs（决策 5）。
7. **（S1/P-1 实测补丁）** §1 头部「零 unsafe」→ 更正为「**opaque 机制零 unsafe（已验证）；`TElement` 表面需 5 个具名 `unsafe fn`**」
   ⇒ §1/§8 的 unsafe 纪律条目更正；§6 增 R17（白名单决策点）；S6 步骤前置该决策点（见更正注记）。

## 追加修正注记（2026-10-13 · R17 落点裁决 = ADR-0024；维护者 18:10 批准）

- §「更正注记（S1/P-1 实测）」的**开放处置**已由 **ADR-0024** 落定：**采用「E′+R」合成**——
  **白名单形态 = E′（尾斜杠目录条目 `crates/layout-backend/src/stylo_host/`）** + **Rust 门禁 a/b/c/d**；
  **不采纳**整 crate 条目（A）、按文件条目（E 草案）、薄桥 crate（B）。
- 待实测登记 **T5** 更新为：`TElement` 5 名 `unsafe fn` 落 **目录模块** `src/stylo_host/`；
  **实际体内 `unsafe` 面 = 0（P-1b 终值）**，门禁 b 阈值 = 0；`layout-backend` 本地 `deny`。
- **S6 门**：**已随维护者批准守卫数据行解冻**（ADR-0024 决策 2 已落地）。**P-4 已 PASS ⇒ R1 解除**。
- **实现期不变量（S7-b③，2026-10-13）**：`TElement` 句柄 `E` **必须 8 字节**（stylo 样式共享缓存
  `SharingCache<E>` vs `TypelessSharingCache` 等尺寸断言，`stylo-0.20.0/sharing/mod.rs:605`；
  `ThreadLocalStyleContext::new()` 无条件构造 ⇒ 违反 panic）⇒ **`NodeRef<'a> = &'a ElementSlot<'a>`（主理人裁 (A)，与 blitz `&'a Node` 同构）**；
  **(B)**（`usize` + 线程局部）**否决**（叠加全局态）。施工图 §1.2.1/§2.1/§6 R18。

## 交叉引用
- 锁版 → **ADR-0021**；工具链/MSRV → **ADR-0022**；排程 → **ADR-0023**
- 借用纪律 → **ADR-0016·D2**；导航清缓存 → **ADR-0018·D2**
