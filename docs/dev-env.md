# 开发环境说明（双机工作流）

> 配套：`AGENTS.md`「开发环境」章节。本文件是操作细节，AGENTS.md 是硬约束摘要。

## 角色分工

| | 本地（macOS） | 远程（`ssh ubuntu@124.221.1.78`） |
|---|---|---|
| 角色 | 主力开发机 | Linux x86_64 目标平台验证机 |
| 职责 | 代码生成、编辑、编译、日常测试 | 合并前的 Linux 测试门禁；linux x86_64 wheel 发布构建（内存允许时） |
| 路径 | `/Users/dennis/WorkBuddy/mmv8` | `~/mmv8`（固定） |
| 系统 | macOS | Ubuntu 24.04 · x86_64 · 4 核 · 3.6 GB 内存 · 磁盘余约 56 GB |

## 环境清单（已安装，2026-10-12）

| 组件 | 本地 | 远程 |
|---|---|---|
| rustc / cargo | 1.92.0 | 1.99.0 |
| rustfmt / clippy | 随工具链 | 随工具链（default profile） |
| uv | 0.9.11 | 0.13.0 |
| Python | 3.13.12 | uv 按需装 |
| git / rsync | 有 | 有 |

工具链按需继续安装由 AI 自主完成（维护者已授权），限定用户级安装；系统级改动仍需决策上报。

## 为什么这样分工（回答"mac 开发的版本能不能在 Linux x86 跑"）

- **源码层**：同一份 Rust 源码在 mac 与 Linux 都能编译运行，前提是遵守跨平台纪律（见 AGENTS.md）。开发在 mac 上完全没问题。
- **产物层**：**二进制不跨平台**。mac 编译出的是 mac 产物，Linux x86_64 的 wheel 必须在 Linux x86_64（或等价容器/CI）上构建。abi3 稳定 ABI 只合并 Python 版本矩阵，不合并平台。
- **风险闭环**：真正的风险不是"开发平台不对"，而是**平台差异 bug 只在另一边暴露**（文件名大小写、路径、平台 API、依赖的平台支持）。对策不是换机器开发，而是：① 编码纪律（AGENTS.md 跨平台五条）；② **合并前在远程跑一遍 `cargo test` 的 Linux 门禁**。
- 不选交叉编译（mac → linux x86_64）的原因：rusty_v8 依赖按平台分发的预编译 V8 静态库，PyO3 扩展还需要目标平台的 Python 链接环境，交叉编译收益低、坑多；直接在 Linux 上构建最稳。

## 常用命令

```bash
# 同步本地 → 远程（排除 .workbuddy / .venv）
rsync -av --delete --exclude='.workbuddy' --exclude='.venv' /Users/dennis/WorkBuddy/mmv8/ ubuntu@124.221.1.78:~/mmv8/

# 远程跑 Linux 测试门禁（远程 PATH 不含 cargo，需先 export）
ssh ubuntu@124.221.1.78 'export PATH=$HOME/.cargo/bin:$PATH; cd ~/mmv8 && cargo test'

# Python 侧（一律 uv 管理，pytest 测试）
uv venv && uv pip install -r requirements.txt && uv run pytest
```

## 镜像源配置（两机已配置）

| 源 | 地址 | 本地配置 | 远程配置 |
|---|---|---|---|
| cargo（crates.io 代理） | `https://rsproxy.cn/index/` | `~/.cargo/config.toml` | `~/.cargo/config.toml` |
| Python（uv） | `https://pypi.tuna.tsinghua.edu.cn/simple` | `~/.config/uv/uv.toml` | `~/.config/uv/uv.toml` |
| Python（pip 直用） | 同上 | `~/.config/pip/pip.conf` | `~/.config/pip/pip.conf` |

cargo 配置要点：

```toml
[source.crates-io]
replace-with = "rsproxy-sparse"

[source.rsproxy-sparse]
registry = "sparse+https://rsproxy.cn/index/"
```

另设 `RUSTUP_DIST_SERVER=https://rsproxy.cn`、`RUSTUP_UPDATE_ROOT=https://rsproxy.cn/rustup` 可加速 rustup 自身与工具链下载。

**实测（2026-10-12，远程目标机）**：从官方 `static.rust-lang.org` 拉取时单个小文件建连约 **11.6 s**，
安装 stable 的 6 个组件跑了 17 分钟仍未完成；改用 `rsproxy.cn` 后建连 **0.52 s**，同一安装 **25 秒**完成。
差距约 20 倍。**远程机的 `~/.bashrc` 顶部已写入这两个变量**（非交互式 ssh 也会生效；
备份在 `~/.bashrc.iv8-backup-20261012`）。若哪天远程 rustup 又变慢，先确认这两个变量是否还在。

验证（两机均已通过）：`cargo fetch` 成功下载 crate；`uv pip install --dry-run requests` 成功解析。

## 操作注意事项（踩坑记录）

1. **远程前台长命令会被 SIGTERM 中断**（exit 137；曾导致 rustup 装一半留下残缺工具链，报 `missing manifest in toolchain`）。长任务一律后台跑再轮询：
   ```bash
   ssh ubuntu@124.221.1.78 'cd ~/mmv8 && nohup bash -c "cargo build > /tmp/build.log 2>&1; echo EXIT=\$? >> /tmp/build.log" >/dev/null 2>&1 &'
   sleep 30; ssh ubuntu@124.221.1.78 'tail -5 /tmp/build.log'
   ```
2. **残缺工具链修复**：`rustup toolchain uninstall stable && rustup toolchain install stable --profile default`（同样后台跑）。`--profile default` 才带 rustfmt/clippy，`minimal` 不带。
3. macOS 无 `timeout` 命令，脚本里别用；改用后台 + 轮询。
4. 远程 ssh 执行 cargo/rustc 前需 `export PATH=$HOME/.cargo/bin:$PATH`。
5. **远程 rustup 必须走镜像源**：不设 `RUSTUP_DIST_SERVER` 时它会去 `static.rust-lang.org`，
   实测慢约 20 倍（17 分钟未完成 vs 镜像 25 秒完成）。变量已写入远程 `~/.bashrc` 顶部，
   手动构造 ssh 命令时可再显式 `export` 一次兜底。
6. **不要用 `pkill -f rustup` 去终止远程卡住的 rustup**：该模式会匹配到 sshd 上承载你这条
   命令的进程自身（命令行里含 "rustup" 字样），导致自己的连接被切断（ssh exit 255）。
   改用 `ps -eo pid,etimes,comm | grep rustup` 找到 pid 后按 pid 终止。
7. **⚠️ rsync-mtime 陈旧构建陷阱（2026-10-12 实踩）**：`rsync -a` 保留源文件 mtime，而 cargo
   的「新旧判定」基于 mtime。当同步过来的源码 mtime **早于**远程既有构建产物时，cargo 会
   **静默跳过重编**，导致跑的其实是**陈旧二进制**。
   - **甄别方法**：跑完后检查日志（a）有没有 `Compiling <crate>` 行；（b）输出里有没有
     **该源码当前应产生的标记**（例如新加的输出行）。两者缺一即高度可疑。
   - **处置**：远程 `touch` 源文件强制刷新 mtime 后重跑（touch 只改 mtime、不改内容；
     善后时比对两端 `sha256` 即可自证）；或同步时加 `--checksum`/不保留 mtime。
   - **预防**：改动 POC 源码后同步到远程，跑之前先确认日志出现 `Compiling`；或统一在
     同步后 `touch` 变更文件。
8. **⚠️ `.venv` 跨平台覆盖陷阱（2026-10-13 实踩）**：`rsync -a` 会把本地 macOS 的 `.venv/`
   （内含 macOS 的 Python 解释器 / `.so` / `bin` 软链）同步到远程，**覆盖**远程的 Linux `.venv`
   ⇒ 远程 Python 侧报错。**处置**：由 `uv`/`maturin` 在远程**重建** `.venv`；
   **预防**：同步时 `--exclude='.venv'`（已加入上方命令），各机**自建** `.venv`。
   - **同源教训（工具链漂移）**：远程 `rustc **1.99.0**` 相对本地 `1.92.0` 引入**新 clippy lint**
     （如 `clippy::double_must_use`，G1-K-2；QA-R1「D1」为**又一实例**）⇒ **仅远程触发、本地绿**。
     **对策**：门禁**两端都跑**（本地 + 远程 `cargo clippy --all-targets --all-features -- -D warnings`）。
     归入 `TASKS.md` **K-14（工具链漂移）**。

## 内存注意事项（远程 3.6 GB）

- 日常 `cargo test`（debug 构建）一般够用。
- **release 链接 V8/rusty_v8 相关 crate 可能 OOM**。对策优先级：限制并行度（`cargo build -j2 --release`）→ 走决策上报评估加 swap → 升配 / 用 CI 构建。
- 不许自行在远程改系统配置（swap、sysctl 等）。
