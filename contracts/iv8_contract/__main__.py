"""命令行入口。

```bash
cd contracts
uv run python -m iv8_contract contract     # 跑双 pipeline（规范 / iv8 兼容）
uv run python -m iv8_contract manifest     # 打印 manifest 统计与阻断项
uv run python -m iv8_contract differential # 跑差分（空清单可跑通）
```

G0 阶段引擎是 :class:`~iv8_contract.runner.StubEngine`，其结果**不代表**真实运行时；
报告里的 ``engine`` 字段会如实打印，禁止把它当成兼容性结论。
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

from .cases import load_cases
from .differential import run_empty_manifest_harness
from .manifest import load as load_manifest
from .runner import StubEngine, divergence_table, run_both_modes

#: 仓库内契约资产根目录。
ROOT = Path(__file__).resolve().parent.parent
CASES_DIR = ROOT / "cases"
MANIFEST_PATH = ROOT / "compat-manifest.json"


def _stub_engine(cases: tuple) -> StubEngine:
    """按用例构造确定性替身：每个脚本返回其规范期望值。

    这样**规范模式**在 G0 阶段就是绿的（pipeline 自身无缺陷），
    而 **iv8 模式**会把双期望不一致的用例判为失败——正好把「社区版偏离规范」
    这件事显式暴露出来。
    """
    return StubEngine({c.script: c.spec_expect for c in cases})


def cmd_contract(_: argparse.Namespace) -> int:
    cases = load_cases(CASES_DIR)
    if not cases:
        print("用例清单为空（G0 起步状态）。先在 contracts/cases/ 添加 *.yaml。")
    spec, iv8 = run_both_modes(cases, _stub_engine(cases))

    print(f"规范模式：{spec.passed}/{len(spec.results)} 通过，engine={spec.engine}")
    for r in spec.failures_sorted():
        print(f"  [FAIL/{r.severity}] {r.case_id}: 期望 {r.expected!r} 实得 {r.actual!r} {r.error}")

    print(f"\niv8 兼容模式：{iv8.passed}/{len(iv8.results)} 通过，engine={iv8.engine}")
    for r in iv8.failures_sorted():
        print(f"  [FAIL/{r.severity}] {r.case_id}: 期望 {r.expected!r} 实得 {r.actual!r}")

    table = divergence_table(cases)
    print(f"\n双期望不一致（社区版偏离规范）用例：{len(table)} 条")
    for row in table:
        print(f"  - {row['id']} ({row['severity']}): 规范={row['spec_expect']!r} "
              f"iv8={row['iv8_expect']!r}")

    ok = spec.ok and iv8.ok
    print(f"\n结论：{'通过' if ok else '未通过'}")
    return 0 if ok else 1


def cmd_manifest(_: argparse.Namespace) -> int:
    if not MANIFEST_PATH.exists():
        print(f"manifest 不存在：{MANIFEST_PATH}")
        return 1
    manifest = load_manifest(MANIFEST_PATH)
    print(json.dumps(manifest.counts(), ensure_ascii=False, indent=2))
    blocking = manifest.blocking_entries()
    print(f"条目 {len(manifest)} 条，阻断 {len(blocking)} 条")
    for e in blocking:
        print(f"  [{e.status.value}] {e.id} {e.note}")
    return 0 if not blocking else 1


def cmd_differential(_: argparse.Namespace) -> int:
    cases = load_cases(CASES_DIR)
    if not cases:
        report = run_empty_manifest_harness()
    else:
        print("非空清单需注入真实 Chromium 后端（G1 接入 CDP），当前仅支持空清单跑通。")
        return 1
    sys.stdout.write(report.to_text())
    return 0 if report.ok else 1


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(prog="iv8_contract")
    sub = parser.add_subparsers(dest="cmd", required=True)
    sub.add_parser("contract", help="跑双 pipeline").set_defaults(func=cmd_contract)
    sub.add_parser("manifest", help="打印 manifest 统计").set_defaults(func=cmd_manifest)
    sub.add_parser("differential", help="跑差分").set_defaults(func=cmd_differential)
    args = parser.parse_args(argv)
    return int(args.func(args))


if __name__ == "__main__":
    raise SystemExit(main())