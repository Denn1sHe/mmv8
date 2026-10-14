"""`G4-S0a` 验收：manifest **九字段 schema** + `behavior_level` 三态 + **完备性双门禁**。

口径来源：`docs/design-g4-真身-2026-10-15.md` `§6`（`R-1` schema / `R-3` 分母完备性）+ `§4.7`。

每条断言均附**判别性理由**；负对照用例显式标注（`R-8`：负对照制度化）。
"""

from __future__ import annotations

import importlib.util
import json
import sys
from pathlib import Path

import pytest

from iv8_contract.completeness import check_completeness, load_members
from iv8_contract.manifest import (
    BEHAVIOUR_EVIDENCE_MARKERS,
    BehaviorLevel,
    Entry,
    EntryStatus,
    Manifest,
    ManifestError,
)


#: 仓库根（`contracts/tests/` ↑2）。
REPO_ROOT = Path(__file__).resolve().parents[2]
#: **发货态**实物：manifest 与成员集（`F1` 实物断言的读取对象）。
SHIPPED_MANIFEST = REPO_ROOT / "contracts" / "compat-manifest.json"
SHIPPED_MEMBERS = REPO_ROOT / "contracts" / "community-v0.1.4-members.json"
#: **冻结分母**（`S0` 封口值；此后增删须 `ADR` 显式）。
#: `ADR-0028`（2026-10-16）：**显式变更 1669 ⇒ 1698** —— F3 联合类型漏列 7 条 +
#: `D-S2-1` Credentials 家族扩源 22 条；抽取器正则同批修复；Khronos 独立来源族（`D-2`）。
FROZEN_DENOMINATOR = 1698


def _load_seed_tool():
    """按路径加载 `tools/seed_manifest.py`（`contracts/tools/` 非包，故用 spec 装载）。"""
    tool = REPO_ROOT / "contracts" / "tools" / "seed_manifest.py"
    spec = importlib.util.spec_from_file_location("seed_manifest", tool)
    assert spec and spec.loader
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


def _seed_main(argv: list[str]) -> int:
    """跑一次 seed 生成器，返回退出码（`main` 的 `SystemExit` 由调用方断言）。"""
    return _load_seed_tool().main(argv)


def _complete_entry(**overrides: object) -> Entry:
    """一条**九字段齐**的条目（其余用例在此基线上做单点破坏）。"""
    base: dict[str, object] = {
        "target_version": "v0.1.4",
        "id": "Window.setTimeout",
        "realm": "main",
        "descriptor": "method",
        "behavior_level": BehaviorLevel.CONTRACT,
        "test_ids": ("test_page_control.py::test_set_timeout",),
        "status": EntryStatus.VERIFIED,
        "evidence": "契约：签名 (handler, timeout?) => number",
        "implementation_owner": "engine-v8",
    }
    base.update(overrides)
    return Entry(**base)  # type: ignore[arg-type]


# ---------------------------------------------------------------- 九字段完备性


def test_schema_complete_entry_passes() -> None:
    """九字段齐 ⇒ 完备性门禁通过（**正例**：门禁本身不得恒红）。"""
    entry = _complete_entry()
    entry.assert_schema_complete()
    entry.assert_behavior_evidence()


@pytest.mark.parametrize("field", ["test_ids", "implementation_owner", "descriptor", "realm"])
def test_schema_missing_field_raises(field: str) -> None:
    """**负对照**：缺任一必填字段 ⇒ 红（逐字段参数化，防「只查一个」）。"""
    empty = () if field == "test_ids" else ""
    with pytest.raises(ManifestError):
        _complete_entry(**{field: empty}).assert_schema_complete()


def test_manifest_empty_is_rejected() -> None:
    """**负对照**：空 manifest ⇒ 完备性门禁红（防「空表通过」——完备性验收的核心风险）。"""
    with pytest.raises(ManifestError):
        Manifest(version=1, entries=()).assert_schema_complete()


def test_manifest_with_complete_entry_passes() -> None:
    """非空 + 九字段齐 ⇒ 通过（正例）。"""
    Manifest(version=1, entries=(_complete_entry(),)).assert_schema_complete()


# ------------------------------------------------------- behavior_level 三态/错标


def test_behavior_level_三态_parse() -> None:
    """三态齐（`stub` / `contract` / `behaviour`）。"""
    assert BehaviorLevel.parse("stub") is BehaviorLevel.STUB
    assert BehaviorLevel.parse("contract") is BehaviorLevel.CONTRACT
    assert BehaviorLevel.parse("behaviour") is BehaviorLevel.BEHAVIOUR


def test_behavior_level_unknown_raises() -> None:
    """未知值报错（不静默降级）。"""
    with pytest.raises(ManifestError):
        BehaviorLevel.parse("surface")


def test_behaviour_requires_behaviour_evidence() -> None:
    """**错标负对照 ①**：标 `behaviour` 而证据无状态转移/时序 ⇒ 红。"""
    entry = _complete_entry(
        behavior_level=BehaviorLevel.BEHAVIOUR,
        evidence="契约：签名存在",  # 无任何行为标记
    )
    with pytest.raises(ManifestError):
        entry.assert_behavior_evidence()


def test_behaviour_with_timing_evidence_passes() -> None:
    """标 `behaviour` 且含时序钉值 ⇒ 通过（正例）。"""
    entry = _complete_entry(
        behavior_level=BehaviorLevel.BEHAVIOUR,
        evidence="timing: 第 N 轮触发（预算 16/轮）",
    )
    entry.assert_behavior_evidence()


def test_contract_must_not_carry_behaviour_evidence() -> None:
    """**错标负对照 ③**：标 `contract` 却附行为证据 ⇒ 红（本应为 `behaviour`）。

    与 ①② 合观 ⇒ **三态错标格全覆盖**：`behaviour`×无标记 / `stub`×有标记 / `contract`×有标记 皆红；
    另三格（`behaviour`×有标记 / `stub`×无标记 / `contract`×无标记）为**合法格**（各有正例）。
    """
    entry = _complete_entry(
        behavior_level=BehaviorLevel.CONTRACT,
        evidence="timing: 某时序钉值",  # 行为证据 ⇒ 应标 behaviour
    )
    with pytest.raises(ManifestError):
        entry.assert_behavior_evidence()


def test_contract_with_contract_evidence_passes() -> None:
    """正例：`contract` + 契约级证据（无行为标记）⇒ 通过。"""
    entry = _complete_entry(
        behavior_level=BehaviorLevel.CONTRACT,
        evidence="契约：描述符 readonly attribute nodeType",
    )
    entry.assert_behavior_evidence()


def test_stub_must_not_carry_behaviour_evidence() -> None:
    """**错标负对照 ②**：标 `stub`（面级）却附行为证据 ⇒ 红（面级禁止行为证据）。"""
    entry = _complete_entry(
        behavior_level=BehaviorLevel.STUB,
        evidence=f"{BEHAVIOUR_EVIDENCE_MARKERS[0]}: 某状态转移",
    )
    with pytest.raises(ManifestError):
        entry.assert_behavior_evidence()


# ---------------------------------------------------------------- 序列化往返


def test_denominator_fixed_roundtrip(tmp_path: Path) -> None:
    """`denominator_fixed` 与九字段**往返不丢**（`S0` 末置真的载体）。"""
    original = Manifest(
        version=1,
        entries=(_complete_entry(),),
        denominator_fixed=True,
    )
    path = tmp_path / "manifest.json"
    path.write_text(original.to_text(), encoding="utf-8")
    reloaded = Manifest.from_json(json.loads(path.read_text(encoding="utf-8")))
    assert reloaded.denominator_fixed is True
    assert reloaded.entries[0].test_ids == original.entries[0].test_ids
    assert reloaded.entries[0].behavior_level is BehaviorLevel.CONTRACT


# ---------------------------------------------------------------- 发货态（`F1`）


def test_shipped_manifest_denominator_is_fixed() -> None:
    """**发货态断言**（`F1`）：仓库内**实物** `compat-manifest.json` 的冻结位**必须为真**。

    **为什么必须有本测**：既有 `test_denominator_fixed_roundtrip` 只在 `tmp_path` 上验**内存往返**，
    **从不读实物** ⇒ 实物被 seed 静默打回 `false` 时**无人红**（静默家族原型，`F1` 实证）。
    **负对照**：把实物 `denominator_fixed` 置回 `false` ⇒ 本测红。
    """
    shipped = Manifest.from_json(json.loads(SHIPPED_MANIFEST.read_text(encoding="utf-8")))
    assert shipped.denominator_fixed is True, (
        f"实发 {SHIPPED_MANIFEST} 的 denominator_fixed 应为 true（S0 末已冻结）；"
        "若为 false —— 多半是 seed 重跑时把它静默打回了"
    )
    assert len(shipped.entries) == FROZEN_DENOMINATOR, (
        f"冻结分母应为 {FROZEN_DENOMINATOR} 条（S0 封口值；**此后增删须 `ADR` 显式**），"
        f"实得 {len(shipped.entries)}"
    )
    # `F15`（架构师 `551f1ad`）：**直钉成员集条目数**（与冻结分母同值）—— 不只钉 manifest
    # （`4.7` 双向核是**间接**钉；这里补**直接**钉，防「清单缩水而 manifest 未同步」）。
    assert len(load_members(SHIPPED_MEMBERS)) == FROZEN_DENOMINATOR, (
        f"实发成员集 {SHIPPED_MEMBERS} 应为 {FROZEN_DENOMINATOR} 条（与冻结分母同值），"
        f"实得 {len(load_members(SHIPPED_MEMBERS))}"
    )


def test_seed_preserves_denominator_freeze(tmp_path: Path) -> None:
    """`F1`：重跑 seed ⇒ **保留**已冻结的 `denominator_fixed`（不得静默打回 `false`）。

    **负对照**：撤掉 `seed_manifest.main` 的「保留冻结位」逻辑（无条件写 `False`）⇒ 本测红。
    """
    out = tmp_path / "manifest.json"
    frozen = Manifest(version=1, entries=(_complete_entry(),), denominator_fixed=True)
    out.write_text(frozen.to_text(), encoding="utf-8")

    assert _seed_main([str(SHIPPED_MEMBERS), "--out", str(out)]) == 0

    reloaded = Manifest.from_json(json.loads(out.read_text(encoding="utf-8")))
    assert reloaded.denominator_fixed is True, "seed 重跑不得静默解冻分母"


def test_seed_refuses_shrinking_frozen_denominator(tmp_path: Path) -> None:
    """`F1`：已冻结且新成员集**条目更少** ⇒ **拒绝写入**（分母**只防删**；缩水须 `ADR` 显式）。

    **负对照**：撤掉 seed 的 `len(members) < frozen_entries` 拒绝分支 ⇒ 本测红（文件被悄悄缩水）。
    """
    out = tmp_path / "manifest.json"
    frozen = Manifest(
        version=1,
        entries=(
            _complete_entry(id="Alpha.one"),
            _complete_entry(id="Beta.two"),
        ),
        denominator_fixed=True,
    )
    out.write_text(frozen.to_text(), encoding="utf-8")
    members = tmp_path / "members.json"
    members.write_text(json.dumps({"members": ["Alpha.one"]}), encoding="utf-8")

    with pytest.raises(SystemExit):
        _seed_main([str(members), "--out", str(out)])

    untouched = Manifest.from_json(json.loads(out.read_text(encoding="utf-8")))
    assert len(untouched.entries) == 2, "拒绝写入后原文件不得被改动"
    assert untouched.denominator_fixed is True


# ---------------------------------------------------------------- 完备性双门禁


def test_completeness_subset_ok() -> None:
    """两侧一致 ⇒ 通过（正例）。"""
    manifest = Manifest(version=1, entries=(_complete_entry(),))
    report = check_completeness(manifest, frozenset({"Window.setTimeout"}))
    report.assert_ok()
    assert report.common == 1


def test_completeness_missing_member_raises() -> None:
    """**负对照 ①（漏列）**：社区清单有、manifest 缺 ⇒ 红。"""
    manifest = Manifest(version=1, entries=(_complete_entry(),))
    community = frozenset({"Window.setTimeout", "Document.createElement"})
    report = check_completeness(manifest, community)
    with pytest.raises(ManifestError):
        report.assert_ok()
    assert report.missing_from_manifest == frozenset({"Document.createElement"})


def test_completeness_fabricated_id_raises() -> None:
    """**负对照 ②（杜撰）**：manifest 有、社区清单无 ⇒ 红。"""
    manifest = Manifest(
        version=1,
        entries=(_complete_entry(), _complete_entry(id="Window.madeUp")),
    )
    report = check_completeness(manifest, frozenset({"Window.setTimeout"}))
    with pytest.raises(ManifestError):
        report.assert_ok()
    assert report.fabricated_in_manifest == frozenset({"Window.madeUp"})


def test_load_members_missing_file_and_empty_rejected(tmp_path: Path) -> None:
    """**负对照 ③**：清单缺失 / 空数组 ⇒ 红（**不得以空集代替**——空集会令门禁假通过）。"""
    with pytest.raises(ManifestError):
        load_members(tmp_path / "nope.json")
    empty = tmp_path / "empty.json"
    empty.write_text('{"members": []}', encoding="utf-8")
    with pytest.raises(ManifestError):
        load_members(empty)


def test_load_members_ok(tmp_path: Path) -> None:
    """正例：最小格式可读。"""
    good = tmp_path / "good.json"
    good.write_text('{"members": ["Window.setTimeout"]}', encoding="utf-8")
    assert load_members(good) == frozenset({"Window.setTimeout"})
