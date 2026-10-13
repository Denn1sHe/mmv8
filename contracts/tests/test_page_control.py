"""`page_control.py` ↔ Rust 权威定义的逐项对应契约测试（ADR-0017·D11 / 施工图 §2.7 / §3.8 / §4）。

**不手写第二份期望源码**：本测试直接读取 Rust 文件**文本**做断言（方向：Python → 读 Rust 文件），
因此不存在「需要人工同步的副本」这一腐化源。
"""

from __future__ import annotations

import re
from pathlib import Path

from iv8_contract import page_control

# contracts/tests/test_page_control.py → parents[2] = 仓库根
REPO_ROOT = Path(__file__).resolve().parents[2]
RUNLOOP_RS = REPO_ROOT / "crates" / "scheduler" / "src" / "runloop.rs"
CONTROLLER_RS = REPO_ROOT / "crates" / "navigation" / "src" / "controller.rs"
ERROR_RS = REPO_ROOT / "crates" / "engine-v8" / "src" / "error.rs"
PAGE_RS = REPO_ROOT / "crates" / "navigation" / "src" / "page.rs"


def _text(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def _parse_enum_variant_names(body: str) -> list[str]:
    """从 ``enum`` 块内部文本中抽取**顶层**变体名（按花括号深度，排除 struct 变体字段）。"""
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


def _enum_variant_names(text: str, enum_decl: str) -> list[str]:
    located = re.search(re.escape(enum_decl) + r"\s*\{(?P<body>.*?)\n\}", text, re.DOTALL)
    assert located is not None, f"未能定位 {enum_decl} 的变体块"
    return _parse_enum_variant_names(located.group("body"))


def _struct_field_names(text: str, struct_decl: str) -> list[str]:
    located = re.search(re.escape(struct_decl) + r"\s*\{(?P<body>.*?)\n\}", text, re.DOTALL)
    assert located is not None, f"未能定位 {struct_decl} 的字段块"
    return re.findall(r"pub\s+([a-z_][a-z0-9_]*)\s*:", located.group("body"))


def test_run_outcome_names_match_rust() -> None:
    """`RunOutcome` 变体名集合与 `runloop.rs` 逐项相等（5 变体）。"""
    rust = sorted(_enum_variant_names(_text(RUNLOOP_RS), "pub enum RunOutcome"))
    py = sorted(page_control.RUN_OUTCOME_NAMES)
    assert rust == py, f"RunOutcome 变体名两侧不相等：Rust={rust} Python={py}"
    assert len(py) == 5


def test_ready_state_names_match_rust() -> None:
    """`ReadyState` 变体名集合与 `controller.rs` 逐项相等（3 变体）。"""
    rust = sorted(_enum_variant_names(_text(CONTROLLER_RS), "pub enum ReadyState"))
    py = sorted(page_control.READY_STATE_OUTCOME_NAMES)
    assert rust == py, f"ReadyState 变体名两侧不相等：Rust={rust} Python={py}"
    assert py == ["Complete", "Interactive", "Loading"]


def test_js_error_kind_names_match_rust() -> None:
    """`JsErrorKind` 变体名集合与 `error.rs` 逐项相等（**既有 12 变体，含 Terminated**）。"""
    rust = sorted(_enum_variant_names(_text(ERROR_RS), "pub enum JsErrorKind"))
    py = sorted(page_control.JS_ERROR_KIND_NAMES)
    assert rust == py, f"JsErrorKind 变体名两侧不相等：Rust={rust} Python={py}"
    assert len(py) == 12, f"JsErrorKind 变体数应为 12，实际 {len(py)}"
    assert "Terminated" in py, "watchdog 截断须复用既有 Terminated（G2 不新增变体）"


def test_capabilities_keys_match_rust_struct() -> None:
    """`CAPABILITY_KEYS` 与 `Navigation` 侧 `Capabilities` 字段名逐项相等。"""
    rust = sorted(_struct_field_names(_text(PAGE_RS), "pub struct Capabilities"))
    py = sorted(page_control.CAPABILITY_KEYS)
    assert rust == py, f"Capabilities 字段名两侧不相等：Rust={rust} Python={py}"


def test_capabilities_reports_g3_surface() -> None:
    """G3 能力面口径（G3-B16：geometry=computed、shadow_dom=on；原 `page_geometry_is_documented_stub` 更名）。"""
    values = page_control.CAPABILITY_VALUES
    assert set(values) == page_control.CAPABILITY_KEYS
    assert values["parsing"] == "streaming"
    assert values["document_write"] is True
    assert values["navigation"] is True
    assert values["network"] == "offline"
    assert values["input"] == "explicit_target"
    assert values["geometry"] == "computed"
    assert values["shadow_dom"] is True
    assert values["script_interruptible"] is True
    assert values["input_user_activation"] == "sticky_only"
    assert values["single_realm_navigation"] is True
