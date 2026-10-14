#!/usr/bin/env python3
"""`G4-S0b` 工具：**多源成员集合并**（并集 + 逐源 provenance）。

`G4-S0b` 的成员来自**两个源**（`R-3`：来源逐条可追）：

- **源 B-1**：`extract_webidl_members.py`（规范面 WebIDL，webref 快照）；
- **源 B-2**：`extract_cdp_members.py`（CDP JSON，devtools-protocol 快照）。

本工具把二者**并集**为 `community-v0.1.4-members.json`，并在 ``sources`` 数组中**逐源登记**
`source`/`source_version`/`source_sha256`/`method`（**可追性 + 可证复现**）。

## 用法

```bash
uv run python contracts/tools/merge_members.py a.json b.json --out community-v0.1.4-members.json
```
"""

from __future__ import annotations

import argparse
import json
from datetime import date
from pathlib import Path


def main(argv: list[str] | None = None) -> int:
    """合并 N 份成员 JSON ⇒ 一份并集（**任一源为空即报错**，防静默丢源）。"""
    parser = argparse.ArgumentParser(description="多源成员集合并（G4-S0b）")
    parser.add_argument("inputs", type=Path, nargs="+")
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args(argv)

    members: set[str] = set()
    sources: list[dict[str, object]] = []
    for path in args.inputs:
        payload = json.loads(path.read_text(encoding="utf-8"))
        source_members = list(payload["members"])
        if not source_members:
            raise SystemExit(f"[merge] 源 {path} 为空 ⇒ 拒绝合并（防静默丢源）")
        members.update(source_members)
        sources.append(
            {
                "source": payload["source"],
                "source_version": payload.get("source_version", ""),
                "source_sha256": payload.get("source_sha256", ""),
                "method": payload.get("method", ""),
                "members": len(source_members),
                "needs_community_review": payload.get("needs_community_review", True),
            }
        )

    merged = {
        "members": sorted(members),
        "source": "spec-webidl+cdp-protocol",
        "sources": sources,
        "extracted_at": date.today().isoformat(),
        "needs_community_review": any(bool(s["needs_community_review"]) for s in sources),
    }
    args.out.write_text(json.dumps(merged, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(
        f"[merge] 写出 {args.out}：{len(merged['members'])} 条（{len(sources)} 源；"
        f"各源 {[s['members'] for s in sources]}）"
    )
    return 0


if __name__ == "__main__":  # pragma: no cover - 由 CLI 测覆盖
    raise SystemExit(main())
