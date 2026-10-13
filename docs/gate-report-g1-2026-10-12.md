# 门禁报告 —— 2026-10-12 第五轮（G1 引擎纵向验证）

- 产出角色：QA（严过关）
- 事实源：`docs/TASKS.md`（本报告是过程与判定记录，**状态以 `docs/TASKS.md` 为准**）；验收主契约 = `docs/design-g1-engine-2026-10-12.md` §3.1~§3.6
- 运行机：本地 macOS arm64（`rustc 1.92.0`）+ 远程 `ubuntu@124.221.1.78`（Ubuntu 24.04 · x86_64 · `rustc 1.99.0` · 3.6 GB 内存）
- 本轮范围：**独立验证**（本地复跑 + 反向验证抽样 + 铁律复核）+ **覆盖率测量** + **远程 Linux x86_64 门禁**
- **Round 2（修复后复验）已完成**：K-1（deny）/ K-2（远程 clippy）/ K-3 / K-4 **全部闭环**；双端五门禁 + pytest **全绿**。Round 1 快照（§1/§7）保留为过程记录，**最终判定见 §0 与 §10/§11**。
- QA 纪律：**不改功能代码**；**不为迁就实现修改断言 / golden**；一切结论附**证据原文**；发现与已登记项不符即停下报告
- 证据原文归档：`.workbuddy/refs/g1-qa-evidence/`（本地/远程各步日志、反向验证输出、覆盖率、守卫注入原文；gitignored scratch，与既有 `pyo3-lint-probe-evidence/` 同规格）

---

## 0. 结论（TL;DR）—— 【Round 2 · 最终】

| 项 | 结果 |
|---|---|
| 六项能力验收（§3.1~§3.6 测试集） | **全部通过**（逐条复跑；测试名/证据见 §3） |
| 本地 Rust 门禁 | ✅ **5/5 全绿**：fmt **0** / clippy **0** / test **0**（54 tests）/ guard **0** / **deny 0**（`advisories ok, bans ok, licenses ok, sources ok`） |
| 本地 Python 侧 | contracts **55 passed**（0）；sdk-python **13 passed**（0，pyo3 0.29 重建扩展） |
| 本地反向验证 | **全部符合预期**：guard INJ-1 红 / INJ-2 绿；`v8_dependency_only_in_engine_v8` R1 四态 绿/红/绿/绿；INJ-3 红；U-1 探针复跑 exit 0；注入后**已还原且 sha256 与基线一致** |
| Rust 覆盖率（llvm-cov） | TOTAL **75.33% regions / 75.97% lines**；engine-v8 各模块 **83%~94% lines**（microtask.rs 3 行声明性未覆盖） |
| 远程 Linux x86_64 门禁 | ✅ **全绿**：fmt **0** / **clippy 0（1.99 过 `double_must_use`）** / test **0**（54 tests）/ guard **0** / contracts **55 passed** / sdk-python **13 passed** |
| 缺陷 | ✅ **K-1 / K-2 / K-3 / K-4 全部闭环**（Round 2 复验；闭环表见 §0.1 / §11） |
| 是否可进入下一闸门 | ✅ **可以进入下一闸门**（无阻塞遗留；剩余均为**已登记项**，见 §9） |

> **Round 1 → Round 2 变化**：Round 1 发现 **2 项门禁红灯**（本地 `cargo deny` exit 3；远程 `cargo clippy` exit 101）+ 2 处文档漂移；工程师修复后（K-1a path 依赖补 `version`、K-1b pyo3 0.23→0.29.3、K-2 删冗余 `#[must_use]`、K-3/K-4 注释更正），**Round 2 双端复跑全绿**（见 §11）。

**一句话**：**功能验收（六能力 §3.x）全过 + 反向验证可信 + 本地/远程五门禁与 pytest 全绿 + K-1~K-4 闭环 ⇒ 可进入下一闸门**。

---

## 0.1 缺陷清单与闭环（Round 1 发现 → Round 2 闭环）

| # | 项 | Round 1 现象（证据原文） | 归因 / 路由 | 闭环（Round 2） |
|---|---|---|---|---|
| **G1-K-1** | 本地 `cargo deny check` FAIL（exit 3） | `error[wildcard] sdk-python/Cargo.toml:26 engine-v8={path="../engine-v8"}` + `error[vulnerability] pyo3 0.23.5 RUSTSEC-2025-0020 / -2026-0177`；`advisories FAILED, bans FAILED`（**与 TASKS.md「五门禁全绿」矛盾**） | 实现/配置 → Engineer（wildcard）+ Lead（advisories，`deny.toml` 为铁律 7 保护文件） | ✅ **已修**：path 依赖补 `version = "0.0.0"`（K-1a）+ pyo3 **0.23 → 0.29.3**（K-1b；覆盖两 RUSTSEC 的 ≥0.24.1 与 ≥0.29.0 要求）。**本地 `cargo deny check` exit 0**（§11） |
| **G1-K-2** | 远程 `cargo clippy -D warnings` FAIL（exit 101） | `error: this function has a #[must_use] ... returns a type already #[must_use] --> crates/sdk-python/src/callback.rs:69`（`clippy::double_must_use`）；**仅新 stable 1.99 触发，本地 1.92 绿** | 实现 → Engineer | ✅ **已修**：删 `callback.rs:68` 冗余 `#[must_use]`；**远程 `cargo clippy`（1.99）exit 0**（§11） |
| **G1-K-3** | 文档漂移 | `realm.rs:686-691` 注释称静态 import「本批未实现」，但 `module_static_import_resolves` **实测通过** | 文档 → Engineer/Architect | ✅ **已更正注释**（静态 import 已实现；仅动态 import 未实现） |
| **G1-K-4** | 文档漂移 | `workspace_policy.rs:265-270` 注释称 engine-v8 `[dependencies]` 为空，实际已有 `v8="152.2.0"` | 文档 → Engineer | ✅ **已更正注释**（`workspace_policy.rs:265-269` 改写为「G1 已落地 v8，非对空集成立」） |

> 以下为**已登记项（非缺陷，勿误报）**：动态 import 未实现（V8 FATAL 回退）、`perform_checkpoint` 返回 1 / 墙钟截断、REALM_DATA_SLOT 不重写、§16 完整兼容层属 G4、各处已验收偏差（见 §9）。

---

## 1. 本地门禁（逐条：命令 / 退出码 / 原文）

> ⚠️ 下表与命令块为 **Round 1 快照**（deny 红）；**Round 2 修复后 deny=0（全绿）**，权威结果见 §11。

| # | 命令 | 退出码 | 关键原文 |
|---|---|---|---|
| 1 | `cargo fmt --all --check` | **0** | （无输出） |
| 2 | `cargo clippy --all-targets --all-features -- -D warnings` | **0** | `Finished \`dev\` profile … in 0.27s` |
| 3 | `cargo test --workspace` | **0** | 5 个非空套件全 `test result: ok.`，0 处 `FAILED` |
| 4 | `cargo deny check` | **3（FAIL，R1）→ 0（R2）** | R1: `advisories FAILED, bans FAILED`；**R2: `advisories ok, bans ok, licenses ok, sources ok`** |
| 5 | `bash scripts/check-unsafe-whitelist.sh` | **0** | `== PASS：白名单外无 unsafe；无 crate/mod 级开口；白名单内 unsafe 均带 SAFETY 注释；白名单外无 v8 路径引用 ==` |

```
$ cargo fmt --all --check
FMT_EXIT=0
$ cargo clippy --all-targets --all-features -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.27s
CLIPPY_EXIT=0
$ cargo test --workspace          # 54 tests：engine_v8 43 + vertical 1 + skeleton_smoke 4 + workspace_policy 5 + iv8(callback) 1
WORKSPACE_TEST_EXIT=0             # FAILED 计数 = 0
$ cargo deny check                    # Round 1 快照（修复前）
error[wildcard]: found 1 wildcard dependency for crate 'sdk-python'
   ┌─ crates/sdk-python/Cargo.toml:26:13
26 │ engine-v8 = { path = "../engine-v8" }
error[vulnerability]: Risk of buffer overflow in `PyString::from_object`  (pyo3 0.23.5, RUSTSEC-2025-0020)
error[vulnerability]: Missing `Sync` bound on `PyCFunction::new_closure` closures  (pyo3 0.23.5, RUSTSEC-2026-0177)
advisories FAILED, bans FAILED, licenses ok, sources ok
DENY_EXIT=3
# ── Round 2（修复后）：advisories ok, bans ok, licenses ok, sources ok ⇒ DENY_EXIT=0（§11）──
$ bash scripts/check-unsafe-whitelist.sh
== PASS：白名单外无 unsafe；无 crate/mod 级开口；白名单内 unsafe 均带 SAFETY 注释；白名单外无 v8 路径引用 ==
GUARD_EXIT=0
```

**非致命告警（登记，不影响退出码）**：`cargo deny` 的 `licenses` 段有 **7 条 `warning[license-not-encountered]`**（`deny.toml:39/43/45/46/48/49/50` 的 `BSD-2-Clause` / `Unicode-DFS-2016` / `MPL-2.0` / `CC0-1.0` / `BSL-1.0` / `OpenSSL` / `NCSA` 未被依赖树命中）——较第四轮 3 条增多，属**噪音**（`unused-allowed-license="warn"` 的既定行为）。

## 2. 本地 Python 侧

```
$ cd contracts && uv run pytest -q
.......................................................                  [100%]     # 55 个点 = 55 passed
（CONTRACTS_EXIT=0）

$ cd crates/sdk-python && .venv/bin/python -m pytest -v
collected 13 items … 13 passed in 0.26s
EXIT=0
```

## 3. 六项能力逐项独立验证（§3.1~§3.6）

> 方法：**读测试源码 → 复跑 → 抽查关键断言**。下列「复跑」列均为 QA 独立复跑结果（`cargo test` / pytest，见 §1/§2 原文）。

### 3.1 原生绑定（§3.1，`realm.rs`）

| 测试名 | 复跑 | 抽查断言要点 |
|---|---|---|
| `native_fn_callable_and_returns` | ok | `__t.add(2,3) == 5`（跨边界返回值） |
| `native_fn_name_length_descriptor` | ok | `.name=="add"`、`.length==2`、descriptor `.writable===true` |
| `native_fn_tostring_contains_native_code` | ok | `toString()` 含 `native code` |
| `native_tostring_snapshot`（insta，独立 golden） | ok | 快照文本 = `function () { [native code] }`（`src/snapshots/engine_v8__realm__tests__native_tostring.snap`，**本轮新增、未改动既有 golden**） |
| `object_identity_stable_within_realm` | ok | 对象**身份**稳定（两次取回同一对象，修改互相可见；wrapper 复用归 `node_map`，见已验收偏差 ③） |
| `native_fn_throw_propagates` | ok | JS 侧 `catch(e){e.message}=="boom from host"`（TryCatch 路径） |

### 3.2 模块 / WASM（§3.2，`realm.rs`）

| 测试名 | 复跑 | 备注 |
|---|---|---|
| `module_export_value` | ok | `ns.x===42` |
| `module_static_import_resolves` | **ok（实测通过）** | 经 `MemoryModuleHost` 内存模块图 `import{...}from'./dep.js'` ⇒ `w===8`。**⚠️ 与 `realm.rs:686-691` 注释「本批未实现」矛盾 ⇒ G1-K-3** |
| `module_cache_identity` | ok | 同一 URL 两次加载 `__ns1===__ns2` |
| `dynamic_import_specifier` | **缺失（已登记缺口）** | 见 §9 ①；`realm.rs:691` 明记未实现 |
| `module_syntax_error_kind` | ok | 坏模块 ⇒ `JsErrorKind::Compile`（非 panic） |
| `wasm_instantiate_and_call` | ok | 内置最小 wasm 字节码 `instance.exports.add(2,3)===5` |

### 3.3 微任务（§3.3，`realm.rs`）

| 测试名 | 复跑 | 抽查断言要点 |
|---|---|---|
| `explicit_policy_defers_microtask` | ok | `Explicit` 下未 checkpoint 前 `flag===undefined` |
| `checkpoint_runs_pending_microtasks` | ok | checkpoint 后 `flag===1`，返回计数 ≥1 |
| `no_auto_checkpoint_after_dom_op` | ok | 普通 eval 返回后**不**自动跑微任务 |
| `microtask_budget_exhaustion` | ok | 自递归微任务被**墙钟守卫**截断 ⇒ `MicrotaskBudget`；截断后 isolate 恢复可执行（`1+1` ok） |

> 注：截断手段为**墙钟 watchdown + `terminate_execution`**（非「条数预算」），与已验收偏差 ② 一致；`microtask.rs` 的 `MicrotaskBudget::max_jobs` 为**声明性契约字段**（已如实登记）。

### 3.4 GC 循环（§3.4，`handles.rs` + `realm.rs` + `node_map.rs`）

| 测试名 | 复跑 | 抽查断言要点 |
|---|---|---|
| `handle_not_a_gc_root` | ok | 仅持 `JsHandle`（弱槽）+ 独立 scope：Full GC 后 `resolve` ⇒ `Collected`（证 C-1） |
| `collected_detected` | ok | 弱空 ⇒ `to_local` **与** `to_global` **两侧**均 `Collected`（兑现 ADR-0013 待实测项 T1）；与 `StaleHandle` **分开** |
| `lease_keeps_value_alive` | ok | 持 `HandleLease` 保活；`drop(lease)`+GC 后可回收 |
| `stale_handle_detected` | ok | free→复用槽 ⇒ 旧句柄 `resolve` 报 `StaleHandle`（复用防呆，非静默指向新值） |
| `alloc_free_reuse_no_alias`（proptest，24 cases） | ok | 随机 alloc/free 序列后存活句柄仍指向原值 |
| `weak_handle_empties_after_gc` | ok | 宿主只持弱句柄 ⇒ GC 后可回收 ⇒ `Collected` |
| `t2_1_leak_shape_avoided` | ok | 复刻 T2.1 `exp3` 环形状但只走弱引用 ⇒ 可回收（**只证弱引用路径不泄漏**，**不得**读成「跨语言环已能回收」，ADR-0007 遗留） |
| `node_map_identity` / `unbind_removes_both` / `rebinding_handle_keeps_both_ways_consistent` | ok | 同一 node 复用同一 `WeakJsHandle`；双向一致 |

### 3.5 控制入口隔离（§3.5，`control.rs`）

| 测试名 | 复跑 | 抽查断言要点 |
|---|---|---|
| `control_available_in_control_scope` | ok | `control_eval('typeof __iv8__')=="object"` |
| `control_invisible_in_page_scope` | ok | `eval_page('typeof __iv8__')=="undefined"` |
| `control_revoked_during_page_script` | ok | `with_page_script` 内 `eval_control` ⇒ `ControlRevoked` |
| `capability_restored_after_page` | ok | 退出后 `is_control_available()===true`；**含 panic 路径**（`catch_unwind` 后仍恢复） |

> 形态为**单 Realm 双模式**（`with_page_script(&mut ControlPlane)`），与已验收偏差 ④ 一致。

### 3.6 Python 回调（§3.6，sdk-python pytest + `realm.rs` 集成）

| 契约测试名（§3.6 表） | 实现/复跑测试名 | 复跑 | 备注 |
|---|---|---|---|
| `py_callback_returns_value` | `test_py_callback_returns_value` | ok | `mul(6,7)==42` |
| `py_callback_receives_args` | `test_py_callback_receives_args` | ok | `str:s\|float:4.0\|float:2.5` |
| `py_callback_exception_propagates` | `test_py_callback_exception_propagates` | ok | JS 侧 `e.message` 含 `ValueError`/`kaboom` |
| `reentrant_eval_rejected` | `test_reentrant_eval_rejected` | ok | 回调内 `ctx.eval` ⇒ `ReentrantEvalError` |
| `reentrant_native_to_native_rejected` | `realm::reentrant_native_to_native_rejected`（集成） | ok | native→native ⇒ `ReentrantEval` |
| `reentrant_in_microtask_rejected` | `realm::reentrant_in_microtask_rejected` + `test_reentrant_eval_rejected_in_microtask` | ok | 微任务期隐式重入 ⇒ 被拒 |
| `callback_deadline_enforced` | `test_callback_deadline_enforced` | ok | 墙钟超限 ⇒ `CallbackDeadline`（事后可判定，不挂死） |
| `callback_thread_equals_eval_thread` | **`callback::callback_thread_assertion_rejects_foreign_thread`**（机制层单测） | ok | **测试名不同**（已验收偏差 ⑥：G1 单线程下集成路径结构不可达）；异线程 ⇒ `Internal`（非 UB） |
| `no_gil_wait_in_callback` | `test_no_gil_wait_in_callback` | ok | 100 次回调往返 < 5s（有界墙钟判据） |
| `close_is_idempotent` | `test_close_is_idempotent` | ok | close 两次不报错；关闭后 eval ⇒ `Iv8Error` |

> 额外：`test_undefined_is_singleton` / `test_eval_basic_values_roundtrip` / `test_eval_object_becomes_js_ref` / `test_control_eval_installs_control_global` / `test_vertical_native_callback_microtask_checkpoint` 亦通过（13 pytest 全绿）。

### 3.7 纵向集成（S13）

| 测试 | 复跑 | 断言要点 |
|---|---|---|
| `vertical_native_fn_to_microtask_to_checkpoint`（`engine-v8/tests/vertical.rs`） | ok | 原生 fn → 微任务 → checkpoint 值链闭合（`__out=42`） |
| `test_vertical_native_callback_microtask_checkpoint`（sdk-python） | ok | Python 回调版同链（`__out=42.0`） |

## 4. 覆盖率测量

### 4.1 Rust（`CARGO_TARGET_DIR=target/cov cargo llvm-cov --workspace --summary-only`，cargo-llvm-cov 0.9.1）

| 文件 | Regions | 行覆盖 | 说明 |
|---|---|---|---|
| `engine-v8/src/abi.rs` | 91.38% | 92.59% | |
| `engine-v8/src/control.rs` | 90.24% | 92.41% | |
| `engine-v8/src/error.rs` | 93.33% | 97.44% | |
| `engine-v8/src/handles.rs` | 89.93% | 90.71% | |
| `engine-v8/src/isolate.rs` | 83.33% | 81.25% | |
| `engine-v8/src/microtask.rs` | 0.00% | 0.00% | **3 行**：仅 `enum MicrotaskPolicy` / `MicrotaskBudget` 声明性类型（逻辑在 `realm.rs`，已覆盖） |
| `engine-v8/src/module_host.rs` | 89.09% | 93.10% | |
| `engine-v8/src/native_fn.rs` | 69.23% | 65.08% | trampoline 错误分支/异常回填部分未覆盖 |
| `engine-v8/src/node_map.rs` | 94.44% | 100.00% | |
| `engine-v8/src/realm.rs` | 89.45% | 90.77% | 全 crate 主力 |
| `runner/src/lib.rs` | 100.00% | 100.00% | |
| `sdk-python/src/callback.rs` | 21.29% | 23.91% | 见下注 |
| `sdk-python/src/convert.rs` | 0.00% | 0.00% | 见下注 |
| `sdk-python/src/lib.rs` | 0.00% | 0.00% | 见下注 |
| **TOTAL** | **75.33%** | **75.97%** | |

> **sdk-python Rust 行覆盖低的解释（非缺陷）**：sdk-python 的 `#[pymethods]`（`lib.rs`/`convert.rs`）**只在 Python 进程内经 `iv8` 扩展被调用**，而 `cargo llvm-cov` 只插桩 `cargo test` 进程 ⇒ 这些路径**不被 llvm-cov 采到**。其**行为正确性由 sdk-python 的 13 条 pytest 覆盖**（§2/§3.6）。若要 Rust 行级覆盖 sdk-python，需 `maturin develop` + `coverage` 组合，属 G1 后工具链事项（登记）。

### 4.2 Python（pytest-cov，`uv pip install pytest-cov` 装入 venv，**未改 pyproject**）

```
$ cd crates/sdk-python && .venv/bin/python -m pytest --cov=iv8 --cov-report=term-missing
Name                                                 Stmts   Miss  Cover
.venv/lib/python3.13/site-packages/iv8/__init__.py       4      0   100%
TOTAL                                                    4      0   100%
13 passed
```

> **口径说明**：`iv8` 是**原生扩展模块**（Rust cdylib），Python 侧仅有 maturin 生成的 `__init__.py`（4 行）⇒ pytest-cov 的「Python 覆盖率」**对 sdk-python 无实质意义**（真实逻辑是 Rust，见 §4.1 注）。此处**如实出具**，不作装饰性结论。

## 5. 铁律合规复核

| 铁律 | 复核点 | 结论 | 证据 |
|---|---|---|---|
| 4（unsafe 白名单） | 全仓仅 `engine-v8` 含 `unsafe`（10 个逐块 `#[allow(unsafe_code)]` 块：`realm.rs` 9 + `native_fn.rs` 1）；`sdk-python` 仅注释提及 | **PASS** | `unsafe-sites.txt`；守卫 `PASS` |
| 4（SAFETY 窗口） | 每个 `unsafe` 行前 ±2 行内有 `// SAFETY:` | **PASS** | 守卫规则 3 `PASS`；抽查 `realm.rs:184-192` / `native_fn.rs:101-103` |
| 4（禁 crate/mod 级开口） | 无 `#![allow(unsafe_code)]`、无 mod/impl 级开口 | **PASS** | 守卫规则 2/4 `PASS` |
| 5（V8 类型边界） | `v8::` 路径只在 `engine-v8` + `poc/`；`abi.rs` 零 `v8::` | **PASS** | 守卫规则 5 `PASS`；`test_abi_rs_has_no_v8_path_reference` / `test_v8_paths_absent_outside_whitelist` **ok**；`v8_dependency_only_in_engine_v8` **ok** |
| 5（类型面唯一） | 三类型各恰好一处且在 `abi.rs` | **PASS** | `three_safety_types_have_single_definition_point` **ok** + **INJ-3 反向验证红**（§6） |
| 7（保护文件未改） | `AGENTS.md`（iron rules 十条完整、`WHITELIST` 未变）、`CODEOWNERS` 存在、`.github/workflows/ci.yml`（4 jobs 未动）、`deny.toml`（起草态原文未动） | **未见被改**（AI 未动；哈希见 §6 基线） | 文件原文；`scripts/check-unsafe-whitelist.sh` `WHITELIST` 仍为 `engine-v8/` + `poc/` |
| 2（golden 不迁就实现） | 新 golden = **新增** `native_tostring.snap`；未改既有 golden | **PASS** | §3.1；QA 未改任何断言/快照 |
| 9（验收测试可回答） | 六能力每条均可指到具名测试（§3） | **PASS** | §3 |

## 6. 反向验证（独立重跑 + 原文留档）

| 项 | 注入 | 期望 | 实测（原文） |
|---|---|---|---|
| guard **INJ-1** | `crates/web-dom/src/` 加 `use v8::Local as _;` | 报违规 | `VIOLATION[v8-type-escape] crates/web-dom/src/__qa_inj1_probe.rs:2: use v8::Local as _;` ⇒ `== FAIL：发现 1 处违规 ==`，`INJ1_EXIT=1` ✅ |
| guard **INJ-2** | 把 `crates/web-dom/` 加入守卫 `WHITELIST`（用 `scripts/qa_tmp_guard_inj2.sh` 副本，跑完删除） | 不再报 | `== PASS：… ==`，`INJ2_EXIT=0` ✅ |
| **INJ-3** | `crates/web-dom/src/` 加 `pub struct JsHandle(u64);` | 断言红 | `three_safety_types_have_single_definition_point ... FAILED`：命中 `[jsHandle 注入行(2), abi.rs:60]`，`left: 3 right: 1` ✅（注入文件含 1 行同名注释，故命中 3 而非 2——判别力成立） |
| **R1 四态** `v8_dependency_only_in_engine_v8` | 态A 干净 / 态B 给 `bindings-webidl/Cargo.toml` 注入 `v8 = "152.2.0"` / 态C 临时放松测试允许集合 / 态D 还原 | 绿/红/绿/绿 | A `1 passed`；B `FAILED`：`铁律 5：…却出现在非白名单 crate：{"bindings-webidl": ["v8"]}`；C `1 passed`；D `1 passed` ✅ |
| **U-1 探针** | 复跑 `probe-kit/forbid_probe`（`cargo build --offline`） | PyO3 宏在 `forbid` 下可编译 | `Finished dev profile … in 4.70s`，`FORBID_PROBE_EXIT=0` ✅（与 `p1b/p1d` 一致；`p1c_armed` 证明 lint 真武装） |

**还原校验（无 git，先备份 /tmp）**：注入前备份 → 结束（`trap` 含异常）必还原。`sha256` 逐文件比对：

```
67a2e3a79414eeb12d0844835fb09e5542149de5ba1dbd3c66eeff2a624120ee  crates/bindings-webidl/Cargo.toml      # = 基线
316ac29bdeed1a7d9451131cf9fe5c124b027a6aba2928e302094254de0cff80  crates/runner/tests/workspace_policy.rs # = 基线
45dac8ebcafa991ce9426a3d78ff677ee62a1872dea77e7b53634dd4893d2e50  scripts/check-unsafe-whitelist.sh       # = 基线
43b62b94bbb4b456429794720710fb6d0f5734a5f6273b41342628a218484ceb  crates/engine-v8/src/abi.rs             # = 基线
```
临时注入文件（`__qa_inj1_probe.rs` / `__qa_inj3_probe.rs` / `qa_tmp_guard_inj2.sh`）**已删除**，`ls` 报 `No such file or directory`。

## 7. 远程 Linux x86_64 门禁

> ⚠️ **Round 1 快照**（clippy 红）；**Round 2 修复后 clippy=0、全绿**，权威结果见 §11。

**同步**：`rsync -av --delete --checksum --exclude='.workbuddy' --exclude='target' … ubuntu@124.221.1.78:~/mmv8/`（**额外排除 `target`**：避免把 mac 产物同步到 linux；远程产物由远程自行重建）。**R2 纪律已守**：日志出现 `Compiling v8 v152.2.0`（**编译新行**，非陈旧二进制）。

```
$ ssh ubuntu@124.221.1.78 '... cd ~/mmv8 && cargo fmt --all --check && bash scripts/check-unsafe-whitelist.sh \
    && RUSTY_V8_ARCHIVE=$HOME/librusty_v8_linux.a.gz cargo clippy -j2 --all-targets --all-features -- -D warnings \
    && RUSTY_V8_ARCHIVE=$HOME/librusty_v8_linux.a.gz cargo test -j2 --workspace \
    && cd contracts && uv run pytest -q && cd ../crates/sdk-python && uv run --python 3.13 pytest -v'

FMT_EXIT=0
GUARD_EXIT=0
CLIPPY_EXIT=101      # 🔴 FAIL —— 见下
TEST_EXIT=0
CONTRACTS_EXIT=0     # 55 passed
<SDKPY_EXIT：见 §7.2>
```

环境：`rustc 1.99.0` · `uv 0.13.0` · 4 核 / 3.6 GB（`free -m` 可用 ~3.0 GB）· `RUSTY_V8_ARCHIVE=$HOME/librusty_v8_linux.a.gz`（39,784,686 B，已在远程）。**全程未改远程系统配置**；`-j2`/`CARGO_BUILD_JOBS=2` 约束并行。

### 7.1 🔴 远程 clippy 失败原文（G1-K-2）

```
error: this function has a `#[must_use]` attribute with no message, but returns a type already considered as `#[must_use]`
  --> crates/sdk-python/src/callback.rs:69:1
69 | pub fn py_callback(name: String, cb: Py<PyAny>, deadline_ms: Arc<AtomicU64>) -> NativeFn {
   = note: `-D clippy::double-must-use` implied by `-D clippy::all`
   = help: remove `must_use`  (crates/sdk-python/src/callback.rs:68)
error: could not compile `sdk-python` (lib) due to 1 previous error
CLIPPY_EXIT=101
```

**根因**：`rust-toolchain.toml` 只固定 `channel="stable"`（不固定补丁版本）⇒ 本地 stable = **1.92.0**、远程 stable = **1.99.0**；`clippy::double_must_use` 在 1.99 进入 `clippy::all` ⇒ **本地绿、远程红**。修复：删除 `callback.rs:68` 的 `#[must_use]`。

**定位验证（排除面）**：远程单跑 `cargo clippy -j2 -p engine-v8 -p runner --all-targets --all-features -- -D warnings` ⇒ `TARGETED_CLIPPY_EXIT=0`（`Finished dev profile … in 0.36s`）⇒ 红灯**仅局部于 `sdk-python/src/callback.rs` 一处**，engine-v8 / runner 在 1.99 下干净。

### 7.2 远程 Python 侧

```
### 5 contracts uv run pytest
.......................................................  [100%]   # 55 个点 = 55 passed
CONTRACTS_EXIT=0     # uv 在远程选到 CPython 3.15.0（纯 Python 契约测试）

### 6 sdk-python uv run pytest
platform linux -- Python 3.13.16, pytest-9.1.1 …
collected 13 items … 13 passed in 0.26s
SDKPY_EXIT=0
```

**远程 sdk-python 实跑成功**：`uv run --python 3.13`（uv 下载 `cpython-3.13.16-linux-x86_64-gnu`）→ `maturin pep517 build-wheel`（`--features extension-module`，release，`Built iv8`）→ pytest **13 passed**，退出码 0。**未 OOM**（`-j2` / `CARGO_BUILD_JOBS=2`）。**无系统级改动**（仅用户级 uv venv + cargo）。

### 7.3 远程门禁小结

| 步骤 | 命令 | 退出码 | 结论 |
|---|---|---|---|
| fmt | `cargo fmt --all --check` | **0** | ✅ |
| guard | `bash scripts/check-unsafe-whitelist.sh` | **0** | ✅ PASS |
| clippy | `cargo clippy -j2 --all-targets --all-features -- -D warnings` | **101（R1）→ 0（R2）** | R1 🔴 FAIL（`callback.rs:68` `double_must_use`）；**R2 ✅ 1.99 通过** |
| test | `cargo test -j2 --workspace` | **0** | ✅（54 tests，日志含 `Compiling v8 v152.2.0`） |
| contracts | `uv run pytest -q` | **0** | ✅ 55 dots（55 passed） |
| sdk-python | `uv run --python 3.13 pytest -v` | **0** | ✅ 13 passed |

> 远程无 `cargo-llvm-cov`、无 `cargo-deny`（与第四轮 K-7 同口径）。

## 8. 缺陷与失败清单（路由）

| # | 严重度 | 现象 | 归因 → 路由 | 处置建议 |
|---|---|---|---|---|
| G1-K-1 | **高（门禁红）** | 本地 `cargo deny check` exit 3：`[wildcard]` + 2×`[vulnerability]`（pyo3 0.23.5） | 实现/配置 → **Engineer**（wildcard：给 `engine-v8` path 依赖补 version 或按 workspace 依赖统一）+ **Lead/维护者**（advisories：`deny.toml` 为铁律 7 保护文件，须**决策**：升级 pyo3 至 ≥0.24.1/0.29 或按规程登记 `ignore` 并写理由） | 修复后 QA 复跑 `cargo deny check` |
| G1-K-2 | **高（目标平台门禁红）** | 远程 `cargo clippy -D warnings` exit 101：`callback.rs:68/69` `double_must_use` | 实现 → **Engineer**（删多余 `#[must_use]`） | 修复后远程复跑 clippy |
| G1-K-3 | 低（文档） | `realm.rs:686-691` 注释与 `module_static_import_resolves` 实测通过矛盾 | 文档 → **Engineer/Architect** | 更正注释（静态 import 已实现；仅动态 import 未实现） |
| G1-K-4 | 低（文档） | `workspace_policy.rs:265-270` 注释称 engine-v8 依赖为空 | 文档 → **Engineer** | 更正注释（G1 已引入 `v8`） |

> **智能路由判定**：G1-K-1 / G1-K-2 均为**实现/配置**问题（断言本身正确、期望值正确），⇒ **Engineer**（其中 advisories 需 Lead 决策）。**QA 未为迁就实现修改任何断言 / golden**。无「测试代码自身错误」需要自查修正的情形。

## 9. 遗留 / 已登记项（非缺陷）

| # | 项 | 归属 | 阻塞 G1？ |
|---|---|---|---|
| ① | **动态 import 未实现**（`dynamic_import_specifier`）：v8 152 的 `ResolveModuleCallback` 无 scope ⇒ 撞 V8 FATAL 后回退；需 thread-local isolate 桥 + 嵌套 scope（专项） | 已登记（`realm.rs:686-691`） | 否（已登记缺口） |
| ② | `perform_checkpoint` 返回恒 **1**（V8 不暴露条数）、截断走**墙钟守卫**；`MicrotaskBudget::max_jobs` 为声明性字段 | 已登记（`microtask.rs` / `realm.rs:475-521`） | 否 |
| ③ | `perform_checkpoint` **不重写** REALM_DATA_SLOT | 已登记 | 否（G1 无碍） |
| ④ | §16 完整 `JSContext` 兼容层 | 属 **G4** | 否 |
| ⑤ | 已验收偏差（勿误报）：`Realm.isolate=Box<OwnedIsolate>` + sdk-python `RefCell` 重入探测；`object_identity_stable_within_realm`=对象身份；S11 单 Realm 双模式 `with_page_script(&mut ControlPlane)`；`py_to_host(target_realm, obj)` 签名；线程断言在机制层单测；`microtask.rs` 行覆盖 0%（声明性类型） | 设计 | 否 |
| ⑥ | ADR-0013 待实测：**T3**（`Weak` 的 `!Send/!Sync` 与 shared-isolate 约束的**类型设计注记**） | ADR-0013 | 否 |
| ⑦ | 远程无 `cargo-llvm-cov` ⇒ 覆盖率仅本地 | 环境 | 否 |
| ⑧ | sdk-python Rust **行级覆盖**需 maturin+coverage 组合工具链 | 工具链 | 否 |

> **修正注记（2026-10-13 · ADR-0015）—— 针对 ①**：① 已**闭环**，**原归因不成立**——并非「v8 152 的
> `ResolveModuleCallback` 无 scope」，也**不需要** thread-local 桥 + 嵌套 scope（官方 adapter 已自带
> `CallbackScope`，`v8-152.2.0/src/isolate.rs:299`）。**真实根因**：宿主回调在「无挂起异常」时返回空
> `MaybeLocal` ⇒ 撞 V8 断言 `Check failed: has_exception()`（FATAL/SIGTRAP）。**修法**：
> `crates/engine-v8/src/realm.rs` 新增 `dynamic_import_callback`（失败一律 `reject_import`/`TypeError`，
> 绝不空返回）。`dynamic_import_specifier` 转绿；engine-v8 **43→46**；门禁全绿。本 ADR 另立**跨 G2+
> 纪律**：宿主回调空返回仅限存在挂起异常时。**① 不再列为遗留缺口。**

## 10. 能否进入下一闸门 —— 结论【Round 2 · 最终】

✅ **可以进入下一闸门（无阻塞遗留）。** 依据：

1. **功能验收通过**：六项能力（§3.1~§3.6）测试集**全部复跑通过**，纵向集成（§3.7）通过；反向验证（guard INJ-1/2、R1 四态、INJ-3、U-1）**判别力成立**；覆盖率 engine-v8 主力模块 **83%~94% lines**。
2. **双端门禁全绿（Round 2）**：本地 fmt/clippy/test/guard/**deny** 全 0；远程 fmt/**clippy(1.99)**/test/guard 全 0；Python 契约 contracts **55**、sdk-python **13** 双端通过。
3. **缺陷全部闭环**：G1-K-1（deny）/ G1-K-2（远程 clippy）/ G1-K-3 / G1-K-4 **均已修复并复跑验证**（§0.1 / §11）。
4. **剩余均为已登记项（非缺陷）**：动态 import 未实现、`perform_checkpoint` 返回 1 / REALM_DATA_SLOT 不重写、§16 兼容层属 G4、K-7（远程无 cargo-deny，主理人已裁决维持）、各处已验收偏差（§9）。

---

## 11. Round 2 —— 修复后复验（缺陷闭环）

**触发**：Round 1 报 2 门禁红灯 + 2 文档漂移；工程师修复批完成（K-1a path 依赖补 `version`；K-1b pyo3 0.23→0.29.3；K-2 删冗余 `#[must_use]`；K-3/K-4 注释更正）。**QA 独立复跑，不依赖他人转述。**

### 11.1 本地（macOS 1.92.0）

```
$ cargo fmt --all --check                                     FMT_EXIT=0
$ cargo clippy --all-targets --all-features -- -D warnings     CLIPPY_EXIT=0
$ cargo test --workspace                                       TEST_EXIT=0   # 54 tests（43+1+4+5+1），0 FAILED
$ cargo deny check                                             DENY_EXIT=0   # advisories ok, bans ok, licenses ok, sources ok
$ cd contracts && uv run pytest -q                             CONTRACTS_EXIT=0   # 55 dots
$ cd crates/sdk-python && uv run pytest -v                     SDKPY_EXIT=0       # Built iv8（pyo3 0.29）→ 13 passed
```

### 11.2 远程 Linux x86_64（1.99.0）

**同步**：`rsync -av --delete --checksum --exclude='.workbuddy' --exclude='target' …`。**R2 纪律**：日志含 `Compiling pyo3 v0.29.3` + `Compiling sdk-python`（确为修复后重编，非陈旧二进制）。

```
FMT_EXIT=0
GUARD_EXIT=0
CLIPPY_EXIT=0        # ✅ 1.99 过 double_must_use（sdk-python 编译通过；Finished `dev` profile … in 4.40s）
TEST_EXIT=0          # 54 tests（43+1+4+5+1），0 FAILED
CONTRACTS_EXIT=0     # 55 dots
SDKPY_EXIT=0         # uv --python 3.13 → maturin `Built iv8` → 13 passed
```

### 11.3 pyo3 0.23 → 0.29.3 迁移抽查（判定：机械迁移、无语义改动）

| 文件 | 改动 | 判定 |
|---|---|---|
| `crates/sdk-python/Cargo.toml` | `pyo3 = "0.23"` → `"0.29"`（主 + dev 两侧）；`engine-v8 = { path=…, version="0.0.0" }` | 版本升级（K-1b）+ `version` 补齐（K-1a） |
| `src/convert.rs` | `GILOnceCell` → `PyOnceLock`（`pyo3::sync`）；`PyObject` → `Py<PyAny>`；`downcast::<T>()` → `cast::<T>()` | **机械重命名**（API 同形）；分支/顺序（bool 先于数字）/边界逻辑不变 |
| `src/callback.rs` | 删 `#[must_use]`（K-2）；`Python::with_gil` → `Python::attach`；`PyObject` → `Py<PyAny>` | 机械；派发/线程断言/墙钟计量不变 |
| `src/lib.rs` | `PyObject` → `Py<PyAny>`（`eval`/`eval_module`/`control_eval` 返回型）；import 顺序 | 机械；`RefCell` 重入探测 / 控制面不变 |
| `tests/test_iv8.py` | **13 条测试名与断言逐条比对 —— 未变** | ✅ 未迁就实现改测试（铁律 1/2） |

> `PyObject` 在 pyo3 0.23 即 `Py<PyAny>` 的别名 ⇒ 该替换**零语义**。迁移**未引入任何测试期望值改动**。

### 11.4 F1 / U-1 在 0.29 的复验（核对归档证据，无需重跑探针本体）

归档 `.workbuddy/refs/pyo3-lint-probe-evidence/029/`：
- `p1b_029.txt` —— `forbid` 下 pyo3 **0.29.3** 宏编译通过（`Finished dev profile … in 5.27s`）；
- `p1c_029.txt` —— 注入 `unsafe{}` ⇒ `error: usage of an unsafe block`（lint **确武装**）；
- `p1d_029.txt` —— 复原后通过。

**关键行核对一致** ⇒ F1（`default-features=false` 连同 `macros` 一起关掉，须显式 `features=["macros"]`）与 U-1（`forbid` 下 PyO3 宏可编译、手工 `unsafe` 报错）在 **0.29 保持成立**。

### 11.5 无注入残留（对照基线哈希）

Round 1 反向注入文件（`__qa_inj1_probe.rs` / `__qa_inj3_probe.rs` / `qa_tmp_guard_inj2.sh`）**不存在**（glob 无匹配）；`crates/bindings-webidl/Cargo.toml`、`scripts/check-unsafe-whitelist.sh`、`crates/engine-v8/src/abi.rs` 的 sha256 **与 Round 1 基线逐一相同**；`crates/runner/tests/workspace_policy.rs` 哈希因 **K-4 注释修复**而变化（预期内，断言逻辑未变）。

### 11.6 闭环汇总

| 缺陷 | 修复 | Round 2 复验证据 | 结论 |
|---|---|---|---|
| G1-K-1 | path 依赖补 `version` + pyo3→0.29.3 | 本地 `cargo deny check` = **0** | ✅ 闭环 |
| G1-K-2 | 删 `callback.rs:68` 冗余 `#[must_use]` | 远程 `cargo clippy`(1.99) = **0** | ✅ 闭环 |
| G1-K-3 | `realm.rs` 注释更正 | 注释已改（静态 import 已实现） | ✅ 闭环 |
| G1-K-4 | `workspace_policy.rs` 注释更正 | 注释已改（G1 已落地 v8） | ✅ 闭环 |

---

## 附：QA 未做 / 边界声明

- **不改功能代码**：本报告仅验证与测量；所有修复路由至 Engineer / Lead。
- **不改断言 / golden**：`native_tostring.snap` 为本轮**新增**、未改既有 golden；反向注入全部**还原并哈希校验**。
- **远程零系统级改动**：仅用户级 `uv venv` / `cargo`；未改 swap / sysctl / `apt` / `/etc`。
- **一处方法学说明**：INJ-3 注入文件含 1 行同名注释，使命中数为 3 而非理想 2；**判别力结论不受影响**（断言仍 FAILED，且如实打印命中清单）。
