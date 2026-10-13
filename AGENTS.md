# AGENTS.md — AI 协作硬约束

本仓库是 **iv8 自研运行时**（Rust 宿主 + rusty_v8 + PyO3 的浏览器 API 模拟层，兼容冻结的 iv8 社区版契约）。
本文件是对所有 AI 编码代理的**强制约束**，与 CI 门禁同级。**读完本文件再动手。**

## 权威文档（冲突时优先级从高到低）

1. `docs/adr/` — 唯一架构事实源
2. 《iv8 自研运行时完整技术方案》（GPT 版 docx，基准文档）
3. `docs/vibe-coding-质量保障方案.md` — 质量流程细则
4. 本文件 — 行为约束摘要
5. `docs/decision-brief-template.md` — 决策上报模板（拍板走外脑时的格式约束）

对话中产生的任何架构决定，必须**先落 ADR 再写代码**。AI 不得发明架构。

## 铁律（违反任何一条 = PR 拒绝）

1. **测试先行**：先写/改测试（红），再实现（绿）。实现 PR 里禁止同时修改测试期望值。
2. **golden 独立修改**：快照、`compat-manifest.json`、差分基线只能通过独立 PR 修改，并写明理由（规范变更 / 基线升级 / 确认是 bug 修复）。**禁止改 golden 来迁就实现。**
3. **一事一 PR**，diff ≤ 400 行；功能与重构严格分离；禁止跨 crate 的"顺手"改动。
4. **unsafe 全仓禁止**（workspace lints 强制）。仅白名单 crate（`engine-v8` 及显式声明的 FFI 薄桥）例外，且每个 unsafe 块必须带 `// SAFETY:` 不变量注释。
5. **V8 / rusty_v8 类型不得越出 `engine-v8` crate**（退出机制，见技术方案第 16 节）。
6. **高风险路径改动必须走「决策上报协议」（见下节）获批**，AI 产出仅为初稿：
   - `crates/engine-v8/**` 中的 unsafe 块与 GC 桥（rusty_v8/cppgc 循环回收）
   - crypto 原语封装与随机数
   - scheduler（事件循环 / 消息泵 / 暂停恢复 / 时序）
   - PyO3 回调边界（GIL 获取与释放）
7. **AI 永远不得修改**：`.github/workflows/**`、`CODEOWNERS`、安全策略文件、`deny.toml`、golden 文件（另见第 2 条）、本文件自身。
8. **兼容率单调性**：任何改动不得使 compat-manifest 的 verified 条目数下降（CI 阻断）。
9. **每个提交必须能回答**：本次改动的验收测试是哪一条？答不出来 = 不提交。
10. **不确定就停**：架构疑问先走「决策上报协议」交维护者，不猜、不静默降级、不用"能跑就行"糊弄。

## 决策上报协议（所有需要维护者拍板的场合）

**维护者不阅读 Rust 代码。** 凡是需要拍板的事，不允许只给简短摘要，必须产出一份**自包含的决策上报（decision brief）**，让维护者可以整段复制给外部顶级 AI 讨论并带回结论。模板：`docs/decision-brief-template.md`。

触发场合：
- 铁律第 6 条的高风险路径改动
- 新建或变更 ADR（架构决策）
- golden / manifest / 差分基线的修改
- unsafe 白名单的增删
- 铁律之间冲突、或任何拿不准的场合

brief 的硬性要求：
1. **自包含**：读者（外部 AI）没有本仓库任何上下文。相关代码、报错、测试输出**原文粘贴**，不转述、不省略、不"详见某文件"。
2. **面向不懂 Rust 的维护者**：Rust 与系统编程术语必须配一句平实解释（例：unsafe ≈ "绕过语言内存安全检查的代码块"）。
3. **必备内容**：背景与位置、要拍板的具体问题、至少两个备选方案（含做法、关键代码原文、优缺点、风险、长期影响）、证据原文、本仓库相关约束条款（外部 AI 的建议必须在这些边界内给出）、本代理的推荐及理由（明确标注"仅供参考"）。
4. **单文档交付**：填完模板即为一份可直接粘贴的完整文档。

**结论回传后的流程**：把维护者带回的结论整理成 ADR（记录备选方案、讨论要点、选择理由），ADR 落档后才动代码。**没有 ADR 落档的结论不执行。**

## 会话纪律

- 动手前先读：相关 ADR、目标 crate 的模块边界、涉及的 manifest 条目。
- 长会话中的决定及时落地到 ADR / manifest / TODO，不依赖对话记忆。
- **所有给维护者的说明，默认其不读 Rust**：解释要用平实语言，证据要贴原文。
- Web IDL 绑定代码由生成器产出，**禁止手改** `*-generated` 文件。
- 提交信息 / PR 描述必须含三段：意图（对应哪条 manifest 条目或 ADR）、影响面（哪些 crate 与测试）、证据（测试输出、快照 diff）。

## 开发环境（本地主力开发 + 远程目标机验证）

- **本地（macOS）**：主力开发机。代码生成、编辑、编译、日常测试都在本地做（内存充足，链接 V8 无压力）。
- **远程目标机**：`ssh ubuntu@124.221.1.78`（SSH key 免密登录，已验证连通）。Ubuntu 24.04 · x86_64 · 4 核 · 3.6 GB 内存 · 磁盘余量约 56 GB。已装 git / rsync / Rust 1.99.0（cargo、rustfmt、clippy 齐全）/ uv 0.13.0。操作细节见 `docs/dev-env.md`。
- **远程角色**：**Linux x86_64 目标平台验证机**。发布产物（wheel）面向 manylinux x86_64。"mac 上开发的代码能否在 Linux 跑"由**合并前在远程跑一遍测试门禁**保证，而不是靠在同一平台开发。
- **远程项目路径**：`~/mmv8`（固定，不要另起路径）。
- **同步方式**：`rsync -av --delete --exclude='.workbuddy' /Users/dennis/WorkBuddy/mmv8/ ubuntu@124.221.1.78:~/mmv8/`
- **内存约束（远程 3.6 GB）**：远程只跑测试与必要时的发布构建；链接 V8 若 OOM，走决策上报协议评估加 swap / 升配 / 改用 CI 构建。**不许自行修改远程机器的系统配置。**
- **跨平台纪律（防"mac 能编译、Linux 跑不了"）**：
  1. 文件名 / 模块名**大小写必须严格一致**——mac 文件系统默认不区分大小写而 Linux 区分，这是最经典的跨平台坑；
  2. 路径处理一律用 `std::path`，禁止手拼 `/`、禁止硬编码平台路径；
  3. 平台相关代码必须 `#[cfg(...)]` 显式门控，禁止静默依赖 mac-only 行为；
  4. 新增依赖必须确认支持 linux x86_64；
  5. wheel 按平台分别构建：mac 产物给 mac，linux x86_64 wheel 在远程或 CI 构建（abi3 只省 Python 版本矩阵，不省平台）。
- 提交前自检命令日常在**本地**执行；合并前另在**远程**执行一遍 `cargo test`（Linux 门禁）。

### 工具链策略（维护者已授权）

- **工具链与开发依赖由 AI 自主安装，不需逐项请示**：包括 rustup / Rust 组件 / cargo 子命令（cargo-deny、insta 等）/ uv / Python 解释器与包。
- 限定为**用户级安装**（`~/.cargo`、`~/.local`、`~/.rustup`、uv 管理的 venv），不改系统包配置。
- **仍需走决策上报**：系统级改动（swap、sysctl、`apt` 系统包、改 `/etc`），以及任何不可逆或影响机器其他用途的操作。
- 新增工具链前先确认跨平台可用（本地 macOS + 远程 Linux x86_64 两边都能装）。

### Python 约定

- **一律用 `uv` 管理**：解释器、虚拟环境、依赖安装都用 uv（`uv venv` / `uv pip install` / `uv run`），不直接裸用 pip、不用 conda。
- **Python 侧测试统一 `pytest`**：`uv run pytest`。Python 侧指 Python SDK 绑定、契约 runner、差分 harness、WPT runner 等宿主侧代码。
- Rust 侧测试仍用 `cargo test`；两边分开跑，门禁都要过。

### 镜像源（两机一致，已配置）

- cargo → `rsproxy.cn`（sparse index，写入 `~/.cargo/config.toml`）
- Python（uv / pip）→ 清华源 `https://pypi.tuna.tsinghua.edu.cn/simple`（uv 写 `~/.config/uv/uv.toml`，pip 写 `~/.config/pip/pip.conf`）
- 换源或新增源时，两机同步修改；不要在代码里硬编码镜像地址。

## 提交前自检（本地日常执行；合并前在远程再跑一遍，必须全绿）

```bash
# Rust 侧
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo deny check
# 铁律 4 的机器执行：白名单外禁 unsafe、禁 crate/mod 级开口、白名单内须 SAFETY 注释
bash scripts/check-unsafe-whitelist.sh

# Python 侧（uv + pytest）
cd contracts && uv run pytest
```

（Cargo workspace 建立前，本节在 workspace 就位后生效。）

## 冲突处理

本文件与 CI 实际门禁冲突时，以 CI 为准，并向维护者报告差异。
