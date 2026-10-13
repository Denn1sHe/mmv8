"""`compat-manifest.json` 的结构、状态流转与单调性校验。

本模块是**铁律 8**（兼容率单调性）的机器载体，也是 G0 契约冻结的产物载体。

## 设计要点

1. **条目粒度 = WebIDL 成员级**，不是「接口族矩阵」。每条 :class:`Entry` 对应一个
   具体成员（属性 / 方法 / 描述符 / 时序行为），因此 ``id`` 必须能唯一定位到
   ``Interface.member`` 形式，而不是 ``Window`` 这种族级名字。
2. **状态四值**，其中只有 ``verified`` 不阻断发布：

   - ``verified`` —— 已实现且与契约一致；
   - ``failed``   —— 已实现但与契约不一致；
   - ``missing``  —— 契约要求但未实现；
   - ``unknown``  —— 尚未判定（含「无法运行探针」）。

   后三者一律阻断（技术方案 §18 的 G0 定义）。
3. **单调性**是集合包含关系，而不只是「计数不下降」：一次改动不能把某个
   已 ``verified`` 的条目降级，同时靠新增另一条 ``verified`` 凑平总数。
   因此 :meth:`Manifest.assert_monotonic` 比对的是 ``verified`` **集合**。
"""

from __future__ import annotations

import json
from collections.abc import Iterable, Mapping
from dataclasses import dataclass
from enum import Enum
from pathlib import Path
from typing import Any

__all__ = [
    "Entry",
    "EntryStatus",
    "Manifest",
    "ManifestError",
    "load",
    "dump",
]


class ManifestError(Exception):
    """manifest 结构非法，或违反单调性。"""


class EntryStatus(str, Enum):
    """单条契约条目的状态。"""

    VERIFIED = "verified"
    FAILED = "failed"
    MISSING = "missing"
    UNKNOWN = "unknown"

    @classmethod
    def parse(cls, raw: str) -> "EntryStatus":
        """把字符串解析为状态；未知值报错（不静默降级为 ``unknown``）。"""
        try:
            return cls(raw)
        except ValueError as exc:  # pragma: no cover - 错误信息由测试断言
            allowed = ", ".join(s.value for s in cls)
            raise ManifestError(
                f"未知的条目状态 {raw!r}；合法值为：{allowed}"
            ) from exc


#: 一律阻断发布的状态集合（技术方案 §18 G0）。
BLOCKING_STATUSES: frozenset[EntryStatus] = frozenset(
    {EntryStatus.FAILED, EntryStatus.MISSING, EntryStatus.UNKNOWN}
)


@dataclass(frozen=True, slots=True)
class Entry:
    """一条契约条目（成员级，不是族级）。

    Attributes:
        id: 成员级唯一标识，形如 ``Window.setTimeout`` 或 ``Document#childNodes``。
        status: 四值状态之一。
        category: 便于分组统计的分类（自由文本，默认 ``uncategorized``）。
        note: 备注；``failed`` 时建议写明差异，便于人工复核。
    """

    id: str
    status: EntryStatus
    category: str = "uncategorized"
    note: str = ""

    def __post_init__(self) -> None:
        if not self.id or "." not in self.id:
            raise ManifestError(
                f"条目 id {self.id!r} 非法：必须是成员级标识（形如 'Window.setTimeout'），"
                "族级名称会让'接口族矩阵'冒充成员级清单"
            )

    @property
    def blocking(self) -> bool:
        """该条目是否阻断发布。"""
        return self.status in BLOCKING_STATUSES

    def to_json(self) -> dict[str, Any]:
        """转为可 JSON 序列化的字典（字段顺序稳定，便于 diff）。"""
        raw: dict[str, Any] = {
            "id": self.id,
            "status": self.status.value,
            "category": self.category,
        }
        if self.note:
            raw["note"] = self.note
        return raw

    @classmethod
    def from_json(cls, raw: Mapping[str, Any]) -> "Entry":
        """从字典构造；缺字段或类型不对均报 :class:`ManifestError`。"""
        if not isinstance(raw, Mapping):
            raise ManifestError(f"条目必须是对象，收到 {type(raw).__name__}")
        missing = {"id", "status"} - set(raw)
        if missing:
            raise ManifestError(f"条目缺少必填字段：{sorted(missing)}")
        return cls(
            id=str(raw["id"]),
            status=EntryStatus.parse(str(raw["status"])),
            category=str(raw.get("category", "uncategorized")),
            note=str(raw.get("note", "")),
        )


@dataclass(frozen=True, slots=True)
class Manifest:
    """`compat-manifest.json` 的不可变内存表示。"""

    version: int
    entries: tuple[Entry, ...]

    def __post_init__(self) -> None:
        seen: set[str] = set()
        for entry in self.entries:
            if entry.id in seen:
                raise ManifestError(f"条目 id 重复：{entry.id}")
            seen.add(entry.id)

    # ---------------------------------------------------------------- 查询

    def __len__(self) -> int:
        return len(self.entries)

    def __iter__(self):
        return iter(self.entries)

    def by_id(self) -> dict[str, Entry]:
        """按 id 索引。"""
        return {entry.id: entry for entry in self.entries}

    def counts(self) -> dict[str, int]:
        """按状态计数；四种状态恒出现（无条目时为 0），便于直接进报告。"""
        counter = {status.value: 0 for status in EntryStatus}
        for entry in self.entries:
            counter[entry.status.value] += 1
        return counter

    def verified_ids(self) -> frozenset[str]:
        """已 verified 的条目 id 集合——单调性比对的依据。"""
        return frozenset(e.id for e in self.entries if e.status is EntryStatus.VERIFIED)

    def blocking_entries(self) -> tuple[Entry, ...]:
        """全部阻断发布的条目。"""
        return tuple(e for e in self.entries if e.blocking)

    def is_release_blocking(self) -> bool:
        """是否存在任一阻断条目。"""
        return any(e.blocking for e in self.entries)

    def replace(self, entries: Iterable[Entry]) -> "Manifest":
        """用新条目集合产生新 manifest（不可变，便于对比）。"""
        return Manifest(version=self.version, entries=tuple(entries))

    # ---------------------------------------------------------------- 单调性

    def regressions_from(self, baseline: "Manifest") -> tuple[str, ...]:
        """相对 ``baseline`` 的 verified **集合**回退项（空元组表示无回退）。

        只报「曾经 verified 现在不是」，不报计数——新增条目不会掩盖回退。
        """
        lost = baseline.verified_ids() - self.verified_ids()
        return tuple(sorted(lost))

    def assert_monotonic(self, baseline: "Manifest") -> None:
        """铁律 8：``baseline`` 中任一 verified 条目在 ``self`` 中不得降级。"""
        lost = self.regressions_from(baseline)
        if lost:
            joined = ", ".join(lost)
            raise ManifestError(
                f"兼容率回退：{len(lost)} 条此前 verified 的条目不再是 verified —— {joined}。"
                "按铁律 8，verified 条目数不得下降；若变更确属规范修订，"
                "须以独立 PR 修改 golden / manifest 并写明理由"
            )

    # ---------------------------------------------------------------- 序列化

    def to_json(self) -> dict[str, Any]:
        """转为可 JSON 序列化的字典。"""
        return {
            "version": self.version,
            "entries": [e.to_json() for e in self.entries],
        }

    @classmethod
    def from_json(cls, raw: Mapping[str, Any]) -> "Manifest":
        """从字典构造并做结构校验。"""
        if not isinstance(raw, Mapping):
            raise ManifestError(f"manifest 顶层必须是对象，收到 {type(raw).__name__}")
        missing = {"version", "entries"} - set(raw)
        if missing:
            raise ManifestError(f"manifest 缺少必填字段：{sorted(missing)}")
        raw_entries = raw["entries"]
        if not isinstance(raw_entries, list):
            raise ManifestError("manifest.entries 必须是数组")
        return cls(
            version=int(raw["version"]),
            entries=tuple(Entry.from_json(item) for item in raw_entries),
        )

    def to_text(self) -> str:
        """序列化为稳定的 JSON 文本（2 空格缩进，行尾换行）。"""
        return json.dumps(self.to_json(), indent=2, ensure_ascii=False) + "\n"


def load(path: str | Path) -> Manifest:
    """从磁盘读取 manifest。"""
    text = Path(path).read_text(encoding="utf-8")
    return Manifest.from_json(json.loads(text))


def dump(manifest: Manifest, path: str | Path) -> None:
    """把 manifest 写回磁盘（UTF-8、稳定缩进）。"""
    Path(path).write_text(manifest.to_text(), encoding="utf-8")