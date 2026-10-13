# G3 闸门「布局复用验证」施工图（2026-10-13）· **裁决修订版 R1**

> **本文档是什么**：把技术方案 §18 的 **G3 六项必产出（Blitz/Stylo 适配、动态失效、字体、
> 几何、Shadow 用例）** 转成**工程师可直接按图施工**的设计。深度与写法对齐
> `docs/design-g2-page-2026-10-13.md`（§1 到函数签名级 / §2 模块设计 / §3 测试契约 /
> §5 S 编号任务表 / §6 风险 / §7 未知项 / §8 共享知识 / §9 依赖图）。
>
> **⚠️ 本版为「裁决修订版 R1」**：原稿经**两位独立评审**（`-辩论意见-A.md` 11 条 / `-B.md` 20 条）
> 背对背辩论 + **主理人对全部事实指控的原文级亲验（0 驳回）** ⇒ **方向认可、原版本不通过、须按裁决修订**。
> **效力**：与现稿冲突处，以 **ADR-0020~0023**（已落档）+ **裁决书**为准；本文档给出「修订点 → 落点」总索引。
> 修订依据：`docs/decision-briefs/2026-10-13-G3-裁决-主理人.md`。
>
> **前置 ADR（全部「已接受」）**：
> - **ADR-0011**（T2.4 裁决 `SWITCH_TO_STYLO`）+ **ADR-0020 更正注记**（理由表述更正 = **策略自主/侧表**，
>   非「自建绕开」；「结构性限制」降级为「上游实现所限、修正不可控于本仓」）。
> - **ADR-0020**（G3-D1：路线 + **opaque/rehydrate 策略**）、**ADR-0021**（G3-D2：精确锁版 + 升级流程 + 补丁通道）、
>   **ADR-0022**（G3-D3：钉 `1.92.0` + **MSRV 更正**）、**ADR-0023**（G3-D4：残留排程修正）。
> - **ADR-0016**（结构性借用规则 / arena + generation）、**ADR-0017**（执行协议 + as-built）、
>   **ADR-0018**（导航重绑）、**ADR-0019**（输入最小模型）、**ADR-0009**（V8 类型边界）、**ADR-0010**（时区）。
>
> **纪律（硬）**：`AGENTS.md` 十条铁律。本轮**只做设计与 brief，不写功能代码**
> （结论已由主理人裁决并落 ADR-0020~0023；**S1 全绿前工程师不得动** `layout-backend`/`web-style`/
> `web-dom`/`navigation`）。本文档函数签名是**设计契约**（工程师据此写测试先行）。
>
> **高风险路径声明（铁律 6）**：G3 触及 **`layout-backend` 的样式/布局投影**（新高风险集成面）
> 与 **opaque 机制**（`unsafe` 边界）⇒ 对应设计经决策上报（ADR-0020）。

---

## 0. 一句话

> **在 `layout-backend` 内建「直接 Stylo + Taffy + Parley」适配器**（**不依赖 blitz-dom**）：把
> `web-dom` 的 arena 节点树投影成 stylo 消费的 `style::dom::{TNode,TElement,TShadowRoot}`，
> 用**线程局部 opaque 侧表**（零 unsafe 为准）解 opaque/rehydrate，跑级联 + 计算值；再把样式树投影成
> Taffy 盒树做布局、Parley 整形文本（含字体回退）；`web-dom` 加**最小 Shadow 支撑面**、`web-style` 长成
> **CSSOM（内联 + 样式表 + 计算值投影最小面）**；`navigation::binding` 把 G2 几何 stub 换成**真实读数**。
> 路线由 **ADR-0020** 固定；产出 = 这条路线可行的**实证**（几何可读、失效可判、字体可确定、Shadow 作用域可判），
> 通过后进入完整尺寸与动画。**

---

## 0.1 决策与修订索引（**S1+ 以 ADR-0020~0023 + 本节为准**）

### 0.1.1 决策定稿（D1~D9）

| # | 决策点 | 定稿 | ADR | 施工图落点 |
|---|---|---|---|---|
| **D1** | 布局后端路线 + opaque 策略 | **维持 `SWITCH_TO_STYLO`**；**opaque = 线程局部侧表**（零 unsafe 为准）；**P-4 失败 ⇒ 停上报**；备选 D（blitz+上游修形）入册 | **0020** | §1.2 / §1.2.1 / §1.5 / §7 |
| **D2** | 锁版与补丁 | **精确锁版**（`=` 语义）+ `Cargo.lock` 入库 + 升级=独立 PR+全探针回归+裁决 + 补丁通道**默认休眠** | **0021** | §2.4 / §6 R2 |
| **D3** | 工具链钉版 | **钉 `1.92.0`**；**MSRV 更正**（`parley 0.11.1`=1.88、`stylo_taffy`=1.89.0/edition 2024 ⇒ `rust-version="1.85"` 已实质违背，须登记处置）；CI 矩阵收敛=**待批条**（`ci.yml` 零改动） | **0022** | §6 R5 / §8 |
| **D4** | 残留排程 | **timer / 脚本任务化 / 宿主 `try_borrow` → G4**；**模块 watchdog（U-12）→ G3 独立 P0 小步**；S12 绑定结构**拆开** | **0023** | §5 / §7 U-1~U-4 |
| **D5** | 几何口径 | `offset`=border-box 取整 / `client`=padding-box / `gBCR`=视口坐标 / `getComputedStyle`=stylo 计算值；**与 Chromium 逐属性对齐降级**（G3-B3） | 0020 | §1.3 / §3.4 |
| **D6** | Shadow 边界 | 影子根=`NodeKind::ShadowRoot` 独立节点；slot **按需计算**；**Rust 层为主 + JS 最小面**（`attachShadow`/`shadowRoot`）；`querySelector` 与事件 retargeting**推 G4** | 0020 | §1.5 / §3.5 / §5 |
| **D7** | 字体确定性 | 测试/门禁**用内嵌字体**；跨机判据由「逐字节相等」**改为容差 ≤0.5px**（G3-B10/B11） | 0020 | §1.4 / §3.6 / §7 P-10 |
| **D8** | 缓存随导航清空 | `LayoutBackend::reset()` 清 `node_slots`/几何/样式（ADR-0018·D2 延续） | 0018 | §1.2 / §3.5 / §6 R6 |
| **D9** | 范围显名登记 | 几何真值仅覆盖「块级 + 内联文本 + 简单盒模型」；flex/grid 完整、transform、滚动、table/float、SVG、替换元素、表单控件 **均登记 G4** | — | §0.2 / §6 R7 / §7 U-6 |

### 0.1.2 阻断项处置索引（G3-B1~G3-B13，两评合并去重）

| # | 问题 | 处置（裁决） | 落点 |
|---|---|---|---|
| **G3-B1** | opaque/rehydrate 机制未设计、P-1/P-4 判据不含它 | **P-1 首要判定**：施工图给**线程局部侧表草案**（键=`OpaqueElement`、锚点稳定、生命周期），**零 unsafe 为准**；探针若发现必须 unsafe ⇒ **停上报** | §1.2.1 / §7 P-1、P-4 |
| **G3-B2** | S1/S6 次序矛盾（P-4 依赖 S6 的 ShadowRoot 数据模型） | 允许前置「**S2-数据模型最小面**」（`NodeKind::ShadowRoot`+`attach_shadow`，**不含 JS 面**）作为 P-4 前置；JS 面仍按 **S10** 完整步 | §5（S2/S3/S10） |
| **G3-B3** | 「与 Chromium 对齐」不可执行（仓内仅 `NullDifferentialBackend`） | **降级**：判据 = 自洽口径 + 与规范的**具名偏差登记**；CDP 差分**不属 G3**（如需未来单列） | §1.3 / §7 P-5 / §6 R11(c) |
| **G3-B4** | `clientWidth==94` 算错（应 96） | 更正 96；全部盒口径算术逐一复核（期望值须可由 CSS 推导） | §3.2 / §3.4 |
| **G3-B5** | crate 路径错（`stylo_dom::*`） | 全改 `style::dom::{TNode,TElement,TShadowRoot}`（`style`=stylo）+ `selectors::Element`；`stylo_dom` 仅状态位；`servo` feature/build 以 P-1 实测为准 | §1.2 / §2.4 |
| **G3-B6** | `LayoutBackend` 缺 `: Debug` | 加 `: std::fmt::Debug` | §1.2 |
| **G3-B7** | UA 样式表来源未定义 | 明确来源（仓内**固定最小 UA css** + 测试），进文件清单与 S 步骤 | §1.1 / §3.1 / §5 |
| **G3-B8** | Shadow JS 面缺失 | P-4/Shadow 单测**以 Rust 层为主**；JS 面**最小补** `attachShadow`/`shadowRoot`；`querySelector` 不做（登记 G4） | §1.5 / §3.5 / §5 |
| **G3-B9** | P-2 恒真 | 强化：必须**经 stylo→taffy 桥**（`to_taffy_style`）让「样式改 ⇒ 布局盒变」可判 | §7 P-2 |
| **G3-B10** | 缺「同步改 DOM 后立即读几何」语义 | 补语义定义 + 专测（同任务内变更→读取时点规则） | §2.5 / §3.4 |
| **G3-B11** | 弱断言 `>0` | 全改**可推导期望值**；为反向注入预置 | §3.6 / §4 |
| **G3-B12** | 风险缺项 | 补 R12~R16（trait 工作量 / `InlineStyle`→stylo 映射 / `computed_value` 序列化范围 / 增量 restyle 性能 / prefs 初始化） | §6 / §7 |
| **G3-B13** | 「结构性限制」措辞过强 + 缺备选 | 见 D1（措辞更正 + 备选 D 入册） | ADR-0020 / §7 |

> **配套修订（评审 B 的 P1/P2 项，与本批裁决一致）**：① **变更→脏级映射表**（§2.2，使
> `mark_*_dirty` 有驱动源）；② **restyle root** 定义（§2.2，防「按需」名实不符）；③ **S 步骤再拆**
> （§5，S4 原合并项拆为 S6/S7/S8/S9；S5 拆 box_tree/text；S7 拆作用域/slot）以符铁律 3「≤400 行/步」；
> ④ `attach_shadow` **mode 语义定死**（§1.2）；⑤ `ElementData.shadow` clone 注记（§1.2）；
> ⑥ `stylo_taffy` MSRV/edition 已亲验（§6 R5 / ADR-0022）；⑦ 类图与 §1.2 字段对齐（`docs/class-diagram-g3.mermaid`）。

### 0.1.3 **S1 实测回填**（2026-10-13 · `poc/t3_g3_probe` + `.workbuddy/refs/g3-probe-evidence/`）

> **S1 判定汇总（探针原文）**：`P-8=PASS`、`P-11=PASS`、`P-1(opaque 机制)=PASS`、
> **`P-1(总:含投影编译)=BLOCK`**、`P-2=PEND` ⇒ **`G3_S1_VERDICT=BLOCK`**（停上报，符合 ADR-0020 决策 2 纪律）。

| 项 | 实测结果 | 回填落点 |
|---|---|---|
| **P-8 锁版整组** | ✅ 版本全匹配；**但 `taffy` 须 `default-features=false`**（否则 E0063，feature 统一） | §2.4 / ADR-0021 更正注记 / R2 |
| **P-11 prefs 无 GUI** | ✅ `P11_HEADLESS_OK=true`（`GLOBAL_STYLE_DATA_INIT=true`） | §2.4 / §7 P-11 |
| **P-1 opaque 机制** | ✅ **零 unsafe + 反查命中**（`OPAQUE_ADDRESS_STABLE=true`、`IMPLICIT_SCOPE_REVERSE_LOOKUP=Some(100)`） | §1.2.1 |
| **P-1 总（投影编译）** | ⛔ **BLOCK**：`TElement` 5 个必需 `unsafe fn` vs `forbid(unsafe_code)` | §1 头 / §6 R17 / §5 S6 / ADR-0020 更正注记 |
| **P-2 stylo→taffy 桥** | PEND（未跑；S1 已 BLOCK，S3 未开） | §7 P-2 |

> **⇒ 新增阻塞决策点（记 **R17**；编号避与评审 B 的 B14~B20 冲突）：`TElement` 的 unsafe 落点 ⇒
> **白名单方案 A（整 `layout-backend`）/ B（极薄桥 crate）/ E′（子目录条目）**
> ⇒ 决策 brief `docs/decision-briefs/2026-10-13-G3-unsafe-whitelist-layout-backend.md`（**R17 已裁 = 「E′+R」，见 ADR-0024 草案**）。
>
> **跟进（2026-10-13）**：R17 裁决 = **E′+R**（白名单形态 = 尾斜杠目录 `crates/layout-backend/src/stylo_host/`
> + Rust 门禁 a/b/c/d）；**待维护者批准守卫数据行**。**P-1b 已定形：signatures:5 / bodies:0 / 腔体:0**
> （commit `407c0aa`；零 unsafe 出路成立）⇒ **门禁 b 阈值定格 0**；**S6 门仅待批准**。
> 草案包：`docs/decision-briefs/2026-10-13-G3-R17-落档草案包.md`（**未落地**）。
>
> **S2/S4 进度回填（2026-10-13）**：**S2**（web-dom Shadow 数据模型最小面）已完成（`59a30d8`）；
> **S4**（layout-backend 骨架）已完成（`14e019d`+`a5246a4`；`cargo test -p layout-backend` 6/6）。
> **S4 两处偏离（已裁定受理，登记）**：① **`web_style::SheetOrigin` 于 S4 提前落位**（门面签名 `add_stylesheet`
> 编译所需；`sheet.rs` 其余面由 S5 续扩）；② **`LayoutErrorKind` 由 8 → 9 变体**（新增 `UnsupportedProperty`，
> 供 §3.4 `computed_value_unsupported_property_errors` 契约）——类图错误枚举同步 9 项。
>
> **S5/S6 进度回填（2026-10-13）**：**S5**（web-style `Stylesheet` / `ComputedStyle` + remove 语义）已完成（`473514a`）；
> **S6 ✅ 完成**（**四态注入首验通过**，证据 `.workbuddy/refs/g3-s6-whitelist-evidence/`）。
> **S6 偏离（已裁定受理，具名登记；与 S2 447 行同类）**：**S6-1 单提交 572 行**（`stylo_host/mod.rs` **433 > 400**）
> + **S6-1c 556 行**（`dom.rs` **484 行为最小可编译单元**）—— **单一 API 面不可分割**，故不拆。
> **S6-G（geiger）顺延 S14**：`cargo geiger` ≥0.13 需 rustc ≥1.94~1.97 > 钉版 1.92 ⇒ 仅**远程 S14** 跑，
> 属**基线观测（非门禁）**。**S7-a ✅**（`a885eb6`，`invalidate.rs` 两级失效簿记 + §2.2 映射；layout-backend **14/14**、workspace **185**）。
> **S7-b① ✅**（`0b4d1d4`，`font_metrics.rs`；layout-backend **16/16**）+ **S7-b② ✅**（`6416c03`，`style_system` 最小面：Device/Stylist/表登记去重/remove/flush；**17/17**）；**S7-b③（逐元素级联）在途**。
> **S7-b 子组件 `font_metrics.rs`**（stylo `Device` 所需 **`Box<dyn FontMetricsProvider>`** 的**最小实现**；
> 复用 P-3/P-10 的 `fontique`/`skrifa` 集合 + **内嵌回退字体 Tuffy**；**与 S9 文本整形无重复**）——已登记 §1.1/§1.2/§2.1。
>
> **探针门总账（2026-10-13）—— 100% 收口**：**全 8 项 ✅** —— **P-1 机制 ✅**（零 unsafe+反查命中；**总判随落档**，= E′+R / ADR-0024）、
> **P-1b ✅**（信封 = 5 声明 + 0 腔体，终值）、**P-2 ✅**（「样式改⇒布局盒变」**非恒真 + 负对照**，`128dee9`）、
> **P-3 ✅**、**P-4 ✅**（三判据全过，含负对照 ⇒ **R1 解除**）、**P-8 ✅**、**P-10 ✅**、**P-11 ✅**。
> **S1/S3 探针组已收口**；S2 ✅ / S4 ✅。
>
> **R17 落档（维护者 2026-10-13 18:10 批准）**：**ADR-0024（E′+R）已落**；**守卫数据行已落地**
> （`scripts/check-unsafe-whitelist.sh` 的 `WHITELIST += "crates/layout-backend/src/stylo_host/"`，**零逻辑**）；
> **ADR-0002 / ADR-0020 修正注记已落**；**S6 门已解**。维护者**预授权模式**（G6 总目标）：此后受保护面/决策项
> **不再逐项请示**，但**记录纪律不变**（全量 ADR/台账/登记）。

---

## 0.2 目标 / 范围 / 非目标

### 目标（G3 通过判据的正面集合）

1. **样式复用成立**：投影 `web-dom::NodeTree` → `style::dom::{TNode,TElement,TShadowRoot}`，
   完成 **UA + author 级联 + 计算值**；`getComputedStyle` 返回**真实计算值**。
2. **布局复用成立**：样式树 → Taffy 盒树 → 布局；`offsetWidth/Height`、`getBoundingClientRect`
   返回**与已布局盒一致的真实值**（块级 + 内联文本最小集）。
3. **字体链成立**：`fontique` 发现字体 + Parley 整形；**测试用内嵌字体**（跨机**容差 ≤0.5px**）。
4. **动态失效成立**：DOM 变更 ⇒ **style 级** *与* **layout 级** 两级**均可观测**（对齐 T2.4 双读数范式）。
5. **Shadow 用例成立**（最小面）：`attachShadow`、`<slot>` 分发、**作用域隔离**（light 查询/遍历不含影子内容）、
   `:host`/`::slotted` 生效、shadow 节点**参与布局**。
6. **几何 API stub → 真实**：`capabilities.geometry` 由 `"stub"` 升为 **`"computed"`**（D5 定名；契约钉死）。

### 范围（本轮纳入）

- **`layout-backend`**：stylo/taffy/parley 适配器（**新实现面**）+ opaque 侧表 + 几何 API + 失效 API + **UA css 资源**。
- **`web-style`**：CSSOM **计算值投影最小面**（内联 + 样式表文本登记 + `ComputedStyle`）；**非** CSSOM 广度扩张。
- **`web-dom`**：Shadow **最小**数据结构（`attachShadow`/影子根/`slot`/`assigned_nodes`）+ **最小 JS 面**。
- **`navigation`**：`binding` 几何真值接线 + 同步读语义；`page` 持 `LayoutBackend` + `relayout_if_dirty` + 导航 `reset`；**模块 watchdog（S14）**。
- **`sdk-python` / `contracts`**：几何能力面口径更新。

### 非目标（**显式不做 / 登记 G4**）

- **动画**（§18：「通过后…开展完整尺寸与动画」）。
- **完整几何与变换**：transform、`scrollWidth/Height` 完整、`offsetTop/Left/offsetParent` 完整、滚动、
  绝对/固定定位完整、**命中测试**、table/float/writing-mode、复杂分页、SVG 几何、替换元素、表单控件。
- **事件 retargeting / `composedPath` / `slotchange` / `querySelector` / Custom Elements**（G4）。
- **执行模型收口**：**脚本任务化 / timer 事件循环 / 宿主 `try_borrow` 残余**（**ADR-0023 ⇒ G4**；
  G3 只做**模块 watchdog**）。
- **CDP / 真实 Chromium 差分**（技术方案 T1.3 未完；G3 判据降级为自洽 + 具名偏差，G3-B3）。
- **不改** `.github/workflows/**`、`CODEOWNERS`、`deny.toml`、`AGENTS.md`、golden（铁律 7）。

---

## Part A：系统设计

### 1. 文件级设计

> **crate 归属**：**`layout-backend` 首次落地**（此前为 T0 骨架）；给 `web-style`/`web-dom`/`navigation`/
> `sdk-python`/`contracts` 各加**增量**。**不新增 workspace 成员**（ADR-0009）。
> **依赖方向**：`layout-backend → { web-dom, web-style }`（**不再依赖 `policy`**——原稿此项无用途，**删除**）
> + 外部 `style`(=stylo)/`stylo_dom`/`stylo_traits`/`stylo_taffy`/`taffy`/`parley`/`selectors`/`skrifa`/`fontique`；
> `navigation → layout-backend`（**新边**）；**无环**；`web-style` **不依赖** `web-dom`（保持 `web-dom → web-style` 单向）。
> **unsafe 纪律**：`engine-v8` 为**既有唯一** unsafe 白名单 crate。**S1/P-1 实测更正（见 §0.1.3）**：
> **opaque 侧表机制已证零 unsafe**（不 deref 指针）；**但 `style::dom::TElement` 强制 5 个必需 `unsafe fn`
> ⇒ 承载 `TElement` 实现的 crate 不可能零 unsafe** ⇒ 须**把该 crate 加入白名单**（方案 A/B，
> **决策上报中**：`docs/decision-briefs/2026-10-13-G3-unsafe-whitelist-layout-backend.md`）。
> **该决策未闭合前，`layout-backend` 的 `[lints]`/守卫脚本零改动，S2+ 不得开工。**

#### 1.1 文件清单（新增/修改）

| crate | 文件 | 职责（一句话） | 关键类型 | unsafe |
|---|---|---|---|---|
| **layout-backend** | `Cargo.toml`（改） | 依赖 **精确锁版**（`style`/`stylo_dom`/`stylo_traits`/`stylo_taffy`/`taffy`/`parley`/`selectors`/`skrifa`/`fontique`，ADR-0021）。**⚠️ `taffy` 须 `default-features = false` + 兼容特性集（以实测为准，S1/P-8）** | — | 零（`TElement` 5 `unsafe fn` 待白名单裁决，R17） |
| | `assets/ua.css`（**新**） | **内嵌最小 UA 样式表**（`html/body/div/p/h1/ul/li/span` 的 `display`/`margin` 等），`include_str!` 加载（G3-B7） | — | — |
| | `src/lib.rs`（改） | 模块接线 + 文档 | — | 零 |
| | `src/backend.rs`（新） | **门面 trait**（`LayoutBackend` **`: Debug`**）+ 工厂 + 错误 | `LayoutBackend` / `LayoutError` / `LayoutErrorKind`（**9 变体**，含 `UnsupportedProperty`） | 零 |
| | `src/geometry.rs`（新） | 几何查询类型 + 坐标口径 | `Size` / `Rect` / `NodeGeometry` | 零 |
| | `src/opaque.rs`（新） | **线程局部 opaque 侧表**（§1.2.1；零 unsafe） | `OpaqueSideTable` / `NodeAnchor` / `SIDE` | 零（待 P-1） |
| | `src/stylo_host/mod.rs`（新，**目录模块**） | **核心**：`impl style::dom::{TDocument,TNode,TElement,TShadowRoot}` + `selectors::Element` over `web-dom`；**unsafe 白名单桥（ADR-0024）** | `StyloHost<'a>` / `NodeRef<'a>` | 5 `unsafe fn`（**桥内具名；ADR-0024**） |
| | `src/style_system.rs`（新） | `Stylist` 持有 + 样式表加载 + 计算值查询 | `StyleSystem` / `StyleSheetHandle` | 零 |
| | `src/serialize.rs`（新） | **`computed_value` 序列化**：**属性白名单**（G3-B12③）→ stylo 值字符串 | `PropertyWhitelist` | 零 |
| | `src/font_metrics.rs`（新，**S7-b 子组件**） | **最小 `FontMetricsProvider`**（stylo `Device::new` 所需；复用 P-3/P-10 的 fontique/skrifa 集合 + **内嵌回退字体**；确定性口径 = 测试内嵌 / 真实页系统发现+内嵌回退，对齐 D7） | `FontMetricsProvider` | 零 |
| | `src/box_tree.rs`（新） | 样式树 → Taffy 盒树（块级 + 内联文本；匿名盒） | `BoxTree` / `BoxId` | 零 |
| | `src/text.rs`（新） | Parley 整形桥 + 字体上下文（`fontique`）+ 内嵌字体注册 | `TextShaper` / `FontChain` | 零 |
| | `src/shadow.rs`（新） | **最小 Shadow**：stylo `TShadowRoot` 作用域 + slot 分发 | `ShadowSupport` | 零（待 P-4） |
| | `src/invalidate.rs`（新） | 失效簿记（style/layout 两级脏集 + **变更→脏级映射** + restyle root） | `Invalidation` / `DirtySet` | 零 |
| **web-style** | `src/lib.rs`（改） | 接线 + 导出 | — | 零 |
| | `src/sheet.rs`（新） | 样式表文本登记（origin：UA/author；media 占位） | `Stylesheet` / `SheetOrigin` | 零 |
| | `src/computed.rs`（新） | **计算值投影**（属性名 → 值字符串） | `ComputedStyle` | 零 |
| | `src/error.rs`（改） | 新增样式表/计算值错误 | `StyleErrorKind` | 零 |
| **web-dom** | `src/node.rs`（改） | 新增 `NodeKind::ShadowRoot`；`ElementData` 增 `shadow`（形态见 §1.2；**clone 注记**） | `NodeKind` / `ElementData` | 零 |
| | `src/tree.rs`（改） | `attach_shadow`（**mode 语义定死**）/`shadow_root`/`assigned_nodes`/`flattened_children` | `NodeTree` | 零 |
| | `src/document.rs`（改） | Document 级 shadow 查询面 | `Document` | 零 |
| **navigation** | `Cargo.toml`（改） | 新增 path 依赖 `layout-backend` | — | 零 |
| | `src/binding.rs`（改） | **几何 stub → 真实**：`offsetWidth/Height`、`clientWidth/Height`、`getBoundingClientRect`、`getComputedStyle`；**同步读语义**（§2.5）；`attachShadow`/`shadowRoot` 最小 JS 面 | `BindingInputs`（+`layout` 句柄） | 零 |
| | `src/page.rs`（改） | `BrowsingContext` 持 `LayoutBackend`；`relayout_if_dirty`；导航 `reset`；`Capabilities` 更新 | `BrowsingContext` / `Capabilities` | 零 |
| | `src/parser.rs`（改） | **模块 eval 接 watchdog（U-12，S14）** | `ParserHost` | 零 |
| **sdk-python** | `src/page.rs`（改） | 几何能力面 + 可选 `getBoundingClientRect` 暴露 | `PyPage` | 零 |
| **contracts** | `iv8_contract/page_control.py`（改） | `geometry` 值 `"stub"` → `"computed"`；`shadow_dom` `false` → `true` | — | — |
| | `tests/test_page_control.py`（改） | 同步断言（**test 名 `capabilities_reports_g3_surface`**；原 `page_geometry_is_documented_stub` **不存在**，G3-B16 更名） | — | — |
| **rust-toolchain.toml**（改） | — | `channel = "1.92.0"`（ADR-0022；CI 矩阵收敛=待批条，`ci.yml` 零改动） | — | — |
| **deny.toml** | **不改**（铁律 7）。新传递依赖若引入白名单外许可 ⇒ **上报** | — | — | — |

> **命名纪律（跨平台铁律 1）**：文件名全 **小写 + 下划线**；`mod` 名逐字符一致；**不用 `#[path]`**。
> **禁止** `#[pyclass]` 用 `Probe` 作名（ADR-0014 遗留坑）。

#### 1.2 关键类型与函数签名（设计契约，原文）

> 术语（给不懂 Rust 的维护者）：`&NodeTree` ≈「对节点树的只读借用」；`trait` ≈「一组约定的行为」；
> stylo 通过 **trait**（`TDocument`/`TNode`/`TElement`/`TShadowRoot`）反向**回调**我们的 DOM。

##### `layout-backend::geometry`

```rust
/// 盒尺寸（CSS px）。G3 语义：`border_box` 含 border，`content_size` 为内容盒。
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct Size { pub width: f32, pub height: f32 }

/// 矩形（CSS px）。`getBoundingClientRect` 语义：视口坐标、border box。
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct Rect { pub x: f32, pub y: f32, pub width: f32, pub height: f32 }

/// 一个节点的可读几何（G3 最小面）。
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct NodeGeometry {
    pub border_box: Rect,   // getBoundingClientRect：视口坐标 border box
    pub offset: Size,       // offsetWidth/Height：border box 取整
    pub client: Size,       // clientWidth/Height：padding box（G3 无滚动条）
}
```

##### `layout-backend::backend`（后端门面 trait —— **`: Debug`**；**接口不随后端变化**，ADR-0011·C14）

```rust
/// 布局后端门面。**上层只依赖本 trait**（§8「业务接口保持不变」）。
/// `: std::fmt::Debug` 是**必需**（G3-B6）：`BindingInputs`/`PageBinding` 均 `#[derive(Debug)]`
/// 且要装 `Box<dyn LayoutBackend>`（`navigation/src/binding.rs:47/66`）。
pub trait LayoutBackend: std::fmt::Debug {
    fn set_viewport(&mut self, viewport: Size);
    fn add_stylesheet(&mut self, origin: web_style::SheetOrigin, css: &str)
        -> Result<StyleSheetHandle, LayoutError>;
    /// 移除一张样式表（`add_stylesheet` 对称面；G3-B11）。
    fn remove_stylesheet(&mut self, handle: StyleSheetHandle) -> Result<(), LayoutError>;
    /// **重算**：级联 + 计算值 + 布局（对齐 blitz `resolve` 语义）。
    fn resolve(&mut self, tree: &web_dom::tree::NodeTree, now: f64) -> Result<(), LayoutError>;
    fn geometry(&self, node: web_dom::node::NodeId) -> Result<NodeGeometry, LayoutError>;
    /// 读计算值（`getComputedStyle` 语义；**属性白名单**见 `serialize.rs`，G3-B12③）。
    fn computed_value(&self, node: web_dom::node::NodeId, prop: &str)
        -> Result<String, LayoutError>;
    fn mark_style_dirty(&mut self, node: web_dom::node::NodeId);
    fn mark_layout_dirty(&mut self, node: web_dom::node::NodeId);
    #[must_use] fn is_dirty(&self) -> DirtyState;
    /// **导航重绑**：清空 opaque 侧表 + 几何/样式缓存（ADR-0018·D2 + D8）。
    fn reset(&mut self);
}

/// 两级失效状态（对齐 T2.4 的 style/layout 双层读数）。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct DirtyState { pub style: bool, pub layout: bool }
```

```rust
/// 布局层错误类别（**9 变体**）。
/// ⚠️ S4 已落：`UnsupportedProperty`（供 §3.4 `computed_value` 白名单外属性契约；G3-B12③）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LayoutErrorKind {
    StyleSheet,
    Cascade,
    Stylo,
    Taffy,
    Font,
    UnknownNode,
    NotLaidOut,
    UnsupportedProperty,   // S4 补：computed_value 白名单外属性（§1.3 / §3.4）
    Internal,
}
```

##### `layout-backend::backend::StyloBackend`

```rust
/// 直接 Stylo + Taffy + Parley 适配器。
#[derive(Debug)]   // 门面 trait 要求
pub struct StyloBackend {
    style: StyleSystem,
    boxes: BoxTree,
    text: TextShaper,
    shadow: ShadowSupport,       // 与类图对齐（G3-B18）
    invalidate: Invalidation,    // 与类图对齐（G3-B18）
    viewport: Size,
    /// **NodeId(raw u64) → 后端槽**。随 `reset()` 清空（ADR-0018·D2）。
    node_slots: std::collections::HashMap<u64, SlotRef>,
}
impl StyloBackend { #[must_use] pub fn new(viewport: Size) -> Self; }
impl LayoutBackend for StyloBackend { /* … */ }
// 注：`SlotRef` 等内部字段类型均须 `Debug`（或手写 `impl Debug`）以满足 `#[derive(Debug)]`。
```

##### 1.2.1 **opaque/rehydrate 线程局部侧表**（G3-B1；**零 unsafe 为准**）

> **为什么必须设计它**：`selectors::Element::opaque(&self) -> OpaqueElement` 是**必需方法、无默认实现**
> （`selectors-0.40.0/tree.rs:50`）；`OpaqueElement(NonNull<()>)`（同文件 `:17`）**唯一反解**是
> `unsafe fn as_const_ptr`（`:41`）；而 Shadow 作用域经**无 `self` 的关联函数**
> `E::implicit_scope_for_sheet_in_shadow_root(opaque_host, sheet_index)`（`stylo-0.20.0/dom.rs:976`；
> 调用点 `stylist.rs:3150`）反查宿主 ⇒ 必须能从 `OpaqueElement` 回到节点。
> blitz 因以 `NodeId` 作 opaque、需 `Slab` 反查而 `todo!()`（`blitz-dom/src/stylo.rs:577` 注释）。

```rust
/// 线程局部「opaque 侧表」——把 `OpaqueElement` 当作**不透明键**，**从不反解指针**。
#[derive(Default)]
pub struct OpaqueSideTable {
    /// 每节点一个**稳定地址**锚点（`Box` ⇒ 地址不随 `NodeTree.nodes` 增长而变）。
    anchors:   std::collections::HashMap<web_dom::node::NodeId, Box<NodeAnchor>>,
    /// `OpaqueElement` → `NodeId`（关联函数据此反查）。
    by_opaque: std::collections::HashMap<selectors::OpaqueElement, web_dom::node::NodeId>,
    /// 当前 `resolve` 的树身份（关联函数**无 `self`** ⇒ 由线程局部提供）。
    active_tree: std::cell::Cell<Option<TreeId>>,
}
/// 锚点：稳定地址载体（`OpaqueElement::new(&*anchor)` 的取址对象）。
struct NodeAnchor { tree: TreeId, id: web_dom::node::NodeId }

impl OpaqueSideTable {
    /// `Element::opaque()`：取/建锚点，返回其**稳定地址**的 `OpaqueElement`（键稳定）。
    pub fn opaque_for(&mut self, id: web_dom::node::NodeId) -> selectors::OpaqueElement;
    /// 关联函数反查：`OpaqueElement` → `NodeId`（**纯查表，不 deref 指针 ⇒ 零 unsafe**）。
    #[must_use] pub fn node_of(&self, opaque: selectors::OpaqueElement) -> Option<web_dom::node::NodeId>;
}
thread_local! { static SIDE: std::cell::RefCell<OpaqueSideTable> = std::cell::RefCell::new(OpaqueSideTable::default()); }
```

- **零 unsafe 论证**：`OpaqueElement::new(&*anchor)` 为**安全**函数（取 `&T`）；我方**从不**调用
  `unsafe fn as_const_ptr`，只把 `OpaqueElement` 当 `Eq + Hash` **键**用 ⇒ **无 unsafe 面**。
- **锚点稳定**：锚点为 `Box<NodeAnchor>`（堆分配地址稳定，规避 `Vec<Option<NodeData>>` 在 append 时 realloc）。
- **生命周期**：侧表在**每次 `resolve` 起清空/复用**；`reset()`（导航）时清空。
- **穷尽纪律**：`implicit_scope_for_sheet_in_shadow_root` 经 `SIDE` 的 `active_tree` + `by_opaque` 反查；
  **P-1 首要判定**含此机制（建 ≥2 节点，走 `opaque()`→关联函数反查路径）。
- **⚠️ 执行纪律 + S1 实测**：**P-1 若发现必须 unsafe ⇒ 停下上报**（不开白名单、不静默）。
  **S1 实测**：**本机制本身已证零 unsafe 且反查命中**（`OPAQUE_ADDRESS_STABLE=true`、
  `IMPLICIT_SCOPE_REVERSE_LOOKUP=Some(100)`）；**阻断点不在本机制**，而在上游 `TElement` 表面
  （5 个必需 `unsafe fn`，见 §0.1.3 / §6 R17）——**二者须分清**。**opaque 机制零 unsafe** 指**机制本身**（不含 `TElement` 表面的 5 个 `unsafe fn`）。

##### `layout-backend::stylo_host`（**核心**：实现 stylo traits）

```rust
/// stylo 宿主视图：**只读借用** DOM + 提供 stylist 与映射。
/// 形态对齐 `blitz-dom::stylo.rs`（`type BlitzNode<'a> = &'a Node`）：本仓取 `NodeRef<'a>{tree,id}`。
/// **trait 路径（G3-B5 更正）**：`style::dom::{TDocument,TNode,TElement,TShadowRoot}`
/// （`style` = 包名 `stylo`；`stylo_dom` 仅状态位）+ `selectors::Element`。
pub struct StyloHost<'a> { tree: &'a web_dom::tree::NodeTree, style: &'a StyleSystem }

#[derive(Clone, Copy, Debug)]
pub struct NodeRef<'a> { tree: &'a web_dom::tree::NodeTree, id: web_dom::node::NodeId }
// impl style::dom::TNode    for NodeRef<'_> { … }   // TNode: Copy+Clone+Debug+NodeInfo+PartialEq
// impl style::dom::TElement  for NodeRef<'_> { … }   // 含 selectors::Element（opaque() 走 §1.2.1 侧表）
// impl selectors::Element    for NodeRef<'_> { fn opaque(&self) -> OpaqueElement { SIDE.opaque_for(self.id) } … }
// impl style::dom::TShadowRoot for ShadowRootRef<'_> { … }  // §1.5
```

> **⚠️ 与 ADR-0016·D2 的接口**：`resolve` 期间 stylo 回调读 DOM **只读**（`&NodeTree`），
> **不跨任何 V8 回调边界**；`resolve` **必须在释放 `NodeTree` 可变借用之后调用**（技术方案 §7 docx:189）。

##### `layout-backend::serialize` / `web-style` 增量

```rust
/// 计算值序列化：**属性白名单**（G3-B12③：`computed_value` 只支持白名单属性，其余 `Err(UnsupportedProperty)`）。
pub struct PropertyWhitelist;   // 如 width/height/color/display/font-size/…（S7 定表）

pub struct Stylesheet { pub origin: web_style::SheetOrigin, pub text: String }
#[derive(Clone, Copy, PartialEq, Eq, Debug)] pub enum SheetOrigin { UserAgent, Author }
// ⚠️ S4 偏离（已裁定受理）：`SheetOrigin` 于 **S4 提前落位**（`layout-backend` 门面签名
//    `add_stylesheet(origin: web_style::SheetOrigin, …)` 编译所需）；`web-style/src/sheet.rs`
//    其余面（`Stylesheet` 续扩）按 **S5** 完成。
pub struct ComputedStyle { entries: Vec<(String, String)> }   // 见 web-style::computed
```

##### `layout-backend::font_metrics`（**S7-b 子组件**）

> **为什么需要**：级联需 `style::device::servo::Device::new(...)`，其参数含 **`Box<dyn FontMetricsProvider>`**
> （stylo **无内置实现**）⇒ 本仓提供**最小**实现：复用 **P-3/P-10 的 `fontique`/`skrifa` 集合** + **内嵌回退字体**。

```rust
/// 最小字体度量提供者（stylo `Device` 的 `FontMetricsProvider` 实现）。
/// **确定性口径（对齐 D7）**：测试/门禁走**内嵌字体**；真实页走 `fontique` 系统发现 + **内嵌回退**。
pub struct FontMetricsProvider { fonts: fontique::Collection /* + 内嵌回退字体 */ }
// impl style::device::servo::FontMetricsProvider for FontMetricsProvider { … }
```
> **与 S9 的边界**：本组件只提供 **Device 度量**（stylo 级联所需）；**文本整形**（Parley 逐字 advance/换行）
> 另属 **S9**（`text.rs`）——**二者无重复**。

##### `web-dom` 增量（最小 Shadow 支撑）

```rust
/// 影子根：`NodeKind::ShadowRoot`（**不是** Element）。
impl NodeTree {
    /// 给 `host`（Element）挂影子根。**mode 语义定死（G3-B13）**：
    /// - 已存在且 **mode 相同** ⇒ 幂等返回既有根；
    /// - 已存在且 **mode 不同** ⇒ `Err(DomError::NotSupported)`（对齐规范 `NotSupportedError`）。
    pub fn attach_shadow(&mut self, host: NodeId, mode: ShadowMode) -> Result<NodeId, DomError>;
    /// 影子根（Rust 面返回 mode，由 JS 层按 open 过滤 —— G3-B14）。
    #[must_use] pub fn shadow_root(&self, host: NodeId) -> Option<(NodeId, ShadowMode)>;
    /// `slot` 的被分配节点（默认 + 具名简单匹配；**按需计算，无持久表**）。
    #[must_use] pub fn assigned_nodes(&self, slot: NodeId) -> Vec<NodeId>;
    /// **composed/flattened 遍历**（跨 shadow 边界）供布局使用。
    #[must_use] pub fn flattened_children(&self, node: NodeId) -> Vec<NodeId>;
}
#[derive(Clone, Copy, PartialEq, Eq, Debug)] pub enum ShadowMode { Open, Closed }
```

> **clone 注记（G3-B15）**：`ElementData` `#[derive(Clone)]` ⇒ `shadow: Option<NodeId>` 随 host 克隆会
> 出现两指针指向同一影子根。**约定：host 不克隆**，或 clone 时置 `None`（S2 落定，`web-dom` 注释显名）。

##### `navigation::binding` 增量（几何 stub → 真实 + 同步读语义）

```rust
pub struct BindingInputs {
    // …既有字段…
    pub layout: Rc<RefCell<Box<dyn LayoutBackend>>>,   // 几何 + 计算值 + 同步读 flush
}
```

JS shim 几何段**替换**（原 stub 见 `binding.rs:633-641`）：

```js
Object.defineProperty(node, 'offsetWidth', { configurable:true, enumerable:true,
  get: function () { return nGeometry(this, 'offsetWidth'); } });
// …offsetHeight/clientWidth/clientHeight 同理…
node.getBoundingClientRect = function () {
  var r = nGeometryRect(this);
  return { x:r.x, y:r.y, width:r.width, height:r.height,
           top:r.y, left:r.x, right:r.x+r.width, bottom:r.y+r.height };
};
g.getComputedStyle = function (el, _pseudo) { return nComputed(el); };
// G3-B8：Shadow 最小 JS 面
node.attachShadow = function (init) { return nAttachShadow(this, String((init&&init.mode)||'open')); };
Object.defineProperty(node, 'shadowRoot', { configurable:true, enumerable:true,
  get: function () { return nShadowRoot(this); } });   // 仅 open 返回，closed 返回 null
// 注：querySelector / 事件 retargeting / composedPath / slotchange —— **不做（G4）**。
```

##### `navigation::page` 增量

```rust
pub struct BrowsingContext {
    // …既有字段…
    pub layout: Rc<RefCell<Box<dyn LayoutBackend>>>,   // 随导航 reset()（ADR-0018·D2）
}
impl BrowsingContext {
    /// DOM 变更后按需重算；**同时是「同步读几何」的 flush 入口**（§2.5）。
    fn relayout_if_dirty(&mut self) -> Result<(), PageError>;
}
```

#### 1.3 几何 API：stub → 真实的**最小面**（D5；**判据降级见 G3-B3**）

| API | G2（stub） | G3（真实） | 口径 |
|---|---|---|---|
| `offsetWidth`/`offsetHeight` | `0` | **border box** 宽/高，**四舍五入到整数** | CSSOM View；非 rendered ⇒ `0` |
| `clientWidth`/`clientHeight` | `0` | **padding box**（不含 border；G3 无滚动条 ⇒ 不扣） | 见 §3.2 算式 |
| `getBoundingClientRect()` | 全 0 | **视口坐标 border box**（G3 视口原点 `(0,0)`，无滚动） | 需视口原点 |
| `getComputedStyle(el).getPropertyValue(prop)` | （G2 无） | **stylo 计算值**（**属性白名单**，G3-B12③）；**白名单外属性 ⇒ `Err(LayoutErrorKind::UnsupportedProperty)`（S4 已落变体）** | `color` 等序列化以 stylo 为准（`::slotted{color:red}` ⇒ 期望串按 stylo 序列化定死，G3-B12/B12） |
| `scrollWidth/Height`、`offsetTop/Left`、`offsetParent` | 无 | **不做（G4）**；JS 访问 ⇒ 确定值 + 登记 | §0.2 |

> **判定力提示（G3-B3/G3-B11）**：几何断言用**布局输入可推导的期望值**（`width:100px` ⇒ `offsetWidth==100`），
> **不得「读数 != 0」**。**「与真实 Chromium 逐属性对齐」降级为「自洽口径 + 具名偏差登记」**（仓内仅
> `NullDifferentialBackend`，`real=False`，不接触浏览器 —— `contracts/iv8_contract/differential.py:128/132`）；
> CDP 差分**不属 G3**。

#### 1.4 字体链（D7；**跨机判据 = 容差 ≤0.5px**）

- 栈：`parley`（整形）← `fontique`（发现/回退）← `skrifa`（字形）。
- 加载：`TextShaper` 持 `fontique::Collection`；启动扫描系统字体。
- 回退：`fontique` 家族回退；未命中 ⇒ **内嵌回退字体**。
- **确定性**：测试/门禁**用内嵌字体**；`offsetWidth` 断言只用内嵌字体场景。**跨机判据 = 容差 ≤0.5px**
  （本地 arm64 vs 远程 x86_64 的 `f32` 不保证逐位相等，G3-B10）。
- **G3 口径（D7 后续注）**：**G3 字体链 = 内嵌确定集合**（`system_fonts = false` + **Tuffy 回退**）；
  **系统字体 discovery 接线随 S13 / 真实页面**（届时 `fontique` 系统发现 + **内嵌回退**）。

#### 1.5 Shadow 支撑面（D6；**Rust 层为主 + JS 最小面**）

> **为什么必须做**：ADR-0011 切 Stylo 的驱动因素即 blitz 无 Shadow；直连路线的价值主张是
> **策略自主（侧表解 opaque）**——**其可行性由 P-4 首证**（ADR-0020 决策 3）。

| 面 | G3 最小实现 | 落点 |
|---|---|---|
| `attachShadow` | `NodeTree::attach_shadow`（**mode 语义定死**，§1.2）+ JS 原语/shim | `web-dom::tree` + `navigation::binding`（S10） |
| 影子树结构 | `NodeKind::ShadowRoot` 独立节点；shadow 树链独立于 light DOM | `web-dom::node/tree`（S2） |
| **作用域隔离** | light 查询/遍历**不含** shadow 内容；`shadowRoot` 仅 open 可见 | **Rust 层**：`ShadowSupport` + `NodeTree` 遍历（S11） |
| `:host` / `::slotted` | 作者表内 `:host` 命中宿主；`::slotted(...)` 命中被分发 light 子节点（**经 opaque 侧表 + `implicit_scope...`**） | `layout-backend::shadow`（S11） |
| **slot 分发** | 默认 + 具名简单匹配；`assigned_nodes`/`flattened_children` **按需计算** | `web-dom::tree`（S2） |
| **布局参与** | shadow 节点经 `flattened_children` 参与布局 | `layout-backend::box_tree`（S11） |
| **不做（G4）** | `querySelector`、事件 retargeting、`composedPath`、`slotchange`、`closed` 完整封装隔离 | 登记 §7 U-6 |

---

### 2. 模块设计（要点；完整调用流见 `docs/sequence-diagram-g3.mermaid`）

#### 2.1 `layout-backend` 内部分层

```
       navigation::page（BrowsingContext）
                │ 持有 Rc<RefCell<Box<dyn LayoutBackend>>>
                ▼
   ┌───────────────────────── layout-backend ─────────────────────────┐
   │  backend::StyloBackend  ── 门面（trait LayoutBackend: Debug）      │
   │      ├── style_system::StyleSystem ── Stylist + 样式表             │
   │      ├── opaque::OpaqueSideTable    ── 线程局部侧表（零 unsafe）    │
   │      ├── stylo_host::StyloHost/NodeRef ── DOM→stylo 投影（只读）   │
   │      ├── serialize::PropertyWhitelist ── computed_value 序列化     │
   │      ├── font_metrics::FontMetricsProvider ── stylo Device 度量（S7-b）│
   │      ├── box_tree::BoxTree          ── 样式树→Taffy（匿名盒/内联）  │
   │      ├── text::TextShaper           ── Parley/fontique 整形        │
   │      ├── shadow::ShadowSupport      ── 影子作用域 + slot 分发      │
   │      └── invalidate::Invalidation   ── 两级脏集 + 变更→脏级映射    │
   └───────────────────────────────────────────────────────────────────┘
                │ 只读借用（resolve 期间；不跨 V8 边界）
                ▼
        web-dom::NodeTree（arena + generation）+ web-style（CSSOM 值）
```

> **S7-b `Device` 初始化口径（§2.1 注）**：`style::device::servo::Device::new(...)` 实参含
> **`PrefersColorScheme::Light`** / **`PointerCapabilities::empty()`** 等（G3 固定值 ⇒ 确定性）；
> 其 `Box<dyn FontMetricsProvider>` = `font_metrics::FontMetricsProvider`（S7-b）。
>
> **字体链口径（D7，§2.1 注）**：**G3 = 内嵌确定集合**（`system_fonts = false` + **Tuffy 回退**）；
> **系统字体发现接线随 S13 / 真实页面**（届时 `fontique` 系统发现 + **内嵌回退**）。

#### 2.2 `resolve` 的两级失效 + **变更→脏级映射**（G3-B8）

**变更→脏级映射表**（使 `mark_*_dirty` 有驱动源；由 `MutationBuffer` 驱动，S7/S13）：

| DOM 变更 | style 级 | layout 级 |
|---|---|---|
| `class` / `id` / 一般属性 / `style` 的**非尺寸**属性（`color` 等） | ✅ | ❌（除下列） |
| `style` 的**尺寸相关**属性（`width`/`height`/`margin`/`padding`/`border`/`font-size`…） | ✅ | ✅ |
| `append_child` / `remove_child` / 文本内容变更 | ✅ | ✅ |
| `set_viewport` | ❌ | ✅（全量） |
| Shadow `attach_shadow` / slot 结构 | ✅ | ✅ |

**restyle root（G3-B8）**：`Invalidation.dirty_nodes` **用于**计算 restyle root（脏节点 ∪ 其子树/祖先链）；
**若某步 as-built 退化为全树 restyle ⇒ 显名登记**（ADR-0017 as-built 范式）。`resolve` 流程：
1. 消费脏集 `take()`；2. **style 级**（若 `style_dirty`）：级联 + 计算值（shadow 走作用域解析）；
3. **layout 级**（若 `style_dirty || layout_dirty`）：Taffy 建/更新 + Parley 整形；
4. 几何可读。

#### 2.3 动画时刻 `now` 的 G3 口径

`resolve(now)` 固定传 `0.0`（T2.4 同款，`main.rs:43`）：动画**不在 G3**（§18），固定 `0.0` 使读数完全确定。

#### 2.4 依赖锁版（**ADR-0021**）

```toml
# layout-backend/Cargo.toml（锁版；最终以 ADR-0021 为准）
style        = { version = "=0.20.0", package = "stylo" }   # import 名 style；需 servo feature + build.rs
stylo_dom    = "=0.20.0"                                    # 仅 ElementState/DocumentState
stylo_traits = "=0.20.0"
stylo_taffy  = "=0.3.0-beta.2"
# ⚠️ S1/P-8 实测更正：必须 default-features=false（否则 feature 统一致 E0063，缺 float/clear/flex_line_count）
taffy        = { version = "=0.14.0", default-features = false, features = [
    "std", "taffy_tree", "flexbox", "grid", "block_layout",
    "content_size", "calc", "detailed_layout_info",
] }
parley       = "=0.11.1"
selectors    = "=0.40.0"
skrifa       = "=0.44.0"
fontique     = "=0.11.1"
# S6 直依赖（stylo 关联类型的具名引用；锁图内、同版 `=`）：
web_atoms    = "=0.2.6"      # SelectorImpl 的 LocalName/Namespace（interned atom；stylo-0.20.0/lib.rs:153/156/159）
euclid       = "=0.22.14"    # query_container_size 的 euclid::Size2D<Option<app_units::Au>>
app_units    = "=0.7.8"
url          = "=2.5.8"      # S7-b：Device::new 视口单位 + UrlExtraData
# 注：Atom 经 style re-export ⇒ 无需直依赖 stylo_atoms；stylo_traits 复用锁组（extern 名 = style_traits）
```

- 版本取 = `poc/t2_4_blitz_probe/Cargo.lock` 中**经 blitz 传递编译通过**的整组；
  **措辞更正（G3-B4(i)）**：直连后须**以 P-1/P-8 复验**（stylo 是传递依赖，非探针直接依赖）。
- **删除不可复核数字**（G3-B4(ii)）：不写「stylo 最新 0.22」「parley 最新 0.12」（本地 registry 无，禁联网）。
- `stylo` 需 **`servo` feature**（`stylo-0.20.0/Cargo.toml:29-30`）+ `build.rs`/`links = "servo_style_crate"`
  （`:17-18`）⇒ **prefs 初始化**是新集成面（P-11）。
- **不依赖 `blitz-dom`**；**不依赖 `policy`**（原稿多余项，删除）。
- `stylo_taffy` MSRV/edition **已亲验**：`edition="2024"`、`rust-version="1.89.0"`（ADR-0022）。
- **taffy 特性不变量（S1/P-8 实测）**：**不得开 `float_layout`/`flexbox_balance`**（除非同时开 `stylo_taffy` 的
  `floats`）；否则 `taffy::Style` 多出 `float`/`clear`/`flex_line_count` 而 `to_taffy_style` 字面量缺字段（E0063）。
  **具体特性组以探针 workspace 实测通过的写法为准**（证据 `.workbuddy/refs/g3-probe-evidence/p8-taffy-e0063-evidence.txt`）。

#### 2.5 **同步读几何语义**（G3-B10；新增）

浏览器读几何（`offsetWidth`/`gBCR`/`getComputedStyle`）会**强制布局 flush**。G3 定死同任务内语义：

> **在页面脚本任务内，首次读几何/计算值前，若为脏 ⇒ 宿主隐式 `relayout_if_dirty()`（sync flush），
> 随后读数即为最新**（对齐浏览器「读即 flush」）。**若某步 as-built 无法同步 flush ⇒ 具名登记偏差**
> （读数可能陈旧），并加**负向测试** `geometry_stale_before_flush_registered`。

---

### 3. 各项能力的可测判据（验收测试名 + 断言要点）

> **具判别力（G3-B11）**：断言用**可推导期望值**，且**为反向注入预置**（删实现 ⇒ 必红）。
> **盒口径算术逐一复核（G3-B4）**。

#### 3.1 样式系统

| 测试名 | 断言要点 |
|---|---|
| `cascade_author_over_user_agent` | author 声明胜出 UA（级联顺序可判） |
| `specificity_and_order_applied` | `#id` > `.class` > 元素；同权重按出现序 |
| `inline_style_wins_over_author` | 内联 `style="…"` 胜出 author |
| `computed_value_is_real` | `#box{width:120px}` ⇒ `getComputedStyle(el).width` 可解析为 `120px`（**已知值自证**） |
| `ua_default_display_block` | `<div>` 默认 `display:block`（**UA css**）；宽度撑满容器（G3-B7） |
| `stylesheet_text_registered` / `stylesheet_remove` | 登记 origin+文本；`remove_stylesheet` 生效（G3-B11） |

#### 3.2 布局与几何（**算术已复核**）

| 测试名 | 断言要点 |
|---|---|
| `block_width_matches_css` | `<div style="width:100px;height:50px">` ⇒ `offsetWidth==100`、`offsetHeight==50` |
| `margin_and_border_in_border_box` | `width:100px;border:2px;padding:3px;box-sizing:border-box` ⇒ `offsetWidth==100`；**`clientWidth==96`**（border-box 100 − 左右 border 4 = **96**；G3-B4） |
| `rect_reflects_position` | 第二块（上方块高 50）⇒ `gBCR().top==50`、`left==0` |
| `inline_text_shaped_width` | 内嵌字体整形 ⇒ `offsetWidth==<P-10 回填的钉死值>`（**非 `>0`**；G3-B11） |
| `non_rendered_element_is_zero` | `display:none` ⇒ `offsetWidth==0` 且 rect 空（**负对照**） |

#### 3.3 动态失效（**两级成对**）

| 测试名 | 断言要点 |
|---|---|
| `style_invalidation_changes_computed` | 改 `class` ⇒ **计算值层**变（T2.4 `c7-style` 同款） |
| `layout_invalidation_changes_geometry` | 改 inline `width` ⇒ **几何层**变（T2.4 `c7-layout`）；**两级成对**。**依赖 S8 几何 ⇒ 与 S8/S12 合验** |
| `layout_only_change_leaves_computed` | 仅容器尺寸变 ⇒ 子几何变、子**计算值 `width` 不变** |
| `dynamic_dom_insert_then_read` | 插入新节点 → `resolve` → 新节点几何与输入一致 |

#### 3.4 几何 API 口径（`navigation::binding`；含**同步读**）

| 测试名 | 断言要点 |
|---|---|
| `offset_size_is_border_box_int` | border box **取整**（`81.5px` ⇒ `82`，钉死舍入口径） |
| `client_size_excludes_border` | `clientWidth == offsetWidth − 左右 border` |
| `bounding_rect_viewport_coords` | `gBCR` 的 `x/y` 视口坐标；`right==x+width` |
| `computed_style_via_js` | `getComputedStyle(el).getPropertyValue('width')` 经 JS 桥返回真值 |
| `computed_value_unsupported_property_errors` | 白名单外属性 ⇒ `Err(LayoutErrorKind::UnsupportedProperty)`（**错误路径**，G3-B12③；**该变体 S4 已落**） |
| `geometry_after_mutation_updated` | 改 style → 读几何**同步反映新值**（§2.5；**非陈旧**） |
| `geometry_stale_before_flush_registered` | （**负向**，仅当 as-built 不支持同步 flush）登记陈旧读数 |

#### 3.5 Shadow 用例（**Rust 层为主** + JS 最小面）

| 测试名 | 断言要点 |
|---|---|
| `attach_shadow_mode_semantics` | 同 mode 重复 ⇒ 幂等返回既有根；**mode 不同 ⇒ `Err`**（G3-B13） |
| `shadow_node_hidden_from_light_traversal` | **Rust 层**：light 遍历（`children`/`flattened_children` 区分）**不含**影子内容（作用域隔离核心；G3-B8） |
| `host_selector_styles_host` | 作者表 `:host{width:120px}` ⇒ host `offsetWidth==120` |
| `slotted_selector_styles_slotted` | `::slotted(span){color:red}` ⇒ 被分发 span 计算值 == **stylo 序列化串**（钉死，G3-B12） |
| `slot_distribution_assigned_nodes` | `<slot>` 收到 host light 子节点；`assigned_nodes` 返回之 |
| `shadow_node_participates_in_layout` | shadow 内块级节点几何可读 |
| `shadow_root_open_only_via_js` | `shadowRoot` 仅 `open` 返回、`closed` 返回 `null`（G3-B14） |

#### 3.6 字体与整形（**钉死值，非 `>0`**）

| 测试名 | 断言要点 |
|---|---|
| `embedded_font_is_used` | 内嵌字体 ⇒ 文本度量 == **钉死值**（P-10 回填） |
| `font_fallback_metrics_nonzero` | 请求不存在家族 ⇒ 落到回退字体且 `offsetWidth > 0`（**仅「不 panic」类负对照保留 `>0`**） |
| `text_shaping_width_matches_advance` | 整形宽度 == 字形 advance 之和（内嵌字体逐字校验） |

#### 3.7 Python 控制面增量

| 测试名 | 断言要点 |
|---|---|
| `capabilities_reports_g3_surface` | `geometry == "computed"`；`shadow_dom == true`；其余键不变（**G3-B16：原名 `page_geometry_is_documented_stub` 不存在，实测为 `test_capability_values_are_g2_documented`**） |
| `page_geometry_real_roundtrip` | Python 读几何得**真实值**（与 §3.2 一致） |
| `shadow_case_from_python` | 影子节点不可经 light 遍历见，但参与几何 |

---

### 4. 测试矩阵（模块 · 测试名 · 断言要点 · 类型 · 工具）

| 模块 | 测试名 | 断言要点 | 类型 | 工具 |
|---|---|---|---|---|
| `layout-backend::style_system` | §3.1 六条 | 级联/优先级/内联/计算值真值/UA display/样式表 remove | 单测 | cargo |
| `layout-backend::box_tree` | §3.2 `block_width_matches_css` / `margin_and_border_in_border_box` / `non_rendered_element_is_zero` | 盒模型真值（**96**）+ 负对照 | 单测 | cargo |
| `layout-backend::geometry` | §3.2 `rect_reflects_position` / §3.4 `offset_size_is_border_box_int` / `client_size_excludes_border` / `bounding_rect_viewport_coords` | 坐标/取整/box 口径 | 单测 | cargo |
| `layout-backend::invalidate` | §3.3 四条 | **两级失效成对**（`layout_invalidation_changes_geometry` **依赖几何 ⇒ 与 S8/S12 合验**） | 单测 | cargo |
| `layout-backend::text` | §3.6 三条 | 内嵌字体钉死值 + 回退 + 整形宽度 | 单测 | cargo |
| `layout-backend::shadow` | §3.5 五条（Rust 层） | 作用域隔离 + `:host`/`::slotted` + slot + 布局 | 单测 | cargo |
| `layout-backend`（属性） | `resolve_idempotent` / `nodeid_slot_stable_across_resolve` / `backend_rebuild_matches_spec_dom` / `viewport_change_relayouts` | resolve 幂等；槽稳定；**后端树只是缓存**（T2.4 C10）；视口变更重排 | 属性/单测 | proptest |
| `layout-backend`（错误路径） | `unknown_node_errors` / `not_laid_out_errors` / `computed_value_unsupported_property_errors` | 每个 `pub` 方法 ≥1 错误路径 | 单测 | cargo |
| `web-style::sheet` / `computed` | `stylesheet_origin_and_dedup` / `computed_style_roundtrip` | 登记/往返 | 单测 | cargo |
| `web-dom::tree`（shadow） | `attach_shadow_mode_semantics` / `assigned_nodes_default_and_named` / `flattened_children_crosses_shadow` | shadow 结构 + 分发 | 单测 | cargo |
| `navigation::binding` | §3.4 `computed_style_via_js` / `geometry_after_mutation_updated` | JS 桥真值 + **同步读** | 单测 | cargo |
| `navigation::page` | `relayout_after_mutation` / `layout_reset_on_navigation` | 变更后重算；导航清缓存（ADR-0018·D2） | 单测 | cargo |
| `navigation::parser`（S14） | `module_deadloop_terminates` | 模块死循环 ⇒ 稳定 `Terminated`（U-12） | 单测 | cargo |
| `sdk-python` | §3.7 三条 | 能力面 + 几何真值 + shadow | pytest | uv |
| `contracts` | `capabilities_reports_g3_surface` | `geometry`/`shadow_dom` 口径 | 契约 | pytest |
| `vertical` | `vertical_g3_style_layout_shadow` | 端到端：加载 → 改样式 → 读几何 → shadow 隔离 | 集成 | cargo |

**判别力反向注入（预置）**：
- `block_width_matches_css`：`offsetWidth` 回路改回恒 `0` ⇒ **必红**。
- `shadow_node_hidden_from_light_traversal`：让 light 遍历包含 shadow ⇒ **必红**。
- `layout_only_change_leaves_computed`：让 `mark_layout_dirty` 也触发重算样式 ⇒ **必红**。
- `computed_value_is_real`：返回内联值而非计算值 ⇒ **必红**。

**覆盖目标**：`LayoutBackend` 每个 `pub` 方法 ≥1 正向 + 1 错误路径；`DirtyState` 两级分别可达；
shadow 隔离**必须有负对照**；`layout-backend` 目标 **≥80% lines**。

---

### 5. 有序任务清单（S 编号 · 门禁先行 · 每步 ≤400 行）

> **为什么这么多步**：`AGENTS.md` **铁律 3**（一事一 PR，diff ≤ 400 行）为**硬约束**（优先于「任务数上限」）。
> 原稿 S4/S5/S7 因**合并过多**（评审 B·B9 指上游 `stylo_taffy` 转换层单文件即 867/569 行）⇒ **本版再拆**。
> **dependency 列只写必须先行的步骤**。**P0 = 闸门阻塞**；**P1 = 闸门需要但可后补**。

| 步骤 | 内容 | 主要文件 | 依赖 | 验收测试 | 优先级 |
|---|---|---|---|---|---|
| **S0（门禁先行）** | **落 ADR-0020~0023**（已完成）+ 施工图修订（本版） | `docs/adr/*`、本文件 | — | — | P0 ✅ |
| **S1（探针 A）** ✅ | **P-1**（投影编译 + 级联 + **opaque 侧表反查**）/ **P-2**（stylo→taffy 桥）/ **P-8**（锁版编译，并入 P-1 build）/ **P-11**（prefs 无 GUI）。**已收口（2026-10-13）** | `layout-backend/tests/probe_*.rs` 或 `poc/t3_*` | S0 | P-1/P-2/P-8/P-11 判定 ✅ | P0 |
| **S2（web-dom 数据模型最小面）** | `NodeKind::ShadowRoot` + `attach_shadow`（**mode 语义定死**）/`shadow_root`/`assigned_nodes`/`flattened_children`；**不含 JS 面**（G3-B2：P-4 前置） | `web-dom/src/{node,tree}.rs` | S0 | `attach_shadow_mode_semantics` 等 | P0 |
| **S3（探针 B）** ✅ | **P-4**（真实 `web-dom` 模型上 shadow 作用域 + opaque 机制）/ **P-3**（字体发现）/ **P-10**（内嵌字体度量）。**已收口（2026-10-13）** | `layout-backend/tests/probe_*.rs` | S1,S2 | P-3/P-4/P-10 判定 ✅ | P0 |
| **S4（骨架）** | `backend`（trait **`: Debug`**）+ `geometry` + `error`（**`LayoutErrorKind` 9 变体，含 `UnsupportedProperty`**）+ **`assets/ua.css`**。**偏离（已受理）**：`web_style::SheetOrigin` 于本步**提前落位**（门面签名编译所需） | `layout-backend/src/{lib,backend,geometry,error}.rs`、`assets/ua.css`、`Cargo.toml` | S1 | 门面编译 + `ua_default_display_block` + Debug 断言 | P0 |
| **S5（web-style）** | `Stylesheet` + `ComputedStyle`（+ remove 语义）。**注**：`SheetOrigin` 已于 **S4 提前落位**，本步续扩 `sheet.rs` | `web-style/src/{sheet,computed,error,lib}.rs` | S0 | §3.1 登记/往返/remove | P0 |
| **S6（stylo_host）** ✅ | `impl style::dom::{TNode,TElement,TShadowRoot}` + `selectors::Element`（`opaque()` 走 §1.2.1 侧表）+ `src/opaque.rs`；**落目录模块 `src/stylo_host/`**；`layout-backend` 本地 `deny` + 逐 `fn` `#[allow]`+`SAFETY`（**5 声明 + 0 腔体**，P-1b 终值）。**前置（R17）已解**（E′+R，ADR-0024）。**✅ 已完成；四态注入首验通过**（证据 `.workbuddy/refs/g3-s6-whitelist-evidence/`）；**S6-G：`cargo geiger` 顺延 S14** | `layout-backend/src/stylo_host/`、`src/opaque.rs` | S4 | P-1 回填 + P-1b 定形 + 编译 ✅ | P0 |
| **S7（级联 + 失效）** | `style_system` + `invalidate`（**变更→脏级映射** + restyle root）+ `serialize`（**属性白名单**）+ **`font_metrics` 子组件（S7-b；stylo `Device` 的 `FontMetricsProvider`）**。**S7-a ✅**（`a885eb6`，`invalidate.rs` 两级失效簿记 + §2.2 映射；layout-backend 14/14、workspace **185**）；**S7-b（级联 + font_metrics）在途** | `layout-backend/src/{style_system,invalidate,serialize,font_metrics}.rs` | S5,S6 | §3.1 六条 + §3.3 四条（几何条**与 S8/S12 合验**） | P0 |
| **S8（盒树）** | `box_tree`（Taffy 建树/匿名盒/内联） | `layout-backend/src/box_tree.rs` | S7 | §3.2 五条 + **§3.3 `layout_invalidation_changes_geometry` 合验**（依赖本步几何） | P0 |
| **S9（文本）** | `text`（Parley 整形 + fontique 回退 + 内嵌字体）。**注**：文本整形另属本步，**与 S7-b 的 `font_metrics` 无重复**（S7-b = Device 度量；S9 = 逐字 advance/换行） | `layout-backend/src/text.rs` | S7 | §3.6 三条（钉死值） | P0 |
| **S10（web-dom Shadow JS 面）** | `attachShadow`/`shadowRoot` 原生原语 + shim（`querySelector` 不做→G4） | `web-dom/src/document.rs`、`navigation/src/binding.rs` | S2,S8 | §3.5 `shadow_root_open_only_via_js` | P0 |
| **S11（shadow 作用域 + slot→布局）** | `layout-backend::shadow`（`:host`/`::slotted` + slot 参与布局） | `layout-backend/src/shadow.rs` | S8,S9 | §3.5（Rust 层五条） | P0 |
| **S12（几何真值接线 + 同步读）** | `binding`：`offset*`/`client*`/`gBCR`/`getComputedStyle` + §2.5 同步读 | `navigation/src/binding.rs`、`Cargo.toml` | S8,S9 | §3.4 七条 + **§3.3 `layout_invalidation_changes_geometry` 合验** | P0 |
| **S13（page 集成）** | `BrowsingContext` 持 `LayoutBackend` + `relayout_if_dirty` + 导航 `reset` + `Capabilities` 更新 | `navigation/src/{page,controller}.rs` | S12 | §4 page 两条 | P0 |
| **S14（模块 watchdog，U-12）** | 模块 eval 接 watchdog（**独立 P0 小步**；ADR-0023）+ **`cargo geiger` 基线留档**（**S6-G 顺延至此**；geiger ≥0.13 需 rustc ≥1.94 > 钉版 1.92 ⇒ **仅远程 S14 跑**，属**基线观测非门禁**）。**geiger 备忘**：`.workbuddy/refs/g3-s6-whitelist-evidence/geiger-S14-备忘.md`（命令/输出格式/关注点 = **unsafe 计数 ≈5 且全在桥内**） | `navigation/src/{parser,page}.rs` | S13 | `module_deadloop_terminates` + geiger 基线 | P0 |
| **S15（控制面）** | `sdk-python` 几何能力面 + `contracts` 口径 | `sdk-python/src/page.rs`、`contracts/iv8_contract/page_control.py`、`tests/test_page_control.py` | S13 | §3.7 三条 | P0 |
| **S16（纵向 + 收口）** | 纵向样例 `vertical_g3_style_layout_shadow` + 覆盖率 + **双端门禁复验** | `navigation/tests/vertical_g3.rs`、`sdk-python/tests/test_page.py` | S11,S14,S15 | §4 vertical + 五门禁 | P0/P1 |

> **S0 已落（本批）**；**S1/S3 探针全绿前不得开工 S2+**（S2 为 P-4 前置例外，见 G3-B2）；
> **P-4 失败 ⇒ 停上报**（ADR-0020 决策 3）。
> **⛔ S1 实测现状：`G3_S1_VERDICT=BLOCK`** —— P-8/P-11/opaque 机制 PASS，但 **P-1 因 `TElement` 5 个
> 必需 `unsafe fn` BLOCK**（R17）⇒ **须先落「unsafe 白名单」决策（A/B）才能进 S6+**；P-2 待补跑。
> **timer / 脚本任务化 / 宿主 `try_borrow` 移出 G3 → G4**（ADR-0023）。

---

### 6. 风险与失败清单（含「不通过则切…」判据）

| # | 风险 | 触发 | 影响 | 对策 | 归属 |
|---|---|---|---|---|---|
| **R1** ✅ | **直连 Stylo 核心价值主张失败**（opaque/rehydrate 做不出 **或** Shadow 作用域做不出） | S1 **P-1** 失败（反查不命中/须 `as_const_ptr`）**或** **P-4** 失败（`:host`/`::slotted` 不经 implicit scope） | **直连路线价值被证伪** ⇒ 停上报 | **已解除（P-4 实测，`e59e562`+`9f91a6e`）**：作用域隔离 / `:host`+`::slotted` **命中（含负对照）** / opaque 侧表在真实 `web-dom` 成立 + 模块**零 unsafe**（P-1 机制亦 PASS）。失败对策（历史）：走 **ADR-0020 备选 D**（blitz+上游 `opaque()` 修形）或重评；**不接受静默换路** | 架构师+工程师 |
| **R2** | **stylo/taffy/parley 锁版冲突** | `stylo_taffy` 钉 `stylo 0.20`；**`taffy` 默认特性致 E0063**，**S1/P-8 已实测** | 阻塞 S4 编译 | **锁版取 t2_4 实测组**（ADR-0021）+ **`taffy` `default-features=false`**（ADR-0021 更正注记）；**P-8 实测已更正** | 工程师 |
| **R3** | **直连路线被误读为「省掉依赖」** | 以为切 Stylo 省 stylo/taffy/parley | 低估工作量 | ADR-0011/0020 明写；**成本量化**（新增 crate 数 = stylo/servo 栈全套；自建环节 = §0.2 非目标清单） | 架构师 |
| **R4** | **字体导致几何跨机不稳** | mac(arm64) vs Linux(x86_64) `f32` | 门禁偶发红 | **内嵌字体**（D7）；跨机判据 **≤0.5px 容差**（非逐字节，G3-B10） | 工程师+QA |
| **R5** | **工具链漂移（K-14）+ MSRV 违背** | 本地 1.92 vs 远程 1.99；`parley 1.88`/`stylo_taffy 1.89.0` vs 声明 1.85 | 单端红；声明失实 | **ADR-0022**：钉 `1.92.0`；**MSRV 更正**（处置既存违背）；CI 收敛=待批条 | 工程师+QA |
| **R6** | **几何/样式缓存跨导航残留** | 导航后旧 `NodeId` 命中旧几何 | 读陈旧几何 | **`reset()` 清 `node_slots`/侧表**（D8）；专测 `layout_reset_on_navigation` | 工程师 |
| **R7** | **几何真值被过度承诺** | 「读了布局」读成「完整布局已实现」 | 高估 G3 | §0.2 **显式非目标**；`capabilities.geometry` **不写**「full」；具名登记 | 架构师 |
| **R8** | **远程 3.6 GB OOM** | stylo/parley 链接 + V8 | 远程 link OOM | `-j2` + `RUSTY_V8_ARCHIVE` 中转；仍 OOM ⇒ 决策上报；**运行期峰值内存并入 P-9** | 工程师+QA |
| **R9** | **`web-style → web-dom` 依赖环** | 计算值放进 web-style 且需读 web-dom | 编译失败 | **保持单向**；计算值由 `layout-backend` 产出；`web-style` 只放**值类型** | 架构师 |
| **R10** | **Shadow 数据模型成第二真相源** | 持久 `assigned_nodes` 与 DOM 不同步 | 状态漂移 | slot 分发**按需计算**（D6） | 工程师 |
| **R11** | **G3 不通过时切哪条路（判据）** | R1 失败或几何/字体无法跨机稳定 | G3 不过 | **不通过判据**：`SHADOW_SCOPE_WORKS==false` **或** `GEOMETRY_MATCHES_EXPECTED==false` **或** `FONT_DETERMINISTIC==false`（**=`FONT_METRICS_WITHIN_TOLERANCE==false`**，G3-B10 同步）。**切换判据**：(a) 纯因 Shadow ⇒ **ADR-0020 备选 D**；(b) 纯因锁版/字体 ⇒ **ADR-0021 备选**（换版本/内嵌字体族）；(c) **几何口径对不齐 Chromium ⇒ 默认走「自洽 + 具名偏差登记」**（G3-B3；CDP 差分**单列**、不属 G3） | 架构师 |
| **R12** | **stylo trait 面工作量与 ≤400 行冲突** | `style::dom::{TNode,TElement}`+`selectors::Element` 数十方法 | 单 PR 超限 | **S6 单列**（trait 面）；`style_system`/`box_tree`/`text`/`shadow` **各自成步**（§5） | 工程师 |
| **R13** | **`InlineStyle`→stylo 声明映射未设计** | `web-style::InlineStyle`（结构化 decl）→ stylo 声明 | 内联样式丢失 | **P-1 补判**：inline `style="…"` → stylo 声明映射；落 S6 | 工程师 |
| **R14** | **`computed_value` 全属性序列化范围未界定** | `computed_value(node, prop)` 对**任意**属性 | 实现爆炸 | **属性白名单**（`serialize.rs`，G3-B12③）；白名单外 ⇒ `Err(UnsupportedProperty)` | 架构师+工程师 |
| **R15** | **resolve 无增量 ⇒ 每次全量 restyle** | 未实现 restyle root | 性能/内存面 | **restyle root**（§2.2）；若退化为全树 ⇒ **具名登记**（ADR-0017 as-built 范式） | 工程师 |
| **R16** | **stylo prefs/`servo` feature 初始化时机** | 无 GUI 环境下 prefs 初始化 | 启动失败 | **P-11 探针**（无 GUI 可运行性）；**S1 实测 PASS**；落 S4 | 工程师 |
| **R17** ✅ | **`TElement` 5 个必需 `unsafe fn` vs 全仓 `forbid(unsafe_code)`**（P-1b 已定形：**5 声明 + 0 腔体**，见 §7 P-1b） | 实现 `TElement`（Stylo 级联必备） | 承载 crate 不能零 unsafe | **已落 = 「E′+R」**（**ADR-0024**；白名单形态 = 尾斜杠目录条目 `crates/layout-backend/src/stylo_host/` + Rust 门禁 a/b/c/d）；**守卫数据行已落地**（维护者 2026-10-13 18:10 批准）；**S6 门已解** | 架构师+主理人 |

---

### 7. 与既有 ADR 冲突 / 不确定点 + **探针清单（P-1~P-11）**

| # | 未知点 | 要证的假设 | 探针 | 判定标准 | 确定性 |
|---|---|---|---|---|---|
| **U-1** | 失效测试可同步完成（U-10-b） | 「G3 失效测试可只用同步脚本 + 显式 `resolve`」 | §3.3 自证 + P-6 | 成立 ⇒ **timer 归 G4** | **已定（ADR-0023）** |
| **U-2** | 模块 watchdog 归属（U-12） | 「watchdog 与布局无耦合，可独立 P0」 | S14 | 成立 ⇒ **独立步归 G3** | **已定（ADR-0023）** |
| **U-3** | 宿主 `try_borrow` 残余（D4） | 「G3 `resolve` 不跨 V8 边界 ⇒ 不放大」 | §2.1 调用链 | 成立 ⇒ 归 **G4** | **已定（ADR-0023）** |
| **U-4** | 脚本任务化（U-10-a） | 「同步脚本够用」 | 同 U-1 | 成立 ⇒ 归 G4 | **已定（ADR-0023）** |
| **U-5** | Shadow 数据模型（独立节点 + 按需分发） | 「可表达规范作用域」 | **P-4** | 作用域 + 分发**可判** | **待实测（S3）** |
| **U-6** | 事件 retargeting/`querySelector`/`slotchange` 边界 | 「可安全推 G4」 | 复核 §18 只要求 Shadow **用例** | 成立 ⇒ 推 G4（§0.2） | **已定（ADR-0020 决策 6）** |
| **U-7** | `style::dom::{TNode,TElement,TShadowRoot}` 在 `web-dom` 上可实现 | 「`NodeRef<'a>{tree,id}` 可实现全套 trait」 | **P-1** | 编译通过 | **待实测（S1）** |
| **U-8** | `stylo_taffy` 可脱离 blitz-dom 直接用 | 「公开 API 不依赖 blitz-dom 类型」 | **P-2** | 经桥出布局 | **待实测（S1）** |
| **U-9** | 几何取整/盒口径 | 「`offset` 取整 / `client` 去 border 与规范一致」 | **P-5** | **自洽 + 具名偏差**（G3-B3） | **待实测（S8/S16）** |
| **U-10** | `getComputedStyle` 计算值语义 | 「以 stylo 计算值为准 + 白名单」 | §3.1 | 白名单内可用 | **本设计定（ADR-0020）** |
| **U-11** | 逻辑时钟与 `now=0.0` | 「与 ADR-0010 正交」 | — | 成立 | **确定** |

#### 探针清单（**修订版 P-1~P-11**，逐条 = 假设 + 最小实验 + 判定标准）

> **形态**：优先落 **`layout-backend/tests/probe_*.rs`**；若需独立 workspace 则 `poc/t3_*/`（先跑 `cargo fmt`，K-3 约定）。
> **P-1 判定含 opaque 机制（G3-B1）；P-4 在真实 `web-dom` 模型上（G3-B2，前置 S2）；P-5 降级（G3-B3）；P-2 强化（G3-B9）。**

| P | 假设 | 最小实验 | 判定标准 | 对应 |
|---|---|---|---|---|
| **P-1（强化·前置门）** | `NodeRef<'a>` 可实现 `style::dom::{TNode,TElement,TShadowRoot}` + `selectors::Element`，且 **opaque/rehydrate 经线程局部侧表**可行 | 建 ≥2 节点 + UA 表 `.x{width:120px}`；跑级联；走 `opaque()`→`implicit_scope_for_sheet_in_shadow_root` **反查路径**；**inline `style="…"`→stylo 声明**（R13）；`servo` prefs 初始化（R16） | 编译通过 **且** 计算值可解析 **且** 反查命中 **且** **零 unsafe**。**S1 实测：opaque 机制 ✅ PASS（零 unsafe+反查命中）；总判定 = 已裁（E′+R，ADR-0024 草案）——随维护者批准落档** | U-7 / G3-B1/B5 / R17 |
| **P-1b（新增·S6 定形）** | `ensure_data`/`clear_data` 的**函数体**是否有**零 unsafe 出路** | 两法最小 impl：① **急切**持有 `ElementDataWrapper`（`ensure_data`=`ElementDataWrapper::borrow_mut()`，stylo 安全方法）；② `UnsafeCell` 腔体（blitz 法） | 判定 = **优先零 unsafe**；若 `clear_data` 语义迫使腔体 ⇒ **具名登记实际 unsafe 面**，供 A/B/E **定粒度**。**P-1b 实测 = PASS：信封 = 「5 声明 + 0 腔体」（终值）**（signatures:5 / bodies:0 / 腔体:0；急切持有 `ElementDataWrapper` + 安全 `borrow_mut`/`DerefMut` 写 default；commit `407c0aa`）⇒ **门禁 b 阈值定格 0（终值）** | R17 / G3-B1 |
| **P-2（强化）** | **经 stylo→taffy 桥**（`to_taffy_style`）「样式改 ⇒ 布局盒变」可判 | 把**真实 `ComputedValues`** 经 `to_taffy_style` 转 `taffy::Style` 并出布局，**断言尺寸由 CSS 推出** | 样式输入变 ⇒ 布局尺寸随之变（**非恒真**，G3-B9）。**S1：PEND（未跑）** | U-8 |
| **P-3** | `fontique` 在远程 Linux 无桌面能发现字体；内嵌字体可注册 | 本地注册内嵌 + 远程发现系统字体 | 本地度量**钉死值**；远程**非 0 且不 panic**。**实测 = ✅ PASS** | D7/R4 |
| **P-4（重定义）** | 真实 `web-dom` 模型上：`attach_shadow` + 影子根内作者表 `:host`/`::slotted` **经 implicit scope 生效** | S2 数据模型上：`attach_shadow` + shadow 内 `::slotted`/`:host` 规则；light 遍历影子内 id | **三者同时成立**：① 作用域隔离（light 遍历不含影子）② `::slotted`/`:host` 命中 ③ **含 opaque 机制步骤 + 零 unsafe**。**P-4 实测 = ✅ PASS**（三判据全过，含负对照；commits `e59e562`+`9f91a6e`）⇒ **R1 解除** | R1/U-5/G3-B1 |
| **P-5（降级）** | 几何读数**自洽**（可由 CSS 推导） | 固定 CSS 场景读 `offsetWidth/clientWidth/rect`，**手算自证** | 自洽 + 偏差**具名登记**（**非 Chromium 对齐**，G3-B3） | U-9 |
| **P-6** | 两级失效可分离 | 改 `color`（只 style）vs 改容器尺寸（含 layout），各读计算值 + 几何 | 前者计算值变几何不变；后者相反；**style 级几何变化须经 layout 级跟随** | §3.3 |
| **P-7** | `NodeId → 后端槽` 稳定 + `reset` 清空 | 两次 `resolve` 后读同节点几何；`reset` 后旧 id 查几何 | 稳定 **且** `reset` 后 `Err(NotLaidOut)`/`UnknownNode` | R6 |
| **P-8** | `selectors 0.40` 与 `stylo 0.20` 兼容 | 锁版后 `cargo build -p layout-backend` | 编译通过（无冲突）；**并入 P-1 build**。**S1 实测 PASS（版本全匹配）；额外更正：`taffy` 须 `default-features=false`（E0063）** | R2 |
| **P-9** | 全栈在 **x86_64 Linux** 编译并跑探针 | 远程 `~/mmv8` 跑（`-j2` + `RUSTY_V8_ARCHIVE`） | exit 0；**含运行期峰值内存观察** | R8 |
| **P-10** | 内嵌字体度量**跨机在容差内** | 同内嵌字体两机整形同段文本 | 同架构**逐位相等**；跨架构 **≤0.5px** + 记录 arch/字体哈希。**实测 = ✅ PASS** | R4/G3-B10 |
| **P-11（新增）** | prefs/初始化在**无 GUI** 环境可运行 | 无桌面环境初始化 stylo prefs（`servo` feature） | 初始化成功、可跑级联（不 panic/不依赖 GUI）。**S1 实测 PASS（`P11_HEADLESS_OK=true`）** | R16 |

> **单向映射表（避免 U/P 两套编号漂移，评审 A·④ 提示）**：U-7↔P-1、U-8↔P-2、U-5↔P-4、U-9↔P-5、
> U-10↔(P-1 级联)、U-11↔(无探针，正交)、U-1/U-2/U-3/U-4↔(ADR-0023 定论)。
> **P-6 无 U**（属实现判据）。

---

## Part B：任务分解（摘要）

### 8. Shared Knowledge（跨切面，工程师必读）

- **后端契约（ADR-0011·C14 + ADR-0020）**：**`LayoutBackend` 对外接口不随后端变化**，且 **`: Debug`**（G3-B6）；
  上层只依赖 trait，**类型面不出现 `style`/`taffy`/`parley` 类型**（类比 C-2 对 `JsHandle`）。
- **opaque 纪律（ADR-0020 决策 2）**：**线程局部侧表**解 opaque；**从不调 `as_const_ptr`**；
  **零 unsafe 为准**；**若必须 unsafe ⇒ 停上报**。
- **借用纪律（ADR-0016·D2）**：`resolve` 期间 stylo 回调读 DOM **只读**，**不跨 V8 边界**；
  **`resolve` 必在释放 `NodeTree` 可变借用之后**（§7 docx:189）；取任何 `Rc<RefCell>` 一律 `try_borrow` ⇒ 可判定错误。
- **导航纪律（ADR-0018·D2 + D8）**：导航 = 重建 `NodeTree` + 清 `node_map`/`registry`/`mutations`
  **+ `LayoutBackend::reset()`**（清 `node_slots`/**opaque 侧表**/几何/样式）；`NodeId` 跨导航**不别名**。
- **几何口径（D5；G3-B3）**：`offset`=border-box 取整；`client`=padding-box；`gBCR`=视口坐标 border box；
  `getComputedStyle`=stylo 计算值（**属性白名单**）。**判据 = 自洽 + 具名偏差**（**非 Chromium 对齐**）。
- **同步读纪律（G3-B10）**：页面脚本任务内读几何/计算值前，若脏 ⇒ **隐式 flush**；否则具名登记 + 负向测试。
- **字体纪律（D7）**：测试/门禁**用内嵌字体**；跨机判据 **≤0.5px 容差**。
- **Shadow 纪律（D6）**：影子根独立节点；slot **按需计算**；**Rust 层为主**；`querySelector`/retargeting **属 G4**。
- **UA 表纪律（G3-B7）**：**内嵌最小 UA css**（`assets/ua.css`），否则几何真值系统性偏。
- **unsafe 纪律（S1/P-1 更正）**：**opaque 机制零 unsafe（已实测）**；**但 `TElement` 强制 5 个必需
  `unsafe fn` ⇒ 承载 crate 不可能零 unsafe** ⇒ 须**入白名单**（方案 A/B，决策上报中）。启用后：
  **逐 `fn` `#[allow(unsafe_code)]` + `// SAFETY:`**；**禁 crate/mod 级开口**；**白名单未获批前零改动**。
- **taffy 特性不变量（S1/P-8）**：**不得开 `float_layout`/`flexbox_balance`**（除非同开 `stylo_taffy` 的 `floats`），
  否则 `taffy::Style` 缺字段（E0063）。
- **锁版纪律（ADR-0021）**：整组**精确锁版** + `Cargo.lock` 入库；升级 = 独立 PR + 全探针回归 + 裁决；
  **不改 `deny.toml`**；新传递依赖许可**上报**。
- **工具链纪律（ADR-0022）**：`channel = "1.92.0"`；MSRV 更正；**CI 收敛待批**。
- **测试**：Rust `cargo test`；Python `uv run pytest`；每条改动必须能回答「验收测试是哪一条」（铁律 9）。
- **跨平台纪律**：文件名全小写下划线；路径用 `std::path`；新增依赖须支持 linux x86_64。

### 9. 任务依赖图

```mermaid
graph TD
    S0["S0 门禁先行（ADR-0020~0023）✅"]
    S1["S1 探针A P-1/P-2/P-8/P-11"]
    S2["S2 web-dom 数据模型最小面（P-4 前置）"]
    S3["S3 探针B P-3/P-4/P-10"]
    S4["S4 layout-backend 骨架（:Debug + UA css）"]
    S5["S5 web-style CSSOM"]
    S6["S6 stylo_host + opaque"]
    S7["S7 style_system + invalidate + serialize"]
    S8["S8 box_tree"]
    S9["S9 text"]
    S10["S10 web-dom Shadow JS 面"]
    S11["S11 shadow 作用域 + slot→布局"]
    S12["S12 binding 几何真值 + 同步读"]
    S13["S13 page 集成"]
    S14["S14 模块 watchdog（U-12 独立 P0）"]
    S15["S15 sdk-python + contracts"]
    S16["S16 纵向 + 覆盖率 + 双端复验"]

    S0 --> S1
    S0 --> S2
    S1 --> S3
    S2 --> S3
    S1 --> S4
    S0 --> S5
    S4 --> S6
    S5 --> S7
    S6 --> S7
    S7 --> S8
    S7 --> S9
    S2 --> S10
    S8 --> S10
    S8 --> S11
    S9 --> S11
    S8 --> S12
    S9 --> S12
    S12 --> S13
    S13 --> S14
    S13 --> S15
    S11 --> S16
    S14 --> S16
    S15 --> S16
```

---

## 附：ADR · Mermaid 图 · 决策 brief · 探针

- **已落 ADR（G3）**：`docs/adr/0020-*`（路线 + opaque 策略）、`0021-*`（锁版 + 升级流程 + 补丁通道）、
  `0022-*`（工具链钉版 + MSRV 更正）、`0023-*`（残留排程修正）。
- **决策 brief**（含**更正标注**，不重写历史）：
  - `docs/decision-briefs/2026-10-13-G3-stylo-direct.md`（D1）
  - `docs/decision-briefs/2026-10-13-G3-blitz-lock.md`（D2）
  - `docs/decision-briefs/2026-10-13-G3-toolchain-pin.md`（D3）
  - `docs/decision-briefs/2026-10-13-G3-residual-scheduling.md`（D4）
  - `docs/decision-briefs/2026-10-13-G3-unsafe-whitelist-layout-backend.md`（**⛔ S1/P-1 新增：`TElement`
    unsafe 白名单，方案 A/B —— 待裁决；**R17**）
- **裁决与辩论**：`docs/decision-briefs/2026-10-13-G3-裁决-主理人.md`、`-G3-辩论意见-A.md`、`-G3-辩论意见-B.md`
- 类图（G3）：`docs/class-diagram-g3.mermaid`（已与 §1.2 对齐）
- 时序图（G3）：`docs/sequence-diagram-g3.mermaid`
- 上游依据：`docs/adr/0011-*`（T2.4 裁决 + 0020 更正注记）、`docs/HANDOFF-2026-10-13.md`、
  `docs/design-g2-page-2026-10-13.md`（几何 stub 现状）。
