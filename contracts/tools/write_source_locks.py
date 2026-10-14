#!/usr/bin/env python3
"""`G4-S0b` 工具：由并集成员集**写逐源锁文件**（`refs/<source>-hash.txt`，≤10 行）。

口径（`ADR-0027 §6.4`）：**每源一个锁文件**（webref / cdp / webgl …），内容 = 该源的
`source_version` / `source_sha256` / `extracted_at` / `method`（+ 成员数）。
⇒ **`refs/*-hash.txt` = 锁的"统一可 grep 落点"**（复核者按 `method` 可**逐字复现**）。

## 用法

```bash
uv run python contracts/tools/write_source_locks.py community-v0.1.4-members.json --dir docs/refs
```
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path

#: 源名 ⇒ 锁文件名（**稳定 slug**；同一 `source` 多快照时以 `source_version` 前缀区分）。
def slug_for(source: str, version: str) -> str:
    """由源名 + 版本造锁文件 slug（如 ``webref`` / ``cdp`` / ``webgl``）。"""
    lowered = f"{source} {version}".lower()
    for key, slug in (
        ("webref", "webref"),
        ("cdp", "cdp"),
        ("khronos", "webgl"),
        ("webgl", "webgl"),
    ):
        if key in lowered:
            return slug
    return source.replace("/", "-").replace(" ", "-").lower()


def main(argv: list[str] | None = None) -> int:
    """读并集成员集 ⇒ 逐源写锁文件（**每源 ≤10 行**）。"""
    parser = argparse.ArgumentParser(description="写逐源锁文件（G4-S0b）")
    parser.add_argument("members", type=Path, help="并集成员集 JSON（含 sources 数组）")
    parser.add_argument("--dir", type=Path, required=True, help="锁文件目录（如 docs/refs）")
    args = parser.parse_args(argv)

    payload = json.loads(args.members.read_text(encoding="utf-8"))
    sources = payload.get("sources") or []
    if not sources:
        raise SystemExit("[locks] 成员集无 sources 数组 ⇒ 拒绝（无法定锁）")

    args.dir.mkdir(parents=True, exist_ok=True)
    written: list[str] = []
    for source in sources:
        slug = slug_for(str(source["source"]), str(source.get("source_version", "")))
        text = (
            f"# G4-S0b 源锁 · {source['source']}\n"
            f"source_version = {source['source_version']}\n"
            f"source_sha256  = {source['source_sha256']}\n"
            f"extracted_at   = {payload.get('extracted_at', '')}\n"
            f"members        = {source['members']}\n"
            f"needs_community_review = {str(source.get('needs_community_review', True)).lower()}\n"
            f"method         = {source['method']}\n"
            f"# 复核：按 method 重跑 ⇒ 应得同 source_sha256 与同 members 数（可证复现）。\n"
        )
        path = args.dir / f"{slug}-hash.txt"
        path.write_text(text, encoding="utf-8")
        written.append(str(path))
    print(f"[locks] 写出 {len(written)} 个锁文件：{[Path(w).name for w in written]}")
    return 0


if __name__ == "__main__":  # pragma: no cover - 由 CLI 测覆盖
    raise SystemExit(main())
