# 门禁报告 —— 2026-10-12 第四轮（T2.2 / T2.4 / T2.5 / G1-PRE-1）

- 产出角色：QA（严过关）
- 事实源：`docs/TASKS.md`（本报告是过程与判定记录，**状态以 `docs/TASKS.md` 为准**）
- 任务事实源回写：已完成（T2.2 / T2.4 / T2.5 / G1-PRE-1 → `done`；T0.9 保持 `blocked`）
- 运行机：本地 macOS arm64（`rustc 1.92.0`）+ 远程 `ubuntu@124.221.1.78`（Ubuntu 24.04 · x86_64 · `rustc 1.99.0` · 3.6 GB 内存）

---

## 0. 结论（TL;DR）

| 项 | 结果 |
|---|---|
| 本地 Rust 五条门禁 | **全绿**（fmt / clippy / test / deny / 守卫，退出码均 0） |
| 本地 Python 侧 `uv run pytest` | **48 passed**，退出码 0 |
| 本地三个探针 | **全绿**（`T2_2_PROBE_OK` / `T2_4_PROBE_OK` / `T2_5_PROBE_OK`，退出码均 0） |
| 远程 Linux x86_64 主门禁 | **全绿**（fmt / clippy / test / guard，退出码均 0） |
| 远程 Python 侧 | **48 passed**，退出码 0 |
| 远程三个探针 | ✅ **全部实跑成功**（`T2_2_PROBE_OK` / `T2_4_PROBE_OK` / `T2_5_PROBE_OK`，退出码均 0）—— V8 静态库经**本地中转**供给 |
| 守卫（铁律 4） | **PASS**（首跑曾 FAIL 3 处，工程师修复后 PASS） |
| 缺陷闭环 | **K-1 / K-2 / K-4 / K-5 / K-6 全部闭环**（K-5 由远程 x86_64 Linux 复跑为证） |
| 是否可进入下一闸门 | **可以**（无阻塞遗留）—— K-3 **已决策**（不纳管，登记为已知偏差）；剩余 K-7 / K-8 / K-9 / K-10 均为**已登记项，非缺陷** |

**一句话**：本轮**可执行门禁全部通过**（本地 + 远程 + 远程三探针），**全部已发现缺陷均已修复并经 QA 双向复验**。
剩余事项**均为已登记项/已决策项，无缺陷、无阻塞**。

---

## 0.1 缺陷闭环总表（最终）

| # | 缺陷 | 状态 | 复验证据 |
|---|---|---|---|
| K-1 | T2.5 `D0a` 具名断言缺失 | **已修（eng-t25）** | 本地+远程 `WATCHDOG_USES_ISOLATE_HANDLE=true`、`ASSERT_PASS(m)` |
| K-2 | T2.5 `D0b` 具名断言缺失 | **已修（eng-t25）** | 本地+远程 `CLEAN_RESUME_AFTER_TERMINATE=Some(true)`、`ASSERT_PASS(n)` |
| K-3 | `poc/` 不纳入 root 门禁 | **格式已修**；**已决策（lead）：不纳管，维持 ADR-0008 设计，登记为已知偏差** | 三探针 `cargo fmt --all --check` = 0 |
| K-4 | ADR-0009 §A8 过度声明 | **已修（架构师）** | `grep` 无残留 |
| K-5 | T2.4 `C1` 硬编码常量（恒假门禁） | **已修（eng-t25）** | 远程 Linux `TARGET_LINUX_OK=true` / `VERIFIED_ON_LINUX` |
| K-6 | 远程探针不可行 | **已解决** | 本地中转 V8 归档，三探针远程实跑成功 |

---

## 1. 本地门禁（逐条：命令 / 退出码 / 原文）

| # | 命令 | 退出码 | 关键原文 |
|---|---|---|---|
| 1 | `cargo fmt --all --check` | **0** | （无输出） |
| 2 | `cargo clippy --all-targets --all-features -- -D warnings` | **0** | `Finished \`dev\` profile … in 0.63s` |
| 3 | `cargo test --workspace` | **0** | 40 处 `test result: ok.`，0 处 `FAILED` |
| 4 | `cargo deny check` | **0** | `advisories ok, bans ok, licenses ok, sources ok` |
| 5 | `bash scripts/check-unsafe-whitelist.sh` | **0** | `== PASS：白名单外无 unsafe；无 crate/mod 级开口；白名单内 unsafe 均带 SAFETY 注释 ==` |

```
$ cargo fmt --all --check
FMT_EXIT=0
$ cargo clippy --all-targets --all-features -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.63s
CLIPPY_EXIT=0
$ cargo test --workspace
TEST_EXIT=0
$ cargo deny check
advisories ok, bans ok, licenses ok, sources ok
DENY_EXIT=0
$ bash scripts/check-unsafe-whitelist.sh
== PASS：白名单外无 unsafe；无 crate/mod 级开口；白名单内 unsafe 均带 SAFETY 注释 ==
GUARD_EXIT=0
```

**非致命警告（登记，不影响退出码）**：`cargo deny check` 有 3 条 `warning[license-not-encountered]` —— `deny.toml:42-44` 的 `Unicode-3.0` / `Unicode-DFS-2016` / `Zlib` 未被实际依赖使用。

## 2. 本地 Python 侧

```
$ cd contracts && uv run pytest
48 passed in 1.67s
PYTEST_EXIT=0
```

## 3. 本地三个探针

三个探针各自是**独立 workspace**（`Cargo.toml` 含 `[workspace]`），故 root 的 `cargo test --workspace` **不含**它们，必须单独跑。

| 探针 | 命令 | 退出码 | 汇总标记 | 断言数 |
|---|---|---|---|---|
| T2.2 | `cd poc/t2_2_clock_tz && cargo run --offline` | **0** | `T2_2_PROBE_OK` | **11**（`a b c d e f g h0 h1 h2 h3`） |
| T2.4 | `cd poc/t2_4_blitz_probe && cargo run --offline` | **0** | `T2_4_PROBE_OK` + `T2_4_VERDICT=SWITCH_TO_STYLO` | **19**（`ASSERTIONS_RUN=19`） |
| T2.5 | `cd poc/t2_5_inspector_deadlock && cargo run --offline`（**隔离跑**） | **0** | `T2_5_PROBE_OK` | 见下方条件表 |

**T2.5 条件表汇总原文**（冻结快照，隔离运行）：

```
COMBOS_TRIED=8
COMBO_RUNS=14
COMBOS_DEADLOCK=3
COMBOS_UNMEASURABLE=4
UNMEASURABLE_REASON=two-factor-confound(F1&F2 或 F2&F3 同时为真时无法归因)
```
（`COMBO_RUNS=14` = 组合 1/2/3/7/8 各 1 次 + 组合 4/5/6 各 3 策略（`spin_holding_gil` / `sleeping` / `not_holding`）。）

**复现性核对**：T2.5 提交的 `run-output.txt`（534 行正文）与 QA 独立复跑**逐行比对**，差异 74 行**全部**为 `RESUMED_AT` / `WATCHDOG_FIRED_AT` / `CONTROL_MSGS_SENT` 三个非确定性字段；所有 `VERDICT`、分类行、断言行**逐字一致** ⇒ 结论可复现。

> ⚠️ **隔离要求**：T2.5 必须**空载运行**（见 §10 负载伪影说明）。

## 4. 本地补测：`poc/` 门禁覆盖盲区（root 门禁不覆盖三个探针）

| 探针 | `cargo fmt --all --check`（首测 → **冻结复验**） | `cargo test` |
|---|---|---|
| `poc/t2_2_clock_tz` | **0** → **0** | 0 tests |
| `poc/t2_4_blitz_probe` | **1（未格式化）** → **0** | **3 passed**（含 `#[should_panic]` 回归契约 + `catch_unwind` 正面对照） |
| `poc/t2_5_inspector_deadlock` | **1（未格式化）** → **0** | 0 tests |

⇒ 格式问题**已由 eng-t24 / eng-t25 修复**（现均 = 0）。**`poc/` 不纳入 root 门禁**：三探针各是独立 workspace，root 的 fmt/clippy/test 不覆盖 —— 这是 **ADR-0008 决策 1 的既有设计**；**lead 已决策：不纳管，登记为已知偏差**（K-3）。如未来纳入，须维护者批准后动 CI / `AGENTS.md`（铁律 7）。

**已知偏差（历史探针，按 lead 决策保留原状）**：

| 探针 | `cargo fmt --all --check` | 处置 |
|---|---|---|
| `poc/t2_0_v8_embed` | **1**（未格式化） | **保留原状**（其证据台账含行号引用，改写会漂移） |
| `poc/t2_1_cppgc_cycle` | **1**（未格式化） | **保留原状**（同上） |
| `poc/t2_3_html5ever_pause` | **0** | 本就干净 |

⇒ 记为「**历史探针 `t2_0` / `t2_1` 不在 fmt 门禁内（ADR-0008 决策 1 的既有设计，非疏漏）；本轮在途探针 `t2_2` / `t2_4` / `t2_5` 已全部归一**」。

## 5. 远程门禁（Linux x86_64）

**主门禁**（`nohup` + 轮询；`-j2` 防 OOM）—— 四条全 0：

```
$ ssh ubuntu@124.221.1.78 '<...> cd ~/mmv8 && cargo fmt --all --check && cargo clippy -j2 --all-targets --all-features -- -D warnings && cargo test -j2 --workspace && bash scripts/check-unsafe-whitelist.sh'
FMT_EXIT=0
CLIPPY_EXIT=0
TEST_EXIT=0
GUARD_EXIT=0
```

**远程 Python 侧**：`uv run pytest` → `48 passed in 0.13s`，`PYTEST_EXIT=0`（`uv 0.13.0`）。

**远程三个探针：✅ 全部实跑成功**（退出码均 0）

首次尝试受阻于 V8 静态库下载，QA 改用**本地中转**（未改远程系统配置，只用 `v8` crate 自带的 `RUSTY_V8_ARCHIVE` 机制）后通过：

- 远程直连 GitHub 拉 V8 归档实测 **8,046 B/s**（`…/librusty_v8_release_x86_64-unknown-linux-gnu.a.gz` → HTTP 200，**161,553 B / 20.07 s**；归档 **39,784,686 B**）⇒ 约 **82 分钟 / workspace**；
- 国内镜像候选（`rsproxy.cn/rusty_v8/…`、`cdn.npmmirror.com/binaries/rusty_v8/…`、`mirrors.tuna.tsinghua.edu.cn/rusty_v8/…`）**全部 404**；
- 本机（macOS）同一 URL **360 KB/s ≈ 45× 快** → 下载 39,784,686 B → `scp` 到远程 → 两端 `sha256` 一致 `b6683e9afcb77fbd8cb2c3acf45d34d52029ab216b332522c95179c12f0fed3c`；
- 远程以 `RUSTY_V8_ARCHIVE=$HOME/librusty_v8_linux.a.gz` 构建 ⇒ 三探针全部通过。

```
T2_2  poc/t2_2_clock_tz            T2_2_EXIT=0   → T2_2_PROBE_OK（ASSERT_PASS×11）
T2_4  poc/t2_4_blitz_probe         T2_4_EXIT=0   → T2_4_PROBE_OK / SWITCH_TO_STYLO / ASSERTIONS_RUN=19（18 PASS + 1 FAIL(c9)）
T2_5  poc/t2_5_inspector_deadlock  T2_5_EXIT=0   → T2_5_PROBE_OK
      COMBOS_TRIED=8  COMBO_RUNS=14  COMBOS_DEADLOCK=3  COMBOS_UNMEASURABLE=4
```

**🔴→✅ K-5 的发现与修复（本轮最有价值的一条 QA 发现）**

- **修复前**：T2.4 在 `x86_64` Linux 上**编译并运行成功**（`TARGET_HOST_OS=linux`、`TARGET_HOST_ARCH=x86_64`、`T2_4_PROBE_OK`），**但 `TARGET_LINUX_OK` 仍输出 `false`** —— 因为 `main.rs:388` 是**手写常量** `out.target_linux_ok = false;`（`main.rs:390` 状态串、`main.rs:978` 裁决串同样是硬编码）。⇒ C1 是**恒假门禁**。
- **修复后（QA 远程复验，日志含 `Compiling blitzprobe` ⇒ 确为修复后重编）**：

```
TARGET_HOST_OS=linux            TARGET_HOST_ARCH=x86_64
TARGET_MAC_OK=false             TARGET_LINUX_OK=true
TARGET_LINUX_STATUS=VERIFIED_ON_LINUX (本探针正运行于 x86_64-unknown-linux-gnu，C1 就地验证)
T2_4_VERDICT=SWITCH_TO_STYLO
VERDICT_DRIVER=C9 SHADOW_DOM_WORKS=false（…）+ C1 TARGET_LINUX_OK=true（x86_64 Linux 本机实跑；即便 C1 为 true，C9 仍为 false，裁决不变）
ASSERTIONS_RUN=19    T2_4_PROBE_OK    T2_4_EXIT=0
```
⇒ **K-5 闭环**：同一台远程机、同一命令，修复前 `false` → 修复后 **`true` + `VERIFIED_ON_LINUX`**。

**⚠️ 远程复验踩到的一个陷阱（rsync-mtime 陈旧构建）**：`rsync -a` 保留源 mtime，而 cargo 的新旧判定基于 mtime。首次远程跑 T2.5 时，日志**无 `Compiling` 行**且输出**缺 K-1/K-2 标记** ⇒ 实际跑的是**陈旧二进制**（synced 源码 mtime `21:52` 早于远程既有产物 `21:58`）。**甄别方法**：查日志有无 `Compiling <crate>` + 输出是否含该源码应有的标记。**处置**：远程 `touch` 源文件刷新 mtime 后重跑（仅改 mtime、不改内容；**未改远程系统配置**），重跑后 K-1/K-2 标记齐备。

> 备注：远程主门禁**不链接 V8**（`crates/engine-v8` 当前未依赖 `v8` crate，`src/` 只有 `lib.rs`），故内存无压力、未触发 OOM；探针构建/运行亦未 OOM（`-j2`，峰值内存充裕）。

## 6. 未覆盖 / 遗留项（随行下一闸门）

| # | 项 | 归因 | 路由 | 阻塞下一闸门？ |
|---|---|---|---|---|
| K-1 | ~~T2.5 **D0a** 具名断言未输出~~ → **已修**（`watchdog.rs:66` 常量 + `main.rs:285` 打印 + `ASSERT_PASS(m)`） | 探针代码 | eng-t25（**已修，QA 本地+远程复验**） | 否（已闭环） |
| K-2 | ~~T2.5 **D0b** 具名断言未输出~~ → **已修**（`main.rs:300` 打印 + `ASSERT_PASS(n)`；ADR-0012 新增「D0a / D0b」节） | 探针代码 / ADR | eng-t25（**已修，QA 本地+远程复验**） | 否（已闭环） |
| K-3 | `poc/t2_4_blitz_probe`、`poc/t2_5_inspector_deadlock` 曾**未过 `cargo fmt --check`**（**现均 = 0**）；且 `poc/*` 是独立 workspace，**root fmt/clippy/test 都不覆盖**（ADR-0008 决策 1 的既有设计，**已决策：不纳管，登记为已知偏差**） | 探针代码 | eng-t24（**已修，QA 复验 PASS**）；**纳管与否已决策（lead）：不纳管** | 否（已知偏差） |
| K-4 | **ADR-0009 §A8 过度声明**：「四条断言全部做过双向验证」与 §A9「本轮不要求实现门禁」自相矛盾；仓库佐证未实现（守卫无规则 5、`workspace_policy.rs` 只有 3 测试、无 `abi.rs`、无 `v8_abi.py`）；§A8 第 418 行还引用已废弃的 `v8-abi` | 架构师（ADR） | 架构师（**已修，QA 复验无残留**） | 否（已闭环） |
| K-5 | ~~T2.4 `C1` 硬编码常量（恒假门禁）~~ → **已修**：`main.rs:388` 由 `out.target_linux_ok = false;` 改为 `target_os == "linux" && target_arch == "x86_64"`；状态串/裁决串条件化。**远程 x86_64 Linux 实测 `TARGET_LINUX_OK=true` / `VERIFIED_ON_LINUX`**（修复前同命令为 `false` / `PENDING_REMOTE`） | 探针代码 | eng-t25（**已修，QA 远程复验证实**） | 否（已闭环） |
| K-6 | ~~远程探针不可行~~ → **已解决**（本地中转 V8 归档 + `RUSTY_V8_ARCHIVE`，详见 §5）；远程三探针全部实跑成功 | 环境 | **已闭环** | 否 |
| K-7 | 远程无 `cargo-deny` ⇒ 远程门禁**天然不含 deny** | 环境 | 已知（与 T0.8 同口径） | 否 |
| K-8 | `deny.toml` 3 条未命中 license 白名单（噪音） | 配置 | 可清理 | 否 |
| K-9 | T2.2「同一 Isolate 内 A/B 时区并存」**未测**（ADR-0010 自登记，且明示不得据此推断并存一定失败） | 范围决策 | 已登记 | 否 |
| K-10 | T2.4 **C4**（html5ever 0.39 上重跑 T2.3 断言）移出本轮 → **T2.3-bis** | 范围决策 | 已登记 | 否 |

## 7. 智能路由判定（逐条归因）

- **源码 / 探针 Bug**：无「实现错误导致断言失败」的情形。三个探针在**修复后源码**上**全部 OK**。
  - 一处历史失败：守卫首跑 3 处 `missing-safety-comment`（`SAFETY:` 注释在 ±2 行窗口外）→ 归 **eng-t22 / eng-t25**，**已修并复跑 PASS**。
  - 一次编译失败（T2.5，11 个编译错误）是 QA 与 eng-t25 编辑窗口**抢跑**所致，非缺陷；冻结后复跑 OK。
  - **K-5**：T2.4 `C1 TARGET_LINUX_OK` 原为**硬编码常量**（恒假门禁）⇒ 探针缺陷，归 **eng-t25**，**已修并由远程 Linux 复跑证实**。
- **测试 / 探针代码问题**：K-1 / K-2 / K-3 / K-5 → **QA 不自行修**（`poc/**` 归工程师），已路由并**全部闭环**。
- **文档与实现不符**：K-4 → 架构师（**已修**）。
- **已知遗留（非 Bug）**：K-6（已闭环）/ K-7 / K-8 / K-9 / K-10 → 登记。

> QA **未为迁就实现修改任何 golden / 断言期望值**（铁律 2）。T2.4 的 `ASSERT_FAIL(c9)` 如实保留，其「不通过」正是 `SWITCH_TO_STYLO` 裁决的依据；对冲断言 `c9-probe` 通过，证明判定方法有效（非恒假门禁）。

## 8. 能否进入下一闸门 —— 结论

**可以进入下一闸门（无阻塞遗留）**，依据：

1. 本轮**所有可执行门禁**（本地五条 + 本地 pytest + 本地三探针 + 远程主门禁 + 远程 pytest + **远程三探针**）**全部通过**；
2. 四项任务（T2.2 / T2.4 / T2.5 / G1-PRE-1）**均满足 `docs/TASKS.md` 的验收标准**（最小可运行 + 断言 + 结论落 ADR），并已在证据台账留原文；
3. **全部已发现缺陷（K-1 / K-2 / K-4 / K-5 / K-6）均已闭环**，其中 K-1 / K-2 / K-5 经 QA **本地 + 远程双向复验**。

**随行条件（带入下一闸门）** —— **无缺陷项，仅剩决策与已登记项**：

- **K-3**（**已决策，lead：不纳管**）：`poc/` 维持独立 workspace、不纳入 root 门禁（ADR-0008 决策 1）；在途三探针格式**已归一**，历史探针 `t2_0` / `t2_1` 保留原状；**poc 交付约定「交付前跑 `cargo fmt --all --check`」**；如未来纳入须维护者批准后动 CI / `AGENTS.md`；
- **K-7**（远程无 `cargo-deny`，与 T0.8 同口径）、**K-8**（`deny.toml` 3 条未命中白名单，噪音）：**已登记，非缺陷**；
- **K-9**（T2.2「A/B 并存」未测）、**K-10**（T2.4 `C4` → T2.3-bis）：**范围决策，已登记（ADR-0010 / 0011）**。

## 9. 修复复验记录（回归）

| 项 | 修复者 | QA 复验 | 结论 |
|---|---|---|---|
| K-3 `poc/` 格式 | eng-t24 | `t2_2` / `t2_4_blitz_probe` / `t2_5_inspector_deadlock` 的 `cargo fmt --all --check` **全部 = 0**（原 `t2_4`/`t2_5` 为 1） | **已修** |
| K-3 回归 | — | `t2_4` 复跑 `T2_4_PROBE_OK` + `SWITCH_TO_STYLO` + `ASSERTIONS_RUN=19`，`cargo test` 3 passed（退出码 0）；`t2_5` 复跑 `T2_5_PROBE_OK`、`grep -c '^ASSERT_FAIL'` = 0、`COMBOS_*` 不变（退出码 0） | **无破坏** |
| K-4 ADR-0009 §A8 | 架构师 | `grep "四条断言全部做过双向验证\|v8-abi"` **无输出** | **已修** |
| 守卫 | — | `bash scripts/check-unsafe-whitelist.sh` → **PASS / 0** | **保持 PASS** |
| **K-1 / K-2** | eng-t25 | **本地**：`WATCHDOG_USES_ISOLATE_HANDLE=true`、`CLEAN_RESUME_AFTER_TERMINATE=Some(true)`、`ASSERT_PASS(m)(n)`、15 条 PASS / 0 条 FAIL。**远程**：同样齐备（经 `touch` 强制重编后）。 | **已修（本地+远程）** |
| **K-5** | eng-t25 | **本地 macOS**：`TARGET_LINUX_OK=false` + `PENDING_REMOTE`（照实）。**远程 x86_64 Linux**：`TARGET_LINUX_OK=true` + `VERIFIED_ON_LINUX`（日志含 `Compiling blitzprobe`）。 | **已修（远程为证）** |

## 10. 冻结快照复验（本地 + 远程，2026-10-12 22:00 前后）

**原则**：不复用未冻结快照的结果；对【冻结】后的快照独立复跑，且 **T2.5 隔离运行**（不与任何其他 cargo 任务并发）。

| 复验项 | 结果 |
|---|---|
| 本地五条（fmt / clippy / test / deny / guard） | 退出码 **0 / 0 / 0 / 0 / 0**（40 处 `test result: ok`） |
| 本地 `uv run pytest` | **48 passed**（0） |
| 本地 T2.2 | `T2_2_PROBE_OK`，**11** 条 `ASSERT_PASS`，退出码 0 |
| 本地 T2.4 | `T2_4_PROBE_OK` / `SWITCH_TO_STYLO` / `ASSERTIONS_RUN=19`（18 PASS + 1 FAIL(c9)），`cargo test` 3 passed，退出码 0 |
| 本地 T2.5（**隔离**） | `T2_5_PROBE_OK`，`COMBOS_TRIED=8 / COMBO_RUNS=14 / COMBOS_DEADLOCK=3 / COMBOS_UNMEASURABLE=4`，0 处 `ASSERT_FAIL`，退出码 0；与 ADR-0012 条件表**逐项吻合** |
| 本地三探针 `cargo fmt --all --check` | 均 **0** |
| 远程五条（fmt / clippy / test / guard） | 退出码 **0 / 0 / 0 / 0**（远程无 `cargo-deny`，与 T0.8 一致） |
| 远程 `uv run pytest` | **48 passed**（0） |
| 远程 T2.2 / T2.4 / T2.5 | 退出码 **0 / 0 / 0**，均 `*_PROBE_OK` |

**负载伪影说明（如实记录）**：QA 首轮在**与其他 cargo 并发**的情况下跑 T2.5，曾观测到组合 5（`sleeping`）翻转成 `TIMEOUT`（`GIL_BLOCKED_MS=10183`）。
按 ADR-0012「运行环境要求」节，该探针**必须空载运行**；**隔离复跑后不复现**，与条件表逐项吻合 ⇒ 判定为**负载伪影**，非探针竞态，**未据此判失败**。

## 11. 缺陷修复终验（K-1 / K-2 / K-5，2026-10-12 22:2x–22:3x）

eng-t25 修复落盘：`t2_5/{main,watchdog}.rs` 21:51–21:52、`run-output.txt` 572 行、ADR-0012 556 行（含「D0a / D0b」节）；T2.4 `main.rs` 22:18:50。
**QA 独立复跑，不依赖他人转述。**

### 11.1 K-5 —— T2.4 `C1` 远程 Linux 复验（修复有效性的唯一证明）

| 环境 | 命令 | `TARGET_LINUX_OK` | 状态串 | 退出码 |
|---|---|---|---|---|
| 本地 macOS | `cargo run --offline` | `false` | `PENDING_REMOTE` | 0 |
| **远程 x86_64 Linux（修复前）** | `cargo run -j2` | **`false`**（恒假） | `PENDING_REMOTE` | 0 |
| **远程 x86_64 Linux（修复后）** | `cargo run -j2`（日志含 `Compiling blitzprobe`） | ✅ **`true`** | ✅ `VERIFIED_ON_LINUX (本探针正运行于 x86_64-unknown-linux-gnu，C1 就地验证)` | 0 |

裁决串同步动态化：`C1 TARGET_LINUX_OK=true（x86_64 Linux 本机实跑；即便 C1 为 true，C9 仍为 false，裁决不变）`；`T2_4_VERDICT=SWITCH_TO_STYLO`、`ASSERTIONS_RUN=19`、`T2_4_PROBE_OK` 均保持。
本地 `cargo test --offline` → **3 passed**（含 `should_panic` 回归契约）。

### 11.2 K-1 / K-2 —— T2.5 具名断言（本地 + 远程）

```
WATCHDOG_USES_ISOLATE_HANDLE=true
CLEAN_RESUME_AFTER_TERMINATE=Some(true)
ASSERT_PASS(m): D0a：watchdog 走 IsolateHandle 路径（thread_safe_handle → terminate_execution），且该事实出现在实跑证据中
ASSERT_PASS(n): D0b：终止请求发出后同一 Isolate 仍可干净复用（同 scope 再跑 1+1 成功）—— 边界已由断言覆盖，非悬空
COMBOS_TRIED=8  COMBO_RUNS=14  COMBOS_DEADLOCK=3  COMBOS_UNMEASURABLE=4   T2_5_PROBE_OK
```
本地与远程**均**齐备；15 条 `ASSERT_PASS`（a a2 b c d env g h i i2 j k l m n），**0 条 `ASSERT_FAIL`**。

> ⚠️ **远程首次跑命中 rsync-mtime 陈旧构建陷阱**：日志**无 `Compiling` 行** + 输出**缺 K-1/K-2 标记** ⇒ 实际跑的是**陈旧二进制**（synced 源码 mtime `21:52` 早于远程既有产物 `21:58`，cargo 据 mtime 判定「无需重编」）。
> **处置**：远程 `touch` 源文件刷新 mtime 后重跑，重跑日志出现 `Compiling deadlockprobe`，标记齐备。
> **⚠️ 如实声明**：`touch` **只改 mtime、未改任何文件内容** —— QA 事后比对两端 `sha256` **完全一致**（`main.rs = 3d3fa4af…`、`watchdog.rs = 4161efdd…`），未改远程系统配置。
> **沉淀**：跨机同步后，**必须看日志有无 `Compiling <crate>` 行**，否则可能拿陈旧二进制当证据。

### 11.3 修复后本地门禁全量复验

```
cargo fmt --all --check        FMT_EXIT=0
cargo clippy --all-targets --all-features -- -D warnings   CLIPPY_EXIT=0
cargo test --workspace         TEST_EXIT=0（40 处 test result: ok）
cargo deny check               DENY_EXIT=0
bash scripts/check-unsafe-whitelist.sh   GUARD_EXIT=0（PASS）
cd contracts && uv run pytest  48 passed
poc fmt：t2_2 = 0   t2_4_blitz_probe = 0   t2_5_inspector_deadlock = 0
```

⇒ **K-1 / K-2 / K-5 全部闭环，修复未引入回归。** 剩余 K-3（**已决策：不纳管**，登记为已知偏差）/ K-7 / K-8 / K-9 / K-10 均为**已登记项，非缺陷**。
