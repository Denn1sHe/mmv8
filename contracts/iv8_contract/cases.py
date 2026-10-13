"""探针用例（probe case）的数据模型与加载。

## 两种期望并存，是「双 pipeline」的根本原因

同一个探针脚本有**两份期望**：

- **规范模式（:attr:`Mode.SPEC`）**：按 WHATWG / W3C 规范应得的结果。
  它衡量「这个运行时对不对」。
- **iv8 兼容模式（:attr:`Mode.IV8`）**：按 iv8 社区版 v0.1.4 的**实际行为**（可能含 bug）
  应得的结果。它衡量「换掉 iv8 后，上游的混淆 JS 会不会崩」。

两条期望不一致本身就是有价值的信号：说明社区版在此处偏离规范，
而我们选择**兼容**而非修正（这正是「兼容 iv8 契约」这一产品目标的含义）。
"""

from __future__ import annotations

from collections.abc import Iterable, Mapping, Sequence
from dataclasses import dataclass, field
from enum import Enum
from pathlib import Path
from typing import Any

import yaml

__all__ = ["CaseError", "Mode", "ProbeCase", "load_cases"]


class CaseError(Exception):
    """用例文件结构非法。"""


class Mode(str, Enum):
    """契约 runner 的两种运行模式。"""

    #: 规范模式：期望值取 ``spec_expect``，对照 WHATWG / W3C。
    SPEC = "spec"
    #: iv8 兼容模式：期望值取 ``iv8_expect``，对照社区版 v0.1.4 实际行为。
    IV8 = "iv8"

    @property
    def expectation_field(self) -> str:
        """该模式下应当读取的期望字段名。"""
        return "spec_expect" if self is Mode.SPEC else "iv8_expect"

    @property
    def label(self) -> str:
        """人类可读标签（进报告）。"""
        return {
            Mode.SPEC: "规范模式（对照 WHATWG/W3C）",
            Mode.IV8: "iv8 兼容模式（对照社区版 v0.1.4 实际行为）",
        }[self]


#: 严重度取值，从高到低。报告按此排序。
SEVERITIES: tuple[str, ...] = ("blocker", "critical", "major", "minor", "info")

_REQUIRED_FIELDS: frozenset[str] = frozenset(
    {"id", "category", "script", "spec_expect", "iv8_expect"}
)


@dataclass(frozen=True, slots=True)
class ProbeCase:
    """一条契约探针用例。

    Attributes:
        id: 成员级唯一标识，与 ``compat-manifest`` 的条目 id 对齐。
        category: 分组分类（如 ``timer`` / ``dom`` / ``crypto``）。
        script: 待执行的 JS 源码片段。
        spec_expect: 规范模式期望值（任何可 JSON 序列化的值）。
        iv8_expect: iv8 兼容模式期望值。
        severity: 差异严重度，取自 :data:`SEVERITIES`。
    """

    id: str
    category: str
    script: str
    spec_expect: Any
    iv8_expect: Any
    severity: str = "major"
    #: 双期望不一致（社区版偏离规范）的说明；一致时为空串。
    divergence: str = field(default="", compare=False)

    def __post_init__(self) -> None:
        if self.severity not in SEVERITIES:
            raise CaseError(
                f"用例 {self.id} 的 severity={self.severity!r} 非法，"
                f"合法值为：{', '.join(SEVERITIES)}"
            )
        # 双期望不一致时**自动**补一句说明。放在 __post_init__ 而不是
        # from_mapping，是为了保证「任何构造路径」都不会漏掉它——否则直接
        # new ProbeCase(...) 会得到空说明，让最重要的信号静默消失。
        if self.diverges and not self.divergence:
            object.__setattr__(
                self,
                "divergence",
                "规范期望与 iv8 社区版实际行为不一致：兼容模式按 iv8 期望判定",
            )

    @property
    def diverges(self) -> bool:
        """规范期望与 iv8 期望是否不同（社区版偏离规范的信号）。"""
        return self.spec_expect != self.iv8_expect

    def expectation(self, mode: Mode) -> Any:
        """取该模式下的期望值。"""
        return self.spec_expect if mode is Mode.SPEC else self.iv8_expect

    @classmethod
    def from_mapping(cls, raw: Mapping[str, Any]) -> "ProbeCase":
        """从单条用例字典构造并校验。"""
        if not isinstance(raw, Mapping):
            raise CaseError(f"用例必须是对象，收到 {type(raw).__name__}")
        missing = _REQUIRED_FIELDS - set(raw)
        if missing:
            raise CaseError(f"用例缺少必填字段：{sorted(missing)}")
        # divergence 的缺省填充由 __post_init__ 负责（覆盖所有构造路径）。
        return cls(
            id=str(raw["id"]),
            category=str(raw["category"]),
            script=str(raw["script"]),
            spec_expect=raw["spec_expect"],
            iv8_expect=raw["iv8_expect"],
            severity=str(raw.get("severity", "major")),
            divergence=str(raw.get("divergence", "")),
        )


def load_cases(directory: str | Path) -> tuple[ProbeCase, ...]:
    """从目录加载全部用例（``*.yaml`` / ``*.yml``），按 id 排序保证可复现。

    空目录合法（G0 起步就是空清单）。
    """
    base = Path(directory)
    if not base.is_dir():
        raise CaseError(f"用例目录不存在：{base}")

    cases: list[ProbeCase] = []
    seen: dict[str, str] = {}
    for path in sorted(base.glob("*.y*ml")):
        raw = yaml.safe_load(path.read_text(encoding="utf-8"))
        if raw is None:
            continue
        if isinstance(raw, Mapping):
            raw = [raw]
        if not isinstance(raw, Sequence) or isinstance(raw, (str, bytes)):
            raise CaseError(f"{path.name}: 顶层应为用例数组或单个用例对象")
        for item in raw:
            case = ProbeCase.from_mapping(item)
            if case.id in seen:
                raise CaseError(
                    f"用例 id 重复：{case.id}（{seen[case.id]} 与 {path.name}）"
                )
            seen[case.id] = path.name
            cases.append(case)

    return tuple(sorted(cases, key=lambda c: c.id))


def iter_categories(cases: Iterable[ProbeCase]) -> dict[str, int]:
    """按分类统计用例数（用于报告概览）。"""
    counter: dict[str, int] = {}
    for case in cases:
        counter[case.category] = counter.get(case.category, 0) + 1
    return dict(sorted(counter.items()))