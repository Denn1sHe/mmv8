"""`G4-S0b` 验收：**双门禁对接**（分母完备性 —— 真集绿；漏/造/空三负对照保持红）。

口径：`ADR-0027 §6`（`R-1` 九字段 + `R-3` 分母完备性/`§4.7` 双向核）。
`community-v0.1.4-members.json` = **候选源 B（规范面 WebIDL，webref 快照）** 产出
⇒ **`needs_community_review: true`**（`R-3`：规范面**非定义源**，待社区实物复核）。
"""

from __future__ import annotations

import json
from pathlib import Path

import pytest

from iv8_contract.completeness import check_completeness, load_members
from iv8_contract.manifest import (
    BehaviorLevel,
    Entry,
    EntryStatus,
    Manifest,
    ManifestError,
    load,
)

_CONTRACTS = Path(__file__).resolve().parent.parent
MANIFEST = _CONTRACTS / "compat-manifest.json"
MEMBERS = _CONTRACTS / "community-v0.1.4-members.json"


# ---------------------------------------------------------------- 真集（现行落库态）


def test_seed_is_non_empty_and_schema_complete() -> None:
    """真集：**非空**（种子宁全勿缺）+ **九字段齐** + `behavior_level` 不错标。"""
    manifest = load(MANIFEST)
    assert len(manifest) > 0, "种子不得为空（完备性验收核心风险 = 空表通过）"
    manifest.assert_schema_complete()


def test_seed_completeness_is_green() -> None:
    """真集：`社区清单 ⊆ manifest` ∧ `manifest ⊆ 社区清单` ⇒ **双向核绿**（不漏、不造）。"""
    manifest = load(MANIFEST)
    report = check_completeness(manifest, load_members(MEMBERS))
    report.assert_ok()
    assert report.common == len(manifest), "真集下两侧应完全重合"


def test_seed_members_are_member_level_and_owned() -> None:
    """真集字段面：`id` 成员级（含点）+ `implementation_owner` 非空（`§6` 必填）。"""
    manifest = load(MANIFEST)
    for entry in manifest:
        assert "." in entry.id, f"{entry.id} 非成员级"
        assert entry.implementation_owner, f"{entry.id} 缺 implementation_owner"
    assert all(e.status is EntryStatus.MISSING for e in manifest), "种子起始态应恒为 missing"


def test_seed_behavior_level_is_stub_without_behaviour_evidence() -> None:
    """真集：起始 `behavior_level` 恒 `stub`（面级）⇒ 其 evidence **不得**含行为标记。"""
    manifest = load(MANIFEST)
    assert {e.behavior_level for e in manifest} == {BehaviorLevel.STUB}
    for entry in manifest:  # `assert_schema_complete` 内含 `assert_behavior_evidence`
        entry.assert_behavior_evidence()


def test_members_payload_declares_sources_and_review_flag() -> None:
    """**口径声明**：产出必标**两源**（`spec-webidl` + `cdp-protocol`）+ **`needs_community_review: true`**。

    `R-3`：规范面 **非定义源** ⇒ 逐源登记 `source`/`source_version`/`source_sha256`/`method`
    （**可追性 + 可证复现**）；并集 `source` = 两源名相加。
    """
    payload = json.loads(MEMBERS.read_text(encoding="utf-8"))
    assert payload["needs_community_review"] is True
    assert payload["source"] == "spec-webidl+cdp-protocol"

    sources = payload["sources"]
    assert len(sources) >= 3, "三源都必须登记（webref / devtools / khronos；防静默丢源）"
    for source in sources:
        assert source["members"] > 0, f"{source['source']} 源不得为空"
        assert len(str(source["source_sha256"])) == 64, f"{source['source']} 缺哈希锁"
        assert source["method"], f"{source['source']} 缺逐字命令"
    versions = " ".join(str(s["source_version"]) for s in sources)
    assert "webref@" in versions, "缺 webref 源登记"
    assert "devtools-protocol@" in versions, "缺 devtools-protocol 源登记"
    assert "khronos-webgl@" in versions, "缺 khronos-webgl 源登记"


def test_webgl_family_members_present() -> None:
    """**WebGL 族落行**（裁定 ⑤-2：Khronos 另源）：`WebGLRenderingContext.*` 确在并集中。"""
    members = load_members(MEMBERS)
    assert any(m.startswith("WebGLRenderingContext.") for m in members), "缺 WebGL 1 接口成员"
    assert any(m.startswith("WebGL2RenderingContext.") for m in members), "缺 WebGL 2 接口成员"


def test_cdp_family_members_present() -> None:
    """**CDP 族落行**（裁定 ⑤-1：另源立项）：`<Domain>.<method|event>` 形成员确在并集中。"""
    members = load_members(MEMBERS)
    for probe in ("Runtime.evaluate", "DOM.getDocument", "Network.enable", "Page.navigate"):
        assert probe in members, f"CDP 族应覆盖 {probe}"


# ---------------------------------------------------------------- 三条负对照（保持红）


def test_negative_missing_member_still_red() -> None:
    """**负对照①（漏列）**：从 manifest 抽掉一条 ⇒ `社区 ⊆ manifest` **必红**。"""
    manifest = load(MANIFEST)
    members = load_members(MEMBERS)
    dropped = next(iter(members))
    trimmed = manifest.replace(e for e in manifest if e.id != dropped)
    with pytest.raises(ManifestError):
        check_completeness(trimmed, members).assert_ok()


def test_negative_fabricated_member_still_red() -> None:
    """**负对照②（杜撰）**：塞入不在社区清单的 id ⇒ `manifest ⊆ 社区` **必红**。"""
    manifest = load(MANIFEST)
    members = load_members(MEMBERS)
    fabricated = Entry(
        id="Bogus.notReal",
        status=EntryStatus.MISSING,
        descriptor="attribute",
        test_ids=("g4-seed:pending",),
        implementation_owner="nobody",
    )
    with pytest.raises(ManifestError):
        check_completeness(manifest.replace([*manifest, fabricated]), members).assert_ok()


def test_negative_empty_member_set_still_red(tmp_path: Path) -> None:
    """**负对照③（空集）**：空成员集 ⇒ **`load_members` 直接红**（不得以空集代替 ⇒ 防假通过）。"""
    empty = tmp_path / "empty.json"
    empty.write_text('{"members": [], "source": "spec-webidl"}', encoding="utf-8")
    with pytest.raises(ManifestError):
        load_members(empty)
