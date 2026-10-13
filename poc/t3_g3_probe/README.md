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
| **P-1b**（`TElement` unsafe 定形） | **零 unsafe 体** | 急切 `ElementDataWrapper`（非 Option、无 `UnsafeCell`）⇒ 5 个 `unsafe fn` **函数体零 unsafe**：`signatures:5 bodies:0` |
| **P-2**（stylo→taffy 桥） | **PASS** | 真实 `ComputedValues` 经 `to_taffy_style` + taffy 布局：`width` auto/120/200 ⇒ **300/120/200**（互异，非恒真） |
| **P-3**（字体发现路径） | **PASS（本地）** | `fontique` 系统发现 667 家族 + 内嵌 Tuffy 注册 + 回退非 0 且不 panic（远程复验归 S14） |
| **P-10**（内嵌字体度量确定） | **PASS（本地）** | 内嵌 Tuffy `"Hamburgefonstiv"`@16px ⇒ **112.625 px**（同架构逐位相等；跨架构 ≤0.5px） |
| **P-4**（真实模型 shadow 作用域） | **PASS** | `p4.rs`+`p4_match.rs`：light 隔离 + `:host`/`::slotted` 命中（`selectors::matching`）+ opaque 反查，**零 unsafe** |

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

## P-1b（`TElement` `unsafe` 面定形）

`examples/p1b_eager_data.rs`：以**急切持有** `style::data::ElementDataWrapper`（非 `Option`、我方无
`UnsafeCell`）为载荷，`TElement` 的 5 个必需 `unsafe fn` 的**函数体可全部零 `unsafe`**
（stylo `ElementDataWrapper::borrow`/`borrow_mut` 皆**安全方法**；`ElementData: Default`）。
⇒ **unsafe 面 = 5 个签名 + 0 个 `unsafe {}` 块 + 0 腔体**。产物用 `#![deny(unsafe_code)]` +
**逐方法** `#[allow(unsafe_code)]` 精确复现 R17 备选 ② 的最小许可面。

```bash
cargo run --example p1b_eager_data --offline   # P1B_UNSAFE_SURFACE=signatures:5 bodies:0
```

## P-4（真实 web-dom 模型 shadow 作用域，S3 探针 B）

`src/p4.rs`（真实 `web_dom::NodeTree`）+ `src/p4_match.rs`（`selectors::Element` + `matches_selector`）。
判定（三者同时成立）：① light 遍历不含影子内容；② `:host`/`::slotted` 经 shadow 作用域命中；③ opaque 机制 + 零 unsafe。
模型：`<x-host>` + `#shadow-root(open){<div>,<slot>}` + light `<span>`。
证据：`.workbuddy/refs/g3-probe-evidence/p4-shadow-scope-evidence.md`；`cargo test` 18/18。

## P-3 / P-10（字体，施工图 §1.4 / §7）

`src/fonts.rs`：字体栈 `parley`（整形）← `fontique`（发现/回退）← `skrifa`（字形）。

- **P-3**：`Collection::new({system_fonts:true}).family_names()` 本地发现 **667** 家族；内嵌字体经
  `register_fonts(Blob::new(Arc::new(bytes)), None)` 注册后 `family_id("Tuffy").is_some()`；缺失家族
  回退**非 0 且不 panic**。判定 = 本地度量钉死值；远程非 0 且不 panic（**远程复验归 S14**）。
- **P-10**：内嵌字体 `Tuffy` 整形 `"Hamburgefonstiv"`@16px ⇒ **112.625 px**（本机 aarch64）。
  判定 = 同架构**逐位相等**（测试 `assert_eq!`）；跨架构 **≤0.5px**（`CROSS_ARCH_TOLERANCE_PX`）+ 记录
  `arch`/`font_hash`。§4 测试计划回填见 `.workbuddy/refs/g3-probe-evidence/p3-p10-fonts-evidence.md`。
- **内嵌字体授权**：`fonts/Tuffy.ttf` 为 **public domain**（作者 Thatcher Ulrich 等宣告，原文见
  `fonts/LICENSE.txt`）；`sha256=b97cd500…fdce98`。选内嵌（而非系统字体）是为**跨机确定性**（R4/G3-B10）。

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
