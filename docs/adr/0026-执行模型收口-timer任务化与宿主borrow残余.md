# ADR-0026：执行模型收口 —— timer / 脚本任务化 / 宿主 `try_borrow` 残余（`G3X-S3a/b/c`）

- **状态**：**✅ 已接受（Accepted）**（主理人 2026-10-14 审定；**维护者按总授权登记**，无需单点）—— **ADR 落档，`S3a` 排 `S2` 之后开跑**。
- **日期**：2026-10-14
- **授权依据**：`AGENTS.md` **铁律 6** —— 「**高风险路径改动必须走「决策上报协议」获批**，AI 产出仅为初稿」+「**没有 ADR 落档的结论不执行**」（`:23`/`:50`）。
- **关联**：**ADR-0023**（本三项 **由 G4 移交收尾批**；其中 `timer` 被**点名「若做须单列 ADR」—— 即本 ADR**）｜**ADR-0010**（逻辑时钟 / 时区）｜**ADR-0017**（解析器宿主 · 执行协议 · 调度器核心）｜**ADR-0018**（资源等待与导航语义）｜**`docs/design-g3-收尾批-2026-10-14.md`**（v2 冻结；`G3X-S3a/b/c`）。
- **关联登记**：G3 §7 **U-1 / U-3 / U-4 / U-10a / U-10b**（ADR-0023 定论）；§0.1.3「**脚本可中断面完整（P-1a + U-12 双闭合）**」。

## §1 背景与范围

- G2 留有 4 项「**已登记、非缺陷**」残留（① **脚本任务化** ② **timer 事件循环** ③ **宿主 `try_borrow` 残余** ④ 另项）；**ADR-0023 裁定 → G4**；收尾批承接为 **`G3X-S3a`（timer）/ `S3b`（任务化）/ `S3c`（宿主 borrow）**。
- **为何必须 ADR**：三项**均改主链**（`navigation` 调度 / 解析 / 绑定）⇒ **触铁律 6**；且 `timer` 被 ADR-0023 **点名须单列 ADR**。
- **现状**：G3 = **同步脚本**执行 + **模块 watchdog**（`f531613`，U-12）；**timer / 任务化 / 宿主 borrow 均未做**。

## §2 决策（三拆，对应 `G3X-S3a/b/c`）

### 决策 1 —— `G3X-S3a`：timer 事件循环（**逻辑时钟**）
- **形态**：以 **ADR-0010 逻辑时钟**为**唯一时间源**；事件循环按 **宏任务轮（task round）** 排程；**每轮设预算**（防饿死）；**与真实时钟解耦**。
- **硬约束**：**不得阻塞 `load`**；轮次**可判定**（测试可断言「**第 N 轮**」）。
- **备选**：A 真时钟轮询（**否决**：非确定）；B 无预算轮（**否决**：可饿死）；**C 逻辑时钟 + 预算轮（选定）**。
- **✅ 定裁（§6-1，2026-10-14）**：**预算 =「每轮任务数上限」**（**确定性优先、防饿死**）；**初值 = `16` 任务/轮 + 单次冲刷 ≤ `64` 轮**；**实现期允许校准一次，以测试钉值收口**（**哨兵纪律：钉后仅升须显式**）。

### 决策 2 —— `G3X-S3b`：脚本任务化
- **形态**：引入**宏任务队列** + **复用 V8 微任务**；脚本执行**任务化**（**非逐条同步直呼**）。
- **语义**：**微任务在任务边界冲刷**（与 V8 一致）。
- **备选**：A 维持同步直呼（**否决**：与 timer 无法交织）；**B 任务队列 + 微任务边界冲刷（选定）**。
- **✅ 定裁（§6-2，2026-10-14；语义受控精化 2026-10-14）**：**任务化对象 = `defer` / `module` / `async` + timer 回调**；**`classic` 阻塞脚本保持就地执行（S13 语义不动）** —— **粒度 = 上述单元各 = 一宏任务**（**非每语句**）；**微任务接入点 = 每宏任务边界的 V8 checkpoint**（**复用既有微任务机制**，**G1 面**）。**精化说明**：原表述「每 `<script>` = 一宏任务」**过宽** ⇒ 收敛如上（**`classic` 阻塞脚本不计入任务化**）。

### 决策 3 —— `G3X-S3c`：宿主 `try_borrow` 残余
- **形态（措辞精化 · 主理人 2026-10-14）**：`resolve` **必在释放 `NodeTree` 可变借用之后**（§7 docx:189 口径）；**「宿主驱动路径上的任何 `Rc<RefCell>` 取借一律 `try_borrow`」**（**意图收窄** —— **非**字面「任何」；**可判定错误、不 panic**）—— 沿 **S12/S13 门面**口径（`relayout_if_dirty` 已示范 `try_borrow` 可判定）。**范围 = 7 路径（`.borrow()`）+ 宿主驱动面 `.borrow_mut()`（35 处，经核全部属宿主面 ⇒ 无豁免项；见 §6-3）**。
- **备选**：A 保留 `borrow()`（**否决**：panic 面）；**B 全改 `try_borrow` + 可判定错误（选定）**。

## §3 验收（**可推导期望值 + 负对照**）

| # | 测试 | 钉值 | **负对照** |
|---|---|---|---|
| 3.1 | `timer_fires_after_n_rounds` | **第 N 轮**触发（N 钉死；**预算 `16`/轮 · 冲刷 ≤ `64`**，§6-1） | 同步实现 ⇒ 轮次不符 |
| 3.2 | `timer_does_not_block_load` | `load` 期间**轮不抢**（预算生效） | 阻塞 ⇒ `load` 延后 |
| 3.3 | `task_queue_ordering` | **严格按序** + 微任务边界 | 乱序 ⇒ 红 |
| 3.4 | `no_host_try_borrow_panic` | **可改 6 路径全覆盖（20 处）+ 宿主面 `borrow_mut` 可改全集（35 处）+ 5 处具名约束标注（`stylo_host` 4 + `opaque` 1；非漏改）**；无 panic + 期望值 | 保留 `borrow` / `borrow_mut` ⇒ panic |

## §4 风险

- **铁律 6**：**本 ADR 落档前不动码**；
- **回归面 ≥ G2**（改 `page.rs` / `parser.rs` 主链）；
- **与 `G3X-S1b` 推进点（持久槽存储 / 增量 restyle）**：**无依赖 —— `S3` 不等**（**正交**：timer/任务化与「持久槽存储」互不阻塞；后者**绑增量 restyle 立项**）。

## §5 门禁 / 后果

- **门禁**：`§3` 矩阵（钉值 + 负对照）；回归 `navigation` 全绿；**不阻塞 load** 为**硬门禁**。
- **后果**：执行模型具备 **timer + 任务化 + 无 panic borrow 面** ⇒ 为 **真 G4（宿主 CDP 域）/ G6（动画·rAF）** 铺路。

## §6 裁决（✅ 已定，2026-10-14）

1. **`§6-1` timer 预算 / 轮上限** = **「每轮任务数上限」**（确定性优先）：**初值 `16` 任务/轮 + 单次冲刷 ≤ `64` 轮**；**校准一次**、**测试钉值收口**（**钉后仅升须显式**）。
2. **`§6-2` 任务化粒度 / 接入点** = **每 `<script>` = 一宏任务**；微任务接入点 = **每宏任务边界的 V8 checkpoint**（G1 面）。
3. **`§6-3` 七路径清单**（工程师枚举 `8af1a5a` → 修正 `7498f92` + **架构师源级核可**）：**初版 13 处 / 5 路径 ⇒ 修正后 产品码 `.borrow()` = 22 处 / 7 路径 + 伴生族 `.borrow_mut()` = 35 处**（**全仓扫查**；**完整表 + 可复现命令见下 `### §6-3`**）（`page.rs` **12** + `sdk-python/convert.rs` **1**）—— **① 导航绑定** `page.rs:264`；**② 输入派发**（**可重入**）`:399`/`:448`；**③ 解析 sink 驱动**（**可重入**：阻塞脚本就地执行）`:632`/`:637`/`:651`/`:661`；**④ fetch 泵** `:721`/`:802`/`:935`/`:1193`/`:1194`；**⑤ Python 边界** `convert.rs:181`。**替代形态**：**①–④ `try_borrow → Internal`**；**⑤ `try_borrow → reentrant_pyerr`**。**产品码 0 处的文件**：`binding.rs` / `parser.rs`（其 `.borrow()` **皆在 `#[cfg(test)]` 之后** —— 架构师已核）。
   - **⚠️ 架构师核可分歧（+7 处，建议第 6 路径）**：**`navigation/sink.rs` 7 处**（`:32`/`:57`/`:147`/`:184`/`:206`/`:224`/`:231`）**全产品码**（该文件**无 `#[cfg(test)]`**）⇒ 属 **`html5ever::TreeSink` 回调面**（`DomSink` 持 `Rc<RefCell<NodeTree>>`），与 ③ **同属「解析期可重入面」** ⇒ **建议纳入第 6 路径**（`try_borrow → Internal`）；**若不纳入，须给「回调期无并存 mut 借」的理由**（**举证责任在排除方**）。
   - **可复现 grep**（产品码，按各文件 `#[cfg(test)]` 行号截断）：
     ```
     for f in page sink binding parser; do
       T=$(grep -n '#\[cfg(test)\]' crates/navigation/src/$f.rs | head -1 | cut -d: -f1); [ -z "$T" ] && T=999999
       grep -n '\.borrow()' crates/navigation/src/$f.rs | awk -F: -v t=$T '$1<t'
     done
     grep -n '\.borrow()' crates/sdk-python/src/convert.rs
     ```
     （架构师复核事实：`page.rs` **12** 处 · `sink.rs` **7** 处 · `binding.rs` **0** 处 · `parser.rs` **0** 处 · `convert.rs` **1** 处。）
### §6-3 宿主驱动路径 · `.borrow()`（非 `try_borrow`）源级枚举（2026-10-14 回填）

> 口径：仅**产品码**（各文件 `#[cfg(test)]` 之前）；**枚举 = 全仓扫查**（修正：初版 grep 仅列 4 个文件，
> **漏 `sink.rs`/`stylo_host`**，由架构师核可时抓出 ⇒ 现改全仓，避免同类覆盖缺口）：
> ```bash
> for f in $(find crates -name "*.rs" -path "*/src/*" -not -path "*/tests/*"); do
>   T=$(grep -n '#\[cfg(test)\]' "$f" | head -1 | cut -d: -f1); [ -z "$T" ] && T=999999
>   grep -nE '\.borrow(_mut)?\(\)' "$f" | awk -F: -v t=$T '$1<t'
> done
> ```
> 「宿主驱动」= 由宿主（Python/控制面/解析器/泵）**发起**且**可能重入**（JS 回调内再次触发宿主）的路径。

| # | 路径（分类） | 站点（`文件:行`） | 宿主驱动 | 建议替代（`try_borrow` 化的错误呈现） |
|---|---|---|---|---|
| 1 | 导航/文档绑定 | `page.rs:264` | 是（`load` 宿主发起） | `try_borrow().map_err(→ Internal("NodeTree 借用冲突"))` |
| 2 | 输入/派发 | `page.rs:399`、`page.rs:448` | 是（`dispatch`；JS handler 内可重入） | 同上 |
| 3 | 解析器 sink | `page.rs:632`、`637`、`651`、`661` | 是（阻塞脚本就地执行 ⇒ 回调可重入） | 同上 |
| 4 | defer/fetch 泵 | `page.rs:721`、`802`、`935`、`1193`、`1194` | 是（泵在宿主轮内被驱动） | 同上 |
| 5 | Python 边界 | `sdk-python/src/convert.rs:181` | 是（Python↔JS 值转换在宿主调用内） | `try_borrow().map_err(reentrant_pyerr())` |

| 6 | **解析期 `TreeSink` 回调**（**架构师抓出，我初版遗漏**） | `sink.rs:32`、`57`、`147`、`184`、`206`、`224`、`231` | 是（`DomSink` 持 `Rc<RefCell<NodeTree>>`；**阻塞脚本在解析内就地执行 ⇒ 回调可重入**，与 ③ 同族） | `try_borrow().map_err(→ Internal("NodeTree 借用冲突"))` |
| 7 | **stylo 桥 `ElementData` 借用**（**我本轮补扫发现**） | `layout-backend/src/stylo_host/mod.rs:312`、`319`（`ElementSlot::{borrow_data,has_styles}`） | 桥内回调面（stylo 回调期可能叠借；**跨 crate** ⇒ ✅ **架构师定：纳入 `S3c`**（**同 `RefCell<ElementDataWrapper>`**：`:312`/`:319` `borrow()` 与 `ensure_data` 的 `borrow_mut()` **叠加 = panic 面同源**） | `try_borrow().map_err(→ Internal("ElementData 借用冲突"))` |

**计数（修正后）**：产品码 `.borrow()` = **22 处** —— `page.rs` 12 + `sink.rs` **7** + `convert.rs` 1 + `stylo_host/mod.rs` **2**；`binding.rs`/`parser.rs` 产品码 **0 处**（全在 `#[cfg(test)]` 内）。按 7 路径：**① 1 / ② 2 / ③ 4 / ④ 5 / ⑤ 1 / ⑥ 7 / ⑦ 2 = 22**。

### §6-3·补 符号化清单（2026-10-15；**行号在改造中失效 ⇒ 以符号为准**）

> **方法学**（与「全仓扫查 + 可复现命令」同族）：行号随改造漂移（本批 S3a/S3b 后原表行号**已全数失效**），
> 故站点一律以 **「符号（`fn` 名 + 行内特征）」** 为锚。**实现进度**：`S3c1`（`.borrow()`）已全数转换（**①–④⑥ + ⑤**；
> **⑦ 具名上游约束**）＝ commit `e20da58`/`36a28bd`；`S3c2`（`.borrow_mut()`）首站 `input.rs` ＝ `3d91edc`；**`S3c2-b` ＝ `bfdc9e1`**（**`sink.rs` 16 站清零**：`tree_mut()`/`with_tree_mut`）⇒ **`S3c2` 全量收口（`a/b/c/d` ✅）** —— 余 **5 处具名约束**（**`stylo_host` 4 + `opaque` 1**）；**§6-3 符号表进度列 → 全清**。

| 路径 | 符号（`fn` + 行内特征） | `.borrow()` | `.borrow_mut()` |
|---|---|---|---|
| ① 导航/文档绑定 | `BrowsingContext::load`（`Rc::new(NodeTree…)` 后取 `root()`） | ✅ 已转 | ✅（`S3c2-c` 清零） |
| ② 输入/派发 | `BrowsingContext::input_dispatch`（`is_valid`/`in_document` 块）｜`BrowsingContext::user_activation`（只读 getter） | ✅ 已转 | ✅ `input.rs`（`activation.activate()`） |
| ③ 解析 sink | `BrowsingContext::drain_parse`（`get_attribute("src")`／`script_kind`／`text_content` ×2） | 4 处已转 | ✅（`S3c2-c` 清零） |
| ④ defer/fetch 泵 | `BrowsingContext::run_deferred_scripts`（`deferred` 克隆／`tree.text_content`／`deferred.len`）｜`pump_fetch`（`offline.resolve`）｜`PageExec::has_pending_io`（`fetch_queue`/`fetch_table`） | 5 处已转 | ✅（`S3c2-c` 清零） |
| ⑤ Python 边界 | `convert.rs`（`PyJsRef` 分支 `js_ref`） | ✅ 已转（`JsError(ReentrantEval)`） | — |
| ⑥ 解析期 `TreeSink` | `DomSink::new`／`append_text`／`append_based_on_parent_node`／`append_before_sibling`／`add_attrs_if_missing`／`remove_from_parent`／`reparent_children` | ✅ 7 处已转（**(β) 默认值语义各自独立**） | **16 处 ✅ `S3c2-b`（`bfdc9e1`）清零**（`tree_mut()`/`with_tree_mut`） |
| ⑦ stylo 桥 | `ElementSlot::borrow_data`／`has_styles`／`ensure_data`（`ElementDataWrapper`） | **具名上游约束**（无 `try_borrow`；release 走 `unsafe { &*get() }` ⇒ UB、无护栏） | 同上（上游约束） |
| ⑧ 其它宿主面 | `SharedBundle::send`（离线 transport）／`opaque.rs`（`opaque`／`rehydrate` 调用者 = `stylo_host`） | ✅ `send` 已转 | `opaque.rs` 1 = **`S3c2-d` ✅**：`reset()` `(β)` + **`with_side<R>` 泛型约束（与 ⑦ 同族、异站点；具名）** |

**计数（2026-10-15 **清洁实测**）**：`.borrow()` 产品码 **22**（7 路径）；`.borrow_mut()` **38**（`page.rs` **17**／`sink.rs` 16／`input.rs` 1／`opaque.rs` **1**／`stylo_host` 2）—— **原「`page.rs` 18」含 1 条注释幻影 ⇒ 真实 17**（**工程师清洁复算 + 架构师复核一致** ✓）。**进度 = `S3c2` 全量收口（`a/b/c/d` ✅）**；**符号表进度列全清**。

**📋 最终台账（**验收用 · 定稿，2026-10-15）**：**`.borrow()` = 22 = **20 转 + 2（⑦ `stylo_host`）**；**`.borrow_mut()` = 38 = **35 转 + 3 具名约束（⑦ `stylo_host` ×2 + `opaque::with_side<R>` 泛型 R ×1）**；⇒ **可改站点全部转换，具名约束恰 5 处（`stylo_host` 4 + `opaque` 1；非漏改）**。**主理人残留复核**：产品码未转换点 = **恰 5 处**（`parser`/`binding` 命中均在 `cfg(test)` 区）—— **与架构师清洁复核（余 5）一致** ✓。

**⚠️ 计数纪律（方法论族第 4 块 · 架构师源级复核，2026-10-15）**：**「注释 / 字符串非站点」** —— 计数命令须排除注释：`grep -nE '\.borrow_mut\(\)' <f> | grep -vE ': *//'`（否则 **`page.rs:1557`（注释里的 `timers.borrow_mut()`）** 被计为**幻影站**；**架构师首轮扫描即中招** ⇒ **同族「位置/文本脆弱，须语义级验证」**）。
> **方法论族（四条，同根「位置/文本脆弱 ⇒ 须语义级验证」）**：① **全仓扫查**（勿信选定文件清单）② **清单以符号为准**（勿依行号）③ **批量替换不可信**（逐站编译验证）④ **注释/字符串非站点**（计数须排除注释）。
**非 Result fallback 语义（具名）**：**不 panic + `debug_assert!` 可见 + 保守默认**（⑥/只读 getter）；各默认值**不合并**（"跳过属性" ≠ "空表" ≠ "视为有 pending"）。

**伴生族（`.borrow_mut()`，同属决策 3「**宿主驱动路径上的任何 `Rc<RefCell>` 取借一律 `try_borrow`**」）**：产品码 = **39 处（全集）** —— `page.rs` **18** + `sink.rs` **16** + `input.rs` **1**（✅ 已转 `3d91edc`） + `layout-backend/src/opaque.rs` **2** + `stylo_host/mod.rs` **2**（含 `ensure_data`）。**注**：**较首版 35 处 **+4**，系 `S3b`（defer 任务化）新增 `page.rs` 站点**（**架构师源级复算 2026-10-15 = 38 待转 + 1 已转 = 39**）；**位置敏感 ⇒ 以「符号表」（`fn` + 行内特征）为准**。
**伴生族分类（架构师源级核定，2026-10-14；**计数 2026-10-15 修正为 39**）**：`.borrow_mut()` 产品码 **39 处** ——

| 归类 | 站点（产品码） | 处置 |
|---|---|---|
| **宿主驱动面 ⇒ 转换** | `page.rs` **14**（`timers`/`completed`/`writes`/`fetch_table`/`fetch_queue`/`deferred` 等宿主存储）· `sink.rs` **16**（`TreeSink` 回调内 `self.tree.borrow_mut()`，与 ⑥ 同面）· `input.rs` **1**（`activation`）· `stylo_host/mod.rs` **2**（`ensure_data`/`clear_data` —— **与 ⑦ 同 `RefCell<ElementDataWrapper>`**）· **`opaque.rs:86`/`:91`**（`with_side`/`reset` 的**调用者 = `stylo_host`** —— `opaque()` @ `mod.rs:479`、`rehydrate` @ `dom.rs:426` ⇒ **同属宿主驱动面**） | **全部 35 处** |

**✅ 经核，无豁免项**：`.borrow_mut()` **39 处全属宿主驱动面** ⇒ **全部转换**。（`opaque.rs:86`/`:91` 原列**豁免候选**，经核其**调用者在 resolve/匹配面** ⇒ **改判转换**。）

**与测试对账（✅ 架构师定）**：`§3.4 no_host_try_borrow_panic` **由「5 路径」扩为「7 路径 + 伴生族（`borrow_mut` **39 处**）」**（**`borrow` / `borrow_mut` 叠加 = panic 面同源**），按上表逐路径覆盖。

**实施记录（`S3c1` 首批，`e20da58`，2026-10-15）**：
- **⑤ 已转**：`sdk-python/src/convert.rs:181` ⇒ **`try_borrow → JsError(ReentrantEval)`**（**错误域 `JsError` 正确**）✓（架构师源级复核）。
- **⑦ 裁定 = ✅ 上游约束接受（具名登记，非「豁免」）**：**`stylo::data::ElementDataWrapper` 仅 `borrow`/`borrow_mut`、无 `try_borrow`**（`stylo-0.20.0/data.rs:332`/`:344`；**`try_borrow` 计数 = 0** —— **架构师源级复核**）⇒ **桥内无法「可判定错误」化**。**兜底 = ADR-0016 同语句 + 不叠加借**；**未来「持久槽存储」立项时复评**。
  - **⚠️ 架构师补注（风险精确化，已留档）**：`ElementDataWrapper::borrow()` 在 **release = `unsafe { &*self.inner.get() }`（零检查）**、**debug = `refcell.borrow()`（冲突即 panic）** ⇒ **叠借在 release 是 UB、不是 panic** ⇒ 「**不叠加借**」兜底**在 release 无运行时护栏**，靠**纪律与审查**守住。**关系族 = `R18` 硬边界 / `unsafe` 白名单**（**上游 `unsafe` 优化路径**）。**已转工程师**（**背景强化，不改指令**）。
- **⑥ 裁定 = (β) 标准口径（非 `Result` 上下文）**：**`try_borrow + debug_assert + 默认值`**；**有独立可写通道则同时记一条**；**纯读查询用 (α) + `debug_assert`** ⇒ **语义具名登记**（`sink.rs` 回调面）。
- **剩余**：**①–④ + ⑥（19 处）** —— 工程师 GO 中。

**实施记录（`S3c1-b`，`36a28bd`，153 行；`S3c1` 主段收官）**：
- **①–④ 已转**（**12 处 `try_borrow → Internal`**）｜**⑥ 已转**（**7 回调 `(β)`：保守默认 + `debug_assert`**）｜**只读 getter 口径**；**navigation 81/81、零 ignore**（主理人亲验）。
- **📌 方法学发现（入本 `§6-3`）**：**源码行号在改造中失效** ⇒ **枚举类清单应改「符号（`fn` 名 + 行内特征）」**（**与「全仓扫查」同族**：**位置脆弱、符号稳定**）；**符号清单补丁由工程师出 ⇒ 并入本表**。**同族教训族 +1：「**清单以符号为准，勿以行号为准**」**。
- **`S3c2` 已 GO**（**39 处全集（38 待转 + `input` 首站已转）**，宿主驱动面）；**验收测试授权**（**草案先审**；含 **`debug_assert` × 测试构建的张力处理**）。

**实施记录（`S3c2` 首站 `3d91edc`，2026-10-15）**：
- **首站**：`navigation/src/input.rs` ⇒ **`try_borrow_mut → JsError(ReentrantEval)`**（**该文件 `.borrow_mut()` 归零** ✓）。
- **📌 方法学发现（入族）**：**`borrow_mut` 正则批量替换不可靠**（**破坏取值位点 / 括号边界** ⇒ 语法错 / 吞 `impl` 边界）⇒ **全回退、逐站手改、每笔编译即验** ⇒ **教训族 +1：「批量替换不可信，逐站编译验证」**（**与「全仓扫查 / 清单以符号为准」同族**）。
- **验收设计 4 条（✅ 主理人批）**：① **`Result` 具名错误**；② **⑥ `should_panic` / release 文档化**；③ **39 站覆盖**；④ **⑦ 不入测**。**拆笔 `b/c/d` 准**（**`with_tree_mut` helper 模式**）。
- **顺序**：**符号化补丁先行**（工程师出 ⇒ **并入本表**）→ `S3c2-b/c/d`。

**🔎 核可制度产出清单（跨批次，2026-10-15；**架构师源级核可 → 主理人裁**）**：
1. **G3 半覆盖**（片1：**死路径调用点** ⇒ `eval` 从未写入）；
2. **S7 非对称**（值交叉：Python `10/10` / Rust **漏 `single_realm_navigation`** ⇒ 补 1 行对称化）；
3. **G4-`S0a` `CONTRACT` 态**（`assert_behavior_evidence` **覆盖 `2/3`** ⇒ **「`CONTRACT` 附行为标记」未红** ⇒ 已令补第 3 条错标）。
   > **共性**：**三者均非工程师自报，而由「源级核可」流程长出** ⇒ **该制度为实质防线**（沿用 §6-3 初心：**核可 ≠ 复述，而是独立取证**）。
**诚实边界**：站点为**源级枚举**（`grep` 可复现）；「是否宿主驱动」为**分类判断**（依据调用链与重入可能，非机器可判定）；如与架构师口径不符，**以架构师核可为准**。

4. **`§6-4` 与 `S1b` 先后** = **无依赖，`S3` 不等**（**正交**；「持久槽存储」绑增量 restyle 立项）—— §4 风险行已按此改写。
