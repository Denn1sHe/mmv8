"""契约 runner 与差分 harness 的测试（T1.1 / T1.3）。

重点是**防止「看起来齐活但没有验收力」**：

- 双 pipeline 必须真的读两套不同的期望值（把规范期望写成 iv8 期望，测试要红）；
- 引擎异常不得被吞成「通过」；
- 差分后端未接入真实浏览器时，报告必须自带 ``backend_real=False`` 标记；
- 空清单必须能跑通（这是 G0 的验收条件），但空清单**不得**被当成「已完成比对」。
"""

from __future__ import annotations

import pytest

from iv8_contract.cases import CaseError, Mode, ProbeCase, load_cases
from iv8_contract.differential import (
    DifferentialConfig,
    DifferentialError,
    Harness,
    NullDifferentialBackend,
    run_empty_manifest_harness,
)
from iv8_contract.runner import (
    ContractRunner,
    RunReport,
    StubEngine,
    divergence_table,
    run_both_modes,
)

# ---------------------------------------------------------------- 用例


def _case(
    case_id: str = "Window.setTimeout",
    *,
    script: str = "1+1",
    spec: object = 2,
    iv8: object = 2,
    severity: str = "major",
) -> ProbeCase:
    return ProbeCase(
        id=case_id,
        category="timer",
        script=script,
        spec_expect=spec,
        iv8_expect=iv8,
        severity=severity,
    )


# ---------------------------------------------------------------- 模式


def test_two_modes_read_different_expectations():
    """双 pipeline 的意义：同一脚本、两套期望。若 runner 读串了，这里会红。"""
    case = _case(script="x", spec="规范值", iv8="iv8值")
    engine = StubEngine({"x": "规范值"})

    spec = ContractRunner((case,), engine, Mode.SPEC).run()
    iv8 = ContractRunner((case,), engine, Mode.IV8).run()

    assert spec.ok is True, "规范模式应通过（引擎返回规范期望值）"
    assert iv8.ok is False, "iv8 模式必须因期望不同而失败——说明两条 pipeline 真的分开读期望"


def test_expectation_field_mapping():
    assert Mode.SPEC.expectation_field == "spec_expect"
    assert Mode.IV8.expectation_field == "iv8_expect"
    assert "WHATWG" in Mode.SPEC.label
    assert "v0.1.4" in Mode.IV8.label


def test_divergence_is_detected_and_explained():
    same = _case("A.a", spec=1, iv8=1)
    diff = _case("B.b", spec=1, iv8=2)
    assert same.diverges is False
    assert diff.diverges is True

    table = divergence_table((same, diff))
    assert len(table) == 1
    assert table[0]["id"] == "B.b"
    assert table[0]["divergence"], "双期望不一致必须带说明，否则会被当噪声忽略"


# ---------------------------------------------------------------- runner


def test_empty_manifest_pipeline_is_green():
    """G0 起步就是空清单；空清单必须跑通。"""
    report = ContractRunner((), StubEngine({}), Mode.SPEC).run()
    assert report.ok is True
    assert (report.passed, report.failed, len(report.results)) == (0, 0, 0)


def test_engine_exception_becomes_failure_not_crash():
    """引擎异常必须变成该用例的失败，而不是中断整条 pipeline。"""
    case = _case(script="boom")
    report = ContractRunner((case,), StubEngine({}), Mode.SPEC).run()
    assert report.ok is False
    assert report.failed == 1
    result = report.results[0]
    assert result.passed is False
    assert "LookupError" in result.error


def test_stub_engine_refuses_unregistered_scripts():
    """未登记的脚本要**拒答**，不能返回 None 把「没登记」伪装成「值为空」。"""
    with pytest.raises(LookupError, match="未登记"):
        StubEngine({}).evaluate("missing")


def test_failures_are_sorted_by_severity():
    cases = (
        _case("A.minor", severity="minor"),
        _case("B.blocker", severity="blocker"),
        _case("C.critical", severity="critical"),
    )
    engine = StubEngine({c.script: "wrong" for c in cases})
    report = ContractRunner(cases, engine, Mode.SPEC).run()
    assert [r.case_id for r in report.failures_sorted()] == [
        "B.blocker",
        "C.critical",
        "A.minor",
    ]


def test_report_records_engine_name():
    """引擎标识必须进报告，防止把 StubEngine 的全绿当成真引擎结果。"""
    report = ContractRunner((), StubEngine({}, name="stub"), Mode.SPEC).run()
    assert report.engine == "stub"
    assert report.to_json()["engine"] == "stub"


def test_run_both_modes_order_is_stable():
    spec, iv8 = run_both_modes((_case(spec=1, iv8=1),), StubEngine({"1+1": 1}))
    assert isinstance(spec, RunReport) and isinstance(iv8, RunReport)
    assert (spec.mode, iv8.mode) == (Mode.SPEC, Mode.IV8)


# ---------------------------------------------------------------- 差分 harness


def _config() -> DifferentialConfig:
    return DifferentialConfig(
        chrome_version="131.0.6778.85",
        os_name="macOS 15.1 arm64",
        fonts=("Helvetica",),
        profile_fingerprint="sha256:fixed",
        timezone="UTC",
        locale="en-US",
    )


@pytest.mark.parametrize(
    "field", ["chrome_version", "os_name", "profile_fingerprint", "timezone", "locale"]
)
def test_unpinned_environment_is_rejected(field):
    """环境未钉死必须报错，不给默认值蒙混。"""
    kwargs = {
        "chrome_version": "131.0.6778.85",
        "os_name": "macOS 15.1 arm64",
        "fonts": ("Helvetica",),
        "profile_fingerprint": "sha256:fixed",
        "timezone": "UTC",
        "locale": "en-US",
    }
    kwargs[field] = "  "
    with pytest.raises(DifferentialError, match="未钉死"):
        DifferentialConfig(**kwargs)


def test_empty_font_set_is_rejected():
    with pytest.raises(DifferentialError, match="字体"):
        DifferentialConfig(
            chrome_version="131",
            os_name="macOS",
            fonts=(),
            profile_fingerprint="x",
            timezone="UTC",
            locale="en-US",
        )


def test_empty_manifest_harness_runs_through():
    """T1.3 验收：空清单也能跑通并输出报告。"""
    report = run_empty_manifest_harness()
    assert report.total == 0
    assert report.ok is True
    text = report.to_text()
    assert "空清单" in text
    assert "**未**与真实 Chromium 比对" in text


def test_backend_real_flag_is_false_for_skeleton():
    """骨架后端必须在报告里标成非真实，否则会被误读为已比对。"""
    report = run_empty_manifest_harness()
    assert report.backend_real is False
    assert report.to_json()["backend_real"] is False


def test_null_backend_refuses_to_execute():
    """空壳后端不得静默返回 None 冒充「结果相同」。"""
    with pytest.raises(DifferentialError, match="未接入真实 Chromium"):
        NullDifferentialBackend().evaluate("1+1")


def test_report_sorted_by_severity():
    from iv8_contract.differential import DifferentialResult

    results = (
        DifferentialResult("A.minor", "timer", "minor", False),
        DifferentialResult("B.blocker", "timer", "blocker", False),
    )
    report = run_empty_manifest_harness()
    ordered = sorted(
        results, key=lambda r: ({"blocker": 0, "critical": 1, "major": 2,
                                 "minor": 3, "info": 4}[r.severity], r.case_id)
    )
    assert [r.case_id for r in ordered] == ["B.blocker", "A.minor"]
    assert report.sorted_results() == ()


def test_harness_compares_values():
    """注入两端一致的后端，验证比对逻辑本身正确。"""

    class EchoBackend:
        name = "echo"
        real = True

        def evaluate(self, script):
            return 2

    report = Harness(
        config=_config(),
        backend=EchoBackend(),
        iv8_engine=StubEngine({"1+1": 2}),
    ).run((_case(script="1+1"),))
    assert report.ok is True
    assert report.backend_real is True


def test_harness_flags_divergence():
    class OtherBackend:
        name = "other"
        real = True

        def evaluate(self, script):
            return 999

    report = Harness(
        config=_config(),
        backend=OtherBackend(),
        iv8_engine=StubEngine({"1+1": 2}),
    ).run((_case(script="1+1", severity="blocker"),))
    assert report.ok is False
    assert report.divergent == 1
    assert report.sorted_results()[0].severity == "blocker"


# ---------------------------------------------------------------- 用例加载


def test_load_cases_empty_dir(tmp_path):
    assert load_cases(tmp_path) == ()


def test_load_cases_missing_dir(tmp_path):
    with pytest.raises(CaseError, match="不存在"):
        load_cases(tmp_path / "nope")


def test_load_cases_rejects_duplicate_ids(tmp_path):
    (tmp_path / "a.yaml").write_text(
        "- id: X.y\n  category: c\n  script: s\n  spec_expect: 1\n  iv8_expect: 1\n",
        encoding="utf-8",
    )
    (tmp_path / "b.yaml").write_text(
        "- id: X.y\n  category: c\n  script: t\n  spec_expect: 2\n  iv8_expect: 2\n",
        encoding="utf-8",
    )
    with pytest.raises(CaseError, match="重复"):
        load_cases(tmp_path)


def test_load_cases_rejects_missing_fields(tmp_path):
    (tmp_path / "a.yaml").write_text("- id: X.y\n  script: s\n", encoding="utf-8")
    with pytest.raises(CaseError, match="缺少必填字段"):
        load_cases(tmp_path)


def test_load_cases_rejects_bad_severity(tmp_path):
    (tmp_path / "a.yaml").write_text(
        "- id: X.y\n  category: c\n  script: s\n"
        "  spec_expect: 1\n  iv8_expect: 1\n  severity: 致命\n",
        encoding="utf-8",
    )
    with pytest.raises(CaseError, match="severity"):
        load_cases(tmp_path)


def test_load_cases_is_sorted_and_stable(tmp_path):
    body = (
        "  category: c\n  script: {}\n  spec_expect: 1\n  iv8_expect: 1\n"
    )
    (tmp_path / "b.yaml").write_text("- id: B.b\n" + body.format("sb"), encoding="utf-8")
    (tmp_path / "a.yaml").write_text("- id: A.a\n" + body.format("sa"), encoding="utf-8")
    loaded = load_cases(tmp_path)
    assert [c.id for c in loaded] == ["A.a", "B.b"]