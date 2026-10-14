"""`G4-S0b` 候选源 B 工具自测：WebIDL 成员抽取器。

判别性要点（逐条附理由）：`constructor` 不入成员 ｜ 继承基类成员**不误归**派生接口 ｜
`partial interface` **合并** ｜ 注释剔除 ｜ 产出**必带** `needs_community_review` ｜
产出成员可被 manifest 门禁（成员级 `id`）与 `load_members` 接受。
"""

from __future__ import annotations

import importlib.util
import json
import sys
from pathlib import Path

import pytest

from iv8_contract.completeness import load_members
from iv8_contract.manifest import Entry, EntryStatus

_TOOL = Path(__file__).resolve().parent.parent / "tools" / "extract_webidl_members.py"


def _load_tool():
    """按路径加载工具模块（`contracts/tools/` 非包，故用 spec 装载）。"""
    spec = importlib.util.spec_from_file_location("extract_webidl_members", _TOOL)
    assert spec and spec.loader
    module = importlib.util.module_from_spec(spec)
    # `dataclass(slots=True)` 需模块已注册（否则 dataclasses 取不到 `__dict__`）——先注册再执行。
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


IDL_SAMPLE = """
// 行注释：应被剔除 —— attribute Fake.notAMember;
/* 块注释：interface Ghost { attribute long ghostMember; }; */
[Exposed=Window]
interface IDBFactory : EventTarget {
  constructor();
  readonly attribute DOMString name;
  IDBOpenDBRequest open(DOMString name, optional unsigned long long version);
  undefined deleteDatabase(DOMString name);
};

partial interface IDBFactory {
  const unsigned short VERSION = 1;
};
"""


def test_extracts_attributes_methods_constants() -> None:
    """属性 / 方法 / 常量**三类**均入成员（成员语义 = WebIDL `.member`）。"""
    members = _load_tool().extract_members(IDL_SAMPLE)
    assert "IDBFactory.name" in members
    assert "IDBFactory.open" in members
    assert "IDBFactory.deleteDatabase" in members
    assert "IDBFactory.VERSION" in members


def test_constructor_is_not_a_member() -> None:
    """**负对照**：`constructor(...)` **不是成员名** ⇒ 不得出现在产出（防同名构造器补足计数）。"""
    members = _load_tool().extract_members(IDL_SAMPLE)
    assert not any(m.endswith(".constructor") for m in members)


def test_inheritance_base_members_not_attributed() -> None:
    """**负对照**：继承基类（`: EventTarget`）的成员**归其自身接口** ⇒ 不得记到派生接口名下。"""
    members = _load_tool().extract_members(IDL_SAMPLE)
    assert "IDBFactory.addEventListener" not in members  # EventTarget 的成员


def test_comments_are_stripped() -> None:
    """**负对照**：注释内的伪成员（`Fake.notAMember` / `Ghost.ghostMember`）不得被抽出。"""
    members = _load_tool().extract_members(IDL_SAMPLE)
    assert "Fake.notAMember" not in members
    assert "Ghost.ghostMember" not in members


def test_partial_interface_is_merged() -> None:
    """`partial interface` 与主块**合并**（同一接口名的成员并集）。"""
    members = _load_tool().extract_members(IDL_SAMPLE)
    # 主块的 name 与 partial 块的 VERSION 应同时存在
    assert {"IDBFactory.name", "IDBFactory.VERSION"} <= set(members)


def test_mixin_members_are_merged_into_includer() -> None:
    """**mixin 合入**：`X includes M;` ⇒ `M` 的成员属 `X` 的接口表面（**不修 = 分母被静默剪小**）。

    实测背景：`encoding.idl` 的 `TextDecoder` 原只抽到自有成员 `decode`；
    `encoding`/`fatal`/`ignoreBOM` 在 `interface mixin TextDecoderCommon` 内 ⇒ 修复后四项齐。
    """
    idl = """
    interface mixin Common { readonly attribute boolean shared; };
    interface Widget { undefined own(); };
    Widget includes Common;
    """
    members = _load_tool().extract_members(idl)
    assert members == ("Widget.own", "Widget.shared")


def test_mixin_not_included_is_absent() -> None:
    """**负对照**：**未** `includes` 的 mixin 成员**不得**归到任何接口（防张冠李戴）。"""
    idl = """
    interface mixin Orphan { readonly attribute boolean lonely; };
    interface Widget { undefined own(); };
    """
    members = _load_tool().extract_members(idl)
    assert members == ("Widget.own",)


def test_members_are_member_level_ids() -> None:
    """**全部产出必须是成员级 `id`**（含点）⇒ 可被 manifest 门禁接受（族级名当场拒）。"""
    members = _load_tool().extract_members(IDL_SAMPLE)
    assert members, "样例应产出非空成员集"
    for member in members:
        Entry(id=member, status=EntryStatus.MISSING)  # 构造成功即通过成员级校验


def test_extract_path_and_payload_shape(tmp_path: Path) -> None:
    """端到端：目录抽取 ⇒ 产出含可追性字段 + **必带** `needs_community_review: true` + 可被 `load_members` 读。"""
    tool = _load_tool()
    (tmp_path / "sample.idl").write_text(IDL_SAMPLE, encoding="utf-8")
    extraction = tool.extract_path(tmp_path)
    payload = tool.build_payload(extraction, "webref@deadbeef")

    assert payload["source"] == "spec-webidl"
    assert payload["needs_community_review"] is True
    assert payload["source_version"] == "webref@deadbeef"
    assert len(str(payload["source_sha256"])) == 64  # sha256 十六进制
    assert payload["members"], "成员集不得为空"

    # 产出**必须**能被完备性门禁读取（fail-closed 的 `load_members`）。
    out = tmp_path / "members.json"
    out.write_text(json.dumps(payload, ensure_ascii=False), encoding="utf-8")
    assert load_members(out) == frozenset(payload["members"])


def test_extract_path_missing_input_rejected(tmp_path: Path) -> None:
    """**负对照**：无 IDL 输入 ⇒ 立即报错（**不得产出空清单**——空清单会被门禁判"假通过"风险）。"""
    with pytest.raises(SystemExit):
        _load_tool().extract_path(tmp_path)


def test_cli_writes_file(tmp_path: Path) -> None:
    """CLI 端到端：`--out` 写文件（工具即可执行形态）。"""
    tool = _load_tool()
    idl = tmp_path / "x.idl"
    idl.write_text("interface X { attribute long y; };", encoding="utf-8")
    out = tmp_path / "out.json"
    assert tool.main([str(idl), "--out", str(out)]) == 0
    payload = json.loads(out.read_text(encoding="utf-8"))
    assert payload["members"] == ["X.y"]
