# G3 S1 探针 —— 直连 Stylo + Taffy + Parley 路线前置验证

**一句话**：验证 ADR-0020/0021 定下的**直连路线**（`layout-backend` 内建直接 Stylo + Taffy + Parley
适配器，不依赖 `blitz-dom`）在 S1 阶段的四项前置探针 **P-1 / P-2 / P-8 / P-11**。

## 结论速览（本批）

| 探针 | 判定 | 依据 |
|---|---|---|
| **P-8**（精确锁版整组） | **PASS** | ADR-0021 决策 1 的 9 个 `=` 锁版整组编译通过 + 版本逐条一致 + 无 `blitz-dom` |
| **P-11**（prefs 无 GUI） | **PASS** | stylo prefs 为**编译期静态**；`GLOBAL_STYLE_DATA`/`STYLE_THREAD_POOL` 为 `LazyLock`；无 GUI 依赖 |
| **P-1-机制**（opaque 侧表） | **PASS** | `#![forbid(unsafe_code)]` 下编译通过 + `OpaqueElement` 反查命中（零 unsafe） |
| **P-1-总**（投影编译+级联+零 unsafe） | **BLOCK ⛔** | `style::dom::TElement` 的 **5 个必需 `unsafe fn`** 与全仓 `unsafe_code = "forbid"` **冲突** |
| **P-2**（stylo→taffy 桥） | PENDING | 依赖 P-1 投影（`TElement`）成立；P-1 阻断 ⇒ 未执行 |

**综合判定 = `BLOCK` ⇒ 停上报**（ADR-0020 决策 2 / 施工图 §7 P-1 铁律）。

## ⛔ P-1 阻断项（首要发现）

`stylo-0.20.0/dom.rs` 的 `TElement` trait 声明了 **5 个必需方法且无默认实现**，全部是 `unsafe fn`：

| 方法 | 位置（`stylo-0.20.0/dom.rs`） |
|---|---|
| `set_handled_snapshot` | `:648` |
| `set_dirty_descendants` | `:675` |
| `unset_dirty_descendants` | `:680` |
| `ensure_data` | `:739` |
| `clear_data` | `:744` |

Rust 要求**实现**这些方法时也声明为 `unsafe fn`。而 `#![forbid(unsafe_code)]`（以及本仓
`Cargo.toml` 的 workspace lint `unsafe_code = "forbid"`）**拒绝 `unsafe fn` 的声明与实现**
（最小语言级复现见 `.workbuddy/refs/g3-probe-evidence/forbid_unsafe_fn_repro.rs` 与其 stderr
`…repro.txt`）。

⇒ **`layout-backend`（unsafe 白名单之外 + 通过 `[lints] workspace = true` 继承 `forbid`）
无法实现 `TElement`**，与施工图 §1「G3 新增代码全在 `engine-v8` 白名单之外 ⇒ 零 unsafe 为准」
**直接冲突**。

- **参考实现佐证**：`blitz-dom-0.3.0-beta.2/src/stylo.rs:671/675/680/692/697` 亦逐条把上述方法
  实现为 `unsafe fn`（blitz-dom 自身**未** `forbid(unsafe_code)`）。
- **这不否定 opaque 机制**：opaque 侧表本身**确为零 unsafe**（本探针 `src/opaque.rs` 在
  `#![forbid(unsafe_code)]` 下编译并通过反查单测）；阻断点是 **trait 表面**，不是机制。

处置方向（**须维护者/架构师裁决**，本探针不擅自换路）：

- **①** 把 `layout-backend`（或承载 `stylo_host` 的 crate）纳入 **unsafe 白名单**（AGENTS 铁律 6
  ⇒ 走决策上报；且守卫脚本规则 4 禁 `impl` 级 `#[allow]`，须逐块开口 + `// SAFETY:`）；
- **②** 把 crate/workspace lint 由 `forbid` 放宽为 `deny(unsafe_code)` + 对**单个** `impl TElement`
  块做**具名** `#[allow(unsafe_code)]` + `// SAFETY` 说明（`deny` 可被 `allow` 覆盖，`forbid` 不能）；
- **③** 将 `TElement` 实现移入已白名单的 `engine-v8`（不推荐：违反类型面纪律的初衷）。

> 详见回传主理人的报告；**P-1 未解前不得开工 S2+**（施工图 §5）。

## 运行

```bash
cd poc/t3_g3_probe
cargo run --offline          # 跑全部 S1 探针，输出二值结论 + G3_S1_VERDICT
cargo test  --offline        # opaque 侧表反查单测 + prefs 无 GUI 单测 + 锁版核对单测
```

末端单行汇总：`G3_S1_VERDICT=PASS|FAIL|BLOCK|PENDING`。

## 依赖纪律

精确锁版整组（ADR-0021 决策 1，`=` 语义）：`stylo=0.20.0` / `stylo_dom=0.20.0` /
`stylo_traits=0.20.0` / `stylo_taffy=0.3.0-beta.2` / `taffy=0.14.0` / `parley=0.11.1` /
`selectors=0.40.0` / `skrifa=0.44.0` / `fontique=0.11.1`。**不依赖 `blitz-dom`**（直连路线）。

> 构建期要求：`stylo/build.rs` 需 `python3` 跑属性生成器（非运行期 GUI 要求）。

## unsafe

`src/lib.rs` 与 `src/main.rs` 均为 `#![forbid(unsafe_code)]` —— 它们**编译通过**即证明
opaque 机制 / prefs / 依赖整组**零 unsafe**。阻断项的复现文件与 stderr 存于
`.workbuddy/refs/g3-probe-evidence/`（该目录 intentionally 触发 `forbid` 以留证）。
