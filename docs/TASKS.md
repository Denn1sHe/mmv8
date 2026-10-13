# TASKS.md —— iv8 自研运行时 · 多 agent 共享任务事实源

> 本文件是**唯一**的任务状态事实源（`docs/adr/` 是唯一的架构事实源，两者不混用）。
> 任何角色开始工作前先读本文件；任何状态变更**在同一轮工作内**回写本文件，
> 不依赖对话记忆（AGENTS.md 会话纪律）。
>
> 角色定义见 `docs/handoff-开发启动prompt.md` 第 0 节：
> **lead**（拆解/ADR/决策上报，不写功能代码）、**dev**（实现，一事一 PR）、
> **review**（按铁律打回，不改功能代码）、**test**（测试与门禁，不改 golden 迁就实现）。
>
> 协作闭环：dev 提交 → review 审 → test 补测并跑门禁 → lead 合并。任一环红灯不进下一步。
>
> **状态为 `done` 的条目必须在「证据台账」有对应原文；没有证据的一律不得标 `done`（铁律 9）。**

- 最近更新：2026-10-13（**第七轮启动：G3 布局复用——设计批已下发**；**D-04 解锁**：仓库已挂 origin + SSH 首推成功（`5b41d48`）——CODEOWNERS 句柄替换待维护者一句确认；第六轮 G2 已完成：S0~S17 全交付、QA 两轮全绿 + 判别力获证、双端门禁全绿、覆盖率 83.47%R/82.66%L）
- ✅ **第五轮完成（2026-10-13 · G1 引擎纵向）**：决策链闭合（ADR-0013/0014 落档、ADR-0009/0012 修正注记）；engine-v8 43、sdk-python 13、contracts 55（**第六轮末分别达 52 / 22 / 60**）；`cargo deny` exit 0（pyo3 升 **0.29.3** 清两个安全公告）；远程 Linux 全绿（含 1.99 clippy）；K-11 已修复（ADR-0015 已落）。详细台账见文末「第五轮证据台账」。
- 当前阶段：T0 / T1 / T2 / **G1 / G2 全部完成**；**第七轮（G3 布局复用验证）进行中**（设计批已下发）。**D-04 已解锁**（2026-10-13）：仓库 `https://github.com/Denn1sHe/mmv8.git`——origin 已挂（fetch HTTPS / push SSH）、**首提交 `5b41d48` 已推送**；提交身份 = 本机既有 `dennis / dennis.he@mirmiles.com`；**CODEOWNERS 句柄 `@dennis → @Denn1sHe` 替换待维护者一句确认**（铁律 7）。详细台账见文末「第六轮证据台账」。
  ✅ 远程 Linux x86_64 上**三个新探针已实跑成功**（`T2_2_PROBE_OK` / `T2_4_PROBE_OK` / `T2_5_PROBE_OK`，退出码均 0）。远程 V8 静态库经由**本地中转**（`RUSTY_V8_ARCHIVE`）供给，详见证据台账「第四轮 · 远程」。
  ✅ 三条缺陷已闭环：**K-1 / K-2**（T2.5 的 `WATCHDOG_USES_ISOLATE_HANDLE` / `CLEAN_RESUME_AFTER_TERMINATE` 具名断言，本地+远程均见）、**K-5**（T2.4 `C1` 由硬编码常量改为**平台判定**，远程 x86_64 Linux 上输出 `TARGET_LINUX_OK=true` + `VERIFIED_ON_LINUX`）。
  ✅ 剩余事项**均为已登记项 / 已决策项，非缺陷、无未决项**：**K-3 已决策（lead）：`poc/` 维持独立、不纳入 root fmt/clippy/test 门禁**（维持 ADR-0008 决策 1 的既有设计，登记为已知偏差）；K-7（远程无 `cargo-deny`）、K-8（`deny.toml` 未命中项噪音）、K-9 / K-10（范围决策，已登记）。详见「本轮遗留与已知问题」。

> ### 授权变更（2026-10-12，维护者指示原文）
>
> > 「完成所有开发。拿不准的你们 ai 内部开多个子 agent 讨论根据讨论结果来决策就行。」
>
> 含义：决策上报协议的**执行方式**由「交外部 AI 讨论后带回结论」变更为
> **「AI 内部开多个独立子 agent 辩论 → 结论落 ADR → 依 ADR 执行」**。
> 协议的**形式要求不变**（自包含、≥2 备选方案含代码原文、证据原文、本仓库约束条款、
> 推荐须标注"仅供参考"）；唯一变化是**评审者由外部 AI 改为内部独立子 agent**，
> 且结论不再阻塞等待，**落 ADR 后即可执行**。
>
> 因此 T0.4 / T0.6 / T0.9 的 `blocked` 状态由本轮解除：
> D-01（unsafe 白名单）→ 落 ADR-0002；G1-PRE-1（V8 类型退出）→ 落 ADR-0003；
> D-02（许可证）/ D-03（MSRV 与 CI 矩阵）/ D-04（托管与 CODEOWNERS）/ D-05（守卫接入）
> → 落 ADR-0004 并直接执行。**铁律 7 保护文件的改动仍须在 ADR 中逐条记录理由**。

## 状态图例

| 标记 | 含义 |
|---|---|
| `todo` | 未开始 |
| `wip` | 进行中（须注明角色） |
| `blocked` | 被外部条件阻塞（须写明阻塞物与责任人） |
| `done` | 验收通过（须在证据台账附原文） |

---

## T0 — 工程底座

| ID | 任务 | 角色 | 状态 | 验收标准 |
|---|---|---|---|---|
| T0.1 | 按技术方案 §16 建立 Cargo workspace 骨架（19 个成员 crate） | dev | done | `cargo metadata` 可解析；19 个成员全部编译通过 |
| T0.2 | workspace 级 lint（`unsafe_code`、`missing_docs`、clippy all/pedantic） | dev | done | 18 个非白名单成员经 `[lints] workspace = true` 继承；白名单 crate `engine-v8` 单独声明等价集合（见 D-01）；clippy `-D warnings` 全绿 |
| T0.3 | 测试基建就位（`insta` 快照 + `proptest` 属性测试） | dev/test | done | 二者在真实测试中运行通过（非仅声明依赖） |
| T0.4 | unsafe 白名单机制可实施（engine-v8 单点放行） | lead/dev | done | 方案 A 已拍板（[ADR-0002](adr/0002-unsafe-白名单最终形态.md)）；三条必跑路径 + 三条反向注入验证 |
| T0.5 | unsafe 白名单守卫脚本 | dev | done | 干净仓库 PASS；10 个注入场景全部按预期 PASS/FAIL |
| T0.6 | 门禁文件（`.github/workflows/ci.yml`、`CODEOWNERS`、`deny.toml`） | lead | done | ci.yml 已按 ADR-0005 更新（4 jobs：lint / contract / test / deny）；仍需维护者提供托管地址以启用分支保护 |
| T0.7 | 本地门禁全绿（fmt / clippy / test / deny / 守卫） | test | done | 五条命令退出码均为 0 |
| T0.8 | 远程 Linux x86_64 门禁全绿 | test | done | 远程 `~/mmv8` 上 `cargo fmt --check` / `cargo clippy` / `cargo test` 退出码均为 0 |
| T0.9 | 版本控制基线（`git init` + 首次提交 + PR 模板） | lead | **blocked** | 仓库已 `git init -b main`；**首次提交身份与托管地址须维护者提供（D-04），AI 不代填** |
| T0.10 | workspace 一致性门禁（名单 ↔ Cargo.toml、lint 防漂移） | dev/test | done | 三条断言通过；且三个反向注入场景均被抓住 |
| T0.11 | 脚手架工具链文件（`rust-toolchain.toml` / `rustfmt.toml` / `.gitignore`） | dev | done | 本地与远程均按同一 channel 与组件解析 |

### T0 遗留的待拍板项 —— 第三轮已全部决策（维护者授权 AI 内部决策）

| 编号 | 事项 | 决策结果 | 落档 |
|---|---|---|---|
| D-01 | `unsafe_code = "forbid"` 与「白名单可用 unsafe」互斥 | **方案 A**：全仓 `forbid` + `engine-v8` 单独声明 `deny` + 机器守卫。否决「拆独立 FFI crate」（破坏 §16 成员边界）与「只靠 geiger」（无法区分合法与裸 unsafe） | [ADR-0002](adr/0002-unsafe-白名单最终形态.md) |
| D-02 | 仓库许可证 | `Apache-2.0 OR MIT` 双许可；MPL-2.0 依赖的合规约束已说明 | [ADR-0005](adr/0005-t0遗留待拍板项的决策.md) 决策 1 |
| D-03 | MSRV 与 CI 矩阵 | 「声明即验证」：`rust-version = "1.85"` 暂不作为门槛，CI 矩阵固定 `stable` + `1.92.0`；真实 MSRV 待 `cargo-msrv` 自动探测后回写。`cargo geiger` 以 **warnings 级** 接入 | [ADR-0005](adr/0005-t0遗留待拍板项的决策.md) 决策 2 |
| D-04 | 托管地址与 CODEOWNERS 句柄 | **仍需维护者提供**：AI 不代填提交身份与托管地址。`CODEOWNERS` 的 `@dennis` 仍是占位，文件头已标注「句柄错误则保护静默失效」 | — |
| D-05 | 守卫脚本接入必跑路径 | **已接入三条**：`ci.yml` lint job、`AGENTS.md` 自检清单、本地开发流程 | [ADR-0005](adr/0005-t0遗留待拍板项的决策.md) 决策 4 |

### T0 交付物清单（本轮新增/修改的文件）

| 文件 | 说明 |
|---|---|
| `Cargo.toml` | workspace 根：19 成员、resolver 3、workspace.package / dependencies / lints |
| `crates/<19 个>/Cargo.toml`、`crates/<19 个>/src/lib.rs` | 19 个成员骨架（`runner` 含脚手架自检代码） |
| `crates/runner/tests/skeleton_smoke.rs` | 名单/属性/快照冒烟测试（4 个） |
| `crates/runner/tests/workspace_policy.rs` | workspace 一致性门禁（3 个）；脚手架期寄放，`xtask` 就位后迁出 |
| `crates/runner/tests/snapshots/skeleton_smoke__t0_scaffold_status.snap` | 首个 golden；**仅脚手架期有效**，G0 后随接口一并删除（须独立 PR） |
| `rust-toolchain.toml` / `rustfmt.toml` / `.gitignore` | 工具链与跨平台一致性 |
| `deny.toml` / `CODEOWNERS` / `.github/workflows/ci.yml` | 门禁文件草案（**待批准**） |
| `scripts/check-unsafe-whitelist.sh` | 铁律 4 的机器执行部分 |
| `docs/TASKS.md`、`docs/decision-briefs/2026-10-12-T0-unsafe-whitelist.md` | 任务事实源与决策上报 |

---

## 本轮 review 复核结论与处置（第二轮）

独立 **review 角色**（只读）对本轮交付做了逐项审查，提出 3 项 P0、8 项 P1、7 项 P2。
处置如下：

| 编号 | 问题 | 处置 | 状态 |
|---|---|---|---|
| P0-1 | 守卫脚本没有任何必跑路径（不在 AGENTS.md 自检清单，CI 又待批准） | 记录为 **D-05**；脚本本身已修正并在 CI 中挂载（待批准生效） | 部分（脚本已就绪；接入需批准） |
| P0-2 | 白名单 crate 的 `deny` 可被一行 crate 级 `#![allow(unsafe_code)]` 静默重置；守卫脚本原规则 2 恰好跳过白名单 | 守卫脚本新增规则 2（crate 级开口一律违规）与规则 4（禁止 mod/impl 级开口）；已补入 D-01 证据 | 已修（形态仍待 D-01 拍板） |
| P0-3 | TASKS 把 T0.1/T0.2/T0.7 标 `done` 但证据台账为空 | 本轮补全证据原文；并把「无证据不得标 done」写入本文件头部 | 已修 |
| P1-1 | 名单测试只比数量，抓不住成员漂移 | 新增 `workspace_policy.rs` 三条断言，集合**完全相等**；反向注入已验证 | 已修 |
| P1-2 | CI 的 `--all-features` 与 deny.toml 的 `all-features = false` 口径相反 | CI 去掉 `--all-features`，两侧加注释互相约束 | 已修 |
| P1-3 | MSRV 注释声称「CI 矩阵据此校验」但无此矩阵 | 注释改为「意向值，未进 CI」，登记 D-03 | 已修 |
| P1-4 | CI 与 AGENTS.md 自检口径不一致（`--all-features`、doctest） | CI 统一为 `cargo clippy --all-targets --all-features -- -D warnings` 与 `cargo test --workspace`（含 doctest） | 已修 |
| P1-5 | 决策上报缺原文证据、未粘贴脚本源码 | 已补三组真实编译输出原文 + 全文粘贴守卫脚本 | 已修 |
| P1-6 | 白名单 crate 的 lint 副本无防漂移机制 | 新增断言「两份 lint 表逐项相等，`unsafe_code` 除外」；反向注入已验证 | 已修 |
| P1-7 | `bindings-webidl` 与 `engine-v8` 的依赖方向/V8 类型归属未定义（铁律 5 隐患） | 记录为 G1 前置 ADR（见下方「G1 前置」） | 待办 |
| P1-8 | TASKS 字段/登记不完整、T0.2 验收与实现不符 | 本轮重写：补交付物清单、修正 T0.2 口径、标注 engine-v8 例外 | 已修 |
| P2-1 | 根 Cargo.toml 注释称「所有成员是占位」，与 runner 不符 | 注释已改 | 已修 |
| P2-2 | CI 注释称 lint job 含 deny/geiger，实际没有 | 注释改为如实描述，geiger 列为 G0 TODO | 已修 |
| P2-3 | 「退出机制」节号引用不一致（实为技术方案第 1 节） | `engine-v8/src/lib.rs` 已更正并标注 AGENTS.md 的笔误 | 已修 |
| P2-4 | `unused-allowed-license = "allow"` 会掩盖白名单腐化 | 改为 `"warn"` | 已修 |
| P2-5 | 守卫脚本误报/漏检面 | 扫描范围扩到全仓（含根级 build.rs 等）；引入「截注释后重新匹配」；已知局限写入脚本头 | 已修 |
| P2-6 | 首个 golden 偏弱且计划性废弃 | 在测试注释中显式标注「仅脚手架期有效、删除须独立 PR」 | 已修 |
| P2-7 | actions 未 pin SHA、无 `permissions:` | 已加 `permissions: contents: read`；pin SHA 登记为 G0 TODO | 部分 |

### G1 前置（新登记）

| ID | 事项 | 角色 | 状态 | 说明 |
|---|---|---|---|---|
| G1-PRE-1 | ADR：`bindings-webidl` ↔ `engine-v8` 依赖方向与 V8 类型退出机制的类型面（`RealmId` / `JsHandle` / `HostValue`） | lead | done | ADR 已落档（[ADR-0009](adr/0009-bindings-webidl与engine-v8的V8类型边界.md)，状态「已接受」）：类型面落 `crates/engine-v8/src/abi.rs` + 契约落 `contracts/iv8_contract/v8_abi.py`；`v8` crate 只许出现在 `engine-v8/Cargo.toml`；**不新增 workspace 成员**。⚠️ 该 ADR §A8 曾有**过度声明**（「四条断言全部做过双向验证」，与 §A9 及仓库现状矛盾）—— **已由架构师修复，QA 复验无残留**（见「本轮遗留与已知问题」K-4） |

---

## T1 — G0 契约 runner 与差分 harness 骨架（已完成）

| ID | 任务 | 角色 | 状态 | 验收标准 | 落地 |
|---|---|---|---|---|---|
| T1.1 | 契约测试 runner（规范模式 / iv8 兼容模式双 golden） | dev/test | done | 两条 pipeline 跑通并输出报告 | `contracts/iv8_contract/runner.py`；`uv run python -m iv8_contract contract` 两条 pipeline 均跑通 |
| T1.2 | `compat-manifest.json` 结构与状态流转 | lead/dev | done | 结构有契约测试覆盖；后三者阻断发布 | `contracts/iv8_contract/manifest.py` + 21 条契约测试；单调性**按 verified 集合**判定（计数持平的回退也会被拒） |
| T1.3 | Chromium 差分 harness（固定 Chrome/OS/字体/Profile） | dev/test | done | 空清单能跑通并输出按严重度排序的报告 | `contracts/iv8_contract/differential.py`；环境任一项未钉死即报错；骨架后端强制标注 `backend_real=False` |
| T1.4 | Python 侧测试环境（`uv venv` + `pytest`）落地 | dev | done | `uv run pytest` 可跑 | `contracts/pyproject.toml` + `.venv`；**48 条测试全绿** |
| T1.5 | CI 中接入 contract / differential job | lead | done | 门禁文件已按 ADR-0005 决策更新 | `ci.yml` 新增 `contract` job（5 步）；`test` job 改 stable + 1.92.0 双档矩阵 |

## T2 — 五项阻断性 PoC

| ID | PoC | 角色 | 状态 | 验收标准 | 结论 |
|---|---|---|---|---|---|
| T2.1 | `rusty_v8`/`cppgc` 跨语言循环回收 | dev/test | done | 最小可运行 + 断言 + 结论落 ADR | **跨语言环无法回收 → 泄漏**。判定方法：`WeakRef` + 独立 `HandleScope`（`Drop` 与 finalizer 都不可信）。见 [ADR-0007](adr/0007-T2.1-cppgc跨语言环无法回收.md) |
| T2.2 | 逻辑 Date 与多 Profile 时钟/时区隔离 | dev/test | done | 同上 | **不可隔离**：V8 时区是**进程级单值全局**，`set_default_time_zone` + `Skip` 通知只是**全局替换**（override 生效），**不是** Profile 并存隔离；一 Profile 一 Isolate 也不行，隔离单元只能是**进程边界**。探针 `poc/t2_2_clock_tz` 实测 `T2_2_PROBE_OK`，**11 条断言**（`a b c d e f g h0 h1 h2 h3`）。见 [ADR-0010](adr/0010-T2.2-时钟时区进程级与Profile隔离边界.md) |
| T2.3 | `html5ever` 脚本暂停/恢复与 `document.write` 插入点 | dev/test | done | 同上 | **可行，但驱动层必须自建**：`RawData(ScriptData)` + `TokenSinkResult::Script` + 产出用 **`push_front`** 注入。html5ever 自带 driver 有 `FIXME` 未实现。见 [ADR-0006](adr/0006-T2.3-html5ever-脚本暂停与document-write插入点.md) |
| T2.4 | Blitz 动态 DOM / 样式失效 / Shadow DOM | dev/test | done | 同上 | **裁决 `SWITCH_TO_STYLO`**：动态 DOM / 样式失效 / 布局失效 / NodeId 稳定 / 后端树只是缓存 **均成立**，但 **Shadow DOM 未实现**（`blitz-dom 0.3.0-beta.2` 的 `stylo.rs:231/238` 是 `todo!()`，`:311-313` 恒 `None`）⇒ 按技术方案 §8 切**直接 Stylo + Taffy + Parley**。探针 `poc/t2_4_blitz_probe` 实测 `T2_4_PROBE_OK`，**19 条断言**（含对冲断言 `c9-probe`：真的调用 `TShadowRoot::host()` 并捕获到上游 panic）；`cargo test` 3 个测试通过（含 `#[should_panic]` 回归契约）。**C1（Linux 可用性）已在远程 x86_64 Linux 实跑验证**：`TARGET_LINUX_OK=true` / `VERIFIED_ON_LINUX`（`main.rs:388` 为真实平台判定）。见 [ADR-0011](adr/0011-T2.4-Blitz裁决与Stylo直连路线.md) |
| T2.5 | V8 Inspector 暂停消息泵与 Python 回调死锁边界 | dev/test | done | 同上 | **三条路径（GIL / 重入 / 锁序）里只有 GIL 一条会死锁，且 watchdog 打不断它**。探针 `poc/t2_5_inspector_deadlock` 实测 `T2_5_PROBE_OK`，`COMBOS_TRIED=8 / COMBO_RUNS=11 / COMBOS_DEADLOCK=3 / COMBOS_UNMEASURABLE=4`，PyO3 接真解释器（3.13.12）跑通 D4。见 [ADR-0012](adr/0012-T2.5-Inspector暂停泵死锁边界与GIL不可中断.md)（含「D0a / D0b」节）。具名断言 `WATCHDOG_USES_ISOLATE_HANDLE=true` 与 `CLEAN_RESUME_AFTER_TERMINATE=Some(true)`（`ASSERT_PASS(m)(n)`）**本地+远程均实测通过** |

> T2.0（V8 嵌入前置）：`poc/t2_0_v8_embed` 实测通过——
> `PROBE_EVAL 1+2 = 3`、`PROBE_CALLBACK host fn invoked, argc = 3`、
> `PROBE_CALLBACK_RESULT = callback-ok`、`PROBE_THROW propagated as None`。
> 这是 T2.1 / T2.2 / T2.5 的共同前置。注意 crate 名是 **`v8`**（152.2.0），
> 仓库名才是 `rusty_v8`；`rusty_v8` 这个 crate 名在 crates.io 上停在 0.32.1（2021）。

### ADR 索引（按序号）

| ADR | 主题 | 关联任务 |
|---|---|---|
| [0001](adr/0001-agents-md-作为ai代理硬约束入口.md) | AGENTS.md 作为 AI 代理硬约束入口 | — |
| [0002](adr/0002-unsafe-白名单最终形态.md) | unsafe 白名单最终形态（方案 A） | T0.4 / D-01 |
| [0005](adr/0005-t0遗留待拍板项的决策.md) | T0 遗留待拍板项的决策（D-02~D-05） | T0 |
| [0006](adr/0006-T2.3-html5ever-脚本暂停与document-write插入点.md) | html5ever 脚本暂停与 `document.write` 插入点 | T2.3 |
| [0007](adr/0007-T2.1-cppgc跨语言环无法回收.md) | cppgc 跨语言环无法回收 | T2.1 |
| [0008](adr/0008-T2探针落盘与poc-unsafe白名单.md) | T2 探针落盘与 `poc/` unsafe 白名单 | T2 |
| [0009](adr/0009-bindings-webidl与engine-v8的V8类型边界.md) | `bindings-webidl` ↔ `engine-v8` 的 V8 类型边界 | **G1-PRE-1** |
| [0010](adr/0010-T2.2-时钟时区进程级与Profile隔离边界.md) | T2.2：时钟/时区是进程级，隔离须走进程边界 | **T2.2** |
| [0011](adr/0011-T2.4-Blitz裁决与Stylo直连路线.md) | T2.4 裁决：`SWITCH_TO_STYLO` | **T2.4** |
| [0012](adr/0012-T2.5-Inspector暂停泵死锁边界与GIL不可中断.md) | T2.5：Inspector 暂停泵死锁边界与 GIL 不可中断 | **T2.5** |

> ⚠️ **编号更正（QA 实测）**：本文档上方「授权变更」段（第 30-31 行）写
> 「D-02~D-05 → 落 ADR-0004」「G1-PRE-1 → 落 ADR-0003」，但 `docs/adr/` 下**不存在 0003 与 0004**（实际连续编号为
> `0001 / 0002 / 0005 / 0006 / 0007 / 0008 / 0009 / 0010 / 0011 / 0012`）。
> 实际落档为：**D-02~D-05 → [ADR-0005](adr/0005-t0遗留待拍板项的决策.md)**；**G1-PRE-1 → [ADR-0009](adr/0009-bindings-webidl与engine-v8的V8类型边界.md)**。
> 上表为准，第 30-31 行的编号属第三轮的预留笔误。

### 本轮遗留与已知问题（2026-10-12 第四轮，QA 登记）

> 以下均**不是**「已验证通过」，不得当作结论引用。

| # | 问题 | 归因 | 状态 |
|---|---|---|---|
| K-1 | ~~T2.5 `D0a` 具名断言缺口~~ → **已修**：`watchdog.rs:66` 新增 `pub const WATCHDOG_USES_ISOLATE_HANDLE: bool = true;`，`main.rs:285` 打印 `WATCHDOG_USES_ISOLATE_HANDLE={..}`，`main.rs:290` 配 `ASSERT_PASS(m)`；输出实测 `WATCHDOG_USES_ISOLATE_HANDLE=true`（本地 + 远程 x86_64 均见） | 探针代码 | **已修并 QA 双向复验通过**（eng-t25） |
| K-2 | ~~T2.5 `D0b` 具名断言缺口~~ → **已修**：`main.rs:300` 打印 `CLEAN_RESUME_AFTER_TERMINATE={..:?}`，`main.rs:311` 配 `ASSERT_PASS(n)`（测「终止请求发出后同一 Isolate 仍可干净复用」）；ADR-0012 新增「D0a / D0b」节（`75` 行起）。输出实测 `CLEAN_RESUME_AFTER_TERMINATE=Some(true)`（本地 + 远程均见） | 探针代码 / ADR | **已修并 QA 双向复验通过**（eng-t25） |
| K-3 | **`poc/` 不纳入 root 门禁（已决策为「**已知偏差**」，非缺陷）**：三个探针各自是独立 workspace（`Cargo.toml` 含 `[workspace]`），故 root 的 `cargo fmt --all --check` / `clippy` / `test --workspace` **都不覆盖它们**。这是 **ADR-0008 决策 1 的既有设计**（有意避免重依赖拖入主门禁），非疏漏。QA 首测：`t2_2` fmt=0，**`t2_4_blitz_probe` fmt=1、`t2_5_inspector_deadlock` fmt=1** | 探针代码 / 范围 | **已决策（lead）：不纳管，维持 ADR-0008 设计，登记为已知偏差**。配套：在途探针 `t2_2`/`t2_4`/`t2_5` **已全部 fmt 归一**（QA 复验 = 0）；历史探针 `t2_0`/`t2_1` 保留原状（fmt=1，其台账含行号引用，改写会漂移）、`t2_3` 本已干净；**poc 交付流程约定：交付前跑 `cargo fmt --all --check`**（暂记于本文件；如需固化到 `poc/README.md` 属 `poc/**`，归工程师/维护者）。**如未来纳入，作为 G0/G1 任务由维护者批准后动 CI / AGENTS.md**（铁律 7 保护文件） |
| K-4 | **ADR-0009 §A8 过度声明**：§A8（`421-422` 行）原称「**四条断言全部做过双向验证**」（完成时态），与 §A9（`426` 行「本 ADR **不要求**本轮就把门禁实现完毕」）**自相矛盾**；仓库现状佐证未实现：守卫**只有规则 1-4**（无规则 5 / `v8-type-escape`）、`workspace_policy.rs` **只有 3 个测试**、无 `engine-v8/src/abi.rs`、无 `contracts/iv8_contract/v8_abi.py`。另 §A8 表 `418` 行原引用**已废弃**的 `cargo build -p v8-abi` | 架构师（ADR） | **已修且 QA 复验无残留**（架构师；`grep` 已无「四条断言全部做过双向验证」与 `v8-abi` 字样） |
| K-5 | ~~T2.4 `C1` 硬编码常量（恒假门禁）~~ → **已修**：`main.rs:388` 原为 `out.target_linux_ok = false;`，现为 `out.target_linux_ok = target_os == "linux" && target_arch == "x86_64";`（真实平台判定）；状态串与裁决串条件化。**远程 x86_64 Linux 实测**：`TARGET_LINUX_OK=true`、`TARGET_LINUX_STATUS=VERIFIED_ON_LINUX`（修复前同一命令输出 `false` + `PENDING_REMOTE`） | 探针代码 | **已修并 QA 双向复验通过**（eng-t25，远程实测为证） |
| K-6 | ~~远程探针不可行~~ → **已解决**：远程直连 GitHub 拉 V8 静态库实测 **~8 KB/s**（归档 39,784,686 B ⇒ 约 82 分钟/workspace），国内镜像 rsproxy / npmmirror / tuna **均 404**。改用**本地中转**（本地下载 360 KB/s ≈ 45× 快 → `scp` 到远程 → 以 `RUSTY_V8_ARCHIVE=$HOME/librusty_v8_linux.a.gz` 构建，sha256 两端一致 `b6683e9a…`）后**三探针全部实跑成功** | 环境 | **已解决**（未改远程系统配置；仅用 v8 crate 自带的 `RUSTY_V8_ARCHIVE` 机制） |
| K-7 | **远程无 `cargo-deny`**（`~/.cargo/bin` 无该子命令）⇒ 远程门禁天然**不含 deny 项**（与 T0.8 口径一致） | 环境 | 已知 |
| K-8 | `cargo deny check` 有 **3 条 `warning[license-not-encountered]`**（`deny.toml:42-44` 的 `Unicode-3.0` / `Unicode-DFS-2016` / `Zlib` 未被实际依赖使用）。**不影响退出码** | 配置噪音 | 可清理 |
| K-9 | **T2.2 未覆盖项**（ADR-0010 自登记）：「同一 Isolate 内 A/B 两时区并存互不干扰」本轮**未测**（EXP1c 有位置依赖）；ADR 明示「不得据此推断并存一定失败」 | 范围决策 | 已登记（ADR-0010） |
| K-10 | **T2.4 `C4` 未交付**：html5ever `0.39` 上重跑 T2.3 的 `a~e` 断言不属本轮（本地 registry 只有 `html5ever-0.27.0`），登记为 **T2.3-bis** | 范围决策 | 已登记（ADR-0011） |

---

## 证据台账（原文粘贴，不转述）

### T0.1 / T0.2 / T0.7 —— 本地门禁（2026-10-12）

命令与回显退出码：

```
$ cargo fmt --all --check
FMT_EXIT=0
$ cargo clippy --all-targets --all-features -- -D warnings
CLIPPY_EXIT=0
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.06s
$ cargo test --workspace
TEST_EXIT=0
    （40 处 `test result: ok.`，0 处 FAILED）
$ cargo deny check
DENY_EXIT=0
advisories ok, bans ok, licenses ok, sources ok
$ bash scripts/check-unsafe-whitelist.sh
GUARD_EXIT=0
== unsafe 白名单守卫（AGENTS.md 铁律 4） ==
== PASS：白名单外无 unsafe；无 crate/mod 级开口；白名单内 unsafe 均带 SAFETY 注释 ==
```

构建：`cargo build --workspace` 输出 19 个成员全部编译通过，例如

```
   Compiling engine-v8 v0.0.0 (/Users/dennis/WorkBuddy/mmv8/crates/engine-v8)
   Compiling web-dom v0.0.0 (/Users/dennis/WorkBuddy/mmv8/crates/web-dom)
   Compiling runner v0.0.0 (/Users/dennis/WorkBuddy/mmv8/crates/runner)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 3.82s
```

### T0.3 —— 测试基建

`cargo test --workspace` 中真实运行的测试：

```
     Running tests/skeleton_smoke.rs
test roster_has_no_duplicates ... ok
test roster_matches_tech_plan_section_16 ... ok
test roster_entries_are_kebab_case ... ok          ← proptest 属性测试
test scaffold_status_snapshot ... ok               ← insta 快照
test result: ok. 4 passed; 0 failed
     Running tests/workspace_policy.rs
test workspace_members_equal_scaffold_roster ... ok
test engine_v8_lint_block_matches_workspace_except_unsafe_code ... ok
test only_whitelisted_crate_opts_out_of_workspace_lints ... ok
test result: ok. 3 passed; 0 failed
```

### T0.5 —— 守卫脚本十项自测（注入场景，原文摘录）

| # | 注入场景 | 期望 | 实测输出（摘录） | 退出码 |
|---|---|---|---|---|
| 1 | 干净仓库 | PASS | `== PASS：白名单外无 unsafe；无 crate/mod 级开口；…` | 0 |
| 2 | 非白名单 crate（web-dom）写 `#[allow(unsafe_code)]` + `unsafe` | FAIL | `VIOLATION[outside-whitelist] crates/web-dom/src/violation_tmp.rs:3: pub fn bad() …` | 1 |
| 3 | 白名单 crate 写 crate 级 `#![allow(unsafe_code)]` | FAIL | `VIOLATION[crate-level-allow] …:2: #![allow(unsafe_code)]` | 1 |
| 4 | 白名单 crate 写 `#[allow(unsafe_code)] mod …` | FAIL | `VIOLATION[module-level-allow] …:2: #[allow(unsafe_code)]` | 1 |
| 5 | 白名单 crate 写 `unsafe` 但无 SAFETY 注释 | FAIL | `VIOLATION[missing-safety-comment] …:2: pub fn bad2() …` | 1 |
| 6 | 白名单 crate 写 `unsafe` 且带 SAFETY 注释 | PASS | `== PASS：…` | 0 |
| 7 | 仓库根出现 `unsafe`（覆盖根级 build.rs 情形） | FAIL | `VIOLATION[outside-whitelist] build_tmp.rs:1: pub fn root_bad() …` | 1 |
| 8 | 行尾注释含 `unsafe`（防误报） | PASS | `== PASS：…` | 0 |
| 9 | 白名单内函数级逐块开口（`// SAFETY:` + `#[allow]` + `unsafe`） | PASS | `== PASS：…` | 0 |
| 10 | 清理后复核 | PASS | `== PASS：…` | 0 |

### T0.10 —— workspace 一致性门禁的反向验证（原文摘录）

```
### 反向验证 1b：把名单里的 graphics 换成 graphicx（kebab-case 互换，数量仍为 19）
crate 边界出现分叉：Cargo.toml 多出 ["graphics"]，脚手架名单多出 ["graphicx"]
test result: FAILED. 1 passed; 2 failed; 0 ignored

### 反向验证 2：engine-v8 lint 漂移（missing_docs 改成 deny）
assertion `left == right` failed: 白名单 crate 的 lint 副本与 workspace lints 发生漂移；全仓 lint 升级时必须同步
test result: FAILED. 2 passed; 1 failed; 0 ignored

### 反向验证 3：某 crate 去掉 [lints] workspace = true
web-dom 必须写 [lints] workspace = true，否则会静默脱离全仓 lint 约束
test result: FAILED. 2 passed; 1 failed; 0 ignored

### 恢复后复核
test result: ok. 3 passed; 0 failed
```

### T0.8 —— 远程 Linux x86_64 门禁（2026-10-12）

```
$ ssh ubuntu@124.221.1.78 'export PATH=$HOME/.cargo/bin:$PATH; cd ~/mmv8 && cargo fmt --all --check && cargo clippy --all-targets -- -D warnings && cargo test --workspace'
FMT_EXIT=0
CLIPPY_EXIT=0
TEST_EXIT=0
```

（远程 `rustc 1.99.0 (b940084d7 2026-09-28)`、`cargo 1.99.0 (5f94df478 2026-08-27)`；
本地 `rustc 1.92.0 (ded5c06cf 2025-12-08)`。两端同一 `channel = "stable"`。）

---

### 第三轮（2026-10-12）证据台账

**Rust 侧（本地）**

```
$ cargo fmt --all --check                                   FMT_EXIT=0
$ cargo test --workspace                                    TEST_EXIT=0（40 处 test result: ok）
$ bash scripts/check-unsafe-whitelist.sh                     GUARD_EXIT=0
== PASS：白名单外无 unsafe；无 crate/mod 级开口；白名单内 unsafe 均带 SAFETY 注释
```

**Python 侧（本地，`contracts/`）**

```
$ uv run pytest
48 passed in 0.68s

$ uv run python -m iv8_contract contract
规范模式：0/0 通过，engine=stub
iv8 兼容模式：0/0 通过，engine=stub
结论：通过

$ uv run python -m iv8_contract manifest
{"verified": 0, "failed": 0, "missing": 0, "unknown": 0}
条目 0 条，阻断 0 条

$ uv run python -m iv8_contract differential
后端：null（真实浏览器：否）
用例 0 条，差异 0 条，结论：通过
⚠️ 本次**未**与真实 Chromium 比对，仅为骨架跑通。
```

**T2.1 探针（`poc/t2_1_cppgc_cycle`）**

```
=== 汇总（以 WeakRef 为准）===
  exp1: alive=false
  exp2: alive=false
  exp3: alive=true
BASELINE_VALID=true
CYCLE_LEAK=true
```

**T2.3 探针（`poc/t2_3_html5ever_pause`）**

```
ASSERT_PASS(a): 前缀 feed 后即产出 token —— 增量/可暂停成立
ASSERT_PASS(b): script 体内 <b> 未被解析为标签 —— RawData(ScriptData) 生效
ASSERT_PASS(c): </script> 处产生 1 个暂停点 —— TokenSinkResult::Script 生效
ASSERT_PASS(d): document.write 内容落在 </script> 之后 —— 插入点语义成立
ASSERT_PASS(e): 恢复后 </script> 后的 <i> 正常解析 —— tokenizer 回到 Data 状态
H5PROBE_OK
```

**守卫脚本反向验证（新增 `poc/` 白名单）**

```
# 加白名单前，守卫抓到 poc 里的 4 处 unsafe（证明全仓扫描有效）
VIOLATION[outside-whitelist] poc/t2_1_cppgc_cycle/src/main.rs:47: unsafe impl v8::cppgc::GarbageCollected ...
VIOLATION[outside-whitelist] poc/t2_1_cppgc_cycle/src/main.rs:72: unsafe {
VIOLATION[outside-whitelist] poc/t2_1_cppgc_cycle/src/main.rs:85: unsafe { v8::Object::wrap::<TAG, DomNode>(...) };
VIOLATION[outside-whitelist] poc/t2_1_cppgc_cycle/src/main.rs:95: let node = unsafe { $ptr.as_ref() };

# 加白名单后，poc 内缺 SAFETY 注释仍被抓到（约束不放宽）
VIOLATION[missing-safety-comment] poc/t2_1_cppgc_cycle/src/main.rs:51: unsafe impl v8::cppgc::GarbageCollected ...
```

**license 字段验证**

```
$ cargo metadata --no-deps --format-version 1
成员数 = 19
license 全为 MIT OR Apache-2.0 = True
```

---

### 第四轮（2026-10-12）证据台账 —— T2.2 / T2.4 / T2.5 / G1-PRE-1

> 全部为**命令与回显原文**，退出码为 `echo $?` 实测值。运行机：本地 macOS arm64（`rustc 1.92.0`）。
> 探针源文件指纹（sha256）：`t2_2/main.rs=cf3c382f…`、`t2_4_blitz_probe/main.rs=39eb7b28…`、
> `t2_5/{main,inspector,probe,pyside,watchdog}.rs` 于 **21:21:54 冻结**（三份二进制均晚于其源码 ⇒ 无「跑旧二进制」）。

**① 本地 Rust 五条门禁（仓库根）**

```
$ cargo fmt --all --check
FMT_EXIT=0
$ cargo clippy --all-targets --all-features -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.63s
CLIPPY_EXIT=0
$ cargo test --workspace
    （40 处 `test result: ok.`，0 处 FAILED）
TEST_EXIT=0
$ cargo deny check
advisories ok, bans ok, licenses ok, sources ok
DENY_EXIT=0
$ bash scripts/check-unsafe-whitelist.sh
== unsafe 白名单守卫（AGENTS.md 铁律 4） ==
== PASS：白名单外无 unsafe；无 crate/mod 级开口；白名单内 unsafe 均带 SAFETY 注释 ==
GUARD_EXIT=0
```

> **【冻结】后复验**：以上五条在冻结快照上**重跑一遍，退出码与上表逐项一致**（fmt=0 / clippy=0 / test=0（40 处 ok）/ deny=0 / guard=0），`uv run pytest` 亦为 `48 passed`（0）。

**守卫的同轮两次状态（如实留档）** —— 20:31 首跑曾 FAIL，工程师修复后复跑 PASS：

```
$ bash scripts/check-unsafe-whitelist.sh        # 20:31 首跑
== unsafe 白名单守卫（AGENTS.md 铁律 4） ==
VIOLATION[missing-safety-comment] poc/t2_5_inspector_deadlock/src/inspector.rs:407: unsafe { v8::Local::from_non_null(context_ptr) };
VIOLATION[missing-safety-comment] poc/t2_5_inspector_deadlock/src/inspector.rs:408: Some(unsafe { v8::CallbackScope::new(context) })
VIOLATION[missing-safety-comment] poc/t2_2_clock_tz/src/main.rs:116: let shared = unsafe { owned.try_into_shared() }
== FAIL：发现 3 处违规（AGENTS.md 铁律 4） ==
GUARD_EXIT=1
```
根因：`// SAFETY:` 注释落在守卫「规则 3」的 **±2 行窗口之外**（`inspector.rs` 的 SAFETY 在 403-405 行，unsafe 在 407/408）。
归属：`inspector.rs` → T2.5 工程师；`t2_2/main.rs` → T2.2 工程师；`poc/t2_4_blitz_probe/` **零命中**。现均 PASS。

**② 本地 Python 侧（`contracts/`）**

```
$ uv run pytest
48 passed in 1.67s
PYTEST_EXIT=0
```

**③ 三个探针（各自独立 workspace；`cargo run --offline`）**

T2.2 `poc/t2_2_clock_tz` —— **`T2_2_EXIT=0`**（**11 条断言**）：

```
GLOBAL_LEAK_OBSERVED isolate=B tz_before=300 tz_after=-540
GLOBAL_LEAK_OBSERVED isolate=A tz_before=300 tz_after=-540
DATE_NOW_ISOLATED=false
  ⇒ 时间值同样是 process-global，不可按 Isolate 隔离
⇒ EXP1 里「两读数不同」来自 per-isolate 缓存，不是隔离
VERIFY_REDETECT_BREAKS_SETUP=true
ASSERT_PASS(a) (b) (c) (d) (e) (f) (g) (h0) (h2) (h3) (h1)
T2_2_PROBE_OK
```
> ⚠️ **字段名语义陷阱（照抄必读）**：探针另有 `TZ_PER_PROFILE_ISOLATED=true` 一行，它表示
> 「**override 生效**」（`set_default_time_zone` + `Skip` 通知让 Date 跟随新的全局时区），
> **不是**「时区可按 Profile 隔离」。时区是**进程级单值全局**；探针自己在 EXP1 后打印了
> 「不是隔离」的澄清行。**不得**把该字段读成隔离成功。

T2.4 `poc/t2_4_blitz_probe` —— **`T2_4_EXIT=0`**（**19 条断言**，`ASSERTIONS_RUN=19`）：

```
ASSERT_PASS(c9-probe): 确实调用了 TShadowRoot::host() 并捕获到上游 todo!("Shadow roots not implemented") —— C9 的探测方法有效（非恒假门禁）
ASSERT_FAIL(c9): blitz-dom 0.3.0-beta.2 未实现 shadow root：as_shadow_root().is_some()=false（impl 恒返回 None，stylo.rs:311-313）…
=== 二值结论汇总 ===
TARGET_LINUX_OK=false
TARGET_MAC_OK=true
DYNAMIC_DOM_WORKS=true
STYLE_INVALIDATION_WORKS=true
LAYOUT_INVALIDATION_WORKS=true
NODE_ID_MAP_CONSISTENT=true
BACKEND_IS_CACHE=true
SHADOW_DOM_WORKS=false
=== 裁决 ===
T2_4_VERDICT=SWITCH_TO_STYLO
ASSERTIONS_RUN=19
PROBE_METHOD_VALID=true
T2_4_PROBE_OK
```

T2.5 `poc/t2_5_inspector_deadlock` —— **`T2_5_EXIT=0`**（按【冻结】后**隔离复跑**，不与任何其他 cargo 任务并发；提交的 `run-output.txt` 534 行正文与 QA 独立复跑**逐行比对，差异 74 行且全为 `RESUMED_AT`/`WATCHDOG_FIRED_AT`/`CONTROL_MSGS_SENT` 计时计数类**）：

```
PYO3_SELFTEST_OK=true
PYTHON_VERSION=3.13.12 (main, Feb  4 2026, 00:05:32) [Clang 21.1.4 ]
WATCHDOG_SELFTEST_OK=true
D4_GIL_PATH_FAILURE_OBSERVED=true
D4_GIL_PATH_PAIRED_CONTROL_CLEAN=true
D4_GIL_EVIDENCE_PRESENT=true
COMBOS_TRIED=8
COMBO_RUNS=14
COMBOS_DEADLOCK=3
COMBOS_UNMEASURABLE=4
UNMEASURABLE_REASON=two-factor-confound(F1&F2 或 F2&F3 同时为真时无法归因)
PAGE_TASKS_STALLED_DURING_PAUSE=true
CLEAN_RESUME=true
ASSERT_PASS(a) (a2) (env) (b) (c) (d) (i) (i2) (j) (k) (l) (g) (h)
T2_5_PROBE_OK
```

**隔离复跑与 ADR-0012 条件表逐项吻合**（关键三行）：
```
| 4 | yes | no | none        | spin_holding_gil | TIMEOUT      | 是 | GIL_BLOCKED_MS=5998; WATCHDOG_FIRED=true |
| 4 | yes | no | none        | sleeping         | CLEAN_RESUME | 否 | GIL_BLOCKED_MS=0 |
| 5 | yes | no | msg_only    | spin_holding_gil | TIMEOUT      | 是 | GIL_BLOCKED_MS=5999; WATCHDOG_FIRED=true |
| 5 | yes | no | msg_only    | sleeping         | CLEAN_RESUME | 否 | GIL_BLOCKED_MS=0 |
| 6 | yes | no | needs_locker| spin_holding_gil | TIMEOUT      | 是 | GIL_BLOCKED_MS=6000; WATCHDOG_FIRED=true |
| 6 | yes | no | needs_locker| sleeping         | CLEAN_RESUME | 否 | GIL_BLOCKED_MS=0 |
```
> ⚠️ **负载敏感性（照 ADR-0012「运行环境要求」节）**：本探针**必须空载运行**（watchdog 只读墙钟，并发 cargo 会拉长调度延迟、把对照行误判成 `TIMEOUT`）。
> QA 首轮曾与工程师并行跑并观测到组合 5 sleeping 翻转（`GIL_BLOCKED_MS=10183`），**隔离复跑后不复现**、与条件表逐项吻合 ⇒ 该翻转是**负载伪影**，非探针竞态。
> `T2_5_PROBE_OK` **不等于「无死锁」**，只表示「已覆盖组合的分类都测出了且方法自证有效」。

**④ `poc/` 独立 workspace 的补测（root 门禁不覆盖它们 —— QA 主动补跑）**

```
$ (cd poc/t2_2_clock_tz          && cargo fmt   --all --check)   FMT_EXIT=0
$ (cd poc/t2_4_blitz_probe        && cargo fmt   --all --check)   FMT_EXIT=1   ← 未格式化
$ (cd poc/t2_5_inspector_deadlock && cargo fmt   --all --check)   FMT_EXIT=1   ← 未格式化
$ (cd poc/t2_2_clock_tz          && cargo clippy --offline --all-targets --all-features -- -D warnings)   CLIPPY_EXIT=0
```

```
$ (cd poc/t2_4_blitz_probe && cargo test --offline)
running 3 tests
test tests::catch_unwind_returns_ok_when_no_panic ... ok
test tests::no_gui_crates_in_lockfile ... ok
test tests::shadow_root_host_is_not_implemented - should panic ... ok
test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
T2_4_TEST_EXIT=0
$ (cd poc/t2_2_clock_tz          && cargo test --offline)   test result: ok. 0 passed; 0 failed   T2_2_TEST_EXIT=0
$ (cd poc/t2_5_inspector_deadlock && cargo test --offline)   test result: ok. 0 passed; 0 failed   T2_5_TEST_EXIT=0
```

**⑤ 远程 Linux x86_64 门禁（`ubuntu@124.221.1.78`，`rustc 1.99.0`；3.6 GB 内存）**

主门禁（`nohup` + 轮询；`-j2`）—— 四条全 0：

```
$ ssh ubuntu@124.221.1.78 '<...> cd ~/mmv8 && cargo fmt --all --check && cargo clippy -j2 --all-targets --all-features -- -D warnings && cargo test -j2 --workspace && bash scripts/check-unsafe-whitelist.sh'
FMT_EXIT=0
CLIPPY_EXIT=0
TEST_EXIT=0
GUARD_EXIT=0
（40 处 `test result: ok.`，0 处 FAILED；内存峰值无压力）
```
> 说明：远程**未 OOM**。原因：主 workspace 的 `engine-v8` **当前尚未依赖 `v8` crate**（`crates/engine-v8/Cargo.toml` 无 `v8` 依赖、`src/` 只有 `lib.rs`），故主门禁根本**不链接 V8**。V8 链接只发生在 `poc/` 探针里（下表）。

远程 Python 侧：

```
$ cd ~/mmv8/contracts && uv run pytest
uv 0.13.0 (x86_64-unknown-linux-gnu)
48 passed in 0.13s
PYTEST_EXIT=0
```

远程三个探针 —— **已实跑成功**（退出码均 0）。首次尝试受阻于 V8 静态库下载，改用**本地中转**后通过：

```
# ① 直连失败（首次）：远程 --offline 缓存未命中；去掉 --offline 后卡在 v8 build.rs 下载
$ (cd poc/t2_2_clock_tz && cargo run -j2 --offline)   → T2_2_EXIT=101
error: no matching package named ... found
$ (cd poc/t2_2_clock_tz && cargo run -j2)             → 卡在 V8 静态库下载（QA 15 分钟后主动 kill）

# 远程直连速度实测（QA）：
code=200 downloaded=161553B speed=8046B/s time=20.07s      ← 38 MB 归档 ⇒ 约 82 分钟/workspace
# 归档大小：curl ... | content-length = 39784686 B
# 国内镜像候选全部 404：rsproxy.cn/rusty_v8/... 、cdn.npmmirror.com/binaries/rusty_v8/... 、mirrors.tuna.tsinghua.edu.cn/rusty_v8/...

# ② 本地中转（本机 360 KB/s ≈ 45× 快）：
$ curl -L -o /tmp/librusty_v8_linux.a.gz <同样 URL>      → 39784686 B
$ shasum -a 256 /tmp/librusty_v8_linux.a.gz              → b6683e9afcb77fbd8cb2c3acf45d34d52029ab216b332522c95179c12f0fed3c
$ scp /tmp/librusty_v8_linux.a.gz ubuntu@124.221.1.78:~/librusty_v8_linux.a.gz
$ ssh ... sha256sum ~/librusty_v8_linux.a.gz             → b6683e9afcb77fbd8cb2c3acf45d34d52029ab216b332522c95179c12f0fed3c（两端一致）
# ③ 以 v8 crate 自带机制指向中转载荷（未改远程系统配置）：
export RUSTY_V8_ARCHIVE=$HOME/librusty_v8_linux.a.gz
$ (cd poc/t2_2_clock_tz && cargo run -j2)          → T2_2_EXIT=0
$ (cd poc/t2_4_blitz_probe && cargo run -j2)       → T2_4_EXIT=0
$ (cd poc/t2_5_inspector_deadlock && cargo run -j2) → T2_5_EXIT=0
```

**远程三探针关键输出原文**：

```
===== T2.2（远程）=====   T2_2_EXIT=0，ASSERT_PASS 计数 = 11
TZ_A=Asia/Tokyo TZ_B=America/Bogota
TZ_READ_A=-540   TZ_READ_B=300
TZ_PER_PROFILE_ISOLATED=true
TZ_CACHE_NOT_ISOLATION=true
GLOBAL_LEAK_OBSERVED isolate=B tz_before=300 tz_after=-540
GLOBAL_LEAK_OBSERVED isolate=A tz_before=300 tz_after=-540
DATE_NOW_ISOLATED=false
TZ_SET_ETC_UNKNOWN_REJECTED=true
TZ_REJECT_BRANCH_WORKS=true
T2_2_PROBE_OK

===== T2.4（远程）=====   T2_4_EXIT=0，ASSERT_PASS=18 / ASSERT_FAIL=1（c9）
TARGET_HOST_OS=linux
TARGET_HOST_ARCH=x86_64
TARGET_MAC_OK=false
TARGET_LINUX_OK=false                    ← ⚠️ 硬编码常量，见 K-5
TARGET_LINUX_STATUS=PENDING_REMOTE (需在 x86_64-unknown-linux-gnu 上复跑)   ← ⚠️ 硬编码
DYNAMIC_DOM_WORKS=true  STYLE_INVALIDATION_WORKS=true  LAYOUT_INVALIDATION_WORKS=true
NODE_ID_MAP_CONSISTENT=true  BACKEND_IS_CACHE=true  SHADOW_DOM_WORKS=false
T2_4_VERDICT=SWITCH_TO_STYLO
ASSERTIONS_RUN=19
T2_4_PROBE_OK

===== T2.5（远程）=====   T2_5_EXIT=0
COMBOS_TRIED=8  COMBO_RUNS=14  COMBOS_DEADLOCK=3  COMBOS_UNMEASURABLE=4
D4_GIL_PATH_FAILURE_OBSERVED=true  D4_GIL_PATH_PAIRED_CONTROL_CLEAN=true
PAGE_TASKS_STALLED_DURING_PAUSE=true  CLEAN_RESUME=true
T2_5_PROBE_OK
```

**⚠️ 远程实测带出的一条探针缺陷（K-5）**：T2.4 在 x86_64 Linux 上**编译并运行成功**（`TARGET_HOST_OS=linux`、`TARGET_HOST_ARCH=x86_64`、`T2_4_PROBE_OK`、除 C9 外断言与 macOS 一致），**但探针的 `TARGET_LINUX_OK` 仍输出 `false`** —— 因为该值是**手写常量**。⇒ C1 的**实质证据已取得**，但探针的 C1 标志是**恒假门禁**（违反 ADR-0008 决策 3 / C9a 纪律），归 eng-t24。

⇒ 远程探针已非阻塞（K-6 关闭）；T2.4 `C1` 转为「实质已证、探针标志有缺陷」（K-5）。

**⑥ 依赖计数核对（T2.4）**

```
Cargo.lock 条目 192 / 去重 190（`syn` 与 `synstructure` 各双版本共存）—— 非缺包
```

**⑦ 文档与实现一致性抽查**

```
$ grep -nE "v8-type-escape|规则 5" scripts/check-unsafe-whitelist.sh
>>> 未找到：规则 5 / v8-type-escape 不存在（守卫脚本仅 185 行，规则 1-4）
$ grep -c '^    fn \|^#\[test\]' crates/runner/tests/workspace_policy.rs
>>> 3 个 #[test]（无 v8_dependency_only_in_engine_v8 / three_safety_types_have_single_definition_point）
$ ls crates/engine-v8/src/            → 只有 lib.rs（无 abi.rs）
$ ls contracts/iv8_contract/v8_abi.py → No such file
```
⇒ 即 K-4（ADR-0009 §A8 过度声明）。**（该抽查为修复前状态；K-4 已由架构师修复并 QA 复验，见 ⑧）**

**⑧ 修复复验（QA 回归，2026-10-12 21:46）** —— 针对 K-3（`poc/` 格式）与 K-4（ADR-0009 §A8）：

```
# K-3：三个 poc workspace 的 fmt 复验
$ (cd poc/t2_2_clock_tz          && cargo fmt --all --check)   FMT=PASS(0)
$ (cd poc/t2_4_blitz_probe        && cargo fmt --all --check)   FMT=PASS(0)   ← 原为 1
$ (cd poc/t2_5_inspector_deadlock && cargo fmt --all --check)   FMT=PASS(0)   ← 原为 1

# K-4：ADR-0009 中的两处残留复验
$ grep -n "四条断言全部做过双向验证|v8-abi" docs/adr/0009-*.md
（无输出 ⇒ 两处均已清除）

# 回归：格式变更后探针行为未变
$ (cd poc/t2_4_blitz_probe && cargo run --offline)
T2_4_VERDICT=SWITCH_TO_STYLO
ASSERTIONS_RUN=19
T2_4_PROBE_OK                      T2_4_V3_EXIT=0
$ (cd poc/t2_4_blitz_probe && cargo test --offline)
test result: ok. 3 passed; 0 failed        T2_4_TEST_V3_EXIT=0
$ (cd poc/t2_5_inspector_deadlock && cargo run --offline)
COMBOS_TRIED=8  COMBOS_DEADLOCK=3  COMBOS_UNMEASURABLE=4
（`grep -c '^ASSERT_FAIL'` = 0）
T2_5_PROBE_OK                      T2_5_V3_EXIT=0

# 守卫复验
$ bash scripts/check-unsafe-whitelist.sh     GUARD_EXIT=0
```
⇒ **K-3 / K-4 已修并复验通过，回归无破坏。**（K-1 / K-2 在当时快照上仍缺失，其后续修复见 ⑨。）

**⑨ 修复复验（K-1 / K-2 / K-5，2026-10-12 22:2x）** —— eng-t25 于 21:52:00 前后落盘（`t2_5/{main,watchdog}.rs`、`run-output.txt` 572 行、ADR-0012 556 行）、T2.4 于 22:18:50 落盘。QA **独立复跑**（非依赖他人转述）：

**K-5（T2.4 `C1`）—— 本地 + 远程两段原文**

```
# 本地 macOS（应输出 false + PENDING_REMOTE，属「照实」而非「实测为否」）
$ (cd poc/t2_4_blitz_probe && cargo run --offline)
TARGET_HOST_OS=macos            TARGET_HOST_ARCH=aarch64
TARGET_MAC_OK=true              TARGET_LINUX_OK=false
TARGET_LINUX_STATUS=PENDING_REMOTE (需在 x86_64-unknown-linux-gnu 上复跑)
VERDICT_DRIVER=… + C1 TARGET_LINUX_OK=false（待远程 x86_64 Linux 复跑；即便 C1 为 true，C9 仍为 false，裁决不变）
T2_4_VERDICT=SWITCH_TO_STYLO    ASSERTIONS_RUN=19    T2_4_PROBE_OK      T2_4_EXIT=0
$ (cd poc/t2_4_blitz_probe && cargo test --offline)   → test result: ok. 3 passed; 0 failed   T2_4_TEST_EXIT=0

# 远程 x86_64 Linux（C1 的关键证据；日志含 `Compiling blitzprobe` ⇒ 确为修复后重编）
$ ssh ubuntu@124.221.1.78 '… cd ~/mmv8/poc/t2_4_blitz_probe && cargo run -j2'
   Compiling blitzprobe v0.0.0 (/home/ubuntu/mmv8/poc/t2_4_blitz_probe)
TARGET_HOST_OS=linux            TARGET_HOST_ARCH=x86_64
TARGET_MAC_OK=false             TARGET_LINUX_OK=true
TARGET_LINUX_STATUS=VERIFIED_ON_LINUX (本探针正运行于 x86_64-unknown-linux-gnu，C1 就地验证)
SHADOW_DOM_WORKS=false
T2_4_VERDICT=SWITCH_TO_STYLO
VERDICT_DRIVER=C9 SHADOW_DOM_WORKS=false（…）+ C1 TARGET_LINUX_OK=true（x86_64 Linux 本机实跑；即便 C1 为 true，C9 仍为 false，裁决不变）
ASSERTIONS_RUN=19    T2_4_PROBE_OK    T2_4_EXIT=0
```
⇒ **修复前后对照**：同一台远程机上、同一命令，修复前 `TARGET_LINUX_OK=false`（恒假），修复后 **`TARGET_LINUX_OK=true` + `VERIFIED_ON_LINUX`**。K-5 **闭环**。

**K-1 / K-2（T2.5 具名断言）—— 本地 + 远程两段原文**

```
# 本地（隔离跑）
$ (cd poc/t2_5_inspector_deadlock && cargo run --offline)
WATCHDOG_USES_ISOLATE_HANDLE=true
CLEAN_RESUME_AFTER_TERMINATE=Some(true)
ASSERT_PASS(m): D0a：watchdog 走 IsolateHandle 路径（thread_safe_handle → terminate_execution），且该事实出现在实跑证据中
ASSERT_PASS(n): D0b：终止请求发出后同一 Isolate 仍可干净复用（同 scope 再跑 1+1 成功）—— 边界已由断言覆盖，非悬空
COMBOS_TRIED=8  COMBO_RUNS=14  COMBOS_DEADLOCK=3  COMBOS_UNMEASURABLE=4   T2_5_PROBE_OK   T2_5_EXIT=0
（15 条 ASSERT_PASS：a a2 b c d env g h i i2 j k l m n；0 条 ASSERT_FAIL）

# 远程 x86_64 Linux（触及并修正了 §「rsync-mtime 陈旧构建」陷阱后重跑）
WATCHDOG_USES_ISOLATE_HANDLE=true
CLEAN_RESUME_AFTER_TERMINATE=Some(true)
ASSERT_PASS(m) / ASSERT_PASS(n)
COMBOS_TRIED=8  COMBO_RUNS=14  COMBOS_DEADLOCK=3  COMBOS_UNMEASURABLE=4   T2_5_PROBE_OK   T2_5_EXIT=0
```

**⚠️ 本轮踩到并已规避的一个陷阱（值得沉淀）**：`rsync -a` **保留源 mtime**，而 cargo 的新旧判定基于 mtime。
当同步一份「本地 mtime 早于远程既有构建产物」的文件时，**cargo 会静默跳过重编、直接跑陈旧二进制**。
QA 首次远程跑 T2.5 时即命中：日志无 `Compiling`、输出缺 K-1/K-2 标记。
**甄别方法**：看日志有无 `Compiling <crate>` 行 + 输出是否含该源码应有的标记。
**处置**：远程 `touch` 源文件强制刷新 mtime 后重跑。**⚠️ 如实声明：`touch` 只改文件 mtime，未改任何文件内容**——QA 事后比对两端 `sha256` **完全一致**，可核：
```
t2_5/src/main.rs     3d3fa4aff35eac3676b750a3524a36fbd48710cf56a534e46ff964286f39424d   本地 = 远程
t2_5/src/watchdog.rs 4161efdd76d8864e1055596b6602c1ceea361d9c274ff6057c4fc892002bc5f4   本地 = 远程
```
**未改远程系统配置**（未加 swap / 未动 sysctl），也只修改了该 workspace 内源文件的 mtime。

**⑩ 修复后本地门禁全量复验（2026-10-12 22:3x）**

```
cargo fmt --all --check                                   FMT_EXIT=0
cargo clippy --all-targets --all-features -- -D warnings  CLIPPY_EXIT=0
cargo test --workspace                                    TEST_EXIT=0（40 处 test result: ok）
cargo deny check                                          DENY_EXIT=0
bash scripts/check-unsafe-whitelist.sh                    GUARD_EXIT=0（PASS）
cd contracts && uv run pytest                             48 passed
poc fmt：t2_2=0  t2_4_blitz_probe=0  t2_5_inspector_deadlock=0
```
⇒ **K-1 / K-2 / K-5 三条缺陷全部闭环，修复未引入回归。**

---

### 第五轮证据台账（2026-10-12 ~ 10-13）—— G1 引擎纵向（S1~S13）

> 组织：架构师（施工图 + 2 决策 brief + ADR + 文档同步）｜工程师 ×2（S1~S13）｜
> 评审员 A/B（背对背辩论）｜QA 严过关（Round 1 独立验证 + Round 2 复验）。**结论：G1 闸门完成。**

**① 交付物总表（S1~S13）**

| 步骤 | 内容 | 验收证据要点 |
|---|---|---|
| S1/S2（PR-3a/3b） | 守卫规则 5 `v8-type-escape`；`v8_dependency_only_in_engine_v8` 断言 | 四态验证（注入红→放松绿→恢复红→清理绿）+ INJ-1/INJ-2 原文；**QA 独立复跑** |
| S3/S3'（PR-1） | `abi.rs`（4 类型 + 12 变体 `error.rs`）；断言 `three_safety_types_have_single_definition_point` | 单测 5；INJ-3 命中 2 处（文件:行:内容）后还原、哈希一致 |
| S4（PR-2） | `contracts/iv8_contract/v8_abi.py` + `tests/test_v8_abi.py` | 7 条逐项对应断言；contracts 48→55 |
| S5 | 句柄表（弱槽 + 自持 `Global` 的 `HandleLease` + generation 回绕退役）+ v8 依赖 | `handle_not_a_gc_root` / `collected_detected` / `stale_handle_detected` / `lease_keeps_value_alive` / `alloc_free_reuse_no_alias`(proptest)；**T1/T2 实测**（`request_garbage_collection_for_testing(Full)`；`to_local`/`to_global` 回收后 `None`）；零 unsafe |
| S6 | `EngineHost`/`AgentRuntime`/`Realm`（eval/get/set/call/close）；D1=Realm 自持 `Box<OwnedIsolate>` | `init_once_idempotent` / `eval_basic_values` / `eval_error_kind` / `get_set_prop_roundtrip` / `close_idempotent` |
| S7 | `native_fn` + trampoline（`REALM_DATA_SLOT` 裸指针桥，逐块 SAFETY）+ insta 快照 | §3.1 六条；快照 `function () { [native code] }` |
| S8 | 微任务（Explicit + checkpoint + 墙钟截断） | §3.3 四条；`microtask_budget_exhaustion` 经 watchdog + `terminate_execution` 实测截断并可恢复；**如实登记：条数预算不可得、`perform_checkpoint` 返回 1** |
| S9 | `node_map`（弱句柄）+ 弱句柄 GC | 5 条含 `t2_1_leak_shape_avoided`（只证弱引用路径） |
| S10 | 模块缓存 / WASM / 静态 import（`CallbackScope::new` 桥） | 5/6 绿；动态 import = 已知失败（V8 FATAL，已回退，见 K-11）**→ 第六轮已修复（ADR-0015 已落，三测绿）**；WASM `add(2,3)=5` |
| S11 | 控制入口隔离（单 Realm 双模式）+ 重入守卫收口（RAII 全入口） | §3.5 四条（含 panic 路径）；`reentrant_native_to_native` / `reentrant_in_microtask` 拒绝实测 |
| S12 | sdk-python（PyO3 **0.29**；R-3/F1 实证：feature 分离不污染、macros 正反） | `test_iv8.py` **13 pytest**（六能力 + deadline + 线程断言 + 重入 + close） |
| S13 | 纵向集成 | `vertical_native_fn_to_microtask_to_checkpoint`（engine-v8）+ pytest 版本 |
| S14(=QA) | 覆盖率 + 门禁复验 | 覆盖率 **75.33% regions / 75.97% lines**（engine-v8 各模块 83%~94% lines） |

**② 最终门禁（QA Round 2 独立复验 + lead 亲验 2026-10-13 01:44）**

```
本地 macOS 1.92：fmt=0 / clippy=0 / test=0（54 = engine-v8 43 + vertical 1 + skeleton 4 + workspace_policy 5 + iv8 1）
                / guard=PASS / deny=0（advisories ok, bans ok, licenses ok, sources ok）
Python：contracts 55 passed；sdk-python 13 passed（pyo3 0.29 重建）
远程 Linux x86_64 1.99：fmt=0 / clippy=0（过 double_must_use）/ test=0（54）/ guard=PASS
                / contracts 55 / sdk-python 13（maturin 出 cp313 wheel）；日志含 Compiling 佐证非陈旧二进制
```

**③ 决策链（全部落 ADR / 注记）**

- 辩论收口：评审员 A（证伪）vs B（从零）→ lead 裁决 → **ADR-0013**（弱槽 + 自持 `Global` 的 lease + `Collected`/回绕退役 + C-4 改挂 C-2 + C-1 属性面读法）、**ADR-0014**（同线程直呼 + `CallbackDeadline` + RAII 全入口 + 线程断言）；**ADR-0012 修正注记**（attach/detach 限定适用范围）；**ADR-0009 修正注记**（R1 措辞更正 + `pub use abi` 唯一例外落定）。评审意见原文：`docs/decision-briefs/2026-10-12-G1-辩论意见-{A,B}.md`。
- 实证事实：U-1（`forbid`×pyo3 宏编译通过 + 反向注入武装；0.23.5 首验、**0.29 复验**）；F1（`macros` 必须显式启用）；E0782 = pyo3 内部 `trait Probe` 命名冲突（issue #4792）。
- **pyo3 0.23 → 0.29.3**（QA R1 发现 RUSTSEC-2025-0020 / RUSTSEC-2026-0177；官方修复版本要求；机械迁移、测试期望零改动）。证据归档：`.workbuddy/refs/pyo3-lint-probe-evidence/`（0.23.5 首验 + `029/` 复验）。

**④ 缺陷闭环（QA Round 1 → 修复 → Round 2）**

| # | 现象 | 处置 | 状态 |
|---|---|---|---|
| G1-K-1 | 本地 deny 红（wildcard 依赖 + 2×RUSTSEC） | path 依赖补 version；pyo3 升 0.29.3 | ✅ R2 `DENY_EXIT=0` |
| G1-K-2 | 远程 clippy 红（1.99 `double_must_use`） | 删冗余属性 | ✅ 同机同命令 exit101→exit0 |
| G1-K-3 / K-4 | 两处注释漂移（realm.rs / workspace_policy.rs） | 更正注释 | ✅ R2 复核 |

**⑤ 第五轮遗留与已知问题（登记，非阻塞）**

| # | 事项 | 说明 | 状态 |
|---|---|---|---|
| **K-11** | **动态 import 未实现** | `set_host_import_module_dynamically_callback` + `PromiseResolver` 触发 V8 FATAL（`V8_Fatal → UnwindAndFindHandler → … → Script::Run`，SIGTRAP）；已整体回退；专项方法 = 对照 v8 crate 官方示例（`isolate.rs:230`）做最小复现 | **✅ 第六轮已修复（ADR-0015 已落）**：根因=宿主回调无挂起异常时空返回（撞 `Check failed: has_exception()`）；修复=全失败路径 reject；三测绿（engine-v8 46→52 期间复跑不回归） |
| K-12 | S8 条数预算不可得 | checkpoint 不可抢占、不给条数 ⇒ 墙钟截断 + `perform_checkpoint` 返回 1（如实口径） | 已登记 |
| K-13 | `perform_checkpoint` 不重写 `REALM_DATA_SLOT` | 依赖上次 `eval` 指针；G1 单 Realm 无碍 | 已登记 |
| K-14 | 工具链漂移观察 | 本地 1.92 vs 远程 1.99（stable）曾致 lint 单端红；**QA-R1·D1（1.99 `assert_is_empty` ×2）为又一实例**（修复后双端绿）；是否钉版待定 | 已登记 |
| — | K-7（远程无 cargo-deny） | 本轮裁决维持（平台无关检查、本地 + CI 覆盖） | 维持 |

---

### 第六轮证据台账（2026-10-13）—— G2 页面纵向（S0~S17）

> 组织：架构师（设计批 + 4 决策 brief + 四 ADR + 全程文档同步）｜工程师 ×2（批次 2A~2H；首会话 2A→2E 绿态交接，次会话接 2F~2H 与修复批）｜评审员 A/B（背对背辩论）｜QA 严过关（R1 + R2）。**结论：G2 闸门完成（QA R2：可进入下一闸门）。**

**① 交付物总表**

| 面 | 内容 | 证据要点 |
|---|---|---|
| 设计批 | `design-g2-page-2026-10-13.md`（**§0.1 修订索引 22 项** + §1~§9 全签名/契约/矩阵）+ 4 决策 brief + 2 图 | lead 逐项抽查 |
| 决策链 | **ADR-0015**（K-11 空返回纪律）、**0016~0019**（D1~D4 裁决）、**0017 as-built 修正注记**、ADR-0014 两条注记 | 评审意见 A（350 行）/ B（470 行）+ 裁决书 `2026-10-13-G2-裁决-主理人.md` |
| 实现 | 8 面：web-dom / web-events / policy / web-style / scheduler / network / **navigation**（parser·sink·document_write·controller·binding·input·page）/ engine-v8 增量 | 批次 2A~2H；每批 test-first + 门禁 |
| Python 面 | `sdk-python`（PyPage/PyNetwork/PyInput + BrowsingContext 持有）；pytest **13→22** | venv + maturin develop |
| 契约 | `page_control.py`（含 `JsErrorKind` 12 变体投影）；contracts **55→60** | `uv run pytest` |
| 纵向样例 | `navigation/tests/vertical_g2.rs` + pytest `test_vertical_page_sample` | §3.9 全链逐段断言 |
| QA | `docs/gate-report-g2-2026-10-13.md`（R1+R2）+ `.workbuddy/refs/g2-qa-evidence/` | 双端 |

**② 最终门禁（QA R2 独立复验 + lead 亲验）**

```
本地 macOS 1.92：fmt=0 / clippy=0 / test=0（158 = engine-v8 52 + vertical 1 + navigation 51 + vertical_g2 1
                + network 6 + policy 3 + scheduler 11 + web-dom 12 + web-events 9 + web-style 2 + runner 4+5 + iv8 1）
                / guard=PASS / deny=0
Python：contracts 60 passed；sdk-python 22 passed
远程 Linux x86_64 1.99：fmt=0 / clippy=0 / test=0（158）/ guard=PASS / contracts 60 / sdk-python 22
                （maturin；日志含 Compiling；rsync 已排除 .venv，U-6 html5ever 首次远程拉取成功）
覆盖率：83.47% regions / 82.66% lines（盲区如实：sink.rs 46%L；sdk-python llvm-cov 0% = pytest 口径）
```

**③ 决策链与关键实证**

- 辩论：评审 A/B 对 D1~D4 四方向与 brief 推荐全部一致；**12 条阻断项**（依赖环 / A/B 混写 / 不可中断 / 重绑缺失 / 无归宿 ×2 / `NativeFn + Send` / seq 双重借用 / `TreeSink` 全表面 / `dispose` / 死句柄 / PyO3 覆盖）→ 裁决 → ADR-0016~0019 全落。
- **K-11 修复（ADR-0015）**：「空返回纪律」（宿主回调空返回仅限挂起异常）。**P-1a** 探针=watchdog 截断可靠（≈grace+2~5ms、可恢复）；**P-1b**=`NativeFn` 去 `+ Send` 可行（0 反例）。
- **P-2 运行时确证**：`Tokenizer<TreeBuilder<DomSink>>` 在 `</script>` ⇒ `Script(node)`（node 即 script 元素）。
- **D5 判别力获证**（QA R2 反向注入）：倒置注入序 ⇒ 强化后两测试必红（`r2-d5-inverted-red.txt`）⇒ 还原复绿 + sha256 一致。
- 模块脚本纳入 G2 最小面（暂停点判 `type` 分流 / defer 队列独立于 ScriptBlocking / TLA 具名排除 / 三序回归 / 非 load-blocker）。
- **as-built 执行模型（U-10）**：阻塞脚本在 `load` 内同步执行（`eval_with_deadline`，固定 2s）；延期=脚本任务化 + timer 事件循环（后续闸门）。

**④ 缺陷闭环（QA R1 → 修复 → R2）**

| # | 现象 | 处置 | 状态 |
|---|---|---|---|
| D1 | 远程 clippy 红（1.99 `assert_is_empty` ×2） | 断言补消息 | ✅ R2 远程 exit 0 |
| D2 | §3.2 `external_script_blocks_until_loaded` 未实现 | `<script src>` 路径（ExternalScript blocker + 停泊 + 命中执行） | ✅ |
| D3 | `node_wrapper_recreated_after_collection` 缺席 | 补测试（真 GC） | ✅ |
| D4 | `try_borrow` 结构性规则未落地（0 处） | 30 处落点（V8 回调边界全覆盖）；**残余具名登记**（宿主驱动路径） | ✅ 闭环 |
| D5 | document.write 两测判别力不足 | 强化断言 DOM 注入序 + 反向注入获证 | ✅ |
| D6 | `fetch_aborted_on_navigation` 无页面级测试 | 补 | ✅ |

**⑤ 第六轮遗留与已知问题（登记，非阻塞）**

| # | 事项 | 状态 |
|---|---|---|
| **U-10** | 执行模型 as-built（`load` 内同步脚本）；延期=脚本任务化 / timer 事件循环 | 登记（ADR-0017 修正注记；QA 复核属实） |
| **U-12** | 模块求值未接 watchdog（设计称覆盖、实测未兑现） | 登记（QA 复核属实） |
| — | D4 残余：宿主驱动路径（`load`/`run_until`/`pump_fetch`/`resolve_completed`/`close`）未转 `try_borrow` | 登记（裁定不扩大） |
| — | `sink.rs` 46%L 盲区；sdk-python llvm-cov 0%（pytest 口径） | 登记 |
| — | 几何 stub（`capabilities.geometry="stub"`；完整几何属 G3）/ TLA 粗扫描 / 单 Realm 导航 JS 全局残留（G4） | 登记 |
| K-12/13/14 | G1 登记延续（K-14 + QA-R1·D1 实例） | 登记 |
| §16 | 完整兼容层属 G4 | 范围 |

**⑥ 批次与会话记录**

- 批次：2A（S1~S4）→ 2B（S5+S6）→ 2C（S7+S8+S9；S9b 结转）→ 2D（S10；S11/S12 并入集成批）→ 2E（S13 核心；会话绿态交接）→ 2F（S13余+S11+S12+S9b；新会话）→ 2G（S14+S15+doc.write 小修）→ 2H（S16+S17+补测）→ 修复批（D1~D6）。
- 会话：engineer（2A~2E 五批连续、绿态关闭）→ software-engineer-2（2F~2H + 修复批）；评审员 A/B、QA 严过关各独立角色。

---

## 变更记录

| 日期 | 变更 | 角色 |
|---|---|---|
| 2026-10-12 | 建立本文件；登记 T0 全部子任务、4 项待拍板项、T1/T2 规划 | lead |
| 2026-10-12 | 第二轮：落地 T0 全部可执行项；据 review 复核修复 3 P0 / 7 P1 / 6 P2；补全证据台账；新增 T0.10 / T0.11、D-05、G1-PRE-1 | lead + review + test |
| 2026-10-12 | 第三轮：维护者授权 AI 内部决策 → D-01~D-05 全部落 ADR（0002 / 0005）；**T1 全量交付**（契约 runner + manifest + 差分骨架 + uv/pytest + CI job，48 测试绿）；**T2.1 / T2.3 PoC 完成并落 ADR（0006 / 0007）**；新增 ADR-0008（T2 探针落盘与 `poc/` unsafe 白名单）；许可证落 `MIT OR Apache-2.0`；`license.workspace = true` 已加到 19 个成员 | lead + dev + test |
| 2026-10-12 | 第四轮：**T2.2 / T2.4 / T2.5 + G1-PRE-1 全部完成**并落 ADR（0009 / 0010 / 0011 / 0012）；本地五条门禁 + `uv run pytest`(48) + 三探针（`T2_2_PROBE_OK` / `T2_4_PROBE_OK`+`SWITCH_TO_STYLO` / `T2_5_PROBE_OK`）全绿；远程 Linux x86_64 主门禁四条全 0、pytest 48 绿；**远程三探针因 V8 静态库下载不可行而受阻（K-6）**；QA 补跑发现 `poc/` 逃逸 root 门禁（K-3）、ADR-0009 §A8 过度声明（K-4）、T2.5 D0a/D0b 具名断言缺口（K-1/K-2）；新增 ADR 索引与「本轮遗留与已知问题」章节。**同日 21:46 QA 回归复验**：K-3（`poc/` 格式）/ K-4（ADR-0009 §A8 过度声明）**已修并复验通过**（三探针格式 PASS、探针行为无回归、守卫仍 PASS）；K-1 / K-2（T2.5 D0a / D0b 具名断言）**仍开放**。**同日【冻结】后 QA 三阶段复验（本地五条+pytest+三探针 → 远程五条+pytest+三探针 → 回写）**：远程三探针经**本地中转 V8 归档**（`RUSTY_V8_ARCHIVE`）**全部实跑成功**；远程实测带出 **K-5 新缺陷**（T2.4 `C1 TARGET_LINUX_OK` 为**硬编码常量**，在 Linux 上实跑成功仍输出 `false`）；T2.5 隔离复跑 `COMBO_RUNS=14` 与 ADR-0012 条件表逐项吻合（首轮的组合 5 翻转确认为**负载伪影**） | lead + dev + test（QA 回写） |
| 2026-10-12 | **第四轮收尾（缺陷闭环）**：**K-1 / K-2**（T2.5 具名断言 `WATCHDOG_USES_ISOLATE_HANDLE` / `CLEAN_RESUME_AFTER_TERMINATE`）与 **K-5**（T2.4 `C1` 由硬编码常量改为**平台判定**）**全部修复（eng-t25）并由 QA 本地 + 远程双向复验**：远程 x86_64 Linux 上 `TARGET_LINUX_OK=true` / `VERIFIED_ON_LINUX`（修复前同命令为 `false` / `PENDING_REMOTE`）；T2.5 本地+远程均见两具名标记与 `ASSERT_PASS(m)(n)`。修复后本地五条 + `uv run pytest`(48) + 三探针 fmt 全绿、守卫 PASS。**附带登记 rsync-mtime 陈旧构建陷阱**（同步文件 mtime 早于远程既有产物时 cargo 静默跳过重编 ⇒ 首次远程 T2.5 跑的是陈旧二进制，已用 `touch` 强制重编）。剩余 K-3 / K-7 / K-8 / K-9 / K-10 均为**已登记项，非缺陷**。**K-3 已决策（lead）：`poc/` 维持独立、不纳入 root fmt/clippy/test 门禁**（维持 ADR-0008 决策 1；在途三探针已 fmt 归一；历史 `t2_0`/`t2_1` 保留原状；poc 交付前约定跑 `cargo fmt --all --check`；如未来纳入须维护者批准后动 CI / `AGENTS.md`）。台账并如实注明：远程为刷新 mtime 所做的 `touch` **仅改 mtime、未改内容**（两端 `sha256` 一致，可核） | lead + dev + test（QA 回写） |
| 2026-10-13 | **第五轮：G1 引擎纵向完成（S1~S13）**——六项能力全过（模块 5/6，动态 import 登记 K-11）；决策链 ADR-0013/0014 + ADR-0009/0012 修正注记；**pyo3 升级 0.29.3** 清两个 RUSTSEC；QA 两轮独立验证（R1 抓 4 项 → R2 全绿）+ 覆盖率 75.33%R / 75.97%L；**本地 + 远程门禁全绿**（含 `deny` exit 0、远程 clippy 1.99 绿）；遗留 K-11~K-14 登记。区间：2026-10-12 23:38 ~ 10-13 收尾 | lead + architect + engineer×2 + reviewer×2 + QA |
| 2026-10-13 | **第六轮：G2 页面纵向完成（S0~S17）**——设计批 + 背对背辩论（评审 A/B）+ 主理人裁决书 + **ADR-0015~0019**（含 as-built 修正注记）；8 crate 实装（批次 2A~2H，含两次绿态会话交接）；**纵向样例双版本绿**（cargo+pytest）；**K-11 修复 + P-1a/P-1b + P-2 运行确证**；QA 两轮（R1 抓 D1~D6 → 全闭环 → **R2 判别力反向验证获证**）**双端门禁全绿**（158/60/22）；覆盖率 83.47%R / 82.66%L；登记项 U-10/U-12/D4 残余等如实在案。 | lead + architect + engineer×2 + reviewer×2 + QA |
