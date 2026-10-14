#!/usr/bin/env python3
"""`G4-S0b` ③-(2)：**共享前缀报告**（同名前缀跨源透明化）。

成员 id 形如 `<Interface>.<member>`；**不同源可能共用前缀**（如 Web IDL 的 `Storage.*`
与 CDP 域的 `Storage.*`）⇒ 本报告**显名列出**共享前缀，使未来真碰撞**即现形**
（`refs/shared-prefix-report.md`）。

## 用法

```bash
uv run python contracts/tools/shared_prefix_report.py community-v0.1.4-members.json \\
    --out docs/refs/shared-prefix-report.md
```
"""

from __future__ import annotations

import argparse
import json
from collections import defaultdict
from datetime import date
from pathlib import Path


def main(argv: list[str] | None = None) -> int:
    """读并集成员集 ⇒ 写共享前缀报告（**跨源**前缀 + 各源计数）。"""
    parser = argparse.ArgumentParser(description="共享前缀报告（G4-S0b ③）")
    parser.add_argument("members", type=Path)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args(argv)

    payload = json.loads(args.members.read_text(encoding="utf-8"))
    provenance: dict[str, str] = payload.get("member_sources") or {}
    per_prefix: dict[str, dict[str, int]] = defaultdict(lambda: defaultdict(int))
    for member, slug in provenance.items():
        per_prefix[member.split(".", 1)[0]][slug] += 1
    shared = {p: dict(v) for p, v in per_prefix.items() if len(v) > 1}

    lines = [
        "# 共享前缀报告（`G4-S0b` ③-(2)）",
        "",
        f"- 生成日：{date.today().isoformat()}；源：`community-v0.1.4-members.json`（{len(provenance)} 条）",
        "- **口径**：成员 id 为 `<Interface>.<member>`；**不同源可共用前缀**（如 `Storage.`）——",
        "  本表**显名**之，使未来**真 id 碰撞**（同一 id 两源）即现形（merge 已带冲突守卫 ⇒ 直接报错）。",
        "",
        "## 跨源共享前缀",
        "",
    ]
    if shared:
        lines += ["| 前缀 | 各源成员数 |", "|---|---|"]
        for prefix, counts in sorted(shared.items()):
            rendered = " ｜ ".join(f"`{slug}`: {n}" for slug, n in sorted(counts.items()))
            lines.append(f"| `{prefix}.` | {rendered} |")
    else:
        lines.append("（无：当前无跨源共享前缀）")
    lines += [
        "",
        "## 结论",
        "",
        f"- 跨源共享前缀数：**{len(shared)}**（**id 级碰撞 = merge 阶段硬报错**，不静默去重）。",
        "- ADR 约定：**同名前缀双源**时，`category`（来源族）随条目承载，供两侧判定归属。",
        "",
    ]
    args.out.write_text("\n".join(lines), encoding="utf-8")
    print(f"[prefix] 写出 {args.out}：共享前缀 {len(shared)} 个 {sorted(shared)}")
    return 0


if __name__ == "__main__":  # pragma: no cover - 由 CLI 测覆盖
    raise SystemExit(main())
