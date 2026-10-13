# 决策 brief —— G3 ②：是否**立即启动「直连 Stylo + Taffy + Parley」路线**（2026-10-13）

> **用途**：按 `AGENTS.md`「决策上报协议」填写。**读者没有本仓库任何上下文，也不预设懂 Rust。**
> 所有代码/命令/输出**原文粘贴**。本 brief 是**待落 ADR 的输入**；推荐**仅供参考**，等待评审辩论后裁决。

---

> ## ⚠️ 更正标注（2026-10-13 裁决后 · **不重写历史**）
>
> 本 brief 为**待落 ADR 的输入**，其主张已被评审辩论 + 主理人裁决**部分更正**。**正文保持原貌**；
> 以下为更正（裁决书 `2026-10-13-G3-裁决-主理人.md` §二·D1/§三；ADR-0020）：
> 1. **核心价值主张表述作废**：「自建 `Element` 可**绕开** blitz 的结构性限制」→ 更正为
>    「**我方与 blitz 面对同一个 opaque/rehydrate 问题**；差异在**策略自主**：`OpaqueElement` 可作键、
>    `implicit_scope_for_sheet_in_shadow_root` 为**无 self 关联函数** ⇒ **必须线程局部/全局侧表**反查（零 unsafe 为准）」。
> 2. **备选 B「结构性限制」措辞更正**：blitz 自述其堵点为**实现选择**（可向上游提 PR）⇒ 降级为
>    「**上游实现所限、修正不可控于本仓**」；**新增备选 D = 「复用 blitz-dom + 在上游 `opaque()` 侧修形」** 入册。
> 3. **备选 C（暂不做 Shadow）确认为稻草人**（不满足 §18），不作为真实备选。
> 4. **P-4 判据重定义**（原判据「打错靶」）：须在**真实 `web-dom` 模型**上、**含 opaque 机制步骤**、
>    判「作用域隔离 + `:host`/`::slotted` **经 `implicit_scope...` 生效** + 零 unsafe」。
> 5. **几何口径 P-5 降级**：删除「与 Chromium 逐属性对齐」，改「**自洽 + 具名偏差登记**」（仓内仅
>    `NullDifferentialBackend`，`real=False`）。
> 6. **crate 路径更正**：`stylo_dom::*` → `style::dom::{TNode,TElement,TShadowRoot}` + `selectors::Element`。
> 7. **✅ P-4 已实测 PASS**（`e59e562`+`9f91a6e`：作用域隔离 / `:host`+`::slotted` **命中含负对照** /
>    opaque 侧表在真实 `web-dom` 成立 + 模块**零 unsafe**）⇒ **本 brief §3·A 缺点③ 与 §6 的「P-4 若失败」
>    分支不成立；核心价值主张（策略自主/侧表）经实测成立；施工图 §6 R1 解除**。
> 8. **✅ P-2 补跑 PASS**（`128dee9`：「样式改⇒布局盒变」**非恒真 + 负对照**）⇒ **探针门 100%**（P-1 机制/P-1b/P-2/P-3/P-4/P-8/P-10/P-11 全 ✅）。
> **落地**：ADR-0020（已接受）；施工图 `design-g3-layout-2026-10-13.md`（裁决修订版 R1）。

---

## 0. 一句话问题

> **上一轮实测发现推荐的「Blitz」库不支持 Shadow DOM，于是裁决改走「直接 Stylo + Taffy + Parley」
> 这条备选路。现在要不要**立即**投入这条路？这条路要自己写「把本仓库的 DOM 树喂给样式引擎」的
> 适配层，工作量显著大于「直接用现成轮子」的直觉——是否现在启动，还是回退/等待？**

## 1. 背景与上下文

- **这个仓库是什么**：`iv8` 是**自研浏览器运行时**（Rust 宿主 + V8 + Python 控制层）。G3 闸门要
  把「页面」从「能解析、能跑脚本」推进到「**能算样式、能算布局、能读几何尺寸**」。
- **涉及模块 / 文件路径**：
  - `crates/layout-backend/`（要建设的适配器所在）
  - `crates/web-dom/`（本仓的 DOM 树：扁平 arena + generation）
  - `crates/web-style/`（当前只有内联声明值）
  - `poc/t2_4_blitz_probe/`（T2.4 探针，实证现场）
  - `docs/adr/0011-*.md`（裁决 `SWITCH_TO_STYLO`）
- **当前状态**：
  - **几何在 G2 是 stub**（`crates/navigation/src/binding.rs:633-641` 原文让 `offsetWidth` 等恒为 0）。
  - `crates/layout-backend/src/lib.rs` 仍是 T0 骨架（`//! 状态：T0 骨架占位`）。
  - T2.4 探针实测：blitz 的**动态 DOM / 样式失效成立**，但 **Shadow DOM 未实现**（见 §4 证据 A）。
- **为什么现在必须决策**：G3 的全部后续步骤（S2~S14）都依赖「适配器怎么写」。
  **路线定错 = 整轮返工**。且 HANDOFF 明确把「是否**立即启动**直连 Stylo 路线」列为**待维护者拍板**项。

## 2. 需要拍板的问题

- **主问题**：G3 是否**立即启动**「直接 Stylo + Taffy + Parley」适配（在 `layout-backend` 自建
  DOM→样式→布局→文本 的投影）？
- **子问题**：
  1. 「Blitz 不支持 Shadow」是否**足以**触发切换（还是可先不做 Shadow）？
  2. 直连路线的**核心价值主张**（自建 `Element`/`TShadowRoot` 从而**绕开** blitz 的结构性限制）
     是否成立？—— **需要一个探针先证**（本 brief 的 P-4）。
  3. 几何口径（`offsetWidth`/`clientWidth`/`getComputedStyle`）按什么语义实现？
  4. Shadow 支撑面在 G3 做到哪一步（事件 retargeting 是否推 G4）？
  5. 字体如何保证 mac/Linux **确定**？

## 3. 备选方案（至少两个）

### 方案 A：**立即启动直连 Stylo + Taffy + Parley 路线**（在 `layout-backend` 自建适配）

- **做法**：`layout-backend` 内实现
  —— ① 把 `web-dom::NodeTree` 投影成 stylo 能消费的 `Element`/`Document`（trait 回调读我们的 DOM）；
  ② 用 stylo 跑级联 + 计算值；③ 把样式树投影成 Taffy 盒树；④ 用 Parley 整形文本；
  ⑤ 读几何；⑥ `web-style` 长成 CSSOM；⑦ `web-dom` 加最小 Shadow 支撑。
- **关键代码**（原文草案 —— 投影形态，对齐 blitz 的 `type BlitzNode<'a> = &'a Node`）：
  ```rust
  // layout-backend::stylo_host
  pub struct NodeRef<'a> { tree: &'a web_dom::tree::NodeTree, id: web_dom::node::NodeId }
  // impl stylo_dom::TNode for NodeRef<'_> { … }
  // impl stylo_dom::Element for NodeRef<'_> { … }   // 我们自己的实现，不是 blitz 的桩
  // impl selectors::Element for NodeRef<'_> { … }
  ```
- **优点**：① 满足 §8「Shadow DOM 不能通过则切 Stylo」的授权；② 自建 `Element` 可**绕开** blitz 的
  `OpaqueElement` rehydrate 结构性限制（**这正是 Shadow 的堵点**）；③ 依赖可控（不引 GUI）。
- **缺点与风险**：① **自建投影工作量大**（DOM→样式→盒→文本 全链）；② stylo 的 trait 面宽、
  实现细节多；③ 若 **P-4 探针失败**（自建也做不出 Shadow），则**核心价值被证伪**。
- **长期影响**：`LayoutBackend` 对外接口不变（C14）；但**大量自建布局环节**在 G3 之后仍需补
  （§8 的 12 项：匿名盒、inline formatting、滚动、定位、transform、hit testing、表格/浮动…）。
- **预估工作量**：**大**（S2~S14，多批次）。

### 方案 B：**回退用 blitz-dom + 自建 Shadow shim**

- **做法**：仍依赖 `blitz-dom`（复用其 DOM 抽象与 `resolve`），但在其 `stylo.rs` 的 `todo!()` 处
  **用本仓的 shim 补齐 Shadow**。
- **关键代码**（原文草案 —— blitz 的堵点原文，`blitz-dom-0.3.0-beta.2/src/stylo.rs`）：
  ```rust
  fn host(&self) -> <Self::ConcreteNode as TNode>::ConcreteElement {
      todo!("Shadow roots not implemented")        // :231
  }
  fn as_shadow_root(&self) -> Option<Self::ConcreteShadowRoot> {
      // TODO: implement shadow DOM
      None                                           // :311-313
  }
  ```
- **优点**：复用 blitz 的 `resolve`（T2.4 已证动态 DOM/样式失效可用），省去 DOM→样式→盒 的投影。
- **缺点与风险**：**大概率不可行**——ADR-0011 已核实一条**上游结构性限制**（下 §4 证据 B）：
  即便补上 `todo!()`，stylo 侧的 `OpaqueElement` **无法 rehydrate**，而这是 Shadow 的**必需前提**。
  且补上游 `todo!()` 属**改第三方源码**（vendor/补丁），维护面大。
- **长期影响**：把风险押在「上游会实现」或「补丁能长期维持」。
- **预估工作量**：中（若可行）——但**可行性本身未被证明**。

### 方案 C：**暂不做 Shadow，其余照方案 A，Shadow 推后**

- **做法**：先做样式/布局/几何/字体（不含 Shadow），把 Shadow 用例推迟。
- **优点**：G3 其余可先过。
- **缺点与风险**：**违反 §18**——G3 明列「**Shadow 用例**」为必产出；且**切换 Stylo 的唯一驱动
  因素就是 Shadow**（ADR-0011），若把 Shadow 推后，「为何切换」的正当性被抽空。
- **长期影响**：G3 名不副实。
- **预估工作量**：小（但**不满足闸门**）。

## 4. 证据（原文粘贴）

**证据 A —— T2.4 探针实测结论（`poc/t2_4_blitz_probe` 运行输出原文）**

```
ASSERT_FAIL(c9): blitz-dom 0.3.0-beta.2 未实现 shadow root：as_shadow_root().is_some()=false
（impl 恒返回 None，stylo.rs:311-313）、作用域隔离不成立…
=== 二值结论汇总 ===
DYNAMIC_DOM_WORKS=true
STYLE_INVALIDATION_WORKS=true
LAYOUT_INVALIDATION_WORKS=true
SHADOW_DOM_WORKS=false
=== 裁决 ===
T2_4_VERDICT=SWITCH_TO_STYLO
```

（阅读指引：**动态 DOM / 样式失效 三行 true** 说明「复用现成轮子」在**样式与布局**上是通的；
**Shadow 一行 false** 是切换的**唯一**驱动因素。）

**证据 B —— ADR-0011 记录的上游结构性限制（`blitz-dom` 源码注释原文）**

> This correctly uses a unique id for the `OpaqueElement` … However, it makes it
> impossible for us to "rehydrate" the `OpaqueElement` back into an actual Element
> which is **required to implement the `implicit_scope_for_sheet_in_shadow_root`
> method** … This is required to enable and use the Shadow DOM functionality in Stylo.

（阅读指引：这是**方案 B 的致命点**——不是「还没实现」，而是**架构上挡住了**做 Shadow。）

**证据 C —— ADR-0011 的路线影响条款（原文）**

> 1. **`layout-backend` 的对外接口不变。**…**切路线只换 `layout-backend` 的内部实现，
>    不动任何上层 crate 的边界**。
> 2. **必须自建的能力清单（C13）**：DOM → 样式树 → box/fragment tree 的转换、匿名盒、
>    inline formatting、字体回退、滚动、绝对/固定定位、transform、hit testing、
>    table/float/writing-mode/复杂分页、SVG 几何、替换元素、表单控件。
> 3. **⚠️ 一条极易误判的事实**：**切「直连 Stylo 路线」省掉的是 blitz 的 DOM 抽象层，
>    不是样式与布局依赖**…**不以「已有轮子」掩盖成本**。

**证据 D —— G2 几何 stub 现状（`crates/navigation/src/binding.rs` 原文）**

```js
// 几何口径 G2 = stub（§2.2 / R1）：确定值 0 / 空 rect，capabilities.geometry == 'stub'。
Object.defineProperty(node, 'offsetWidth', { configurable: true, enumerable: true, value: 0 });
node.getBoundingClientRect = function () {
  return { x: 0, y: 0, width: 0, height: 0, top: 0, right: 0, bottom: 0, left: 0 };
};
```

（阅读指引：G3 就是把这段 stub 换成真值——**接口名不变**。）

## 5. 本仓库的既定约束（外部建议必须在此边界内）

- **技术方案 §8** 原文：「若动态 DOM、**Shadow DOM**、样式失效或 GC 对接不能通过 PoC，则切换为
  **直接 Stylo + Taffy + Parley** 适配；**业务接口保持不变**。该替代会增加集成工作，应视为技术闸门
  结果，**不以「已有轮子」掩盖成本**。」
- **技术方案 §18**：G3 = 「Blitz/Stylo 适配、动态失效、字体、几何和 **Shadow 用例**」。
- **ADR-0011**：裁决 `SWITCH_TO_STYLO`（已接受）。
- **ADR-0016**：结构性借用规则（`resolve` 期间回调读 DOM 只读、不跨 V8 边界）。
- **铁律 7**：**不改 `deny.toml`**；新依赖许可须在白名单内（已核实 stylo/taffy/parley 均在）。
- **铁律 3**：一事一 PR，diff ≤ 400 行 ⇒ G3 拆 S1~S14 多步。
- **unsafe 纪律**：白名单外零 `unsafe`（stylo/taffy/parley 均为安全 Rust API）。

## 6. 本代理的推荐（**仅供参考**）

- **推荐：方案 A（立即启动直连 Stylo + Taffy + Parley），但以「三段探针」为前置门（S1）。**
- **理由**：① ADR-0011 已裁决，方案 B 有已被源码级确证的结构性堵点；② 方案 C 不满足 §18；
  ③ 直连路线的**核心价值**（自建 `Element` 绕开 `OpaqueElement` 限制）**必须先用探针证明**
  —— 故推荐**先跑 S1 的三条探针**（**P-1** 投影可编译 + 级联、**P-2** `stylo_taffy` 可独立用、
  **P-4** 自有 `TShadowRoot` 作用域可判），**全绿才全面投入**。
- **配套的三条「本设计定（可辩）」口径**（请评审重点看）：
  - **几何口径**：`offsetWidth/Height` = border-box 取整；`clientWidth/Height` = padding-box
    （G3 无滚动条）；`getBoundingClientRect` = 视口坐标 border box；`getComputedStyle` = stylo **计算值**。
  - **Shadow 边界**：G3 只做「`attachShadow` + 作用域隔离 + `:host`/`::slotted` + slot 分发 + 参与布局」；
    **事件 retargeting / `composedPath` / `slotchange` 推 G4**。
  - **字体确定性**：测试与门禁**用内嵌字体**，几何断言只用内嵌字体场景（消除 mac/Linux 差异）。
- **我不确定的点**：
  1. **`::slotted`/`:host` 的作用域**在「我们自己的 `Element` 实现」上**到底能不能做出来**
     （blitz 做不到是因为它用了会失效的 `OpaqueElement` id；我们自建应当能，但**必须实测**）——**P-4**。
  2. `stylo_taffy` 是否可以脱离 `blitz-dom` 单独用（否则要自写 stylo→taffy 转换）——**P-2**。

## 7. 请重点回答的问题

1. 在 blitz 有**结构性** Shadow 堵点的前提下，「立即启动直连 Stylo」是否是**负责任**的投入？
   还是应先花更小成本证明直连能做 Shadow（探针）再投入？
2. 我给的**几何口径**（border-box 取整 / padding-box / 视口坐标 / 计算值）与浏览器标准是否一致？
   有无我没想到的歧义？
3. **Shadow 边界**（G3 只做作用域 + 分发 + 布局，事件推 G4）是否可接受？
4. 有没有**第三条路**（既不复用 blitz 的 DOM 抽象、又不从零接 stylo）是我们低估的？

---

> **回传后**：结论整理成 ADR（记录备选、讨论要点、选择理由），落档后方可动代码。
