"""Chromium 差分 harness：把同一批脚本分别跑在 **iv8** 与 **固定环境下的 Chromium**，
比较结果差异。

## 为什么必须钉死环境

差分基线的全部价值来自**可复现**：同一条差异，任何人在任何时刻重跑都应得到同样结论。
只要 Chrome 版本、OS、字体或 Profile 任一浮动，基线就会漂移，而漂移会被误读成
「我们的实现变了」。因此 :class:`DifferentialConfig` 把这些量设为**必填**，
缺失即报错，而不是给个默认值蒙混过关。

## 后端可插拔，且诚实标注

G0 阶段仓库内没有浏览器，也不该在 CI 里装一个漂移不定的 Chromium。
因此：

- :class:`NullDifferentialBackend` —— **不接触任何浏览器**，空清单下直接跑通。
  它产出的报告里 ``backend_real = False``，从结构上就无法被误读成「已与真实
  Chromium 比对过」。
- 真实后端（G1 接入 CDP）实现同一协议即可替换，配置契约不变。
"""

from __future__ import annotations

from collections.abc import Iterable, Sequence
from dataclasses import dataclass
from typing import Any, Protocol, runtime_checkable

from .cases import SEVERITIES, ProbeCase

__all__ = [
    "DifferentialConfig",
    "DifferentialError",
    "DifferentialReport",
    "DifferentialResult",
    "Harness",
    "NullDifferentialBackend",
    "DifferentialBackend",
]


class DifferentialError(Exception):
    """差分配置非法（例如环境未钉死）。"""


@dataclass(frozen=True, slots=True)
class DifferentialConfig:
    """差分环境契约。全部字段必填 —— 没有默认值可给。"""

    chrome_version: str
    os_name: str
    fonts: tuple[str, ...]
    profile_fingerprint: str
    timezone: str
    locale: str

    def __post_init__(self) -> None:
        missing = [
            name
            for name in (
                "chrome_version",
                "os_name",
                "profile_fingerprint",
                "timezone",
                "locale",
            )
            if not str(getattr(self, name)).strip()
        ]
        if missing:
            raise DifferentialError(
                f"差分环境未钉死，缺少：{missing}。"
                "差分基线的价值全在可复现，任何一项浮动都会让基线漂移"
            )
        if not self.fonts:
            raise DifferentialError(
                "字体集为空：字体直接影响文本度量与布局，必须显式钉死"
            )

    def to_json(self) -> dict[str, Any]:
        """环境指纹，进报告元数据（用于判断两份报告是否可比）。"""
        return {
            "chrome_version": self.chrome_version,
            "os_name": self.os_name,
            "fonts": list(self.fonts),
            "profile_fingerprint": self.profile_fingerprint,
            "timezone": self.timezone,
            "locale": self.locale,
        }


@dataclass(frozen=True, slots=True)
class DifferentialResult:
    """单条用例的差分结果。"""

    case_id: str
    category: str
    severity: str
    identical: bool
    iv8_value: Any = None
    chromium_value: Any = None
    note: str = ""

    def to_json(self) -> dict[str, Any]:
        return {
            "case_id": self.case_id,
            "category": self.category,
            "severity": self.severity,
            "identical": self.identical,
            "iv8_value": self.iv8_value,
            "chromium_value": self.chromium_value,
            "note": self.note,
        }


@runtime_checkable
class DifferentialBackend(Protocol):
    """在固定环境里执行脚本并回传结果。"""

    name: str
    #: 该后端是否真的接触了浏览器。**必须诚实**：
    #: 报告里会原样打印，骨架后端冒充真实比对是严重的事故。
    real: bool

    def evaluate(self, script: str) -> Any:
        ...


@dataclass(frozen=True, slots=True)
class NullDifferentialBackend:
    """不接触浏览器的骨架后端；空清单下可完整跑通。"""

    name: str = "null"
    real: bool = False

    def evaluate(self, script: str) -> Any:
        """永远不应被调用：没有 Chromium 可用。

        显式抛错而非返回 ``None``，避免「未比对」被当成「比对结果相同」。
        """
        raise DifferentialError(
            "NullDifferentialBackend 不执行任何脚本：未接入真实 Chromium。"
            "非空清单请注入真实后端（G1 接入 CDP）"
        )


@dataclass(frozen=True, slots=True)
class DifferentialReport:
    """一次差分运行的完整报告。"""

    config: DifferentialConfig
    backend_name: str
    backend_real: bool
    results: tuple[DifferentialResult, ...]

    @property
    def total(self) -> int:
        return len(self.results)

    @property
    def divergent(self) -> int:
        return sum(1 for r in self.results if not r.identical)

    @property
    def ok(self) -> bool:
        """空清单视为通过；有差异则不通过（差异即待处理项）。"""
        return self.divergent == 0

    def sorted_results(self) -> tuple[DifferentialResult, ...]:
        """按严重度降序，再按 id 升序（保证报告稳定可比对）。"""
        order = {name: i for i, name in enumerate(SEVERITIES)}
        return tuple(
            sorted(self.results, key=lambda r: (order[r.severity], r.case_id))
        )

    def to_json(self) -> dict[str, Any]:
        return {
            "config": self.config.to_json(),
            "backend": self.backend_name,
            "backend_real": self.backend_real,
            "total": self.total,
            "divergent": self.divergent,
            "ok": self.ok,
            "results": [r.to_json() for r in self.sorted_results()],
        }

    def to_text(self) -> str:
        """人类可读报告；已按严重度排序。"""
        lines = [
            "Chromium 差分报告",
            f"环境指纹：{self.config.chrome_version} / {self.config.os_name} / "
            f"tz={self.config.timezone} / locale={self.config.locale}",
            f"字体：{', '.join(self.config.fonts)}",
            f"Profile 指纹：{self.config.profile_fingerprint}",
            f"后端：{self.backend_name}（真实浏览器：{'是' if self.backend_real else '否'}）",
            f"用例 {self.total} 条，差异 {self.divergent} 条，结论：{'通过' if self.ok else '有差异'}",
            "",
        ]
        if not self.backend_real:
            lines.append("⚠️ 本次**未**与真实 Chromium 比对，仅为骨架跑通。")
            lines.append("")
        if not self.results:
            lines.append("（空清单——G0 起步状态）")
            return "\n".join(lines) + "\n"
        for r in self.sorted_results():
            mark = "一致" if r.identical else "差异"
            lines.append(f"[{mark}] {r.case_id} ({r.category}, {r.severity})")
            if not r.identical:
                lines.append(f"        iv8      = {r.iv8_value!r}")
                lines.append(f"        chromium = {r.chromium_value!r}")
        return "\n".join(lines) + "\n"


@dataclass(frozen=True, slots=True)
class Harness:
    """把「跑差分」这件事与具体后端解耦。"""

    config: DifferentialConfig
    backend: DifferentialBackend
    #: 执行 iv8 侧的引擎（与契约 runner 共用同一接口）。
    iv8_engine: Any

    def run(self, cases: Iterable[ProbeCase]) -> DifferentialReport:
        """逐条比对。空清单直接产出通过报告，无需接触任何后端。"""
        frozen = tuple(cases)
        results: list[DifferentialResult] = []
        for case in frozen:
            iv8_value = self.iv8_engine.evaluate(case.script)
            chromium_value = self.backend.evaluate(case.script)
            identical = iv8_value == chromium_value
            results.append(
                DifferentialResult(
                    case_id=case.id,
                    category=case.category,
                    severity=case.severity,
                    identical=identical,
                    iv8_value=iv8_value,
                    chromium_value=chromium_value,
                    note="" if identical else "与 Chromium 结果不一致",
                )
            )
        return DifferentialReport(
            config=self.config,
            backend_name=self.backend.name,
            backend_real=self.backend.real,
            results=tuple(results),
        )


def run_empty_manifest_harness() -> DifferentialReport:
    """G0 验收用：**空清单**也必须能跑通并产出报告。"""
    config = DifferentialConfig(
        chrome_version="(未接入)",
        os_name="(未接入)",
        fonts=("(未接入)",),
        profile_fingerprint="(未接入)",
        timezone="UTC",
        locale="en-US",
    )
    return Harness(
        config=config,
        backend=NullDifferentialBackend(),
        iv8_engine=None,
    ).run(())