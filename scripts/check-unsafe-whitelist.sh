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
#   规则 5  白名单目录之外的 .rs **代码行**不得出现 `v8::` 路径引用或 `extern crate v8`
#           （铁律 5：V8 / rusty_v8 类型不得越出 `engine-v8` crate）。
#           匹配 `use v8::` / `v8::Local` / `::v8::` / `extern crate v8` 这类路径引用；
#           与规则 1 同源（截注释后重新匹配），标签为 `VIOLATION[v8-type-escape]`。
#           依据：docs/adr/0009-bindings-webidl与engine-v8的V8类型边界.md 决策 3 / A8。
#
# 扫描范围：**整个仓库**的 .rs 文件（含根级 build.rs、xtask/、benches/、examples/），
# 排除 `target/`、`.git/`，以及 `.workbuddy/` —— gitignored 的 agent 工作数据，
# 不构成产品源码（`.gitignore` 明写「本地工作区的 agent 记忆，不入版本库」）。
#
# 「代码行」判定：截掉行内 `//` 之后的注释，再去首尾空白；为空则不算代码行。
# 判定前会在**截掉注释后**的文本上重新匹配规则，因此行尾注释里的 "unsafe" 不会误报。
# 已知局限（保守误报，可接受）：字符串字面量里的 `unsafe`（如 `let s = "unsafe";`）
# 仍会命中；出现时改写该字符串即可。真正的 lint 盲区只有「依赖 / proc-macro 生成的
# unsafe」，文本脚本无法覆盖，需由 `cargo geiger` 趋势留档观测。
#
# 规则 5 的已知局限（对齐 ADR-0009 决策 3「已知局限」，不宣称完备）：
#   - 只匹配 `v8::` **路径首段**。经类型别名间接引用（如 `use crate::v8_bridge::Val;` 而
#     `Val = v8::Local<...>`）检不出，需由代码评审兜住；
#   - `extern crate v8 as something` 后再引用（以别名 `something::`）检不出本规则仍拦
#     `extern crate v8` 本身，但后续 `something::` 的引用形态不可见；
#   - 依赖 / proc-macro 引入的 v8 类型不在扫描范围（与 ADR-0002 遗留同款盲区）；
#   - `poc/**` 是有意例外（探针允许直接用 `v8::`，见 ADR-0008）。
#
# 退出码：0 = 通过；1 = 发现违规（CI 应据此阻断）；2 = 环境错误。

set -uo pipefail

cd "$(dirname "$0")/.." || exit 2

# 白名单：允许出现 unsafe 的目录（相对仓库根，必须以 / 结尾）。
#
# 两类白名单的**约束强度相同**（同样要求每个 unsafe 带 `SAFETY:` 注释、
# 禁止 crate 级与 mod/impl 级开口），区别只在用途与风险：
#
#   crates/engine-v8/  生产代码。V8/rusty_v8 的 FFI 薄桥只能落在这里（铁律 5）。
#   poc/              探针代码。技术方案 T2 的五项 PoC 需要**故意**演示 unsafe FFI
#                     才能验证可行性（如 cppgc 的 make_garbage_collected / Object::wrap）。
#                     这些代码不进入发布产物（`poc/**/target` 已在 .gitignore），
#                     但**同样受 SAFETY 注释约束**，不得随手开口。
#
# 增删白名单**必须**先经决策上报协议获批（AGENTS.md 铁律 6）。
# `poc/` 白名单的增删见 docs/adr/0008-poc-unsafe-白名单.md。
WHITELIST=(
  "crates/engine-v8/"
  "poc/"
  # stylo FFI 薄桥（显式声明）：承载 `style::dom::TElement` 的 5 个必需 `unsafe fn`。
  # 依 ADR-0024（E′ 目录级白名单）+ ADR-0002 修正注记；维护者 2026-10-13 18:10 批准。
  # 尾斜杠目录条目 ⇒ `[ -d ]` 为真 ⇒ 规则 3/4 对目录内文件照常生效；非桥文件仍受规则 1 文本拦。
  "crates/layout-backend/src/stylo_host/"
)

PAT_UNSAFE='(^|[^[:alnum:]_])unsafe([^[:alnum:]_]|$)'
PAT_ALLOW='allow[[:space:]]*\([[:space:]]*unsafe_code'
PAT_ANY="${PAT_UNSAFE}|${PAT_ALLOW}"
PAT_INNER_ALLOW='#!\[[[:space:]]*allow[[:space:]]*\([[:space:]]*unsafe_code'
PAT_OUTER_ALLOW='#\[[[:space:]]*allow[[:space:]]*\([[:space:]]*unsafe_code'

# 规则 5：v8 路径引用。首段以非标识符字符（或行首）起头，避免误伤 `rusty_v8::` /
# `my_v8::` 等含 `v8` 子串的合法标识符；`extern crate v8` 单独成支并校验尾部边界。
PAT_V8='(^|[^[:alnum:]_])(v8::|extern[[:space:]]+crate[[:space:]]+v8([^[:alnum:]_]|$))'

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
  grep -RInE --include='*.rs' --exclude-dir=target --exclude-dir=.git --exclude-dir=.workbuddy "$pattern" "$@" 2>/dev/null || true
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

# ---------- 规则 5：V8 类型越界（铁律 5） ----------
# 与规则 1 同构：全仓扫描 → 截注释后重新匹配 → 排除白名单目录 → 报违规。
HITS="$(scan "$PAT_V8")"
if [ -n "$HITS" ]; then
  while IFS= read -r raw; do
    [ -z "$raw" ] && continue
    split_hit "$raw"
    code="$(code_part "$CONTENT")"
    [ -n "$code" ] || continue
    matches "$PAT_V8" "$code" || continue
    is_whitelisted "$FILE" && continue
    report "v8-type-escape"
  done <<EOF
$HITS
EOF
fi

if [ "$VIOLATIONS" -gt 0 ]; then
  echo "== FAIL：发现 $VIOLATIONS 处违规（AGENTS.md 铁律 4 / 5） =="
  exit 1
fi

echo "== PASS：白名单外无 unsafe；无 crate/mod 级开口；白名单内 unsafe 均带 SAFETY 注释；白名单外无 v8 路径引用 =="
exit 0
