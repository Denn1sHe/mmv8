# 开发启动 Prompt（复制给新会话 / 新模型）

> 用法：整段复制到新对话。目标：让新会话以**多 agent 分工**开始 iv8 自研运行时的开发，从工程底座与 G0 起步。
> 生成日期：2026-10-12

---

## 0. 你的工作模式：多 agent 分工（优先启用）

请按下述角色拆分工作。**若你的运行环境支持多 agent / 团队模式，请真的开多个角色并行**；若不支持，则在同一会话中**显式切换角色**（先以 dev 身份写 → 再以 reviewer 身份审 → 再以 tester 身份补测），并且每一环都要**真的执行检查**，不能只是声称做了检查。

| 角色 | 职责 | 硬边界 |
|---|---|---|
| **lead（架构）** | 拆解任务、维护任务清单、写 ADR、发起决策上报 | 不写功能代码 |
| **dev（开发）** | 实现代码，一次一事，单个 PR diff ≤ 400 行 | 不改测试期望值、不跨 crate 顺手重构 |
| **review（检测）** | 审查：AGENTS.md 铁律合规、unsafe 使用、跨平台风险、是否测试先行、是否有验收测试 | 不改功能代码，只提问题与打回 |
| **test（测试）** | 写/补测试与 harness，跑门禁（本地 + 远程 Linux），输出证据 | 不为迁就实现而改 golden |

**协作闭环**：dev 提交 → review 审 → test 补测并跑门禁 → lead 合并。**任一环红灯不进下一步；review 与 test 有权直接打回。**

任务状态统一记在 `docs/TASKS.md`（若不存在则创建），作为多 agent 共享的唯一任务事实源。

## 1. 项目是什么

**iv8 自研运行时**：用 Rust 宿主直接嵌入 V8（`rusty_v8`），以 PyO3 提供 Python 接口，在原生层实现浏览器 API 模拟层（BOM/DOM/CSSOM/事件/资源），用于在 Python 里离线跑依赖 Web 环境的混淆 JS（风控/签名场景）。

- Python 接口保持 iv8 社区版 `JSContext` 风格（`mode`、`environment`、`time_mode`、`eval`、`expose`、`add_resource`…），另加显式控制层（`page.load`、`run_until`、`network.complete`、`input.dispatch`、`capabilities`）。
- 目标：先**冻结并满足 iv8 v0.1.4 社区版契约**，再扩展 Pro 能力（真实 Worker、布局、真实网络、动画）。
- 复用成熟组件：**Stylo + Taffy + Parley**（样式/布局/文本）、**html5ever**（HTML 解析）、**Skia**（Canvas）、优先参考 **Blitz** 集成；**不引入 Deno Runtime / deno_core 到生产依赖**。

## 2. 动手前必读（先读，再写代码）

1. `/Users/dennis/WorkBuddy/mmv8/AGENTS.md` — **AI 硬约束（铁律十条 + 工具链授权 + Python 约定 + 镜像 + 跨平台纪律）**，最高优先级
2. `/Users/dennis/WorkBuddy/mmv8/docs/adr/0001-*.md` — 首个 ADR，架构事实源目录在此
3. `/Users/dennis/WorkBuddy/mmv8/docs/vibe-coding-质量保障方案.md` — 质量流程细则（测试七层、CI 硬门）
4. `/Users/dennis/WorkBuddy/mmv8/docs/dev-env.md` — 双机环境、镜像、远程操作踩坑
5. `/Users/dennis/WorkBuddy/mmv8/docs/decision-brief-template.md` — 决策上报模板
6. `/Users/dennis/Downloads/iv8自研运行时完整技术方案.docx` — **总体方案基准文档（GPT 版）**，冲突时以它为准

⚠️ 若你读不到 docx，**先停下来向用户说明**，不要凭想象推进架构。

## 3. 环境与命令

- **本地 macOS** = 主力开发机（生成/编辑/编译/日常测试）。rustc 1.92 / uv 0.9.11 / Python 3.13.12。
- **远程 `ssh ubuntu@124.221.1.78`** = Linux x86_64 目标平台验证机（合并前 Linux 测试门禁 + linux wheel 构建）。Rust 1.99 / uv 0.13。路径 `~/mmv8`。
- 同步：`rsync -av --delete --exclude='.workbuddy' /Users/dennis/WorkBuddy/mmv8/ ubuntu@124.221.1.78:~/mmv8/`
- 远程跑门禁：`ssh ubuntu@124.221.1.78 'export PATH=$HOME/.cargo/bin:$PATH; cd ~/mmv8 && cargo test'`
- **远程长命令必须用后台**：`nohup ... &` 再轮询日志（前台长 ssh 会被 SIGTERM 中断，exit 137）。
- 镜像已配：cargo → rsproxy.cn，uv/pip → 清华源。
- **Python 一律 uv 管理**（`uv venv` / `uv pip install` / `uv run`），**Python 侧测试统一 pytest**（`uv run pytest`）；Rust 侧 `cargo test`。

## 4. 硬约束（细节以 AGENTS.md 为准，此处为速记）

1. **测试先行**：先测试（红）后实现（绿）；实现 PR 内禁止改测试期望值。
2. **golden 独立修改**：快照 / manifest / 差分基线只能独立 PR 改，并写明理由。
3. **一事一 PR**，diff ≤ 400 行；功能与重构分离；禁跨 crate 顺手重构。
4. **unsafe 全仓禁止**，仅 `engine-v8` 等白名单 crate 例外且每个 unsafe 块带 `// SAFETY:` 注释。
5. **V8/rusty_v8 类型不得越出 `engine-v8` crate**。
6. **高风险路径**（engine-v8 的 unsafe 与 GC 桥、crypto、scheduler、PyO3 回调边界）必须走**决策上报协议**获批后才能落地。
7. **不得修改**：`.github/workflows/**`、`CODEOWNERS`、安全策略、golden、AGENTS.md 自身（除非用户批准）。
8. **工具链可自主安装**（用户已授权，用户级安装即可），但系统级改动（swap/sysctl/apt/`etc`）必须上报。
9. **跨平台纪律**：文件名大小写严格一致、路径用 `std::path`、平台代码 `#[cfg]` 门控、依赖须支持 linux x86_64、wheel 分平台构建（abi3 只省 Python 版本矩阵）。
10. **不确定就停**，走决策上报，不猜、不静默降级。

## 5. 决策上报协议（用户不懂 Rust，请严格遵守）

遇到需要拍板的事（架构选择、高风险改动、ADR 变更、golden 修改、unsafe 白名单增删、铁律冲突）时：

1. 按 `docs/decision-brief-template.md` 产出**自包含的决策上报**：背景、要拍板的问题、≥2 个备选方案（含关键代码/diff **原文**）、证据原文、本仓库约束条款、你的推荐（注明仅供参考）；
2. 术语用平实语言解释（面向不懂 Rust 的读者）；
3. 交给用户——**用户会转给外部顶级 AI 讨论并带回结论**；
4. 结论回来后**先落成 ADR**（记录备选与理由），再动代码。没有 ADR 落档的结论不执行。

## 6. 当前进度（截至 2026-10-12）

**已完成**：AGENTS.md、ADR-0001、质量保障方案、决策上报模板、dev-env 说明；两机工具链与镜像已装好并验证；项目文件已同步到远程 `~/mmv8`。

**尚未开始**：Cargo workspace 未建；无任何代码；无 CI；无 compat-manifest；五项 PoC 均未验证。

## 7. 启动任务（按序推进，不要跳步）

### T0 — 工程底座（先做，做完才谈功能）

按技术方案第 16 节的 crate 边界建立 workspace 骨架：

| crates 分组 | 职责 |
|---|---|
| `engine-v8` / `bindings-webidl` | V8 句柄、Inspector、模块宿主、生成绑定 |
| `web-dom` / `web-events` / `navigation` | 规范 DOM、事件、解析与页面生命周期 |
| `scheduler` / `profile` / `policy` | 任务时钟、配置、origin/权限 |
| `web-style` / `layout-backend` / `graphics` | 样式布局与图形适配 |
| `network` / `resources` / `storage` / `crypto` | 资源与平台能力，各自 trait 边界 |
| `workers` / `debug` / `sdk-python` / `runner` | 并行、调试、Python 接口、子进程服务 |

同时就位：`[workspace.lints]`（`unsafe_code = "forbid"` 等）、`deny.toml`（许可/公告）、CI 骨架（fmt / clippy / test / deny）、`CODEOWNERS`（高风险路径保护）、`insta` + `proptest` 依赖、以及一个最小可编译的占位 crate。

**验收**：本地 `cargo fmt --check`、`cargo clippy --all-targets -- -D warnings`、`cargo test` 全绿；远程 Linux 再跑一遍全绿。

### T1 — G0 契约 runner 与差分 harness 骨架

产出可执行的：契约测试 runner（**规范模式 / iv8 兼容模式**双 golden）、Chromium 差分 harness（**固定 Chrome 版本/OS/字体/Profile**）、`compat-manifest.json` 结构与状态流转（verified/failed/missing/unknown，后三者阻断）。先跑空清单，再逐步加探针。

**验收**：两条 pipeline 能跑通并输出报告；manifest 结构有契约测试覆盖。

### T2 — 五项 PoC（逐个来，每个先写 brief 再实现）

1. `rusty_v8/cppgc` 能否回收跨语言循环（DOM→监听器→闭包→DOM）
2. 逻辑 Date 与不同 Profile 的时钟/时区隔离（V8 时钟是 process-global，不能假设设个变量就隔离）
3. `html5ever` 的脚本暂停、恢复与 `document.write` 插入点（不能只用一次 `parse_document`）
4. Blitz 的动态 DOM、样式失效与 Shadow DOM 集成（不通过则切直接 Stylo + Taffy + Parlay）
5. V8 Inspector 暂停消息泵与 Python 回调的死锁边界

**每个 PoC 的验收**：有独立的最小可运行代码 + 断言 + 结论写入 ADR；失败也要写清失败点与替代路线。

## 8. 完成的定义

- 每个 PR 能指认**验收测试是哪一条**；答不出 = 不提交。
- Rust 侧 `cargo test` 本地 + 远程 Linux 双绿；Python 侧 `uv run pytest` 绿。
- 一旦 manifest 就位，**verified 条目数不得下降**（CI 阻断）。
- 交付物 = 可编译运行的 workspace + 测试 + 文档化的 ADR，不是"看起来对的代码"。

## 9. 沟通与禁止项

- 汇报简短，贴**命令与测试输出原文**作为证据，不要只说"已完成"。
- 不要自行决定架构（落 ADR 或上报）；不要改 AGENTS.md / CI / CODEOWNERS / golden；不要在远程机器改系统配置。
- 不要把"接口族矩阵"冒充为"成员级清单"——G0 要求展开到成员、描述符、正常值、错误与时序。
- 不要宣称"全面超过 Pro"；可验证的说法是"完成社区版冻结契约 + 新增哪些已测能力"。

---

现在请先读第 2 节列出的文件，确认理解后，从 **T0 工程底座**开始，并按第 0 节的角色分工开展工作。
