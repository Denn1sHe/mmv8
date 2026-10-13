# 决策上报：workspace 级 `unsafe_code = "forbid"` 与「unsafe 白名单」互相矛盾

> 状态：**等待维护者携带外脑结论回传**（未落 ADR 前不执行最终形态）
> 提出人：AI 代理（lead 角色）｜ 日期：2026-10-12 ｜ 模板：`docs/decision-brief-template.md`
> 关联任务：`docs/TASKS.md` T0.4 / T0.5 / D-01 / D-05

---

## 0. 一句话问题

本项目要求「全仓库禁止使用 unsafe，但允许唯一的 V8 绑定 crate 例外」。可是 Rust 语言里
「全仓禁止」用的那个开关（`forbid`）**从设计上就无法被任何例外打开**，两者不能同时成立。
请拍板：把「全仓禁止」降一档（`deny`，可以用例外），还是让那个唯一的例外 crate 单独
不继承全仓规则？另外请一并确认：**用一段脚本来兜住「例外只能逐块开、不能整 crate 开」是否足够。**

---

## 1. 背景与上下文

- **这个仓库是什么**（面向外行）：我们要用 Rust 写一个「浏览器环境模拟器」，把 Google 的
  JavaScript 引擎 V8 嵌进来，让 Python 能离线跑依赖网页环境的 JS。Rust 里有一类代码叫
  `unsafe`（可理解为「绕过编译器安全检查、需要人自己保证不出内存错误的代码块」）。
  本项目绝大多数代码**不需要** unsafe，只有少数必须跟 V8 的 C++ 接口对话的薄层需要。
- **涉及的模块 / 文件路径**：
  - `/Users/dennis/WorkBuddy/mmv8/Cargo.toml` —— workspace 根，全仓 lint 规则写在这里
  - `/Users/dennis/WorkBuddy/mmv8/crates/engine-v8/Cargo.toml` —— 唯一的白名单 crate
  - `/Users/dennis/WorkBuddy/mmv8/scripts/check-unsafe-whitelist.sh` —— 我写的机器守卫脚本
  - `/Users/dennis/WorkBuddy/mmv8/AGENTS.md` —— 铁律 4、5、6、10
  - `/Users/dennis/WorkBuddy/mmv8/docs/vibe-coding-质量保障方案.md` —— §1
- **当前状态**：T0 工程底座已完成 —— 19 个 crate 骨架、全仓 lint、测试基建（insta/proptest）、
  守卫脚本、门禁文件草案（待批准）。本地与远程 Linux 的门禁全部为绿（证据见 `docs/TASKS.md`）。
  **唯一未定的是本条**：白名单机制到底写成哪种形态。
- **为什么现在必须决策**：下一步（G1 引擎纵向验证）就要写第一个真正调用 V8 的 `unsafe` 块。
  若白名单形态未定，之后所有 `unsafe` 代码的评审口径都是模糊的 —— 这正是铁律 6
  要求「高风险路径改动先获批」的场景。同时我写好的守卫脚本目前**没有任何必跑路径**
  （它只挂在「待批准」的 CI 里），需要维护者决定是否把它写进 `AGENTS.md` 的自检清单（D-05）。

---

## 2. 需要拍板的问题

- **主问题**：全仓 lint 里 `unsafe_code` 该设成哪个级别、白名单 crate 如何开口？
- **子问题**：
  1. 是否接受把全仓级别从 `forbid`（绝对禁止、不可被任何例外覆盖）降到 `deny`
     （默认禁止、但允许在明确写下的地方开例外）？
  2. 如果坚持 `forbid`，是否接受「白名单 crate 不继承全仓规则、自己单独声明一套 lint」？
  3. 是否同意把守卫脚本 `scripts/check-unsafe-whitelist.sh` 写进 `AGENTS.md` 的
     「提交前自检」清单与 CI 门禁，作为这条铁律的**唯一**必跑检查？
  4. 是否需要更强的手段（例如把 `cargo geiger` 做成硬门禁，而不只是趋势留档）？

---

## 3. 备选方案（至少两个）

### 方案 A：全仓 `deny` + 白名单 crate 用 `#[allow(unsafe_code)]` 逐块开口

- **做法**：全仓规则写「默认禁止 unsafe」，但允许在具体代码块上写一句
  「这里允许 unsafe」，并且每处都必须紧跟一句解释「为什么这里是安全的」。
- **关键代码 / diff**（原文）：

  `Cargo.toml`（根）：
  ```toml
  [workspace.lints.rust]
  unsafe_code = "deny"
  missing_docs = "warn"
  ```

  `crates/engine-v8/Cargo.toml`：
  ```toml
  [lints]
  workspace = true      # 继承全仓规则
  ```

  `crates/engine-v8/src/ffi_bridge.rs`：
  ```rust
  // SAFETY: 指针来自 V8 引擎持有的 isolate，调用期间 isolate 不会被释放；
  //         入参句柄由调用方保证在本函数返回前存活。
  #[allow(unsafe_code)]
  fn call_v8(handle: *mut c_void) -> i32 {
      unsafe { v8_call(handle) }
  }
  ```
- **优点**：全仓只有**一份** lint 规则（单一事实源），不会出现「某个 crate 悄悄少检查了几条」；
  白名单开口处一眼可见、可被文本搜索审计。
- **缺点与风险**：`deny` 级别**可以**被任何 crate 用一行 crate 级
  `#![allow(unsafe_code)]` 打开 —— 也就是说，如果没人盯着，其它 crate 也能私自开口。
  必须靠下面第 4 节的守卫脚本 + CI 补上。
- **长期影响**：新增 crate 时不用做额外配置；把「绝对禁止」换成「默认禁止 + 守卫」。
- **预估工作量**：小（已实现，仅需拍板确认）。

### 方案 B：全仓 `forbid` + 白名单 crate 不继承、单独声明一套 lint（**当前实现**）

- **做法**：全仓规则写「绝对禁止 unsafe」（这一档的语义是：**任何**地方都打不开，
  连本 crate 内部的例外语句也无效）。唯一的白名单 crate 干脆不继承全仓规则，
  自己复制一套几乎相同的规则，只把 unsafe 那一项改成「默认禁止」。
- **关键代码 / diff**（原文，即仓库现状）：

  `Cargo.toml`（根）：
  ```toml
  [workspace.lints.rust]
  unsafe_code = "forbid"      # 绝对禁止
  missing_docs = "warn"

  [workspace.lints.clippy]
  all = { level = "deny", priority = -1 }
  pedantic = { level = "warn", priority = -1 }
  ```

  `crates/engine-v8/Cargo.toml`（**注意：没有 `[lints] workspace = true`**）：
  ```toml
  [lints.rust]
  unsafe_code = "deny"        # 只有这一个 crate 被放宽
  missing_docs = "warn"

  [lints.clippy]
  all = { level = "deny", priority = -1 }
  pedantic = { level = "warn", priority = -1 }
  ```
- **优点**：另外 18 个 crate 获得的是**不可打开**的绝对禁止，比方案 A 更硬；
  白名单的「例外」在 manifest 里显式可见。
- **缺点与风险**：白名单 crate 的 lint 列表是**手工复制的副本**，将来全仓规则升级
  （比如加一条新 lint）时容易忘记同步，形成检查盲区。Cargo 官方不允许「继承后再覆盖单项」
  （证据见第 4 节），所以没有更优雅的写法。
  **另外，白名单 crate 内部本身仍然是 `deny`，所以它自己也能被一行 crate 级
  `#![allow(unsafe_code)]` 整 crate 打开（证据见第 4 节场景 (c)）—— 这一点两个方案都存在，
  必须靠守卫脚本的规则 2 兜住。**
- **长期影响**：需要一条约定（已实现为自动化断言，见第 4 节证据 4）保证两个 lint 列表不漂移。
- **预估工作量**：小（已实现）。

### 方案 C（不推荐，仅列出）：放弃白名单，全仓 `forbid`，不写任何 unsafe

- **做法**：连 V8 绑定层也不许写 unsafe 代码。
- **为什么不可行**：V8 是 C++ 引擎，Rust 要调用它**必然**需要 unsafe 或一层 C 语言薄桥；
  完全禁绝等于放弃本项目核心目标。列出仅供参考。

---

## 4. 证据（原文粘贴）

### 证据 1 —— Cargo 官方不允许「继承全仓规则后再覆盖单项」

实测（`/tmp/linttest` 最小复现工程），命令 `cargo build` 的完整输出：

```
error: failed to load manifest for workspace member `/private/tmp/linttest/member`
referenced by workspace at `/private/tmp/linttest/Cargo.toml`

Caused by:
  failed to parse manifest at `/private/tmp/linttest/member/Cargo.toml`

Caused by:
  cannot override `workspace.lints` in `lints`, either remove the overrides or `lints.workspace = true` and manually specify the lints
```

这条排除了「全仓写 `forbid`、白名单 crate 继承后单独把 unsafe 改成 `deny`」这种最理想的写法。

### 证据 2 —— 三组 Rust 编译实验（`rustc 1.92.0`，`--edition 2024`）

**(a) `forbid` + 逐块 `#[allow(unsafe_code)]` → 编译失败（证明 `forbid` 无法被打开）**

输入文件 `a_forbid.rs`：
```rust
#![forbid(unsafe_code)]
#[allow(unsafe_code)]
pub fn f() -> u32 { unsafe { 1 } }
```

命令与完整输出：
```
$ rustc --edition 2024 --crate-type lib a_forbid.rs -o /dev/null
error[E0453]: allow(unsafe_code) incompatible with previous forbid
 --> a_forbid.rs:2:9
  |
1 | #![forbid(unsafe_code)]
  |           ----------- `forbid` level set here
2 | #[allow(unsafe_code)]
  |         ^^^^^^^^^^^ overruled by previous forbid

error: usage of an `unsafe` block
 --> a_forbid.rs:3:21
  |
3 | pub fn f() -> u32 { unsafe { 1 } }
  |                     ^^^^^^^^^^^^
  |
note: the lint level is defined here
 --> a_forbid.rs:1:11
  |
1 | #![forbid(unsafe_code)]
  |           ^^^^^^^^^^^

warning: unnecessary `unsafe` block
 --> a_forbid.rs:3:21
  |
3 | pub fn f() -> u32 { unsafe { 1 } }
  |                     ^^^^^^ unnecessary `unsafe` block
  |
  = note: `#[warn(unused_unsafe)]` (part of `#[warn(unused)]`) on by default

error: aborting due to 2 previous errors; 1 warning emitted
$ echo $?
1
```

**(b) `deny` + 逐块 `#[allow(unsafe_code)]` → 编译通过（证明方案 A 的「逐块开口」可用）**

输入文件 `b_deny_fn_allow.rs`：
```rust
#![deny(unsafe_code)]
#[allow(unsafe_code)]
pub fn f() -> u32 { unsafe { 1 } }
fn main() { println!("{}", f()); }
```

命令与完整输出：
```
$ rustc --edition 2024 b_deny_fn_allow.rs -o /tmp/lintev/b
warning: unnecessary `unsafe` block
 --> b_deny_fn_allow.rs:3:21
  |
3 | pub fn f() -> u32 { unsafe { 1 } }
  |                     ^^^^^^ unnecessary `unsafe` block
  |
  = note: `#[warn(unused_unsafe)]` (part of `#[warn(unused)]`) on by default

warning: 1 warning emitted
$ echo $?
0
```

**(c) `deny` + **crate 级** `#![allow(unsafe_code)]` → 编译通过（证明「整 crate 开口」能一次废掉 deny）**

输入文件 `c_deny_crate_allow.rs`：
```rust
#![deny(unsafe_code)]
#![allow(unsafe_code)]
pub fn f() -> u32 { unsafe { 1 } }
fn main() { println!("{}", f()); }
```

命令与完整输出：
```
$ rustc --edition 2024 c_deny_crate_allow.rs -o /tmp/lintev/c
warning: unnecessary `unsafe` block
 --> c_deny_crate_allow.rs:3:21
  |
3 | pub fn f() -> u32 { unsafe { 1 } }
  |                     ^^^^^^ unnecessary `unsafe` block
  |
  = note: `#[warn(unused_unsafe)]` (part of `#[warn(unused)]`) on by default

warning: 1 warning emitted
$ echo $?
0
```

> 结论：**`deny` 不构成约束**，它随时可以被一行 crate 级 allow 重置。
> 「逐块开口」这个约定无法只靠 lint 级别强制，必须另加机器检查 —— 这就是下面的守卫脚本。

### 证据 3 —— 守卫脚本对 10 个注入场景的实测结果

脚本路径：`/Users/dennis/WorkBuddy/mmv8/scripts/check-unsafe-whitelist.sh`
（完整源码见证据 5，此处只列行为）

| # | 注入场景 | 期望 | 实测输出（原文摘录） | 退出码 |
|---|---|---|---|---|
| 1 | 干净仓库 | PASS | `== PASS：白名单外无 unsafe；无 crate/mod 级开口；白名单内 unsafe 均带 SAFETY 注释 ==` | 0 |
| 2 | 非白名单 crate（web-dom）写 `#[allow(unsafe_code)]` + `unsafe` | FAIL | `VIOLATION[outside-whitelist] crates/web-dom/src/violation_tmp.rs:3: pub fn bad() -> u32 { unsafe { 1 } }` | 1 |
| 3 | 白名单 crate 写 crate 级 `#![allow(unsafe_code)]` | FAIL | `VIOLATION[crate-level-allow] crates/engine-v8/src/violation_tmp.rs:2: #![allow(unsafe_code)]` | 1 |
| 4 | 白名单 crate 写 `#[allow(unsafe_code)]` 修饰 `mod` | FAIL | `VIOLATION[module-level-allow] crates/engine-v8/src/violation_tmp.rs:2: #[allow(unsafe_code)]` | 1 |
| 5 | 白名单 crate 写 `unsafe` 但无 `SAFETY:` 注释 | FAIL | `VIOLATION[missing-safety-comment] crates/engine-v8/src/violation_tmp.rs:2: pub fn bad2() -> u32 { unsafe { 1 } }` | 1 |
| 6 | 白名单 crate 写 `unsafe` 且带 `SAFETY:` 注释 | PASS | `== PASS：…` | 0 |
| 7 | 仓库根出现 `unsafe`（覆盖根级 `build.rs` 情形） | FAIL | `VIOLATION[outside-whitelist] build_tmp.rs:1: pub fn root_bad() -> u32 { unsafe { 1 } }` | 1 |
| 8 | 行尾注释里含 `unsafe`（防误报） | PASS | `== PASS：…` | 0 |
| 9 | 白名单内函数级逐块开口（`// SAFETY:` + `#[allow(unsafe_code)]` + `unsafe`） | PASS | `== PASS：…` | 0 |
| 10 | 清理后复核 | PASS | `== PASS：…` | 0 |

### 证据 4 —— 已有的「lint 副本防漂移」自动化断言

`crates/runner/tests/workspace_policy.rs` 中有三条断言，实测（README 式摘录）：

通过时：
```
test workspace_members_equal_scaffold_roster ... ok
test engine_v8_lint_block_matches_workspace_except_unsafe_code ... ok
test only_whitelisted_crate_opts_out_of_workspace_lints ... ok
test result: ok. 3 passed; 0 failed
```

注入违规时（把 `engine-v8` 的 `missing_docs = "warn"` 改成 `"deny"`）：
```
assertion `left == right` failed: 白名单 crate 的 lint 副本与 workspace lints 发生漂移；全仓 lint 升级时必须同步
test result: FAILED. 2 passed; 1 failed; 0 ignored
```

这解决了方案 B 的主要缺点（手工副本漂移）：一旦两个 lint 表不一致，测试立即红。

### 证据 5 —— 守卫脚本完整源码（供外部 AI 审查正则的正确性）

```bash
#!/usr/bin/env bash
#
# check-unsafe-whitelist.sh —— unsafe 白名单守卫（铁律 4 的机器执行部分）
#
# 检查规则：
#   规则 1  白名单目录之外，任何 .rs **代码行**不得出现 `unsafe` 关键字或 `allow(unsafe_code)`。
#   规则 2  **任何**位置都不得出现 crate 级开口 `#![allow(unsafe_code)]`。
#           （原因：白名单 crate 的 lint 级别是 `deny`，一行 crate 级 allow 会把整个
#             crate 的 `deny` 一次性重置为 allow —— 白名单必须「逐块」开口，不能「整 crate」开口。）
#   规则 3  白名单目录内每个 `unsafe` 代码行，必须在 ±2 行内找到 `SAFETY:` 注释。
#   规则 4  `#[allow(unsafe_code)]`（外层属性）不得直接修饰 `mod` / `impl` 级的项，
#           否则同样构成「整模块开口」，与逐块开口的约定不符。
#
# 扫描范围：**整个仓库**的 .rs 文件（含根级 build.rs、xtask/、benches/、examples/），
# 排除 `target/` 与 `.git/`。
#
# 「代码行」判定：截掉行内 `//` 之后的注释，再去首尾空白；为空则不算代码行。
# 判定前会在**截掉注释后**的文本上重新匹配规则，因此行尾注释里的 "unsafe" 不会误报。
# 已知局限（保守误报，可接受）：字符串字面量里的 `unsafe`（如 `let s = "unsafe";`）
# 仍会命中；出现时改写该字符串即可。真正的 lint 盲区只有「依赖 / proc-macro 生成的
# unsafe」，文本脚本无法覆盖，需由 `cargo geiger` 趋势留档观测。
#
# 退出码：0 = 通过；1 = 发现违规（CI 应据此阻断）；2 = 环境错误。

set -uo pipefail

cd "$(dirname "$0")/.." || exit 2

# 白名单：允许出现 unsafe 的 crate 目录（相对仓库根，必须以 / 结尾）。
# 增删白名单**必须**先经决策上报协议获批（AGENTS.md 铁律 6）。
WHITELIST=(
  "crates/engine-v8/"
)

PAT_UNSAFE='(^|[^[:alnum:]_])unsafe([^[:alnum:]_]|$)'
PAT_ALLOW='allow[[:space:]]*\([[:space:]]*unsafe_code'
PAT_ANY="${PAT_UNSAFE}|${PAT_ALLOW}"
PAT_INNER_ALLOW='#!\[[[:space:]]*allow[[:space:]]*\([[:space:]]*unsafe_code'
PAT_OUTER_ALLOW='#\[[[:space:]]*allow[[:space:]]*\([[:space:]]*unsafe_code'

VIOLATIONS=0

normalize_path() {
  # grep 在扫 "." 时输出 "./crates/..."；统一去掉前导 "./"
  local p="$1"
  case "$p" in
    ./*) p="${p#./}" ;;
  esac
  printf '%s' "$p"
}

is_whitelisted() {
  local path="$1" prefix
  for prefix in "${WHITELIST[@]}"; do
    case "$path" in
      "$prefix"*) return 0 ;;
    esac
  done
  return 1
}

# 取代码部分：截掉行内注释，去首尾空白
code_part() {
  local s="${1%%//*}"
  s="${s#"${s%%[![:space:]]*}"}"
  s="${s%"${s##*[![:space:]]}"}"
  printf '%s' "$s"
}

matches() {
  printf '%s' "$2" | grep -qE "$1"
}

scan() {
  local pattern="$1"
  shift
  if [ "$#" -eq 0 ]; then set -- .; fi
  grep -RInE --include='*.rs' --exclude-dir=target --exclude-dir=.git "$pattern" "$@" 2>/dev/null || true
}

split_hit() {
  FILE="$(normalize_path "${1%%:*}")"
  local rest="${1#*:}"
  LINENO_="${rest%%:*}"
  CONTENT="${rest#*:}"
}

report() {
  echo "VIOLATION[$1] $FILE:$LINENO_: $(code_part "$CONTENT")"
  VIOLATIONS=$((VIOLATIONS + 1))
}

echo "== unsafe 白名单守卫（AGENTS.md 铁律 4） =="

# ---------- 规则 1：白名单外的 unsafe / allow(unsafe_code) ----------
HITS="$(scan "$PAT_ANY")"
if [ -n "$HITS" ]; then
  while IFS= read -r raw; do
    [ -z "$raw" ] && continue
    split_hit "$raw"
    code="$(code_part "$CONTENT")"
    [ -n "$code" ] || continue
    matches "$PAT_ANY" "$code" || continue
    is_whitelisted "$FILE" && continue
    report "outside-whitelist"
  done <<EOF
$HITS
EOF
fi

# ---------- 规则 2：crate 级开口（任何位置都禁止） ----------
HITS="$(scan "$PAT_INNER_ALLOW")"
if [ -n "$HITS" ]; then
  while IFS= read -r raw; do
    [ -z "$raw" ] && continue
    split_hit "$raw"
    code="$(code_part "$CONTENT")"
    matches "$PAT_INNER_ALLOW" "$code" || continue
    report "crate-level-allow"
  done <<EOF
$HITS
EOF
fi

# ---------- 规则 3 / 4：白名单内部 ----------
for prefix in "${WHITELIST[@]}"; do
  [ -d "$prefix" ] || continue

  HITS_INSIDE="$(scan "$PAT_UNSAFE" "$prefix")"
  if [ -n "$HITS_INSIDE" ]; then
    while IFS= read -r raw; do
      [ -z "$raw" ] && continue
      split_hit "$raw"
      code="$(code_part "$CONTENT")"
      matches "$PAT_UNSAFE" "$code" || continue
      lo=$((LINENO_ - 2))
      [ "$lo" -lt 1 ] && lo=1
      window="$(sed -n "${lo},$((LINENO_ + 2))p" "$FILE" 2>/dev/null || true)"
      case "$window" in
        *SAFETY:*) : ;;
        *) report "missing-safety-comment" ;;
      esac
    done <<EOF
$HITS_INSIDE
EOF
  fi

  HITS_ALLOW="$(scan "$PAT_OUTER_ALLOW" "$prefix")"
  if [ -n "$HITS_ALLOW" ]; then
    while IFS= read -r raw; do
      [ -z "$raw" ] && continue
      split_hit "$raw"
      code="$(code_part "$CONTENT")"
      matches "$PAT_OUTER_ALLOW" "$code" || continue
      next="$(code_part "$(sed -n "$((LINENO_ + 1))p" "$FILE" 2>/dev/null || true)")"
      case "$next" in
        mod\ *|pub\ mod\ *|impl\ *|impl\<*|unsafe\ impl\ *)
          report "module-level-allow"
          ;;
      esac
    done <<EOF
$HITS_ALLOW
EOF
  fi
done

if [ "$VIOLATIONS" -gt 0 ]; then
  echo "== FAIL：发现 $VIOLATIONS 处违规（AGENTS.md 铁律 4） =="
  exit 1
fi

echo "== PASS：白名单外无 unsafe；无 crate/mod 级开口；白名单内 unsafe 均带 SAFETY 注释 =="
exit 0
```

### 证据 6 —— 本仓库原始约束的原文（见第 5 节）

---

## 5. 本仓库的既定约束（外部 AI 的建议必须在这些边界内）

`AGENTS.md` 铁律第 4 条原文：

> 4. **unsafe 全仓禁止**（workspace lints 强制）。仅白名单 crate（`engine-v8` 及显式声明的 FFI 薄桥）例外，且每个 unsafe 块必须带 `// SAFETY:` 不变量注释。

`docs/vibe-coding-质量保障方案.md` §1 原文：

> ```toml
> [workspace.lints.rust]
> unsafe_code = "forbid"        # 默认全仓禁止 unsafe
> missing_docs = "warn"
> ```
>
> - **unsafe 白名单制**：仅 `engine-v8`（V8/rusty_v8 绑定）、必要的 FFI 薄桥 crate 允许 `#[allow(unsafe_code)]`，且每个 unsafe 块必须带 `// SAFETY: ...` 注释说明不变量。

`AGENTS.md` 铁律第 6 条（节选）原文：

> 6. **高风险路径改动必须走「决策上报协议」（见下节）获批**，AI 产出仅为初稿：
>    - `crates/engine-v8/**` 中的 unsafe 块与 GC 桥（rusty_v8/cppgc 循环回收）

`AGENTS.md` 铁律第 7 条原文（节选，与 D-05 相关）：

> 7. **AI 永远不得修改**：`.github/workflows/**`、`CODEOWNERS`、安全策略文件、`deny.toml`、golden 文件（另见第 2 条）、本文件自身。

`AGENTS.md` 铁律第 10 条原文：

> 10. **不确定就停**：架构疑问先走「决策上报协议」交维护者，不猜、不静默降级、不用"能跑就行"糊弄。

**约束内的硬边界**：无论选哪种方案，都必须满足 —— ① 白名单外不得出现 unsafe；
② 白名单内每个 unsafe 块必须带 SAFETY 注释；③ unsafe 只能出现在与 V8 的薄桥处。
另注：`AGENTS.md` 受铁律 7 保护，本次**未做任何修改**；若要把守卫脚本写进其自检清单（D-05），
需要维护者批准后由人修改。

---

## 6. 本代理的推荐（仅供参考，等待外脑讨论）

- **推荐方案 B（全仓 `forbid` + 白名单 crate 单独声明）**，即仓库当前实现。理由：
  `forbid` 的不可打开语义最贴近铁律 4 的「全仓禁止」，且它让「非白名单 crate 写出 unsafe」
  在语言层面不可能发生，而不是依赖外部脚本去抓；其唯一缺点（lint 副本漂移）已用自动化断言
  兜住（证据 4）。
- **无论选哪种方案，都建议同时采纳守卫脚本为必跑门禁**（D-05）。因为证据 2(c) 证明
  「方案 B 的白名单 crate 内部同样可被整 crate allow 重置」，脚本规则 2 是当前唯一防线。
- **我不确定的点（希望外部 AI 重点看）**：
  1. `forbid` 与 `deny` 在**实际安全收益**上的差距，是否值得为此接受「lint 列表副本」的
     维护成本？有没有第三种更优雅的写法（Cargo 新特性 / 构建脚本注入 / workspace 继承的
     其他机制）？
  2. 白名单可能不止一个 crate（技术方案提到「必要的 FFI 薄桥 crate」）。若白名单扩到 2–3 个，
     方案 B 的「手工副本」与「不能继承全仓 lint」的成本是否会变得不可接受？
  3. 守卫脚本用 grep + sed 做文本级检查，是否足够撑起这条铁律？是否应当把
     `cargo geiger`（unsafe 计数工具）做成**硬门禁**而非趋势留档？（质量方案 §1 与 §5 目前
     把它定位为趋势指标。）有没有更可靠的现成工具？
  4. 关于脚本规则 3 的「±2 行内找到 `SAFETY:`」判定窗口，是否过宽或过窄？

---

## 7. 请外部 AI 重点回答的问题

1. 在 Rust 现实约束下（继承全仓 lint 后不可覆盖单项；`forbid` 不可被 allow 覆盖；
   `deny` 可被 crate 级 allow 一次性重置），`forbid` + 白名单、`deny` + 逐块 `#[allow]`，
   哪一个更符合「全仓禁止 + 单点例外」的工程意图？请给出明确取舍理由。
2. 是否存在我没想到的第三种写法，能同时得到「不可打开的绝对禁止」与「单一事实源的 lint 列表」？
3. 针对白名单可能扩展（2–3 个 crate）的情形，推荐的组织方式是什么？
4. 文本守卫脚本（证据 5 全文）是否足以承担这条铁律？请指出其中的漏检面与误报面，
   并给出可落地的补强建议（含是否引入 `cargo geiger` 或其他工具、是否应做成硬门禁）。
5. 是否同意把守卫脚本写入 `AGENTS.md` 的「提交前自检」清单（D-05）？若同意，建议放在哪一节、
   用什么措辞（请给出可直接粘贴的条文原文）。
