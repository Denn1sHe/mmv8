# ADR-0008：T2 PoC 的落盘方式与 `poc/` unsafe 白名单

- 状态：**已接受**
- 日期：2026-10-12
- 授权依据：维护者 202-10-12 指示「拿不准的你们 ai 内部开多个子 agent 讨论根据讨论结果来决策就行」
- 关联：`docs/TASKS.md` 的 T2.1 / T2.3、`scripts/check-unsafe-whitelist.sh`

## 背景

T2 的五项 PoC 需要**故意写 unsafe** 才能验证可行性：cppgc 的
`make_garbage_collected` / `Object::wrap` 都是 unsafe FFI，直接把它们写进
`crates/engine-v8` 会违反铁律 3（一事一 PR）与「探针失败也要写清失败点」的可追溯性。

## 决策

### 1. 探针放在 `poc/`，且是**独立 Cargo workspace**

```
poc/
├── README.md
├── t2_0_v8_embed/          # V8 嵌入前置（T2.1/2/5 的共同依赖）
├── t2_1_cppgc_cycle/       # T2.1 跨语言循环回收
└── t2_3_html5ever_pause/   # T2.3 脚本暂停/恢复 + document.write 插入点
```

每个探针的 `Cargo.toml` 顶部声明空 `[workspace]`，使其**脱离主 workspace**。

理由：
- 主 workspace 的 19 个成员名单由 `workspace_policy.rs` 精确锁定，探针进去会破坏它；
- 引入 `v8` 依赖会让每次门禁都要链接 V8（远程 3.6 GB 内存下有 OOM 风险）；
- 探针是**结论的证据**，需要能单独重跑，不该被主门禁的时间/内存预算绑架。

代价：探针的编译不在主门禁覆盖范围内。缓解：每个探针的结论都以
ADR 形式落档，并在 ADR 中给出**实测输出原文**，评审者不必重跑即可核对。

### 2. `poc/` 加入 unsafe 白名单（与 `crates/engine-v8/` 同等约束）

```bash
WHITELIST=(
  "crates/engine-v8/"
  "poc/"
)
```

**两类白名单的约束强度完全相同**：都要求每个 `unsafe` 带 `SAFETY:` 注释、
都禁止 crate 级 `#![allow(unsafe_code)]`、都禁止 mod/impl 级开口。
区别只在用途与是否进入发布产物（`poc/**/target` 已在 `.gitignore`）。

这不是放宽铁律 4，而是把「允许 unsafe 的地方」显式列出来——未列出的目录
依然禁止 unsafe，正是守卫脚本规则 1 的本意。

### 3. 意外收获：守卫脚本抓到了新增的 `poc/`

加白名单**之前**，守卫脚本对 `poc/t2_1_cppgc_cycle/src/main.rs` 报了 4 处违规：

```
VIOLATION[outside-whitelist] poc/t2_1_cppgc_cycle/src/main.rs:47: unsafe impl v8::cppgc::GarbageCollected for DomNode {
VIOLATION[outside-whitelist] poc/t2_1_cppgc_cycle/src/main.rs:72: unsafe {
VIOLATION[outside-whitelist] poc/t2_1_cppgc_cycle/src/main.rs:85: unsafe { v8::Object::wrap::<TAG, DomNode>(...) };
VIOLATION[outside-whitelist] poc/t2_1_cppgc_cycle/src/main.rs:95: let node = unsafe { $ptr.as_ref() };
```

这说明**全仓扫描**（而非只扫 `crates/`）是必要的：新增目录若忘了登记，
会立刻被拦下，而不是静默绕过铁律。

### 4. 附带发现：`SAFETY:` 标记必须在 unsafe 行**前 2 行内**

守卫脚本规则 3 用 `sed -n "$((lineno-2)),$((lineno+2))p"` 取窗口。
若把安全论证写成 5 行、`SAFETY:` 落在**首行**，则它位于 `lineno-3`，
**落在窗口外**，仍会被判违规。

这不是脚本的缺陷，而是与 Rust 社区惯例一致：`// SAFETY:` 应紧邻 unsafe 行。
长篇论证可放在其上，但**必须在紧邻处保留一行 `SAFETY:` 标记**：

```rust
// `GarbageCollected` 是 unsafe trait：V8 在 GC 期间调用 `trace` 且不借用 `&self`。
// 本 impl 的 `trace` 只做一次原子计数自增，不触碰 V8 堆、不访问需要 `&mut` 的
// `listener` 字段，因此不存在别名或生命周期问题；调用次数可用 trace_count 事后核对。
// SAFETY: 上述论证成立 —— trace(&self) 内无可变访问、无跨堆指针，故实现安全。
unsafe impl v8::cppgc::GarbageCollected for DomNode {
```

## 后果

- 探针可独立运行、独立演进，不影响主门禁。
- 铁律 4 的机器执行面从「`crates/`」扩展到「**全仓**」，新增目录漏登记会立即失败。
- 写 `engine-v8` 的 unsafe 代码时必须记住 `SAFETY:` 紧邻约定。