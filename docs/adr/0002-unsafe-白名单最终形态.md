# ADR-0002：unsafe 白名单的最终形态（决策 D-01）

- 状态：**已接受**（维护者已授权 AI 内部决策；方案 B 已实施并通过双向验证）
- 日期：2026-10-12
- 关联：`docs/TASKS.md` 的 D-01、D-05；`docs/decision-briefs/2026-10-12-T0-unsafe-whitelist.md`
- 决策者：AI 内部独立子 agent 讨论 + lead 汇总（依 2026-10-12 授权）

## 问题

铁律 4 要求「unsafe 全仓禁止，仅白名单 crate 例外且每个 unsafe 块带 `// SAFETY:`」。
这两句在 Rust 里**互斥**——必须选一个实现形态。

## 已核实的语言事实（原文证据）

```console
$ rustc --edition 2024 --crate-type lib a_forbid.rs     # #![forbid(unsafe_code)] + #[allow]
error: allow(unsafe_code) incompatible with previous forbid
```
`forbid` **无法**被任何 `#[allow]` 覆盖——这正是「全仓禁止」想要的语义，
但也意味着白名单机制无法实现。

```console
$ cargo build     # workspace deny + 成员 [lints] workspace = true 后覆盖单项
error: lints for `unsafe_code` cannot be overridden
```
Cargo **不允许**「继承 workspace lints 后覆盖单项」。

```console
$ rustc --edition 2024 c_deny_crate_allow.rs             # #![deny] + #![allow(unsafe_code)]
（编译通过）
```
因此白名单 crate 若用 `deny`，**一行 crate 级 `#![allow(unsafe_code)]` 就能把整个 crate
的禁令一次性废掉**。这是必须机器守卫的原因，不是理论担忧。

## 方案比较

| 方案 | 做法 | 优点 | 缺点 |
|---|---|---|---|
| **A（选定）** 全仓 `forbid` + `engine-v8` 不继承、单独声明 `deny` + 机器守卫脚本 | 见下 | 边界最小；白名单只有一个 crate；改动面小 | 依赖守卫脚本兜住「整 crate 开口」 |
| B 拆出独立 `v8-ffi` crate 独占 unsafe，`engine-v8` 自身零 unsafe | 新增 crate，`engine-v8` 只依赖它 | 边界语义更强：连引擎逻辑层都碰不到 unsafe | 违反 §16 的 19 成员边界（须改 ADR）；FFI 层与句柄层本就在同一 crate 内，拆分收益有限而依赖边增加 |
| C 放弃 lint，改用 `cargo geiger` 统计阻断 | 只靠 geiger | 无需处理 lint 继承 | geiger **无法**区分「有 SAFETY 注释的合法 unsafe」与「裸 unsafe」，信号不足 |

## 选定方案 A 的具体形态

```toml
# workspace 根：unsafe_code = "forbid"，成员经 [lints] workspace = true 继承
[workspace.lints.rust]
unsafe_code = "forbid"

# crates/engine-v8/Cargo.toml：不继承，单独声明等价集合并放宽 unsafe_code
[lints.rust]
unsafe_code = "deny"
missing_docs = "warn"
```

**白名单必须被机器执行**，由 `scripts/check-unsafe-whitelist.sh` 四条规则保证：

1. 白名单外任何 `.rs` **代码行**不得出现 `unsafe` 或 `allow(unsafe_code)`；
2. **任何位置**不得出现 crate 级 `#![allow(unsafe_code)]`（否则整 crate 失效）；
3. 白名单内每个 `unsafe` 必须在**前 2 行内**带 `SAFETY:` 注释；
4. `#[allow(unsafe_code)]` 不得修饰 `mod` / `impl` 级项（整模块开口）。

补充约束（ADR-0008 细化）：`SAFETY:` 标记须**紧邻** unsafe 行——长篇论证可在上方，
但必须保留一行标记，否则落在 ±2 行窗口外被判违规。

## 机器可验证性

| 保障 | 实现 |
|---|---|
| 白名单只有一个 crate | `crates/runner/tests/workspace_policy.rs::only_whitelisted_crate_opts_out_of_workspace_lints` |
| `engine-v8` 的 lint 副本不漂移 | `::engine_v8_lint_block_matches_workspace_except_unsafe_code`（全仓 lint 升级时必须同步） |
| unsafe 规则不被绕过 | `scripts/check-unsafe-whitelist.sh`（10 个注入场景自测 + CI lint job + AGENTS.md 自检清单，三条必跑路径） |

三条都做过**反向注入验证**（故意注入违规，确认确实变红）。

## 后果与遗留

- 写 `engine-v8` 的 unsafe 代码必须遵守「`SAFETY:` 紧邻 + 逐块开口」，
  整 crate / 整 mod 开口会被守卫拦下。
- `unsafe_code = "deny"` 在 `engine-v8` 内**可被逐块 `#[allow(unsafe_code)]` 开口**，
  这是白名单机制的必要条件，也是它与 `forbid` 的唯一差别。
- 遗留：脚本只能匹配**文本**，无法覆盖「依赖 / proc-macro 生成的 unsafe」；
  该盲区由 `cargo geiger` 趋势留档观测（CI 中为 warnings 级，不阻断）。