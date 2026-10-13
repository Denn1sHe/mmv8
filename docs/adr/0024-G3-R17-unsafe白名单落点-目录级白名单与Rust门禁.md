# ADR-0024：G3 R17 —— unsafe 白名单落点：**E′ 目录级白名单 + Rust 门禁**（「E′+R」合成）

- 状态：**已接受**（2026-10-13 主理人汇总裁决；两评定向辩论 ⊕ 交叉独立实复现）
- 日期：2026-10-13
- **批准记录**：维护者 **2026-10-13 18:10** 授权（原文：「**批准，后续有问题我都批准。直接推进开发到 g6**」）。
  守卫脚本**数据行**（受保护面）按本 ADR 决策 2 **已落地**（`scripts/check-unsafe-whitelist.sh` 的 `WHITELIST`
  追加 `"crates/layout-backend/src/stylo_host/"`，**零 bash 逻辑改动**）。`AGENTS.md` **维持零改动**（铁律 4
  原文已含「显式声明的 FFI 薄桥」，本次豁免即按此**显式声明**落本 ADR）。
  **后续受保护面改动照此预授权模式执行，但仍全量 ADR/台账记录**。
- 关联：
  - **ADR-0002**（unsafe 白名单最终形态）—— 本 ADR 为其**修正注记**（白名单由「唯一 `engine-v8`」扩为两支）
  - **ADR-0020**（G3-D1 路线 + opaque 策略）—— 本 ADR 为其 S1/P-1 阻断的**落点裁决**
  - **ADR-0009**（V8 类型边界 / 不新增 workspace 成员）—— 本 ADR **不**新增成员（B 案被否）
  - **ADR-0021**（锁版 / 上游升级流程）—— 决策 8「上游升级回收条款」引用之
  - 施工图 `docs/design-g3-layout-2026-10-13.md` §1 / §6 R17 / §7 P-1b / §8
  - **决策 brief**：`docs/decision-briefs/2026-10-13-G3-unsafe-whitelist-layout-backend.md`
  - **评审意见**：`docs/decision-briefs/2026-10-13-G3-R17-辩论意见-A.md`（E′）、`-G3-R17-辩论意见-B.md`（F = Rust 门禁）
  - **裁决书**：`docs/decision-briefs/2026-10-13-G3-R17-裁决-主理人.md`（含 §六 v1.1）
  - **落档草案包**：`docs/decision-briefs/2026-10-13-G3-R17-落档草案包.md`
- 决策者：主理人
- 落地步骤：施工图 **S6**（解门后）；门禁扩 `crates/runner/tests/workspace_policy.rs`

---

## 问题

P-1 实测：`style::dom::TElement` 有 **5 个必需 `unsafe fn`**（`stylo-0.20.0/dom.rs:648/675/680/739/744`，无默认），
而全仓 `unsafe_code = "forbid"`（`Cargo.toml:67`，**不可被 `#[allow]` 覆盖**）⇒ 承载 `TElement` 实现的 crate
**不可能零 unsafe**。需拍板白名单**落点形态**：谁承载 impl、豁免粒度、如何用**机器门禁**保证「unsafe 面不蔓延」。

**实测信封（P-1b，终值）**：**「5 声明 + 0 腔体」**——`ensure_data`/`clear_data` **无需真 `unsafe` 腔体**
（急切持有 `ElementDataWrapper` + 安全 `borrow_mut()`/`DerefMut` 写 `ElementData::default()`；commit `407c0aa`）。

## 讨论要点（两评合成，分歧如实记录）

| 维度 | 评审 A（证伪） | 评审 B（从零） | 主理人裁决 |
|---|---|---|---|
| 白名单形态 | **E′**（目录级条目，零脚本改动） | — | **采纳 E′** |
| 门禁 | — | **F = A + Rust 门禁** | **采纳 R**（替代 bash 计数规则） |
| A（整 crate） | 被 E′ 支配 | 作 F 基础 | **不单采**（crate 条目被目录条目替代） |
| E 草案（按文件） | 前缀误放行 + `[ -d ]` 跳过规则 3/4 | 同判 | **否决原样**（诉求被 E′ 覆盖） |
| ②（deny+allow 无白名单） | 需守卫新分支、无先例 | — | **否决** |
| B（薄桥 crate） | 作用有限 | 唯一保 `forbid` | **不采纳**（impl 不「薄」+ 重开 ADR-0009 冻结名单）；**留维护者覆写口**；**翻盘条件见决策 9** |
| **E′ 的反对（B）** | — | E 的文本最小化**净增益≈0**（编译器 `deny` 已强制逐项 `#[allow]`；宏生成的 unsafe 编译器**亦命中**） | **如实记录**；**保留 E′ 的残余理由 = tamper 场景**（见决策 2） |
| bash 执行器逻辑改动（`-d→-e`/`-H`） | 不做 | 不做 | **一致：不做**（风险不对称；增益被编译器 + R 覆盖） |

**关键实证（两评独立复现，裁决基石）**：`deny` + **函数级** `#[allow]` 可编译；删一个 `#[allow]` ⇒ **编译红**；
裸 `unsafe {}` 在 `deny` 下 ⇒ **编译红** ⇒ 编译器已强制「逐处挂豁免」；`impl` 级 `#[allow]` 也放行 ⇒
**守卫规则 4 是唯一阻止「整块开口」的防线**（载荷关键，不得削弱）。

## 决策

### 决策 1 —— **`layout-backend` 本地 lints（`forbid → deny`，engine-v8 同款）**

`crates/layout-backend/Cargo.toml`：`[lints] workspace = true` → **本地声明**，等价 lint 集合，仅
`unsafe_code` 由 `forbid` 放宽为 `deny`（`missing_docs`/`clippy` 与 workspace **逐项相同**）。
理由：`unsafe_code` 是 **crate 级** lint，`forbid` 不可被 `#[allow]` 覆盖 ⇒ 承载 impl 的 crate 必降 `deny`。

### 决策 2 —— **E′：守卫白名单「数据行」追加尾斜杠目录**（零 bash 逻辑改动；**已落地**）

```diff
 WHITELIST=(
   "crates/engine-v8/"
   "poc/"
+  "crates/layout-backend/src/stylo_host/"
 )
```

- 尾斜杠目录条目 ⇒ `[ -d "$prefix" ]`=true ⇒ **规则 3/4 自然生效**（SAFETY/开口检查覆盖该目录）；
  `grep -RInE ... <目录>` 自带 `文件:行:` 前缀 ⇒ **不需要 `-H`**；
- **非桥文件仍被规则 1 文本拦**（`layout-backend` 其余 `src/**` 不在白名单）；
- **规则 5 对该目录失效**（白名单条目使 `is_whitelisted` 豁免）⇒ 由 **门禁 d** 补位（决策 3）。
- **如实记录 B 的反对**：E′ 的文本最小化**净增益≈0**（编译器 `deny` 已强制逐项 allow；宏生成 unsafe 编译器亦命中）。
- **保留 E′ 的残余理由（tamper 场景）**：当**非受保护的 Rust 门禁与违规改动同 PR 被削弱**时，E′ 下
  **受保护的 bash 规则 1**（`CODEOWNERS` 保护、需批准）**仍拦非桥文件**；而 crate 级条目则全程依赖
  可被同 PR 修改的 Rust 断言。⇒ 维持子目录条目（代价≈0；未来新增目录 = 再加一条白名单项，本属铁律 6 审批范围）。

### 决策 3 —— **R：新增 Rust 门禁断言 a/b/c/d**（机器保证，替代 bash 计数规则）

扩 `crates/runner/tests/workspace_policy.rs`（或同文件新断言）：
- **a**：`src/stylo_host/**` 的 `unsafe fn` **名集合恰 == 上游 5 名**，且**计数 == 5**；
- **b**：`layout-backend/src/**` 除 5 具名方法（含其体内 **≤ N** 处 `unsafe` 块）外，**不得出现任何其他
  `unsafe` 文本**（`unsafe {` / `unsafe impl` / `unsafe trait` / 其他 `unsafe fn`）；
  **阈值 P-1b 已回填：`N = 0`（终值，不上调）**——依据见上「实测信封」与门禁草案注释；
- **c**：`layout-backend` lint 副本**逐项 == workspace**（防漂移，engine-v8 同款；**逐项全等**，非「等价集」）；
- **d**：`layout-backend` 文本**无 `v8::` / `extern crate v8`**（**规则 5 的等效补位**——白名单条目使规则 5 对该目录失效）。

### 决策 4 —— **`workspace_policy.rs` 既有断言修正**（两评独立互证的必做项）

`WHITELISTED_CRATE`（单值 `:19`）→ `WHITELISTED_CRATES`（**集合**）；`only_whitelisted_crate_opts_out_of_workspace_lints`
（`:230-252`）的豁免分支覆盖 `layout-backend`；`engine_v8_lint_block_matches_workspace_except_unsafe_code`
（**含 `:224-227` 逐项全等**）**泛化**为「任一台白名单 crate lint 副本 == workspace（除 `unsafe_code`）」。
（B 案原触的 `workspace_members_equal_scaffold_roster` **不在本裁决范围**——本 ADR 不新增成员。）

### 决策 5 —— **反向注入四态（强制）**

① 删一个 `#[allow]` ⇒ **编译红**；② 非桥文件加 `unsafe {}` ⇒ **门禁 b 红**；③ 恢复 ⇒ **绿**；④ 清理 ⇒ **绿**。

### 决策 6 —— `cargo geiger` 本轮排入 + 留档基线

`cargo geiger` 运行并**留档 unsafe 计数基线**（文本盲区兜底 + 趋势；对齐 ADR-0002 既有 geiger 机制）。

### 决策 7 —— **CI 执行性核验**

`ci.yml` **实际执行**二者（**架构师已核**：lint job 执行 `bash scripts/check-unsafe-whitelist.sh`；
test job `cargo test --workspace` 覆盖 `crates/runner/tests/`）⇒ **无需补**。

### 决策 8 —— 上游升级回收条款

stylo 系升级（按 ADR-0021 流程）时**强制复核 5 名集合与 SAFETY 依据**；集合或语义变更须**重落 ADR**。
上游若把 `unsafe fn` 改安全签名或提供安全 `clear` ⇒ **主动回收**本豁免。

### 决策 9 —— **翻盘条件（登记）**

> **S6 定形若证「unsafe 面 > 2 处且跨多文件」** ⇒ 按「**unsafe 面 × 文件数**」**重评薄桥 crate（B）**。
> 依据：B 是**唯一保持编译器级 `forbid`** 的形态（连 `#[allow]` 都报 E0453）；其价值**随该乘积增长**；
> **本信封下（5 声明 + 0 腔体、集中单目录）不划算**，故本轮否。

## 被否决方案（记录）

| 方案 | 否决理由 |
|---|---|
| **②**（deny + allow，无白名单条目） | 守卫需新增「例外 crate + 仅允许带 allow 的项」分支（无先例、复杂度最高）；文本层 `#[allow]`↔`unsafe fn` 配对极脆 |
| **B（薄桥 crate）** | 孤儿规则（E0117）迫使桥内自建 `NodeRef`/newtype ⇒ 须承担全 `TElement/TNode/selectors::Element` 面 ⇒「极薄」**不可达**；且重开 ADR-0009 冻结名单（19→20 + §16 解冻）。**留维护者覆写口**（若最重「编译器级 `forbid`」）；**翻盘条件见决策 9** |
| **E 草案（按文件）** | 前缀匹配**误放行** `.rs_evil.rs`；单文件 `-R` 无文件名前缀 ⇒ 需加 `-H`；`[ -d ]` 使文件条目**跳过规则 3/4** ⇒ 须改 bash 逻辑（与「不动执行器」冲突）；诉求被 E′ 覆盖 |
| **A（整 crate 条目）** | 被 E′ **完全支配**（同 `[lints]` 改动、同不改脚本；A 让**整个大 crate** 豁免、无补偿收益） |
| **不实现 `TElement`** | 级联不可行 ⇒ 与 ADR-0011 冲突 |

## 后果

### 正面
- 白名单为「`engine-v8` + 显式声明的 **stylo FFI 薄桥（目录 `src/stylo_host/`）**」两支；unsafe 面**机器可证**（a/b/c/d）；
- **零 bash 执行器改动**（守卫只多一行数据）；桥以**目录模块**承载，「薄桥」语义落地；
- 不新增 workspace 成员（不触 ADR-0009）。

### 代价（如实登记）
- `layout-backend` crate 级 `deny` ⇒ 编译器不再对**该 crate** 提供「`forbid` 级」保证（但门禁 b 补文本面）；白名单由「唯一」变两支。
- 守卫**规则 5** 对桥目录失效 ⇒ 依赖 **门禁 d** 补位（若 d 未落，则 `v8::` 可在桥目录逃逸）。

### 待实测登记
| # | 待实测项 | 落点 |
|---|---|---|
| T1 | 门禁 a/b/c/d 四断言可编译且四态判别 | S6 解门后 |
| T2 | 体内 `unsafe` 块上限 **N = 0（终值）**（P-1b 已回填） | P-1b ✅ |

## 对 ADR-0002 的更正注记
1. 白名单由「**唯一** `engine-v8`」→「**两支**：`engine-v8`（V8 FFI 薄桥）+ **`layout-backend/src/stylo_host/`**
   （显式声明的 **stylo FFI 薄桥**，本 ADR-0024）」。
2. 「白名单必须**逐块**开口、禁 crate/mod 级开口」**不变**；新增 Rust 门禁 a/b/c/d 作为**机器执行**。

## 对 ADR-0020 的更正注记
- 决策 2 的「**零 unsafe 为准**」**落点已定**（本 ADR）：opaque 机制零 unsafe（已验证）；`TElement` 5 名
  `unsafe fn` + **0 体内块（P-1b 终值）**落 **`layout-backend/src/stylo_host/`** 目录模块，`layout-backend`
  本地 `deny` + 逐 `fn` `#[allow]` + `// SAFETY:`；**S6 门随批准解**。

## 交叉引用
- 白名单形态 / 门禁 → **ADR-0002**（本 ADR 修正注记）；路线 / opaque → **ADR-0020**（本 ADR 落点）
- 锁版 / 升级回收 → **ADR-0021**；工具链 → **ADR-0022**
