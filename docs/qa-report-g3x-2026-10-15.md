# G3X 收尾批 · 独立 QA 增量复核报告（2026-10-15）

> **范围**：G3「收尾批（G3X，S1a→S8 + 微笔）」相对 G3 验收轮的**增量独立复核**。仓库 HEAD：`6d77d16`。
> 复核人：`software-qa-engineer`（独立；**全部亲手复跑**）。纪律：只读产品码、每条 INJ **按文件恢复**、注入态未 commit。
> 「远程 / Linux 复验」仍按维护者 local-first 策略**搁置**，不在本结论内。

## 1. 门禁表（复跑值 vs 期望/基线）

| # | 门禁 | 复跑结果 | 期望 | 判定 |
|---|------|----------|------|------|
| 1 | `cargo fmt --all --check` | exit 0，无 diff | clean | ✅ |
| 2 | `cargo clippy --all-targets --all-features -- -D warnings` | exit 0，0 lint 警告 | clean | ✅ |
| 3 | `cargo test --workspace` | exit 0，各 crate 计数见下 | 逐 crate | ✅ |
| 4 | `cargo deny check` | exit 0：advisories/bans/licenses/sources 均 ok | clean | ✅ |
| 5 | `scripts/check-unsafe-whitelist.sh` | exit 0，`== PASS ==` | clean | ✅ |
| 6 | `cd contracts && uv run pytest` | exit 0，**60 passed** | 60 | ✅ |
| 7 | `scripts/check-sdk-python-env.sh` | exit 0，**26 passed** | 26 | ✅ |
| 8 | `cargo llvm-cov --workspace --summary-only` | **84.17% Regions / 82.40% Lines**（3183/20105 R、1959/11129 L） | ≥ 基线 83.87R/82.10L | ✅ 上升 |

**第 3 项逐 crate 计数对照**：

| crate/二进制 | 复跑 | 期望 | | crate/二进制 | 复跑 | 期望 |
|---|---|---|---|---|---|---|
| navigation（lib） | **85** | 85 ✅ | | web-dom | 19 | 19 ✅ |
| navigation `tests/vertical_g2` | 1 | 1 ✅ | | web-style | 7 | 7 ✅ |
| navigation `tests/vertical_g3` | 1 | 1 ✅ | | web-events | 9 | 9 ✅ |
| layout-backend | **51** | 51 ✅ | | scheduler | 11 | 11 ✅ |
| engine-v8 | 52 | 52 ✅ | | network | 6 | 6 ✅ |
| （另）engine-v8 `tests/vertical` | 1 | — | | policy | 3 | 3 ✅ |

> 额外项（非异常）：runner `skeleton_smoke` 4 + `workspace_policy` 8、sdk-python lib 1。
> 覆盖率较 G3（83.87R/82.10L）**上升** ⇒ 收尾批新增测试带来真覆盖，无回退。

## 2. INJ 表（注入→红→按文件恢复→绿）

| INJ | 注入（最小改法） | 结果 | 原文摘 |
|-----|------------------|------|--------|
| ① 撤 timer 轮预算 | `page.rs` `RunBudget::tasks(TIMER_TASKS_PER_ROUND)` → `tasks(0)`（不限） | **红** ✅ | `timer_fires_after_n_rounds` FAILED：`16/轮预算 ⇒ 17 个任务应在第 2 轮收束（实测 1）` |
| ② 撤内联递归 | `box_tree.rs` `append_children` 内联分支删 `self.append_children(child…)` | **红** ✅ | `dynamic_shadow_relayout` FAILED：`动态影子内容应有几何: PageError{… "节点无几何（未布局）"}` |
| ③ 撤影子通道 | `stylo_host/dom.rs` `TShadowRoot::style_data()` → `None` | **红** ✅ | `shadow_scope_channels_sentinel` FAILED：`哨兵①：:host 通道（P-11）回归` |
| ④ 非标签 token 走 defer | (a) 删守卫 `if raw & DEFER_TOKEN_TAG == 0 { return Done }`；(b) 强制 `index = 0` | **仍绿** ❌ | 两法 `non_defer_message_token_is_not_handled_as_defer` 均 **ok**（判别力不成立，见 §4） |

**复原纪律**：每条 INJ 后 `git diff --name-only` 为空、对应测试复绿；全部完成后 `git status` 干净、`cargo fmt --check` exit 0；注入态**从未 commit**。

## 3. 夹具真实性 / 形状多样性 / 忽略核

- **S5b `<slot>` 夹具**：**在位** ✅ — `slotted_selector_styles_slotted` 与 `shadow_scope_channels_sentinel` 均含 `<slot name="s">`（`style_system.rs:385/459`），符合 `::slotted` 须经 `assigned_slot()` 沿槽链上溯的前提。
- **S6 内联宿主夹具（非块级）**：**在位** ✅ — `dynamic_shadow_relayout` 用 `<x-h>`（`page.rs:2250`）；内嵌 UA 表（`assets/ua.css`）仅覆盖 `html/body/div/p/h1/ul/li/span` ⇒ 自定义元素回落 initial `display:inline` ⇒ **确走内联分支**（INJ-② 实测证：撤内联下钻即红）。形状多样（块级 `div` 与内联 `x-h` 各有覆盖）成立。
- **`#[ignore]`（测试代码）= 0** ✅ — 全仓 `#[ignore` 匹配仅出现在 docs 散文（非测试属性）。
- **`COLLECTION_FLOOR = 60`** ✅ — `contracts/conftest.py:21` 在位（G3 轮已实证该哨兵有效）。

## 4. 发现清单

- **阻断（Blocker）**：无。
- **重要（Major）**：**INJ-④ 判别力不成立 —— 测试缺陷（弱断言）**。
  `non_defer_message_token_is_not_handled_as_defer`（`navigation/src/page.rs:2538`）**无法**被「非标签 token 误走 defer 分支」这一故障变红：两种最小注入（撤令牌守卫 / 强制 `index=0` 重跑 defer）**均绿**。
  **根因**：该测试的判据观测量 `__ran2` **从不被**被 defer 脚本触及（脚本只写 `__ran = 1`），且误路由/重执行**不产生 error** ⇒ `__ran2==0` 与 `timer_errors` 空**恒成立** ⇒ 故障不可见。属「弱门禁」（声称覆盖却无判别力），与铁律「弱门禁比没门禁更危险」相悖。
  > 注：此结论**非**「实现错误」——是**测试自身无判别力**；产品代码在正确路径下行为正确（其余 3 条 INJ 均真判别）。
- **轻微（Minor）**：无（其余面持平或上升）。

## 5. 智能路由（Routing Decision）

- **无源码缺陷**：INJ-①②③ 均证明关键路径（timer 预算 / 内联下钻 / 影子级联通道）**真判别**；未发现实现错误 ⇒ **不转工程师修源码**。
- **INJ-④ = 测试缺陷** → 虽属「测试缺陷可自修」范畴，但该测试为**验收套件（committed 产品码）**且本轮任务限定「只读产品码」 ⇒ **不自改**，报主理人转工程师**加固**。
  **加固方向（建议）**：令观测量对「误路由」敏感 —— 如把该测试的 defer 脚本改为写 `__ran2`（误路由 ⇒ `__ran2==1` ⇒ 红），或增设 defer 执行计数哨兵，使「非标签 token 不得触发 defer」**可判别**。

## 6. 结论

> **G3X 收尾批增量验收 = PASS（带 1 项测试加固待办）**：8 项门禁全绿且逐项吻合期望，覆盖率**上升**（84.17R/82.40L）；INJ 精选 4 条中 **3 条判别力成立**、1 条（INJ-④）**判别力缺口**；夹具形状多样性与忽略核全部核实。无源码缺陷、无门禁回退。

## 7. 附页 · INJ-④ 微复核（2026-10-15，加固提交 `4da3e7e`）

> **微复核结论：INJ-④ 判别力缺口已闭合（复核 PASS）。** 工程师加固（`4da3e7e`）后，独立复跑 **注入（撤 `raw & DEFER_TOKEN_TAG == 0` 守卫）⇒ 红**（`non_defer_message_token_is_not_handled_as_defer` FAILED：`判别力锚：误路由 ⇒ 2；实得 Number(2.0)`）→ **按文件 `git restore` 复原 ⇒ 绿**（1 passed）；注入态未 commit，`git diff` 空、工作树干净。加固修法（defer 脚本写自增计数 `__defer_runs` + 注入 token `4→0` 使误路由**必**重跑 `deferred[0]`）经复核**有效**——原缺口（观测量恒不变）已消除，弱门禁转为真判别。
