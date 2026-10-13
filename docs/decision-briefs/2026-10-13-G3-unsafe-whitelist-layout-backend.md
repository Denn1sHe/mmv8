# 决策 brief —— G3 ⛔阻断：`TElement` 必需 `unsafe fn` vs 全仓 `unsafe_code = "forbid"`（2026-10-13）

> **用途**：按 `AGENTS.md`「决策上报协议」填写（**unsafe 白名单的增删**是协议明列的触发场合）。
> **读者没有本仓库任何上下文，也不预设懂 Rust。** 代码/命令/输出**原文粘贴**。
> **状态**：本 brief 是**待落 ADR 的输入**；本代理推荐**仅供参考**。**未闭合前不得进入 S2+**（施工图 §5）。

---

> ## ⚠️ 状态更新（2026-10-13 · R17 裁决后 · 不重写历史）
> - **R17 已裁 = 「E′+R」合成**（裁决书 `2026-10-13-G3-R17-裁决-主理人.md`；**ADR-0024 草案**在编，见
>   `2026-10-13-G3-R17-落档草案包.md`）：白名单形态 = **尾斜杠目录条目** `crates/layout-backend/src/stylo_host/`
>   （**零 bash 逻辑改动**）+ **Rust 门禁 a/b/c/d**。
> - **信封口径统一为「5 声明 + 0 腔体」（P-1b 实测终值）**：`ensure_data`/`clear_data` **无需真 `unsafe` 腔体**
>   （**急切持有** `ElementDataWrapper` + 安全 `borrow_mut()`/`DerefMut` 写 default；commit `407c0aa`）。
>   **下文出现的「0~2 腔体」为裁决前假设，已由 P-1b 收敛为 0。**
> - **本 brief 正文保持原貌**；以本块 + ADR-0024 草案为准。

---

## 0. 一句话问题

> **我们要把「样式引擎（Stylo）」接进本仓，但 Stylo 要求我们实现的一个接口里，有 5 个方法被上游
> 强制标成 `unsafe`（Rust 里表示「需要人手保证额外安全性」的函数）；而本仓有一条硬规矩——
> 全仓 `unsafe` 被 `forbid` 级禁止（只在唯一一个白名单 crate 例外）。于是「实现 Stylo 的接口」
> 与「全仓禁 unsafe」直接冲突。要拍板：为这 5 个方法**把哪个 crate 加入白名单**（或走别的路）。**

## 1. 背景与上下文

- **这个仓库是什么**：`iv8` 是**自研浏览器运行时**（Rust 宿主 + V8 + Python 控制层）。G3 闸门要把
  「页面」从「能解析、能跑脚本」推进到「**能算样式、能算布局、能读几何尺寸**」，做法是**接入
  Stylo（样式引擎）+ Taffy（布局）+ Parley（文本）**这一套（`docs/adr/0011`、`docs/adr/0020`）。
- **涉及模块 / 文件路径**：
  - `crates/layout-backend/`（本轮**首次落地**的唯一实现面；当前 `[lints] workspace = true`）
  - `crates/engine-v8/`（**当前唯一**的 unsafe 白名单 crate）
  - `scripts/check-unsafe-whitelist.sh`（unsafe 白名单守卫脚本；`WHITELIST=("crates/engine-v8/","poc/")`）
  - `Cargo.toml`（`[workspace.lints.rust] unsafe_code = "forbid"`）
- **当前状态**：S1 探针 `poc/t3_g3_probe` 已跑（证据 `.workbuddy/refs/g3-probe-evidence/`）。
  **opaque 侧表机制本身已实测零 unsafe 且反查命中**；但**实现 `TElement` 触发本阻断**。
- **为什么现在必须决策**：`TElement` 是 Stylo 级联的**必备接口**；不实现它就无法跑样式计算 ⇒
  G3 全部后续步骤（S6+）**无法开工**。

## 2. 需要拍板的问题

- **主问题**：为承载 `TElement` 实现的代码，采用哪种 unsafe 白名单方案？
- **子问题**：
  1. 是把**整个 `layout-backend`** 加入白名单（方案 A），还是**新建极薄「stylo FFI 薄桥」crate**（方案 B）？
  2. 若方案 A/B，是否接受「白名单不再只有 `engine-v8` 一个 crate」这一结构性变化（须 ADR-0002 修正注记）？
  3. `layout-backend` 的 `[lints]` 是否从 `workspace = true`（继承 `forbid`）改为**本地 `unsafe_code = "deny"`**
     （与 `engine-v8` 同款，使**逐 `fn` `#[allow(unsafe_code)]`** 成为可能）？

## 3. 备选方案（至少两个）

### 方案 A：**把 `layout-backend` 加入 unsafe 白名单**（推荐）

- **做法**：`crates/layout-backend/Cargo.toml` 的 `[lints] workspace = true` 改为**本地**声明，
  把 `unsafe_code` 由 `forbid` 放宽为 **`deny`**（与 `engine-v8` 完全同款）；守卫脚本 `WHITELIST`
  追加 `crates/layout-backend/`；**ADR-0002 修正注记**记录理由。实现处**逐 `fn`** 加
  `#[allow(unsafe_code)]` + `// SAFETY:`。
- **关键代码**（原文草案）：
  ```toml
  # crates/layout-backend/Cargo.toml
  [lints.rust]
  unsafe_code = "deny"      # 由 workspace 的 "forbid" 放宽（同 engine-v8 形态）
  [lints.clippy]
  # …继承等价集…
  ```
  ```rust
  // 实现 TElement 的 5 个必需 unsafe fn。
  // ⚠️ 见证据 E：其中 ensure_data/clear_data 的**函数体**可能含少量真 `unsafe`
  //    （blitz 参考实现即如此）；其余 3 个（set_handled_snapshot/set_dirty_descendants/
  //    unset_dirty_descendants）函数体为纯置位、无 unsafe 操作。**实际面以 S6 定形为准。**
  #[allow(unsafe_code)]
  unsafe fn set_handled_snapshot(&self) { self.set_snapshot_handled(); }   // 纯置位
  // SAFETY: 上游 `style::dom::TElement` 强制本方法为 `unsafe fn`；函数体仅置位本仓自有标志位，
  //         不执行任何不安全操作，不引入额外不变量。
  #[allow(unsafe_code)]
  unsafe fn ensure_data(&self) -> ElementDataMut<'_> {
      // SAFETY: stylo 遍历期对节点有独占访问（上游契约）；此处经 `ElementDataWrapper::borrow_mut`
      //         （stylo 的安全方法）或本仓 `UnsafeCell` 腔体取可变引用 —— 依 S6 选定的存储形态而定。
      unsafe { self.stylo_data().ensure_init() }
  }
  ```
- **优点**：① **改动最小**（与 `engine-v8` 已有的「整 crate 本地 `deny` + 逐块开口」形态一致）；
  ② 不新增 workspace 成员（不触 `ADR-0009`）；③ unsafe 面**具名、可数**：**≥5 处 `unsafe fn` 签名**
  + **真 `unsafe` 腔体**（`ensure_data`/`clear_data`）——**P-1b 终值：0 腔体**（见顶部状态更新）、逐块 `SAFETY`。
- **缺点与风险**：① **整个 `layout-backend` 变为白名单 crate** ⇒ 守卫脚本不再拦截该 crate 内**其它**
  未来的 `unsafe`（靠代码评审 + `geiger` 兜）；② 白名单从「唯一 `engine-v8`」变为两个；
  ③ **unsafe 面可能大于「5 个签名」**（`ensure_data`/`clear_data` 的腔体，见证据 E）⇒ 方案 A **让整 crate 白名单**
  正是为容纳这类「深度 unsafe」（若走 **方案 E（按文件）**，须把这些腔体也放进白名单文件）。
- **长期影响**：`layout-backend` 体量较大 ⇒ 白名单面偏宽；须靠 `cargo geiger` 趋势留档守住「不蔓延」。
- **预估工作量**：小（改 lint + 脚本白名单 + 5 个 `#[allow]`；+ S6 定形 `ensure_data`/`clear_data` 腔体）。

### 方案 B：**新建极薄「stylo FFI 薄桥」crate**（白名单），`layout-backend` 保持零 unsafe

- **做法**：新增 `crates/stylo-bridge/`（白名单，`unsafe_code = "deny"`），**只**放 `TElement`（及必要
  trait）实现；`layout-backend` 依赖它、自身保持 `forbid`（零 unsafe）。`web-dom` 需向桥 crate 暴露
  置位所需的最小接口。
- **关键代码**（原文草案）：
  ```toml
  # crates/stylo-bridge/Cargo.toml
  [package] name = "stylo-bridge"
  [lints.rust] unsafe_code = "deny"
  [dependencies] style = { version = "=0.20.0", package = "stylo" }; web-dom = { path = "../web-dom" }
  ```
- **优点**：unsafe 白名单面**最小且目的单一**；`layout-backend` 维持零 unsafe。
- **缺点与风险**：① **新增 workspace 成员** ⇒ 与 `ADR-0009`「不新增 workspace 成员」**张力**（须修正注记）；
  ② 额外的 crate 边界 + 置位数据跨 crate 暴露（`web-dom` 接口面扩大）；③ 多一层跳转与维护成本。
- **长期影响**：白名单保持「小 crate」形态，但 crate 数 +1。
- **预估工作量**：中。

### 方案 C（**不成立**）：`deny` + 单个 `#[allow(unsafe_code)]` 修饰 impl

- 守卫脚本**规则 1**（白名单外任何 `.rs` 代码行不得出现 `unsafe`）与**规则 4**（`#[allow(unsafe_code)]`
  不得直接修饰 `mod`/`impl`，否则构成「整模块开口」）**均会拒绝** ⇒ 仍**必须先入白名单**（即退回 A 或 B）。

### 方案 D（**不成立**）：不实现 `TElement`

- 无法运行 Stylo 级联 ⇒ 与 `ADR-0011`（切 Stylo）直接冲突。**排除**。

### 方案 E：**细粒度·按文件白名单**（守卫脚本豁免收窄到**具体文件**）

> **定稿（裁决，2026-10-13）**：本「按文件」草案**被否决原样**（前缀误放行 + `[ -d ]` 跳过规则 3/4）；
> **采纳「E′+R」**——**E′ = 目录级条目**（尾斜杠目录，零脚本改动）+ **R = Rust 门禁 a/b/c/d**。
> **以 ADR-0024 为准**（草案：`2026-10-13-G3-R17-落档草案包.md` §3）。下文为**历史草案，保留对照**。

- **做法**：`[lints]` 改动**与方案 A 完全相同**——`layout-backend` 由 `forbid` 放宽为 `deny`
  （原因：Rust 的 `unsafe_code` lint 是**crate 级**，`forbid` **不可被 `#[allow]` 覆盖** ⇒
  **任何「按文件」方案都无法绕开这一步**）。E 与 A 的**唯一差别**在于：**守卫脚本 `WHITELIST` 只列具体文件**
  （而非整目录），并**修改守卫脚本**使文件级条目获得 SAFETY/开口语义检查。
- **关键技术前提（主理人已核，本 brief 复核确认为真）**：
  - `is_whitelisted()`（脚本 `:82-90`）用 `case "$path" in "$prefix"*)` ⇒ **前缀匹配 ⇒ 文件级条目天然可匹配**
    （规则 1/5 会正确豁免该文件）；
  - **但规则 3/4 的循环以 `[ -d "$prefix" ] || continue`（`:157`）为前提** ⇒ **文件级条目会被 `continue` 跳过**
    —— 即该文件**不经 SAFETY/开口语义检查**。故 E 的定稿**必须包含守卫脚本的逻辑改动原文**。
- **必须包含的守卫脚本改动原文**（`scripts/check-unsafe-whitelist.sh`）：

```diff
 # 白名单：允许出现 unsafe 的目录（相对仓库根，必须以 / 结尾）。
 WHITELIST=(
   "crates/engine-v8/"
   "poc/"
+  # 细粒度：仅该文件允许 unsafe（承载 TElement 的 5 个必需 unsafe fn）
+  "crates/layout-backend/src/stylo_host.rs"
+  # 注：opaque 机制已实测零 unsafe ⇒ 不列 src/opaque.rs（除非后续演化需要）
 )

@@ scan() 函数（:104-109）
-  grep -RInE --include='*.rs' --exclude-dir=target --exclude-dir=.git --exclude-dir=.workbuddy "$pattern" "$@" 2>/dev/null || true
+  # 加 -H：对**单文件**实参，GNU/BSD grep 的 -R 是否输出 `文件:行:` 前缀因实现而异；
+  # 脚本依赖 split_hit() 按 `:` 切分文件名 ⇒ -H 强制前缀，消除歧义。
+  grep -RInEH --include='*.rs' --exclude-dir=target --exclude-dir=.git --exclude-dir=.workbuddy "$pattern" "$@" 2>/dev/null || true

@@ 规则 3 / 4：白名单内部（:155-157）
 for prefix in "${WHITELIST[@]}"; do
-  [ -d "$prefix" ] || continue
+  # 由 -d 放宽为 -e：允许**目录或单文件**条目（原 -d 会把文件级条目跳过 SAFETY/开口检查）。
+  if [ ! -e "$prefix" ]; then
+    echo "== WHITELIST 项不存在（疑似 typo/失效路径）：$prefix ==" >&2
+    VIOLATIONS=$((VIOLATIONS + 1))   # 白名单项失效 ⇒ 报错而非静默 continue
+    continue
+  fi
```

- **优点**：① 守卫脚本的**文本豁免面最小**（只放 `stylo_host.rs`，`layout-backend` 其余文件仍受规则 1/5 约束）；
  ② 与「白名单**逐块/逐项**开口」的宪法精神更契合；③ 若 `TElement` impl 日后迁移，改动点**显式可见**。
- **缺点与风险**：
  ① **须改守卫脚本本身**（`[ -d ]`→`[ -e ]` + `-H`）——这是对「铁律 4 的**机器执行器**」的改动，
  **风险高于方案 A**：须在 **mac（BSD grep）与 Linux（GNU grep）双端**各跑守卫脚本自测（含**正样例**
  ＝白名单文件含带 SAFETY 的 unsafe 应 PASS；**负样例**＝该文件 unsafe 无 SAFETY 应 FAIL）；
  ② **`-e` 语义宽于 `-d`**（对**任何**存在路径为真）——`WHITELIST` 是硬编码常量故实际风险低，
  但须防「白名单项 typo/失效路径被静默放过」⇒ **改法已加：白名单项不存在即报错**（见上 diff）；
  ③ **前缀匹配副作用**：文件条目同时匹配形如 `…/stylo_host.rs.*` 的路径（低危，登记）；
  ④ **运维成本**：若 impl 日后拆成多文件，须逐条补条目（A 无此成本）；
  ⑤ **与 A 相同**：crate 级 `deny` ⇒ 编译器对 `layout-backend` 其余文件「**未 `#[allow]` 的 unsafe**」
  **仍会报错**，故 **E 相对 A 的增量防护仅限**「显式 `#[allow]` 逃逸」与守卫文本检查层面。
- **长期影响**：守卫脚本获得「文件级白名单」能力（可复用于未来其它细粒度豁免），但脚本复杂度上升。
- **预估工作量**：中小（crate lint 同 A；另加守卫脚本改动 + 双端自测）。

> **A vs E 的准确差别（防误读）**：**`[lints]` 改动完全相同**（`forbid→deny`；Rust **无** per-file lint）；
> **唯一差别 = 守卫脚本的豁免粒度**（整目录 vs 具体文件）。E 把「整 crate 文本豁免」收窄为「单文件文本豁免」，
> 代价是**改动守卫脚本本身 + 双端自测**。若评审更重「守卫脚本稳定」⇒ 取 **A**；更重「文本豁免面最小」且接受脚本改动 ⇒ 取 **E**。

## 4. 证据（原文粘贴）

**证据 A —— `TElement` 的 5 个必需 `unsafe fn`（本地 registry 原文，`stylo-0.20.0/dom.rs`）**

```
648:    unsafe fn set_handled_snapshot(&self);
675:    unsafe fn set_dirty_descendants(&self);
680:    unsafe fn unset_dirty_descendants(&self);
739:    unsafe fn ensure_data(&self) -> ElementDataMut<'_>;
744:    unsafe fn clear_data(&self);
```
（均为**无默认实现**的必需方法；`:692/697/704` 的 `unsafe fn` **有默认体**，非必需。）

**证据 B —— 探针 P-1 实测输出（`probe-run-output.txt` 原文）**

```
--- P-1 opaque 侧表（零 unsafe 机制）反查 ---
OPAQUE_ADDRESS_STABLE=true
IMPLICIT_SCOPE_REVERSE_LOOKUP=Some(100)
P1_MECHANISM_ZERO_RESTRICTED_CODE_AND_HITS=true

--- P-1 阻断：TElement 必需 unsafe fn vs forbid(unsafe_code) ---
TELEMENT_REQUIRED_unsafe_FNS=5
=> Rust 要求 impl 亦声明为 unsafe fn；forbid(unsafe_code) 拒绝其声明与实现
=> layout-backend（白名单外 + 继承 unsafe_code=forbid）无法实现 TElement
P1_BLOCKED_BY_TRAIT_SURFACE_RESTRICTED_CODE=true
=== S1 判定汇总 ===
P-8 (锁版整组)=PASS  P-11(prefs无GUI)=PASS  P-1(opaque机制)=PASS  P-1(总：含投影编译)=BLOCK
G3_S1_VERDICT=BLOCK
STOP_REASON=P-1 发现 TElement 必须 unsafe fn（5 个），与全仓 unsafe_code=forbid 冲突 —— 停上报
```

**证据 C —— 语言级最小复现（`forbid_unsafe_fn_repro.rs` 原文；`#![forbid(unsafe_code)]` 下）**

```rust
#![forbid(unsafe_code)]
trait TElementLike { unsafe fn set_dirty_descendants(&self); }
struct NodeRef;
impl TElementLike for NodeRef { unsafe fn set_dirty_descendants(&self) {} }
```
（编译预期失败：`declaration of an unsafe method` + `implementation of an unsafe method`。
即 `unsafe fn` 的**签名形态本身**即触发 `forbid`，与函数体是否安全无关。）

> **标注（2026-10-13 · 已重跑）**：本证据原存档 `forbid_unsafe_fn_repro.txt` **实为 rustup 组件下载失败**
> （`os error 2`，非编译输出）。语言断言「`forbid` 拒绝 `unsafe fn` 声明与实现」经**两评独立实复现为真**；
> **存档已由工程师重跑补正**（见 `.workbuddy/refs/g3-probe-evidence/`）。

**证据 D —— 本仓 unsafe 机制现状（原文）**

- `Cargo.toml:67` `unsafe_code = "forbid"`（`[workspace.lints.rust]`；注释明写「不可被覆盖」）。
- `crates/engine-v8/Cargo.toml:18-19` `[lints.rust] unsafe_code = "deny"`（**唯一**白名单 crate 的形态；
  注释说明「`forbid` 连 crate 内 `#[allow]` 都无效，故改为 `deny` 才能逐块开口」）。
- `scripts/check-unsafe-whitelist.sh:56-58`：
  ```
  WHITELIST=(
    "crates/engine-v8/"
    "poc/"
  )
  ```
- **守卫**规则 1（白名单外禁 `unsafe`）、**规则 3**（白名单内每 `unsafe` 行 ±2 行内须有 `SAFETY:`）、
  **规则 4**（`#[allow(unsafe_code)]` 不得修饰 `mod`/`impl`）。**规则 3/4 的循环以 `[ -d "$prefix" ] || continue`
  （`scripts/check-unsafe-whitelist.sh:157`）为前提** ⇒ **文件级白名单条目会被跳过**（方案 E 的改动依据）。
- `is_whitelisted()`（`:82-90`）为**前缀匹配**（`case "$path" in "$prefix"*`）⇒ 文件级条目天然可匹配。

**证据 E —— `ensure_data`/`clear_data` 的**函数体**可能含**真 `unsafe`（blitz 参考实现原文）**

```rust
// blitz-dom-0.3.0-beta.2/src/stylo.rs:692
unsafe fn ensure_data(&self) -> ElementDataMut<'_> {
    // SAFETY: stylo traversal has exclusive access to nodes
    unsafe { self.stylo_element_data().ensure_init() }
}
// :697
unsafe fn clear_data(&self) {
    // SAFETY: stylo traversal has exclusive access to nodes
    unsafe { self.stylo_element_data().clear() }
}
```
```rust
// blitz-dom-0.3.0-beta.2/src/node/stylo_data.rs —— 真 unsafe 的载体（UnsafeCell）
pub struct StyloData { inner: UnsafeCell<Option<ElementDataWrapper>> }
pub unsafe fn ensure_init(&self) -> ElementDataMut<'_> {
    if !self.has_data() {
        unsafe { *self.inner.get() = Some(ElementDataWrapper::default()) };
        // …unsafe { self.unsafe_stylo_only_mut() }…
    } else { unsafe { self.unsafe_stylo_only_mut() }.unwrap() }
}
```
```rust
// stylo-0.20.0/data.rs:344 —— 注意：**stylo 侧的 `borrow_mut` 是安全方法**（其内部 unsafe 属依赖）
impl ElementDataWrapper {
    pub fn borrow_mut(&self) -> ElementDataMut<'_> { /* v: unsafe { &mut *self.inner.get() } */ }
}
```

（阅读指引：`ensure_data(&self)` 的**共享引用**契约 + 返回 `ElementDataMut<'_>`（生命周期挂 `&self`）
⇒ 无法用 `RefCell`/`RefMut` 安全实现（守卫会随函数返回被 drop）⇒ blitz 用 `UnsafeCell` + 真 `unsafe`。
**可能的零 unsafe 出路**：**急切**持有 `ElementDataWrapper`（非 `Option`）后 `ensure_data` = 安全方法调用
`ElementDataWrapper::borrow_mut()`；但 `clear_data` 的「清空」语义**可能**仍需腔体 ⇒ **实际以 S6 定形为准**。）

## 5. 本仓库的既定约束（外部建议必须在此边界内）

- **`AGENTS.md` 铁律 4** 原文：「**unsafe 全仓禁止**（workspace lints 强制）。仅白名单 crate
  （`engine-v8` 及**显式声明的 FFI 薄桥**）例外，且每个 unsafe 块必须带 `// SAFETY:` 不变量注释。」
  ⇒ **「显式声明的 FFI 薄桥」为方案 B 预留了合法性**。
- **`AGENTS.md` 决策上报协议**：触发场合含「**unsafe 白名单的增删**」⇒ 本决策须落 ADR。
- **`AGENTS.md` 铁律 3**：一事一 PR，diff ≤ 400 行。
- **`ADR-0002`**（unsafe 白名单最终形态）：白名单增删须落 ADR 并逐条记录理由（铁律 7 保护文件改动同款）。
- **`ADR-0009`**（`v8::` 类型边界 / **不新增 workspace 成员**）：方案 B 与「不新增 workspace 成员」**有张力**。
- **技术方案 §16**：V8 类型退出机制（与 stylo 无关，但同属「边界纪律」精神）。

## 6. 本代理的推荐（**仅供参考**）

- **推荐：方案 A（把 `layout-backend` 加入白名单，`forbid→deny` + 逐 `fn` `#[allow]` + `// SAFETY`）。**
- **理由**：① 与 `engine-v8` 的**既有形态完全一致**（整 crate 本地 `deny` + 逐块开口），不引入新模式；
  ② 改动最小、不新增 workspace 成员（不触 `ADR-0009`）；③ unsafe 面**恰好 5 处、具名、可数**、
  逐块 `SAFETY`；④ **多数**函数体不执行 unsafe 操作（纯置位）——但 `ensure_data`/`clear_data` **可能**需真
  `unsafe` 腔体（见 §6.4 / 证据 E），故「实际风险极低」**仅对签名面成立，腔体面待 S6 定形**。
- **我不确定的点**：
  1. 「白名单从**唯一** `engine-v8` 变为两个」是否可接受？若维护者重视「唯一性」，则**方案 B**更契合
     `AGENTS.md` 铁律 4 的「FFI **薄桥**」措辞（但须接受 crate +1）。
  2. 是否要求 `layout-backend` 内**除这 5 处外**的 `unsafe` 由守卫脚本**额外**拦截（现状：整 crate 入白名单后
     不再拦）—— 若需要，即改用**方案 E**（按文件白名单 + 守卫脚本 `[ -d ]→[ -e ]`+`-H` 改动，见 §3·E）。
  3. `cargo-msrv` / `cargo geiger` 是否应在本轮排入，以守住 unsafe 趋势与 MSRV。
  4. **unsafe 实际面**（工程师实测提示，已复核为真）：`ensure_data`/`clear_data` 的**函数体**可能需真 `unsafe`
     （blitz 即如此，证据 E）⇒ 面 **P-1b 终值 = 「5 声明 + 0 腔体」**（"急切 `ElementDataWrapper`
     + 安全 `borrow_mut`"的零 unsafe 出路；`clear_data` 或为瓶颈）。**该面之大小直接影响 A/B/E 的风险排序。**
- **（E 的定位）**：**方案 E 与 A 的 `[lints]` 改动完全相同**（`forbid→deny`；Rust 无 per-file lint），
  仅把**守卫脚本的文本豁免面**由「整 crate」收窄为「单文件」，代价是**改动守卫脚本本身 + 双端自测**。
  若评审更重「守卫脚本稳定/改动最小」⇒ 取 **A**；更重「文本豁免面最小化」且接受脚本改动 ⇒ 取 **E**。

## 7. 请重点回答的问题

1. 为**5 个**「上游强制 `unsafe` 签名」（其中 2 个**可能**需真 `unsafe` 腔体，见证据 E）的方法，
   **方案 A（整 crate 白名单）**、**方案 B（极薄桥 crate）**、还是 **方案 E（按文件白名单，含守卫脚本改动）** 更负责？
2. 是否接受「unsafe 白名单不再唯一」？若接受，如何在 `ADR-0002` 里**约束白名单不再蔓延**？
3. 若选**方案 E**：为「文本豁免面最小」而**改动`铁律 4` 的机器执行器（守卫脚本）**是否值得？
   其 `[ -d ]→[ -e ]`+`-H` 改动本身须**双端（BSD/GNU grep）自测**——该风险是否可接受？
4. 是否应**先跑「S6 定形探针」**（敲定 `ensure_data`/`clear_data` 的实际 unsafe 面、并试零 unsafe 出路）**再定 A/B/E**？

---

> **回传后**：结论整理成 ADR（**ADR-0020 修正注记 + ADR-0002 修正注记**，或新 ADR-0024），落档后方可动代码。
> **未闭合前 `layout-backend` 的 `[lints]`/守卫脚本零改动；S2+ 不得开工。**
