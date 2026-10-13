# 门禁报告 —— 2026-10-13 第六轮（G2 页面纵向验证）

- 产出角色：QA（严过关）
- 事实源：`docs/TASKS.md`（本报告是过程与判定记录，**状态以 `docs/TASKS.md` 为准**）；验收主契约 = `docs/design-g2-page-2026-10-13.md` §3.1~§3.9
- 运行机：本地 macOS arm64（`rustc 1.92.0`）+ 远程 `ubuntu@124.221.1.78`（Ubuntu 24.04 · x86_64 · `rustc 1.99.0` · 3.6 GB 内存 · 4 核）
- 本轮范围：**独立验证**（本地复跑 + 反向验证抽样 + 铁律复核）+ **覆盖率测量** + **远程 Linux x86_64 门禁**
- QA 纪律：**不改功能代码**；**不为迁就实现修改断言 / golden**；一切结论附**证据原文**；发现与已登记项不符即停下报告
- 证据原文归档：`.workbuddy/refs/g2-qa-evidence/`（本地各步日志、反向验证输出、覆盖率、守卫注入原文；远程日志在 `remote/` 子目录）
- **Round 2（修复后复验）已完成**：D1~D6 **全部闭环**（详见 §11）；双端全量回归 + 覆盖率复测全绿。**Round 1 记录（§0~§10）保留为过程快照；最终判定见 §11。**

---

## 0.0 Round 2 最终结论（TL;DR）

| 项 | 结果 |
|---|---|
| 缺陷闭环 | ✅ **D1~D6 全部闭环**（D1 远程 clippy 转绿；D2 外链脚本实现+测试；D3/D4/D6 新测试；D5 判别力反向验证通过） |
| 本地 Rust 门禁 | ✅ **5/5 绿**：fmt **0** / clippy **0** / test **0**（**158** tests）/ guard **0** / deny **0** |
| 本地 Python 侧 | contracts **60 passed**；sdk-python **22 passed** |
| 远程 Linux x86_64 | ✅ **workspace 158 / contracts 60 / sdk-python 22 / fmt 0 / guard 0 / clippy 0（1.99，D1 已消）** |
| 覆盖率 | TOTAL **83.47% R / 82.66% L**（较 R1 微降 0.32pp/0.10pp，因新增代码行；盲区 `sink.rs` 46.29% L 不变） |
| D5 判别力 | ✅ 反向注入（`pop_back`→`pop_front`）⇒ 强化后两测试**必红**（`["…","b","i"]` / `["…","b","a"]`）⇒ 还原复绿、sha256 一致 |
| 是否可进入下一闸门 | ✅ **可以进入下一闸门**（双端全绿；剩余均为**已登记项**，见 §8/§11） |

---

## 0. Round 1 结论（TL;DR）——【Round 1 快照，保留】

| 项 | 结果 |
|---|---|
| §3.1~§3.9 测试集 | **绝大多数通过**；但 **5 项契约条目**为「未验证 / 测试不判别」（见 §7 缺陷清单 D2~D6）——**非全绿** |
| 本地 Rust 门禁 | ✅ **5/5 绿**：fmt **0** / clippy **0** / test **0**（153 tests）/ guard **0** / **deny 0**（`advisories ok, bans ok, licenses ok, sources ok`） |
| 本地 Python 侧 | contracts **60 passed**（0）；sdk-python **22 passed**（0） |
| 本地反向验证 | ✅ 全部符合预期：guard INJ-1 红 / INJ-2 绿；`v8_dependency_only_in_engine_v8` 四态 绿/红/绿/绿；INJ-3 红；U-1 探针（0.23 + 0.29）三态复现；注入后**已还原且 sha256 与备份一致** |
| Rust 覆盖率（llvm-cov） | TOTAL **83.79% regions / 82.76% lines**（与工程师报告一致）；`navigation/sink.rs` **46.29% L** 盲区如实登记 |
| 远程 Linux x86_64 门禁 | ⚠️ **workspace test 0（153）/ contracts 60 / sdk-python 22 / fmt 0 / guard 0 全绿**；**`cargo clippy -D warnings` FAIL（exit 101）**——1.99 新增 lint `clippy::assert_is_empty`（`web-dom/src/node.rs:170`） |
| 是否可进入下一闸门 | ⚠️ **有条件——需先闭环 §7 缺陷**（本地/远程 clippy 跨版本红 + 5 项契约条目）；均为**可定点修复**，无架构级阻断 |

**一句话**：**功能实现主体扎实、本地全绿、覆盖率达标、反向验证可信**；但发现 **1 项远程门禁红（1.99 新 lint）** 与 **5 项 §3.x 契约条目未验证/测试不判别**，需路由修复后再判「可进入下一闸门」。

---

## 1. 本地门禁（逐条：命令 / 退出码 / 原文）

| # | 命令 | 退出码 | 关键原文 |
|---|---|---|---|
| 1 | `cargo fmt --all --check` | **0** | （无输出） |
| 2 | `cargo clippy --all-targets --all-features -- -D warnings` | **0** | `Finished \`dev\` profile … in 1.12s`（无 warning） |
| 3 | `cargo test --workspace` | **0** | **153 passed**；0 处 `FAILED`（套件级：engine-v8 52 / vertical 1 / navigation 46 / vertical_g2 1 / network 6 / policy 3 / scheduler 11 / web-dom 12 / web-events 9 / web-style 2 / runner skeleton 4 + workspace_policy 5 / iv8 1） |
| 4 | `cargo deny check` | **0** | `advisories ok, bans ok, licenses ok, sources ok` |
| 5 | `bash scripts/check-unsafe-whitelist.sh` | **0** | `== PASS：白名单外无 unsafe；无 crate/mod 级开口；白名单内 unsafe 均带 SAFETY 注释；白名单外无 v8 路径引用 ==` |

> 证据：`.workbuddy/refs/g2-qa-evidence/local-workspace-test.txt`、`local-five-gates.txt`、`local-clippy.txt`。

## 2. 本地 Python 侧

```
$ cd contracts && uv run pytest
............................................................             [100%]
60 passed in 0.55s
# $ cd crates/sdk-python && .venv/bin/python -m pytest
# tests/test_iv8.py .............  tests/test_page.py .........  →  22 passed in 0.30s
```

证据：`local-contracts-pytest.txt`、`local-sdk-python-pytest.txt`。

---

## 3. §3.1~§3.9 逐项独立验证（读测试源码 → 复跑 → 抽查关键断言）

> 图例：✅ = 契约测试存在且断言实质；◻ = 契约测试**缺席或断言不判别**（见 §7）。

### 3.1 流式解析（✅ 全绿）
| 契约测试 | 实况 | 抽查断言 |
|---|---|---|
| `streaming_parse_incremental_nodes` | ✅ `parser.rs:127` | 未喂完即产出节点 |
| `parser_handles_split_tags_across_chunks` | ✅ `parser.rs:137` | `<di`+`v>` 拼成 `<div>` |
| `tree_building_builds_expected_shape` | ✅ `parser.rs:150` | html/head/body/p 按规范补全 |
| `sink_maps_element_text_comment` | ✅ `parser.rs:166` | Comment/Text 子节点 |

### 3.2 阻塞脚本（⚠️ 有缺口）
| 契约测试 | 实况 | 备注 |
|---|---|---|
| `blocking_script_pauses_parser` | ✅ `parser.rs:182` | P-2 全栈：`feed` ⇒ `BlockedOnScript(node)`，暂停点携带 `<script>` 元素本身 |
| `script_executes_then_resumes` | ◻ 无同名 | 行为由纵向样例（第二 `<script>` 读到第一 `<script>` 的产物）+ `document_write_parses_as_html_js_path` 间接覆盖 |
| `inline_script_runs_in_page_scope` | ◻ 无同名 | G1 `test_iv8.py` 覆盖隔离；G2 `inline_script_executes_during_load`（`page.rs:1114`）在页面 scope 执行 |
| `external_script_blocks_until_loaded` | ◻ **无测试且未实现**（D2） | `script_kind`（`page.rs:779`）只判 `type`/`defer`，**无 `src` 分支**；`<script src>` 被当 classic 空执行，**不登记 load blocker、不 fetch** |
| `script_interruptible_by_deadline` | ✅ `page.rs:1571` | 死循环经 `eval_with_deadline` 截断 ⇒ `PageErrorKind::Terminated` |
| 模块三序 `module_script_deferred_and_executes` / `classic_defer_module_domcontentloaded_order` / `module_top_level_await_unsupported` | ✅ `page.rs:1475/1491/1519` | `inline,defer,module,dcl` 逐字断言；TLA 具名错误 |
| `dynamic_import_resolves_namespace` / `module_unsynced_specifier_fails_deterministically` | ✅ 等价 | 由 `dynamic_import_specifier`（`realm.rs:1744`）+ `dynamic_import_unresolvable_rejects_without_fatal`（`:1761`）覆盖 |

### 3.3 `document.write`（⚠️ 保序测试不判别）
| 契约测试 | 实况 | 备注 |
|---|---|---|
| `document_write_inserted_at_script_point` | ◻ 无同名 | 由 `document_write_injection_order`（`parser.rs:205`）+ `document_write_parses_as_html_js_path`（`page.rs:1534`）部分覆盖 |
| `document_write_parses_as_html` | ✅ 等价 | `document_write_parses_as_html_js_path`：注入 `<u>` 入树 |
| `document_write_after_load_defined` | ✅ `document_write.rs:105`（ledger）+ `page.rs:1551`（JS 路径，断言消息含 `DocumentWriteAfterLoad`） |
| `document_write_multiple_in_order` | ◻ **不判别**（D6） | `document_write.rs:94` 只断言账本 `Vec` 序；`parser.rs:205` 只断言两节点**均存在**、**未断言注入序** ⇒ 原「倒置」bug 也会通过 |

### 3.4 导航（✅ 全绿）
| 契约测试 | 实况 |
|---|---|
| `navigation_bumps_epoch` ✅ `controller.rs:196`；`stale_epoch_task_dropped` ✅ `:209`（代次原语；timer/input 同挂代次为设计登记，未逐类端到端） |
| `stale_document_node_rejected` + `node_id_not_aliased_across_navigation` ✅ `page.rs:1097`（合并测试 `node_id_not_aliased_and_stale_document_rejected`：旧文档根 id 不别名 + 旧树 `is_valid==false`） |
| `ready_state_transitions` ✅ `:221`；`load_waits_for_blockers` ✅ `:233`；`origin_computed_from_url` ✅ `:245`（default-port 折叠 + 端口 8443 断言 + 非法 URL） |

### 3.5 资源等待（⚠️ 有缺口）
| 契约测试 | 实况 |
|---|---|
| `fetch_offline_hit_is_ready` ✅ `page.rs:1142`；`fetch_miss_is_pending` ✅ `:1169`（`WaitingIo`+pending 1）；`network_complete_resolves_promise` ✅ `:1183`；`netlog_records_request_and_response` ✅ `network/service.rs` |
| `complete_is_idempotent_and_stale_rejected` ✅ `service.rs:250`；`fetch_aborted_on_navigation` ◻ **无页面级测试**（D5，仅 `abort_epoch_only_aborts_matching_epoch` `service.rs:285` 覆盖网络原语） |
| `run_until_respects_max_tasks` ✅（`run_until_budget`）；`run_until_respects_deadline` ✅（`run_until_deadline`）；`run_until_returns_waiting_io_when_parked` ✅ `runloop.rs:325`；`logical_clock_fires_timer` ✅ `runloop.rs:392`；`task_source_cross_order_defined` ✅ `runloop.rs:371` |

### 3.6 输入（✅ 全绿，1 项由原语覆盖）
`trusted_click_dispatches_to_target`（`page.rs:1310`，`isTrusted===true`）/ `listener_phase_order_capture_target_bubble`（`:1333`，逐字序）/ `listener_order_at_target_mixed_registration`（`web-events/dispatch.rs`）/ `untrusted_event_not_trusted`（`:1371`，`new Event('click').isTrusted===false`）/ `trusted_activation_sets_user_activation`（`:1383`，sticky=true & transient 不置位 & activeElement 聚焦）/ `once_listener_removed_after_fire`（`:1407`）/ `listener_removed_during_dispatch_not_invoked` / `listener_added_during_dispatch_not_invoked`（`dispatch.rs`）/ `duplicate_add_is_noop` / `remove_ignores_passive_once`（`web-events/target.rs`）/ `explicit_target_must_belong_to_document`（`:1451`，游离 + 无 target 双断言）。**`listener_collected_before_call_skipped`** 无同名，由 `call_weak_atomic`+`resolve_weak_collected`（`realm.rs`）覆盖原语。

### 3.7 DOM 内核最小面（⚠️ 2 项缺席）
`js_create_element_and_append` ✅（`binding.rs:656`）；`js_set_classname_and_style` ✅（`:668`）；`js_get_element_by_id` ✅（`:695`）；`node_wrapper_identity_stable` ✅（`:717`）；`dom_wrapper_not_a_cycle_leak` ✅（`:733`，只证弱路径）；`nodeid_generation_reuse` ✅（`tree.rs:421`）；`mutation_observer_delivered_at_checkpoint` ✅（`page.rs:1221`）。
**`node_wrapper_recreated_after_collection`** ◻ 缺席（D3，机制 `bind_force` 已落 `binding.rs:402`）；**`refcell_reentry_is_error_not_panic`** ◻ 缺席（D4，`try_borrow` 结构性规则未落地）。

### 3.8 Python 控制面（✅ 全绿，8/8）
`test_page.py`：`page_load_streams_snapshot` / `run_until_returns_outcome` / `network_pending_and_complete_roundtrip` / `input_dispatch_click_from_python` / `capabilities_reports_g2_surface`（含 `geometry:"stub"`/`input:"explicit_target"`/`script_interruptible:true`/`single_realm_navigation:true`）/ `page_geometry_is_documented_stub` / `py_control_reentry_is_error`（`ReentrantEval` 非 panic）/ `close_after_navigation_idempotent`。

### 3.9 纵向样例（✅ 全绿）
`vertical_page_blocking_write_fetch_observer_click`（`navigation/tests/vertical_g2.rs`，逐段断言 `wrote,styled,fetched,clicked`）；`test_vertical_page_sample`（`sdk-python/tests/test_page.py`）。**偏差**：契约文字含「导航」段，实际纵向链路以 `close` 幂等收尾（导航由 §3.4/`test_close_after_navigation_idempotent` 另测）。

---

## 4. 反向验证（独立复跑；注入后已还原，sha256 与备份一致）

| 项 | 期望 | 实测（原文） |
|---|---|---|
| guard 基线 | 绿 | `== PASS…==`（exit 0） |
| INJ-1（非白名单注入 `v8::Local`） | 红 | `VIOLATION[v8-type-escape] …:2: use v8::Local as _;` exit 1 |
| INJ-2（放松白名单 + `crates/web-dom/`） | 绿 | `== PASS…==` exit 0 |
| INJ-3（第二份 `pub struct JsHandle`） | 红 | `left: 2 right: 1` FAILED |
| R1 四态 `v8_dependency_only_in_engine_v8` | 绿/红/绿/绿 | 态B：`铁律 5：…却出现在非白名单 crate：{"bindings-webidl": ["v8"]}` |
| U-1 探针（PyO3 lint）0.23 + 0.29 | 绿/红/绿 | 干净 PASS → 注入 `unsafe` `error: usage of an \`unsafe\` block` → 还原 PASS |
| 还原校验 | 哈希一致 | `bindings-webidl/Cargo.toml` / `check-unsafe-whitelist.sh` / `engine-v8/src/abi.rs` 与备份一致；临时文件已删 |

> `runner/tests/workspace_policy.rs` 自 G1 起哈希变化（`316ac29b…`→`37c59ac8…`）——**属合法演进**（G2 新增 5 crate 进入 workspace 名册），非受保护 golden；其门禁语义未弱化（R1 四态判别正常）。
> 证据：`reverse-verify-summary.txt`、`rev-*.txt`、`rev-u1-023.txt`、`rev-u1-029.txt`。

## 5. 铁律合规复核

| 铁律 | 核查 | 结果 |
|---|---|---|
| 4 unsafe | `engine-v8` 12 处 unsafe 代码行，±2 行内均见 `SAFETY:`；白名单外 0；无 crate/mod 级 `allow` | ✅（guard exit 0） |
| 5 V8 边界 | 全仓 `v8::` 仅现于 `crates/engine-v8/**`（poc 除外） | ✅（guard 规则 5 exit 0） |
| 7 受保护文件 | 本轮 QA 未改任何受保护文件；guard 脚本、`abi.rs`、`bindings-webidl/Cargo.toml` 与 G1 基线哈希一致 | ✅ |
| 1/2 测试先行 / golden | 未改 golden / 断言；反向验证为纯注入-还原 | ✅ |
| 6 高风险路径 | scheduler / PyO3 边界 / DRY 均有 ADR-0016~0019 落档 | ✅（本轮仅复核） |

## 6. 覆盖率（`CARGO_TARGET_DIR=target/cov cargo llvm-cov --workspace --summary-only`）

**TOTAL 83.79% regions / 82.76% lines**（与工程师报告一致）。重点 crate/文件：

| 文件 | Regions | Lines |
|---|---|---|
| navigation/page.rs | 89.71% | 89.30% |
| navigation/binding.rs | 81.22% | 82.97% |
| navigation/parser.rs | 93.82% | 93.55% |
| navigation/controller.rs | 95.40% | 96.27% |
| **navigation/sink.rs** | **48.26%** | **46.29%**（登记盲区） |
| engine-v8/realm.rs | 89.82% | 91.41% |
| scheduler/runloop.rs | 96.41% | 96.83% |
| web-dom/tree.rs | 91.84% | 93.77% |
| web-events/dispatch.rs | 97.84% | 99.21% |
| network/service.rs | 97.07% | 96.38% |
| sdk-python/src/{lib,page,convert}.rs | — | **0.00%**（pytest 不经 llvm-cov 采样；覆盖由 22 条 pytest 承担，登记项） |

证据：`local-coverage.txt`。

## 7. 缺陷清单（供路由）

> 均为 **2026-10-13 本轮新发现**；**未在已登记项（§8）中登记**，故如实上报。除 D1 外，全部测试当前**通过**（缺陷为「契约未兑现」而非「运行失败」）。

| # | 严重度 | 缺陷 | 证据原文 | 建议路由 |
|---|---|---|---|---|
| **D1** | **高（门禁红）** | **远程 clippy（1.99）FAIL exit 101**：1.99 新增 lint `clippy::assert_is_empty` 命中 **2 处**（本地 1.92 无此 lint）⇒ 跨版本漂移（同 G1-K-2 类） | ① `--> crates/web-dom/src/node.rs:170:9` `assert!(el.class_list().is_empty());`；② `--> crates/scheduler/src/runloop.rs:445:9` `assert!(exec.log.is_empty());`。原文：`error: used \`assert!\` to check that a value is empty` `= note: \`-D clippy::assert-is-empty\` implied by \`-D warnings\``。**已逐 crate 界定**：engine-v8/network/web-style/web-events/navigation **clean**；仅 web-dom+scheduler 两处（其余 `assert!(x.is_empty())` 因带断言消息或为自定义 `is_empty()` 不触发） | Engineer 修（改断言形态，保持意图） |
| **D2** | **高（契约缺口）** | **§3.2 `external_script_blocks_until_loaded` 未实现且无测试**：`<script src>` 不登记 load blocker、不 fetch、被当空 classic 执行 | `script_kind`（`page.rs:779-792`）只判 `type`/`defer`；全仓无 `<script src` 用例；`add_blocker` 仅在 controller 单测被调 | Engineer 补实现 + 测试；Architect 确认是否 G2 范围 |
| **D3** | 中 | **§3.7 `node_wrapper_recreated_after_collection` 缺席**：强制 GC 后重建 wrapper 的存活性校验未被测试（机制 `bind_force` 已在 `binding.rs:402`） | 全仓无该测试名（仅 doc 引用） | Engineer 补测试 |
| **D4** | 中 | **§3.7 `refcell_reentry_is_error_not_panic` 缺席 + ADR-0016·D2 结构性借用规则未落地**：全仓 `try_borrow`/`try_borrow_mut` **0 处**（仅 1 处 doc 注释）；`binding.rs` 有 **83 处** `.borrow()/.borrow_mut()`（panic 面）。当前靠 copy-out 纪律（dispatch `input.rs:129-133`、MO `mem::take`）规避，但**无结构保证**（ADR 明言「已弃靠纪律」） | `grep -rn try_borrow crates/navigation/src crates/web-dom/src crates/web-events/src` ⇒ 命中仅 `web-dom/src/error.rs:12` 注释 | Engineer（补 `try_borrow` 或在 ADR 具名登记为纪律偏差） |
| **D5** | 中低 | **§3.3 保序测试不判别**：`document_write_multiple_in_order`（账本序）+ `document_write_injection_order`（仅断言两节点均存在）**均未断言 DOM 注入序** ⇒ 原「倒置」缺陷亦会通过 | `parser.rs:215-217` `assert!(a.is_some() && b.is_some(), "两条 write 均应入树");` | Engineer 修测试（断言 `<a>` 在 `<b>` 前） |
| **D6** | 低 | **§3.5 `fetch_aborted_on_navigation` 无页面级测试**：仅网络原语 `abort_epoch_only_aborts_matching_epoch` 覆盖 | `page.rs` 无导航后旧请求「不 resolve」的断言 | Engineer 补测试（可选） |

## 8. 已登记项清单（非缺陷，勿误报）

1. **U-10 执行模型 as-built**：阻塞脚本在 `load` 内同步执行（`eval_with_deadline`，默认 2000ms）；`run_until` 驱动网络/MO/输入泵。延期项：脚本任务化、timer 驱动事件循环。✅ 与实现一致（`page.rs:537` `drain_parse` + `:610` `eval_page_script`）。
2. **模块求值未接 watchdog**：**复核真实性** ⇒ `run_deferred_scripts`（`page.rs:568-588`）模块走 `eval_module`（**无 deadline 参数**），确未接 `eval_with_deadline` ⇒ **登记项属实**。
3. sdk-python llvm-cov 0%（覆盖由 22 条 pytest 承担）✅ 复现。
4. 覆盖率盲区 `navigation/sink.rs` ≈46% L ✅ 复现（46.29%）。
5. 几何 stub（`capabilities.geometry="stub"`；`page_geometry_is_documented_stub`）✅。
6. TLA 检测=粗扫描（`contains_top_level_await` brace 深度扫描，`page.rs:795`）✅；单 Realm 导航 JS 全局残留（G4）✅。
7. K-12（微任务条数预算→墙钟）/ K-13（`REALM_DATA_SLOT` 单 Realm 无碍）✅ 延续。
8. `set_script_deadline_ms`（默认 2000ms）为测试钩子 ✅（`page.rs:618`）。

## 9. 远程 Linux x86_64 门禁（`ubuntu@124.221.1.78`）

同步：`rsync -av --delete --checksum --exclude='.workbuddy' --exclude='target'`（130 MB）。V8 静态库走本地中转 `RUSTY_V8_ARCHIVE=$HOME/librusty_v8_linux.a.gz`；`-j2`；`nohup` + 轮询；未改远程系统配置。

| 命令 | 退出码 | 关键原文 |
|---|---|---|
| `cargo test --workspace` | **0** | 日志含 **61 处 `Compiling`**（非陈旧）；**153 passed**，0 `FAILED`（套件数与本地逐项一致） |
| `cd contracts && uv run pytest` | **0** | `60 passed in 0.12s` |
| sdk-python（`uv venv` + `maturin develop -j2` + pytest） | **0** | `📦 Built wheel for CPython 3.13 … Installed iv8-0.0.0`；`22 passed in 0.31s` |
| `cargo fmt --all --check` | **0** | — |
| `bash scripts/check-unsafe-whitelist.sh` | **0** | `== PASS…==` |
| `cargo clippy --all-targets --all-features -- -D warnings` | **101（FAIL）** | **D1**：2 处 `clippy::assert_is_empty` —— `web-dom/src/node.rs:170` + `scheduler/src/runloop.rs:445`（逐 crate 复核：其余 crate clean） |

- **U-6 远程复核**：`html5ever 0.27.0` + `markup5ever 0.12.1` 首次于远程经 rsproxy 镜像 `Downloaded` + `Compiling` ✅（另有 `markup5ever 0.39.0` 来自其它依赖，符合 R3 记载）。
- 远程 `target` 3.5 GB（G1 遗留，未清）；`-j2` 下 `cargo test` link 期未 OOM。
- 注：rsync 未排除 `.venv`，远程 `contracts/.venv`、`sdk-python/.venv` 被本地（macOS 链接）覆盖，已由 `uv`/`maturin` 重建（无害，但建议后续 rsync 追加 `--exclude='.venv'`）。**→ Round 2 已采纳**（rsync 追加 `--exclude='.venv'`，远程 venv 保留）。

证据：`.workbuddy/refs/g2-qa-evidence/remote/{workspace-test,contracts-pytest,sdk-maturin,sdk-python-pytest,clippy-error}.log`。

## 10. 遗留与建议

- **闭环 D1**（远程 clippy）为进入下一闸门的**前置**；D2~D6 建议同批处理（均 ≤ 单文件改动量级）。
- 建议把 **D1 类「1.99 新 lint」** 纳入双端 clippy 常态化（本地 1.92 / 远程 1.99 漂移，G1 已现一次）——或钉版（K-14）。
- `external` 脚本（D2）若确认不属 G2 范围，应在 §3.2 与 `capabilities` 中**具名登记偏差**（同几何 stub 规格），避免「设计列出但未兑现」被读成已实现。

---

## 11. Round 2 结论与缺陷闭环（2026-10-13 · 修复后复验）

> **方法**：读修复源码/测试 → 复跑 → 抽查关键断言 → 反向注入判别力验证 → 双端全量回归 + 覆盖率复测。**Round 1 记录（§0~§10）保留为过程快照**。

### 11.1 缺陷闭环表

| # | Round 1 现象 | 修复（改动清单，主理人亲验） | Round 2 复验 |
|---|---|---|---|
| **D1** | 远程 clippy 1.99 FAIL（`assert_is_empty` 2 处） | `web-dom/src/node.rs:170`、`scheduler/src/runloop.rs:445` **补断言消息** | ✅ **远程 `cargo clippy -D warnings` EXIT=0**（`errors=0`）；逐 crate 复核消净 |
| **D2** | §3.2 `external_script_blocks_until_loaded` 未实现+无测试 | `page.rs`：`pending_external`(:163) / `src` 分流+命中执行+未命中 `add_blocker(ExternalScript)` 停泊(:560-580) / `drain_parse→Result<bool>` 门控 `mark_parse_finished`(:284-291) / `has_pending_io` 纳入 `pending_external`(:1074)；测试 `external_script_blocks_until_loaded`(:1680) | ✅ 复测：命中⇒`globalThis.__ext==7` & `Complete`；未命中⇒`readyState!=Complete` & `run_until()==WaitingIo`（非 Requeue 自旋） |
| **D3** | §3.7 `node_wrapper_recreated_after_collection` 缺席 | `binding.rs:795` 新测试 | ✅ 绿（`collect_garbage_for_testing` 后 `wrapper_for` ⇒ 新句柄 `assert_ne!`） |
| **D4** | §3.7 `refcell_reentry_is_error_not_panic` 缺席 + `try_borrow` 未落地 | `try_borrow` 落点 **30 处**（page.rs 5 / binding.rs 21 / input.rs 4；R1 全仓 0）；测试 `binding.rs:818` | ✅ 绿（持树借用期间 JS `createElement` ⇒ message 含 `RefCell 重入`，非 panic）。**残余**（宿主驱动路径 `load`/`run_until`/`pump_fetch`/`resolve_completed`/`close`）= **具名登记（主理人裁定不扩大）** |
| **D5** | §3.3 保序测试不判别 | `parser.rs:217` `document_write_injection_order` + `page.rs:1720` `document_write_multiple_in_order_js_path` **断言 DOM 序** | ✅ **判别力反向验证通过**（见 11.2） |
| **D6** | §3.5 `fetch_aborted_on_navigation` 无页面级测试 | `page.rs:1740` 新测试 | ✅ 绿（导航后 `complete_fetch(旧 id)` ⇒ `PageErrorKind::NavigationAborted`） |

### 11.2 D5 判别力反向验证（关键）

> 备份 `parser.rs` → 临时把 `ParserHost::resume` **倒置**（`pop_back`→`pop_front`）→ 强化后测试**必红** → 还原并校验 sha256。

```
$ sed -i 's/pop_back()/pop_front()/' crates/navigation/src/parser.rs
$ cargo test -p navigation --lib
test page::tests::document_write_multiple_in_order_js_path ... FAILED
test parser::tests::document_write_injection_order ... FAILED
  panicked at page.rs:1732: 注入序应为 i 先于 b（实得文档序 ["html","head","body","script","b","i"]）
  panicked at parser.rs:231: 注入序应为 a 先于 b（实得文档序 ["html","head","body","b","a"]）
test result: FAILED. 49 passed; 2 failed; …
$ # 还原
$ shasum -a 256 crates/navigation/src/parser.rs
3a83d7d954ccb6447405717841ab2245c313012cfcf41051d84cb1a516ca3bae   # == 注入前（一致）
```

⇒ 两测试对「注入序」**具判别力**（异于 Round 1 的不判别）。证据：`.workbuddy/refs/g2-qa-evidence/r2-d5-inverted-red.txt`。

### 11.3 Round 2 双端全量回归

| 面 | 本地（1.92） | 远程（1.99） |
|---|---|---|
| `cargo test --workspace` | **158 passed**，0 FAILED（engine-v8 52 / vertical 1 / **navigation 51**（46→51）/ vertical_g2 1 / network 6 / policy 3 / runner 4+5 / scheduler 11 / iv8 1 / web-dom 12 / web-events 9 / web-style 2） | **158 passed**，0 FAILED（日志含 `Compiling`，非陈旧） |
| `cargo clippy -D warnings` | **0** | **0**（D1 已消） |
| `cargo fmt --check` / `check-unsafe-whitelist.sh` | **0 / 0** | **0 / 0** |
| contracts pytest | **60** | **60** |
| sdk-python pytest | **22** | **22**（uv venv + `maturin develop -j2` 重建扩展） |
| 覆盖率（llvm-cov） | **83.47% R / 82.66% L**（R1 83.79/82.76 ⇒ 微降，因新增代码行；`sink.rs` 46.29% L 不变；`binding.rs` 78.44%R/81.47%L） | —（平台无关） |

证据：`.workbuddy/refs/g2-qa-evidence/r2-local-*.txt`、`remote/r2-*.log`。

### 11.4 Round 2 判定

**D1~D6 全部闭环；双端五门禁 + 两 pytest 全绿；D5 判别力经反向注入证明；覆盖率达标。⇒ 可进入下一闸门。**
剩余仅**已登记项**（§8）+ **D4 残余**（宿主驱动路径 `try_borrow`，主理人裁定具名登记不扩大）。
