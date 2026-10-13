"""`compat-manifest` 的契约测试（对应 T1.2 验收标准）。

重点覆盖两件容易「做假」的地方：

1. **结构非法必须报错**，不得静默接受——尤其禁止族级 id（`Window`）
   冒充成员级 id（`Window.setTimeout`）。
2. **单调性按集合判定，不按计数判定**——防止「新增一条 verified 掩盖
   另一条 verified 降级」这种总数持平的回退绕过 CI 阻断。
"""

from __future__ import annotations

import json

import pytest

from iv8_contract.manifest import (
    Entry,
    EntryStatus,
    Manifest,
    ManifestError,
    dump,
    load,
)


def _m(*entries: Entry, version: int = 1) -> Manifest:
    return Manifest(version=version, entries=tuple(entries))


# ---------------------------------------------------------------- 结构


def test_empty_manifest_round_trips():
    """空清单必须可用（G0 起步就是空清单）。"""
    m = _m()
    assert len(m) == 0
    assert m.verified_ids() == frozenset()
    assert m.counts() == {
        "verified": 0,
        "failed": 0,
        "missing": 0,
        "unknown": 0,
    }
    assert Manifest.from_json(json.loads(m.to_text())) == m


def test_all_four_statuses_are_counted_even_when_zero():
    """counts 恒含四个键，便于直接进报告，不需调用方补零。"""
    m = _m(Entry("Window.setTimeout", EntryStatus.VERIFIED))
    counts = m.counts()
    assert set(counts) == {"verified", "failed", "missing", "unknown"}
    assert counts["verified"] == 1
    assert counts["failed"] == 0


def test_unknown_status_is_rejected_not_downgraded():
    """未知状态必须报错，不得静默降级成 unknown（那会掩盖拼写错误）。"""
    with pytest.raises(ManifestError, match="未知的条目状态"):
        EntryStatus.parse("verifed")


def test_family_level_id_is_rejected():
    """族级 id 会让「接口族矩阵」冒充成员级清单，必须拒绝。"""
    with pytest.raises(ManifestError, match="成员级标识"):
        Entry("Window", EntryStatus.VERIFIED)


def test_empty_id_is_rejected():
    with pytest.raises(ManifestError, match="成员级标识"):
        Entry("", EntryStatus.VERIFIED)


def test_duplicate_id_is_rejected():
    with pytest.raises(ManifestError, match="条目 id 重复"):
        _m(
            Entry("Window.setTimeout", EntryStatus.VERIFIED),
            Entry("Window.setTimeout", EntryStatus.FAILED),
        )


def test_missing_required_field_is_rejected():
    with pytest.raises(ManifestError, match="缺少必填字段"):
        Entry.from_json({"id": "Window.setTimeout"})


def test_manifest_requires_version_and_entries():
    with pytest.raises(ManifestError, match="缺少必填字段"):
        Manifest.from_json({"version": 1})


# ---------------------------------------------------------------- 阻断语义


@pytest.mark.parametrize(
    "status",
    [EntryStatus.FAILED, EntryStatus.MISSING, EntryStatus.UNKNOWN],
)
def test_three_statuses_block_release(status):
    """技术方案 §18：verified 之外三者一律阻断。"""
    e = Entry("Window.setTimeout", status)
    assert e.blocking is True
    assert _m(e).is_release_blocking() is True


def test_verified_does_not_block():
    e = Entry("Window.setTimeout", EntryStatus.VERIFIED)
    assert e.blocking is False
    assert _m(e).is_release_blocking() is False


# ---------------------------------------------------------------- 单调性（铁律 8）


def test_no_regression_passes():
    baseline = _m(
        Entry("A.b", EntryStatus.VERIFIED),
        Entry("C.d", EntryStatus.VERIFIED),
    )
    current = _m(
        Entry("A.b", EntryStatus.VERIFIED),
        Entry("C.d", EntryStatus.VERIFIED),
        Entry("E.f", EntryStatus.VERIFIED),  # 新增 verified 是允许的
    )
    current.assert_monotonic(baseline)


def test_verified_downgrade_is_rejected():
    baseline = _m(Entry("A.b", EntryStatus.VERIFIED))
    current = _m(Entry("A.b", EntryStatus.FAILED))
    with pytest.raises(ManifestError, match="兼容率回退"):
        current.assert_monotonic(baseline)


def test_removing_verified_entry_is_rejected():
    """把条目直接删掉也是回退（删除会让 verified 集合缩小）。"""
    baseline = _m(Entry("A.b", EntryStatus.VERIFIED))
    current = _m()
    with pytest.raises(ManifestError, match="兼容率回退"):
        current.assert_monotonic(baseline)


def test_new_verified_cannot_mask_a_downgrade():
    """**计数持平但集合回退**必须被拒绝——这是最容易被做假的绕过方式。

    baseline: A.b(verified) —— 1 条
    current:  A.b(failed) + C.d(verified) —— verified 计数仍是 1，但集合变了。
    若按计数判定会漏过，按集合判定必须报错。
    """
    baseline = _m(Entry("A.b", EntryStatus.VERIFIED))
    current = _m(
        Entry("A.b", EntryStatus.FAILED),
        Entry("C.d", EntryStatus.VERIFIED),
    )
    assert len(baseline.verified_ids()) == len(current.verified_ids()) == 1
    with pytest.raises(ManifestError, match="兼容率回退"):
        current.assert_monotonic(baseline)


def test_regression_message_lists_offending_ids():
    baseline = _m(
        Entry("A.b", EntryStatus.VERIFIED),
        Entry("C.d", EntryStatus.VERIFIED),
    )
    current = _m(
        Entry("A.b", EntryStatus.MISSING),
        Entry("C.d", EntryStatus.VERIFIED),
    )
    with pytest.raises(ManifestError, match=r"A\.b"):
        current.assert_monotonic(baseline)


def test_upgrading_status_is_allowed():
    """missing → verified 是进步，必须允许（否则 CI 会逼人别修 bug）。"""
    baseline = _m(Entry("A.b", EntryStatus.MISSING))
    current = _m(Entry("A.b", EntryStatus.VERIFIED))
    current.assert_monotonic(baseline)


# ---------------------------------------------------------------- 序列化


def test_dump_load_round_trip(tmp_path):
    m = _m(
        Entry("Window.setTimeout", EntryStatus.VERIFIED, category="timers"),
        Entry("Window.clearTimeout", EntryStatus.FAILED, note="参数校验缺失"),
    )
    path = tmp_path / "compat-manifest.json"
    dump(m, path)
    assert load(path) == m


def test_text_is_stable_for_diff():
    """golden 必须稳定：同样内容两次序列化完全一致，且以换行结尾。"""
    m = _m(Entry("A.b", EntryStatus.VERIFIED))
    assert m.to_text() == m.to_text()
    assert m.to_text().endswith("\n")


def test_note_round_trips_only_when_present():
    bare = Entry("A.b", EntryStatus.VERIFIED)
    assert "note" not in bare.to_json()
    with_note = Entry("A.b", EntryStatus.VERIFIED, note="hello")
    assert with_note.to_json()["note"] == "hello"