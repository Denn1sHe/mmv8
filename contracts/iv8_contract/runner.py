"""契约测试 runner：规范模式 / iv8 兼容模式**双 pipeline**。

## 为什么 runner 与引擎分离

G0 阶段真实的 V8 运行时（G1）尚未就位，若 runner 直接依赖引擎，
就无法在引擎缺席时验证 pipeline 本身的正确性。因此这里定义
:class:`Engine` 协议，并提供两个实现：

- :class:`StubEngine` —— **确定性**地按预置表作答，用于验证 pipeline、
  报告与阻断逻辑。它不模拟 JS 语义，只是把「某脚本应当产出某值」这件事
  显式化（G1 落地后由 `sdk-python` 提供真引擎，本类退化为测试替身）。
- 真引擎：由 :mod:`iv8_contract.engine` 在 G1 提供。

**风险提示**：StubEngine 若被误当作真引擎，会让契约测试「全绿但什么都没测」。
因此报告里会显式打印引擎标识（见 :attr:`Engine.name`），并在
:func:`ContractRunner.run_all` 中把它写入报告元数据，不可省略。
"""

from __future__ import annotations

from collections.abc import Iterable, Mapping, Sequence
from dataclasses import dataclass, field
from typing import Any, Protocol, runtime_checkable

from .cases import Mode, ProbeCase, SEVERITIES

__all__ = ["CaseResult", "ContractRunner", "Engine", "RunReport", "StubEngine"]


@runtime_checkable
class Engine(Protocol):
    """执行 JS 片段并返回可比较结果。"""

    #: 引擎标识，会原样写进报告元数据（不可省略，见模块文档）。
    name: str

    def evaluate(self, script: str) -> Any:
        """执行脚本并返回结果。抛出异常视为该用例失败。"""
        ...


@dataclass(frozen=True, slots=True)
class StubEngine:
    """按 ``script -> value`` 预置表作答的确定性引擎（G0 占位）。

    未登记的脚本会**抛异常**而不是返回 ``None``——否则「忘了登记」会被
    误判成「引擎返回了空值」，把缺失伪装成通过。
    """

    table: Mapping[str, Any]
    name: str = "stub"

    def evaluate(self, script: str) -> Any:
        try:
            return self.table[script]
        except KeyError as exc:
            raise LookupError(
                f"StubEngine 未登记该脚本，拒答（避免把「未登记」伪装成空值）：{script!r}"
            ) from exc


@dataclass(frozen=True, slots=True)
class CaseResult:
    """单条用例在某一模式下的结果。"""

    case_id: str
    mode: Mode
    category: str
    severity: str
    passed: bool
    expected: Any
    actual: Any = None
    error: str = ""

    def to_json(self) -> dict[str, Any]:
        raw: dict[str, Any] = {
            "case_id": self.case_id,
            "mode": self.mode.value,
            "category": self.category,
            "severity": self.severity,
            "passed": self.passed,
            "expected": self.expected,
        }
        raw["actual"] = self.actual
        if self.error:
            raw["error"] = self.error
        return raw


@dataclass(frozen=True, slots=True)
class RunReport:
    """一次 pipeline（单模式）的完整结果。"""

    mode: Mode
    engine: str
    results: tuple[CaseResult, ...]

    @property
    def passed(self) -> int:
        return sum(1 for r in self.results if r.passed)

    @property
    def failed(self) -> int:
        return sum(1 for r in self.results if not r.passed)

    @property
    def ok(self) -> bool:
        """整条 pipeline 是否全绿。空清单视为通过（G0 起步即空清单）。"""
        return self.failed == 0

    def failures_sorted(self) -> tuple[CaseResult, ...]:
        """按严重度降序排序的失败项（报告默认展示顺序）。"""
        order = {name: i for i, name in enumerate(SEVERITIES)}
        return tuple(
            sorted(
                (r for r in self.results if not r.passed),
                key=lambda r: (order[r.severity], r.case_id),
            )
        )

    def to_json(self) -> dict[str, Any]:
        return {
            "mode": self.mode.value,
            "mode_label": self.mode.label,
            "engine": self.engine,
            "total": len(self.results),
            "passed": self.passed,
            "failed": self.failed,
            "ok": self.ok,
            # 失败项先列，且已按严重度排序；方便直接 diff 两次运行。
            "failures": [r.to_json() for r in self.failures_sorted()],
            "results": [r.to_json() for r in self.results],
        }


@dataclass(frozen=True, slots=True)
class ContractRunner:
    """按单一模式跑一遍全部用例。"""

    cases: tuple[ProbeCase, ...]
    engine: Engine
    mode: Mode

    def run(self) -> RunReport:
        """逐条执行并比对期望值。

        引擎异常会被捕获为该用例的失败（``error`` 字段），而不是中断整条
        pipeline——否则一条坏用例会让「哪些用例还能跑」这个信息彻底丢失。
        """
        results: list[CaseResult] = []
        for case in self.cases:
            expected = case.expectation(self.mode)
            try:
                actual = self.engine.evaluate(case.script)
            except Exception as exc:  # noqa: BLE001 - 引擎侧任何异常都算该用例失败
                results.append(
                    CaseResult(
                        case_id=case.id,
                        mode=self.mode,
                        category=case.category,
                        severity=case.severity,
                        passed=False,
                        expected=expected,
                        actual=None,
                        error=f"{type(exc).__name__}: {exc}",
                    )
                )
                continue

            passed = actual == expected
            results.append(
                CaseResult(
                    case_id=case.id,
                    mode=self.mode,
                    category=case.category,
                    severity=case.severity,
                    passed=passed,
                    expected=expected,
                    actual=actual,
                )
            )

        return RunReport(
            mode=self.mode,
            engine=self.engine.name,
            results=tuple(results),
        )


def run_both_modes(
    cases: Iterable[ProbeCase], engine: Engine
) -> tuple[RunReport, RunReport]:
    """跑**两条** pipeline：规范模式与 iv8 兼容模式。

    返回顺序固定为 ``(spec, iv8)``，便于报告与 CI 直接消费。
    """
    frozen = tuple(cases)
    return (
        ContractRunner(cases=frozen, engine=engine, mode=Mode.SPEC).run(),
        ContractRunner(cases=frozen, engine=engine, mode=Mode.IV8).run(),
    )


def divergence_table(cases: Sequence[ProbeCase]) -> tuple[dict[str, Any], ...]:
    """列出「规范期望 ≠ iv8 期望」的用例。

    这是双模式 pipeline 最有价值的产出：它把「社区版在哪偏离规范」显式化，
    而这类偏离往往正是上游混淆 JS 依赖的行为，**不能当 bug 修掉**。
    """
    return tuple(
        {
            "id": c.id,
            "category": c.category,
            "severity": c.severity,
            "spec_expect": c.spec_expect,
            "iv8_expect": c.iv8_expect,
            "divergence": c.divergence,
        }
        for c in cases
        if c.diverges
    )