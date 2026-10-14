"""`G4` 笔 2（F2）：**多源合并**的判别性测试。

重点：**冲突判据必须是不可归并键**（`source_version`），不是 slug（`source`）——
slug 可归并（webref 与 khronos 曾共用 `spec-webidl`）⇒ 用 slug 判会**恒假** ⇒ 静默去重。
"""

from __future__ import annotations

import importlib.util
import json
import sys
from pathlib import Path

import pytest

_TOOL = Path(__file__).resolve().parent.parent / "tools" / "merge_members.py"


def _load_tool():
    """按路径加载工具模块（`contracts/tools/` 非包，故用 spec 装载；先注册再执行）。"""
    spec = importlib.util.spec_from_file_location("merge_members", _TOOL)
    assert spec and spec.loader
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


merge = _load_tool()


def _payload(
    members: list[str],
    *,
    source: str,
    version: str,
    sha: str = "deadbeef",
    method: str = "uv run python tools/x.py",
) -> dict[str, object]:
    return {
        "members": members,
        "source": source,
        "source_version": version,
        "source_sha256": sha,
        "method": method,
        "extracted_at": "2026-10-14",
        "needs_community_review": True,
    }


def _write(tmp_path: Path, name: str, payload: dict[str, object]) -> Path:
    path = tmp_path / name
    path.write_text(json.dumps(payload, ensure_ascii=False), encoding="utf-8")
    return path


def test_union_and_per_member_provenance(tmp_path: Path) -> None:
    """并集 + 逐成员 provenance/slug/version 都在。"""
    a = _write(tmp_path, "a.json", _payload(["X.a", "X.b"], source="s1", version="v1"))
    b = _write(tmp_path, "b.json", _payload(["Y.c"], source="s2", version="v2"))
    out = tmp_path / "merged.json"
    assert merge.main([str(a), str(b), "--out", str(out)]) == 0
    merged = json.loads(out.read_text(encoding="utf-8"))
    assert merged["members"] == ["X.a", "X.b", "Y.c"]
    assert merged["member_sources"] == {"X.a": "s1", "X.b": "s1", "Y.c": "s2"}
    assert merged["member_source_versions"] == {"X.a": "v1", "X.b": "v1", "Y.c": "v2"}
    assert [s["source"] for s in merged["sources"]] == ["s1", "s2"]


def test_same_slug_different_version_is_conflict(tmp_path: Path) -> None:
    """**F2 判别力本体**：同 slug、**不同版本** ⇒ 必须报错（曾因用 slug 判而恒假）。"""
    a = _write(
        tmp_path,
        "a.json",
        _payload(["Shared.thing"], source="spec-webidl", version="webref@aaa"),
    )
    b = _write(
        tmp_path,
        "b.json",
        _payload(["Shared.thing"], source="spec-webidl", version="khronos@bbb"),
    )
    out = tmp_path / "merged.json"
    with pytest.raises(SystemExit, match="slug 可归并"):
        merge.main([str(a), str(b), "--out", str(out)])


def test_same_source_repeated_is_idempotent(tmp_path: Path) -> None:
    """同 slug **同版本**（同一源分片）⇒ 合法去重，不报错。"""
    a = _write(
        tmp_path,
        "a.json",
        _payload(["X.a"], source="spec-webidl", version="webref@aaa"),
    )
    b = _write(
        tmp_path,
        "b.json",
        _payload(["X.a", "X.b"], source="spec-webidl", version="webref@aaa"),
    )
    out = tmp_path / "merged.json"
    assert merge.main([str(a), str(b), "--out", str(out)]) == 0
    merged = json.loads(out.read_text(encoding="utf-8"))
    assert merged["members"] == ["X.a", "X.b"]
    assert merged["member_source_versions"] == {"X.a": "webref@aaa", "X.b": "webref@aaa"}


def test_empty_source_is_rejected(tmp_path: Path) -> None:
    """空源 ⇒ 拒绝（防静默丢源 ⇒ 空集会让完备性门禁假通过）。"""
    a = _write(tmp_path, "a.json", _payload(["X.a"], source="s1", version="v1"))
    b = _write(tmp_path, "b.json", _payload([], source="s2", version="v2"))
    out = tmp_path / "merged.json"
    with pytest.raises(SystemExit, match="为空"):
        merge.main([str(a), str(b), "--out", str(out)])


def test_provenance_survives_remerge_of_merged_file(tmp_path: Path) -> None:
    """对**已合并文件**再合并 ⇒ provenance 续接（不被展平 ⇒ 源数不静默减少）。"""
    a = _write(tmp_path, "a.json", _payload(["X.a"], source="s1", version="v1"))
    b = _write(tmp_path, "b.json", _payload(["Y.c"], source="s2", version="v2"))
    mid = tmp_path / "mid.json"
    assert merge.main([str(a), str(b), "--out", str(mid)]) == 0
    c = _write(tmp_path, "c.json", _payload(["Z.e"], source="s3", version="v3"))
    out = tmp_path / "merged.json"
    assert merge.main([str(mid), str(c), "--out", str(out)]) == 0
    merged = json.loads(out.read_text(encoding="utf-8"))
    assert [s["source"] for s in merged["sources"]] == ["s1", "s2", "s3"]
    assert merged["member_source_versions"] == {
        "X.a": "v1",
        "Y.c": "v2",
        "Z.e": "v3",
    }


def test_conflict_leaves_no_partial_output(tmp_path: Path) -> None:
    """冲突 ⇒ 不写半成品（`out` 不得被创建）。"""
    a = _write(
        tmp_path,
        "a.json",
        _payload(["Shared.thing"], source="spec-webidl", version="webref@aaa"),
    )
    b = _write(
        tmp_path,
        "b.json",
        _payload(["Shared.thing"], source="spec-webidl", version="khronos@bbb"),
    )
    out = tmp_path / "merged.json"
    with pytest.raises(SystemExit):
        merge.main([str(a), str(b), "--out", str(out)])
    assert not out.exists(), "冲突时不得写出半成品"
