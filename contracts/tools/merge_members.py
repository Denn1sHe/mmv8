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
    provenance: dict[str, str] = {}
    member_versions: dict[str, str] = {}
    descriptors: dict[str, str] = {}
    sources: list[dict[str, object]] = []
    for path in args.inputs:
        payload = json.loads(path.read_text(encoding="utf-8"))
        source_members = list(payload["members"])
        if not source_members:
            raise SystemExit(f"[merge] 源 {path} 为空 ⇒ 拒绝合并（防静默丢源）")
        # **续接已有 per-member provenance**（输入可能是已合并文件）；否则用本源的 `source`。
        incoming: dict[str, str] = payload.get("member_sources") or {
            member: str(payload.get("source", "unknown")) for member in source_members
        }
        # **续接已有 per-member 来源版本**（`G4` F2：判据必须是**不可归并键**）。
        # 曾用 `source`（slug）判跨源冲突 ⇒ **同 slug 跨源恒假**（webref 与 khronos 共用
        # `spec-webidl`）⇒ 静默去重。**slug 可归并、版本不可归并** ⇒ 以 `source_version` 判。
        incoming_versions: dict[str, str] = payload.get("member_source_versions") or {
            member: str(payload.get("source_version", "")) for member in source_members
        }
        for member in source_members:
            slug = incoming.get(member, str(payload.get("source", "unknown")))
            version = incoming_versions.get(member, str(payload.get("source_version", "")))
            # **跨源 id 冲突 ⇒ 报错**（防"集合静默去重"吞掉一条；`G4-S0b` ③-(1)）。
            if member in provenance and provenance[member] != slug:
                raise SystemExit(
                    f"[merge] id 跨源冲突：{member} 同时来自 {provenance[member]} 与 {slug}"
                    "（**不得静默去重**——先裁决命名）"
                )
            # **同 slug 不同版本 ⇒ 也是跨源冲突**（F2 的真正判据：slug 会撞，版本不会）。
            if member in member_versions and member_versions[member] != version:
                raise SystemExit(
                    f"[merge] id 跨源冲突：{member} 同时来自版本 {member_versions[member]} 与 {version}"
                    f"（slug 均为 {slug!r} —— **slug 可归并，冲突判据必须以 source_version 为准**）"
                )
            provenance[member] = slug
            member_versions[member] = version
        descriptors.update(payload.get("member_descriptors") or {})
        members.update(source_members)
        # **续接已有 `sources`**（输入本身可能是已合并文件）——否则 provenance 被**展平**（源数静默减少）。
        if payload.get("sources"):
            sources.extend(payload["sources"])
        else:
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
        "member_sources": provenance,
        "member_source_versions": member_versions,
        "member_descriptors": descriptors,
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
