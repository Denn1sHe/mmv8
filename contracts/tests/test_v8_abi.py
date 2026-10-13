"""`v8_abi.py` ↔ `abi.rs` 的逐项对应契约测试（ADR-0009 决策 2 / 决策 7 / 方案 E）。

**不手写第二份期望源码**：本测试直接读取 Rust 权威定义
(``crates/engine-v8/src/abi.rs``) 的**文本**做断言（方向：Python → 读 Rust 文件），
因此不存在「需要人工同步的副本」这一腐化源。
"""

from __future__ import annotations

import os
import re
from pathlib import Path

import pytest

from iv8_contract import v8_abi

# contracts/tests/test_v8_abi.py → parents[2] = 仓库根
REPO_ROOT = Path(__file__).resolve().parents[2]
ABI_RS = REPO_ROOT / "crates" / "engine-v8" / "src" / "abi.rs"

# v8 路径引用：首段以非标识符字符（或行首）起头，避免误伤 rusty_v8:: 等含 v8 子串的标识符。
_V8_PATH_RE = re.compile(r"(^|[^\w])v8::")


def _abi_text() -> str:
    return ABI_RS.read_text(encoding="utf-8")


def _count(text: str, needle: str) -> int:
    return len(re.findall(re.escape(needle), text))


def _parse_enum_variant_names(body: str) -> list[str]:
    """从 ``enum`` 块内部文本中抽取**顶层**变体名（按花括号深度，排除 struct 变体的字段）。"""
    names: list[str] = []
    depth = 0
    for raw in body.splitlines():
        line = raw.split("//", 1)[0].strip()
        if line and depth == 0:
            match = re.match(r"([A-Za-z_][A-Za-z0-9_]*)", line)
            if match:
                names.append(match.group(1))
        depth += line.count("{") - line.count("}")
    return names


def _iter_rs_files(root: Path):
    # 跳过 target/.git，以及 gitignored 的本地 scratch（.workbuddy/ 是不入版本库的 agent 记忆）。
    for dirpath, dirnames, filenames in os.walk(root):
        dirnames[:] = [d for d in dirnames if d not in {"target", ".git", ".workbuddy"}]
        for filename in filenames:
            if filename.endswith(".rs"):
                yield Path(dirpath) / filename


def test_three_types_defined_exactly_once():
    """三个类型名在 Rust 侧各恰好定义一次（定义点漂移会立刻变 0 或 2）。"""
    text = _abi_text()
    for needle in ("pub struct RealmId", "pub struct JsHandle", "pub enum HostValue"):
        hit = _count(text, needle)
        assert hit == 1, f"`{needle}` 在 abi.rs 命中 {hit} 次（期望 1）"


def test_hostvalue_variant_names_match_python_enum():
    """`HostValue` 变体名集合必须与 `v8_abi.py` 的枚举成员集合逐项相等（8 变体）。"""
    text = _abi_text()
    located = re.search(r"pub enum HostValue\s*\{(?P<body>.*?)\n\}", text, re.DOTALL)
    assert located is not None, "未能在 abi.rs 中定位 `pub enum HostValue` 的变体块"
    rust_names = sorted(_parse_enum_variant_names(located.group("body")))
    python_names = sorted(v8_abi.HOST_VALUE_VARIANT_NAMES)
    assert rust_names == python_names, (
        f"HostValue 变体名集合两侧不相等：Rust={rust_names} Python={python_names}"
    )
    assert len(python_names) == 8, f"HostValue 变体数应为 8，实际 {len(python_names)}"


def test_python_projection_imports_are_v8_free():
    """`v8_abi.py` 的 import 列表不得引用 `v8`（类型面不得跨语言泄漏 V8 依赖）。"""
    source = Path(v8_abi.__file__).read_text(encoding="utf-8")
    for line in source.splitlines():
        stripped = line.strip()
        if stripped.startswith(("import ", "from ")):
            # 去掉模块自身名 v8_abi，避免把 "import v8_abi" 式的自引用误判为引用 v8。
            assert not re.search(r"\bv8\b", stripped.replace("v8_abi", "")), (
                f"v8_abi.py 的 import 不得引用 v8：{stripped}"
            )


def test_abi_rs_has_no_v8_path_reference():
    """ADR-0009 决策 4 判据 1：类型面 `abi.rs` 中不得出现任何 V8 类型路径引用。"""
    offenders: list[str] = []
    for lineno, line in enumerate(_abi_text().splitlines(), 1):
        code = line.split("//", 1)[0]  # 截掉行内注释
        if _V8_PATH_RE.search(code):
            offenders.append(f"abi.rs:{lineno}: {line.strip()}")
    assert not offenders, "abi.rs 出现 V8 类型路径引用：\n" + "\n".join(offenders)


def test_v8_paths_absent_outside_whitelist():
    """全仓 `.rs` 中 v8 路径引用仅允许出现在 `crates/engine-v8/` 与 `poc/`。"""
    offenders: list[str] = []
    for rs in _iter_rs_files(REPO_ROOT):
        rel = rs.relative_to(REPO_ROOT).as_posix()
        if rel.startswith("crates/engine-v8/") or rel.startswith("poc/"):
            continue
        for lineno, line in enumerate(
            rs.read_text(encoding="utf-8", errors="ignore").splitlines(), 1
        ):
            code = line.split("//", 1)[0]
            if _V8_PATH_RE.search(code):
                offenders.append(f"{rel}:{lineno}: {line.strip()}")
    assert not offenders, "v8 路径引用出现在非白名单位置：\n" + "\n".join(offenders)


def test_u64_width_is_fixed_and_roundtrips():
    """u64 宽度固定（64）且打包/解包往返一致。"""
    assert v8_abi.U64_WIDTH == 64
    assert v8_abi.LOW_HALF_MASK == 0xFFFF_FFFF
    cases = [(0, 0), (0xFFFF_FFFF, 0xFFFF_FFFF), (7, 3), (0, 0xFFFF_FFFF)]
    for index, generation in cases:
        packed = v8_abi.pack_handle(index, generation)
        assert 0 <= packed < (1 << 64)
        assert v8_abi.unpack_handle(packed) == (index, generation)
    for isolate_seq, realm_seq in cases:
        packed = v8_abi.pack_realm(isolate_seq, realm_seq)
        assert 0 <= packed < (1 << 64)
        assert v8_abi.unpack_realm(packed) == (isolate_seq, realm_seq)


def test_out_of_range_is_deterministic():
    """越界 / 类型错误必须是可判定的显式异常，而非静默截断。"""
    with pytest.raises(ValueError):
        v8_abi.pack_handle(1 << 32, 0)
    with pytest.raises(ValueError):
        v8_abi.pack_handle(0, 1 << 32)
    with pytest.raises(ValueError):
        v8_abi.pack_realm(1 << 32, 0)
    with pytest.raises(ValueError):
        v8_abi.unpack_handle(1 << 64)
    with pytest.raises(TypeError):
        v8_abi.pack_handle(True, 0)  # bool 不算 int
