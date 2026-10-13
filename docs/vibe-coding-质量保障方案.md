# iv8 自研运行时 · Vibe Coding 质量保障方案

> 前置文档：《iv8 自研运行时完整技术方案》（GPT 版 docx，为基准）
> 本文档回答一个问题：**当主要代码由 AI 生成时，如何让质量不依赖"人审每一行代码"。**
> 版本 0.1 ｜ 2026-10-12

---

## 0. 核心原则

Vibe coding 的风险不是 AI 写不出代码，而是 AI 写出**看起来对**的代码。对策只有一条：

> **把"对不对"的判断从人转移到机器；人只审机器判不了的东西。**

本项目天然具备三样机器可判定的验收资产（技术方案已定义，无需新发明）：

| 资产 | 判定内容 | 机器可判定性 |
|---|---|---|
| 冻结契约（compat-manifest） | 每个 API 的成员/描述符/正常值/错误/时序 | 完全 |
| 固定 Chromium 差分基线 | 语义与行为偏差 | 完全 |
| WPT 子集 + 纵向样例 | 规范符合度、端到端流程 | 完全 |

由此推出 vibe coding 的**审查对象转移**：人的审查重点从"实现代码"（几万行，审不动）转移到"测试与契约"（声明式、可逐条读）。审测试比审实现便宜一个数量级，且更可靠。

**一条铁律：测试先行。** 每个功能先让 AI 从 manifest/规范生成测试（红），再让 AI 实现（绿），人只验收测试本身。AI 永远不允许在实现 PR 里同时修改测试期望值。

---

## 1. 语言层硬护栏（Rust 免费送的质量，全部开满）

```toml
# 根 Cargo.toml —— workspace 级 lint，所有 crate 强制继承
[workspace.lints.rust]
unsafe_code = "forbid"        # 默认全仓禁止 unsafe
missing_docs = "warn"

[workspace.lints.clippy]
all = { level = "deny", priority = -1 }
pedantic = { level = "warn", priority = -1 }
# 按项目实际放宽个别过于苛刻的条目，但必须显式列出、可审查
```

- **unsafe 白名单制**：仅 `engine-v8`（V8/rusty_v8 绑定）、必要的 FFI 薄桥 crate 允许 `#[allow(unsafe_code)]`，且每个 unsafe 块必须带 `// SAFETY: ...` 注释说明不变量。用 `cargo geiger` 在 CI 里输出 unsafe 计数趋势。
- **`cargo fmt --check` + `cargo clippy --all-targets -- -D warnings`** 作为 CI 合并门槛，不是建议。
- **`cargo deny check`**（licenses / bans / advisories / sources）+ `cargo audit`：许可合规（MPL 边界）和依赖安全公告一次解决，直接对应技术方案第 19 节的发布工程要求。
- **MSRV 固定**并进 CI 矩阵；`Cargo.lock` 提交进仓库，保证可重复构建。

---

## 2. 测试体系（机器验收的主体，七层）

| 层 | 内容 | 工具 | 阻断级别 |
|---|---|---|---|
| L0 | IDL 生成代码自检：从 webref 快照生成的绑定编译即正确，禁止手改生成物 | build.rs + 生成器自带测试 | 合并 |
| L1 | 单元测试 + **快照测试**：错误消息文本、属性描述符表、`getOwnPropertyNames` 顺序、`toString()` 输出——所有"必须逐字对齐"的输出 | `insta` | 合并 |
| L2 | **属性测试**：不变量而非样例。结构化克隆往返等价、DOM 树父子计数守恒、removeNode 后监听器/定时器清理 | `proptest` | 合并 |
| L3 | **契约测试**：compat-manifest 驱动，双模式（规范模式 / iv8 兼容模式）分开 golden，status=verified/failed/missing/unknown，后三者阻断发布 | 自研 runner（G0 交付物） | 发布 |
| L4 | **差分测试**：固定 Chromium 版本/OS/字体/Profile 的探针 diff，输出按严重度排序的差异清单 | 自研 harness（G0 交付物） | 发布 |
| L5 | **纵向样例**：技术方案第 7 节流程（解析→阻塞脚本→document.write→读布局→fetch 注入→MO→click→导航） | 集成测试 | 发布 |
| L6 | **生命周期与故障**：Context 循环创建/销毁的 RSS 断言、多 isolate 并发压测、异常退出清理 | 集成测试 + sanitizer | 发布 |
| Fuzz | URL 解析、HTML 片段、Set-Cookie、结构化克隆序列化 | `cargo-fuzz`，CI 定时跑 | 缺陷跟踪 |

工具约定：Rust 侧用 `cargo test`；**Python 侧（SDK 绑定、契约 runner、差分 harness、WPT runner）一律用 uv 管理环境、pytest 跑测试**（`uv run pytest`）。两边分开跑，门禁都要过。

关键点：

- **L1 快照测试是 vibe coding 的杀器**。AI 最容易在不该变的地方悄悄改变行为（错误消息措辞、枚举顺序、描述符标志位）。insta 快照把这些全部钉死，任何无意漂移在 review diff 里一眼可见。
- **golden 的修改规则**：测试期望值（快照、manifest、差分基线）只能通过**独立 PR** 修改，且必须写明"为什么期望值该变"（规范变更/基线升级/确认是 bug 修复）。这是防止 AI"改测试迁就实现"的唯一防线。

---

## 3. AI 协作流程纪律（vibe coding 特有，与工具无关）

### 3.1 架构事实源在仓库里，不在对话里

- `docs/adr/`（Architecture Decision Records）是唯一架构事实源：选型、crate 边界、模块职责、退出机制（engine-v8 隔离 V8 类型）全部成文。
- **每次 AI 会话开头注入**：ADR 索引 + 当前任务的 crate 边界 + 相关 manifest 条目。AI 不允许发明架构；对话里产生的任何架构决定必须先落 ADR 再写代码。
- 违反边界的代码（如 V8 类型泄漏到 engine-v8 之外）由 lint/类型系统直接拒绝，不靠自觉。

### 3.2 小步提交

- 一事一 PR；diff 超过 ~400 行必须拆分。
- 功能与重构严格分离：AI"顺手重构"是质量事故的头号来源，禁止跨 crate 的顺手改动。
- PR 模板强制三段：**意图**（对应哪条 manifest 条目/哪份 ADR）、**影响面**（改了哪些 crate/测试）、**证据**（测试输出、快照 diff、差分报告）。

### 3.3 高风险面强制评审（CODEOWNERS 思路 + 决策上报协议）

以下文件类别**永远需要评审批准**，AI 生成只是初稿：

| 高风险面 | 原因 |
|---|---|
| `engine-v8` 全部 unsafe 块、GC 桥（rusty_v8/cppgc 循环回收） | 内存安全、悬垂指针 |
| `crypto` 原语封装、随机数 | 指纹可信度、常量时间 |
| `scheduler`（事件循环、消息泵、暂停恢复） | 死锁、公平性、时序正确性 |
| PyO3 回调边界（GIL 获取/释放） | 死锁、崩溃 |
| CI 配置、CODEOWNERS、安全策略文件 | 防止 AI 自己拆掉门禁 |

**评审方式（2026-10-12 修订）：维护者不阅读 Rust 代码。** 上述高风险面的批准一律走**决策上报协议**（定义见 `AGENTS.md`，模板见 `docs/decision-brief-template.md`）：

1. AI 产出一份**自包含**的决策上报——含背景、要拍板的问题、≥2 个备选方案、关键代码/diff **原文**、证据、本仓库既定约束、AI 推荐（仅供参考）；
2. 维护者把上报整段转交**外部顶级 AI** 讨论并带回结论（外脑承担"逐行读代码"的评审职能）；
3. 结论整理成 ADR 落档（记录备选与理由）后，代码方可合并。

即：人的职责从"读代码"变为"组织外脑评审 + 拍板"；证据链（brief → 外脑结论 → ADR）完整可审计。其余代码**默认信任机器验收**（测试 + lint + 快照 + 差分），人只看 PR 描述与测试证据。

### 3.4 会话卫生

- 长会话定期"落地"：把上下文里的决定写进 ADR / manifest / TODO，新会话从文件恢复，不靠对话记忆。
- AI 产出的代码在合并前必须能回答：这条改动的验收测试是哪条？答不出来 = 不合并。

---

## 4. CI 硬门（合并与发布门槛）

```yaml
# .github/workflows/ci.yml —— 骨架示意
jobs:
  lint:
    steps: [cargo fmt --check, cargo clippy --all-targets -- -D warnings,
            cargo deny check, cargo geiger]        # unsafe 计数趋势留档
  test:
    strategy:
      matrix:
        suite: [unit, snapshot, proptest, contract, differential, wpt-subset,
                vertical-sample, lifecycle]
    # contract/differential/wpt 由 G0 交付的 runner 驱动
  sanitizer:
    matrix: { tool: [asan, tsan, lsan] }            # 多 isolate 并发与 Context 循环
  compat-guard:
    steps:
      - run: 生成 capability-report（manifest 统计）
      - run: 与 main 分支报告 diff
      # 兼容率（verified 占比）下降 → 直接 fail，这是最重要的单条门禁
  budget:
    steps: [二进制体积预算检查, 依赖树 diff 留档]     # 防依赖蠕变
```

两条全局规则：

1. **兼容率单调性**：任何 PR 使 verified 条目数下降即阻断（除非独立 PR 显式降级并说明理由）。
2. **没有测试证据的 PR 不能合并**——CI 是唯一裁判，人的 approve 不能替代红灯。

---

## 5. 防退化度量（每周期看一眼的趋势，不做实时监控）

| 指标 | 工具 | 目标 |
|---|---|---|
| 契约兼容率（verified/total） | capability-report | 单调上升 |
| unsafe 块计数 | cargo geiger | 不上升（白名单 crate 除外，需说明） |
| 核心内核 crate 覆盖率 | llvm-cov | 仅对 dom/scheduler/structured-clone 等核心设阈值，不对全仓 |
| 二进制体积 | 构建产物 | 预算内（防依赖蠕变） |
| 差分差异清单规模 | L4 harness | 按严重度分桶，P0 差异数下降 |

---

## 6. 与既有前置工作的合并关系

本方案不新增独立排期，而是并入已有闸门：

- **G0 增量**：契约 runner 与差分 harness 本来就是 G0 交付物 → 同时按"CI 可执行"标准建设（即测试基建先行，正好对应原技术方案"差分基础设施先于功能"的判断）。
- **工程底座增量**：workspace lints、deny.toml、insta/proptest 引入、CODEOWNERS、ADR 目录、CI 骨架 —— 全部是第 16 节工程底座的组成部分，应在第一个功能 crate 之前就位。
- **五项 PoC 不变**：它们验证的是选型可行性，与质量流程正交；PoC 代码本身也走同样的门禁。

## 7. 一页速查

- 对不对 → 机器判（契约/差分/WPT/快照/属性测试）
- 安不安全 → 语言判（forbid unsafe + 白名单 + sanitizer）
- 该不该这样设计 → 人判（ADR + crate 边界）
- 高风险面 → 人逐行审（unsafe/GC 桥/crypto/scheduler/PyO3 边界）
- AI 的产出 → 永远先测试后实现，golden 只许独立 PR 改
