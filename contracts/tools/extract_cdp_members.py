#!/usr/bin/env python3
"""`G4-S0b` 候选源 B-2 工具：**CDP 域成员抽取器**（宿主 CDP 族 · `ADR-0027 §6.2` 第六行）。

**为何另源**：CDP（Chrome DevTools Protocol）是 **JSON 协议**，**非 WebIDL** ⇒ WebIDL 抽取器
（`extract_webidl_members.py`）**不可覆盖**本族（裁定 `⑤-1` ⇒ **另源立项**）。

## 成员语义

| JSON 构造 | 计入成员 | 形式 |
|---|---|---|
| `domain.commands[].name` | ✅ | `<Domain>.<name>` |
| `domain.events[].name` | ✅ | `<Domain>.<name>` |
| `domain.types[]`（含 `parameters`/`properties`） | ❌（**类型非可调用面**；保守不入分母） | — |

**域范围**（`§6.2` 具名）：`Runtime` / `Debugger` / `DOM` / `CSS` / `Network` / `Page` / `Storage` /
`DOMStorage` / `IndexedDB` / `Target`（**不外扩**；CDP 共 50+ 域，超出者不取）。

## 用法

```bash
uv run python contracts/tools/extract_cdp_members.py <json…> --out FILE --source-version <commit>
```

输入可为 `browser_protocol.json`（DOM/CSS/Network/…）+ `js_protocol.json`（Runtime/Debugger）；
产出格式同计划 `§4`（**必带** `needs_community_review: true`）。
"""

from __future__ import annotations

import argparse
import hashlib
import json
import sys
from datetime import date
from pathlib import Path

#: 进分母的 CDP 域（`ADR-0027 §6.2` 第六行具名；**不外扩**）。
CDP_DOMAINS: frozenset[str] = frozenset(
    {
        "Runtime",
        "Debugger",
        "DOM",
        "CSS",
        "Network",
        "Page",
        "Storage",
        "DOMStorage",
        "IndexedDB",
        "Target",
    }
)


def extract_descriptors(payload: dict[str, object]) -> dict[str, str]:
    """抽 **成员 ⇒ descriptor**（CDP 映射：`command → operation`、`event → event`）。"""
    found: dict[str, str] = {}
    for domain in payload.get("domains", []):  # type: ignore[union-attr]
        name = str(domain.get("domain", ""))
        if name not in CDP_DOMAINS:
            continue
        for item in domain.get("commands", []) or []:
            found[f"{name}.{item.get('name', '')}"] = "operation"
        for item in domain.get("events", []) or []:
            found[f"{name}.{item.get('name', '')}"] = "event"
    return {k: v for k, v in found.items() if not k.endswith(".")}


def extract_members(payload: dict[str, object]) -> set[str]:
    """从一份 CDP 协议 JSON 抽 **域成员**（`<Domain>.<command|event>`）；域按:data:`CDP_DOMAINS` 收窄。"""
    members: set[str] = set()
    for domain in payload.get("domains", []):  # type: ignore[union-attr]
        name = str(domain.get("domain", ""))
        if name not in CDP_DOMAINS:
            continue
        for kind in ("commands", "events"):
            for item in domain.get(kind, []) or []:
                member = str(item.get("name", ""))
                if member:
                    members.add(f"{name}.{member}")
    return members


def build_payload(
    members: set[str],
    files: list[Path],
    source_version: str,
    descriptors: dict[str, str] | None = None,
) -> dict[str, object]:
    """造产出（计划 `§4` 格式；`members` 非空校验在 `main`）。"""
    digest = hashlib.sha256()
    for file in files:
        digest.update(file.name.encode("utf-8"))
        digest.update(file.read_bytes())
    return {
        "members": sorted(members),
        "member_descriptors": descriptors or {},
        "source": "cdp-protocol",
        "method": " ".join(sys.argv),
        "extracted_at": date.today().isoformat(),
        "source_version": source_version,
        "source_sha256": digest.hexdigest(),
        "needs_community_review": True,
    }


def main(argv: list[str] | None = None) -> int:
    """命令行入口（**只产清单**）。"""
    parser = argparse.ArgumentParser(description="CDP 域成员抽取器（G4-S0b 候选源 B-2）")
    parser.add_argument("json_files", type=Path, nargs="+", help="CDP 协议 JSON（browser/js）")
    parser.add_argument("--out", type=Path, required=True, help="输出 JSON")
    parser.add_argument("--source-version", required=True, help="devtools-protocol commit SHA")
    args = parser.parse_args(argv)

    members: set[str] = set()
    descriptors: dict[str, str] = {}
    for file in args.json_files:
        payload = json.loads(file.read_text(encoding="utf-8"))
        members |= extract_members(payload)
        descriptors.update(extract_descriptors(payload))
    if not members:
        raise SystemExit("[cdp] 未抽到任何成员 ⇒ 拒绝产出（防空清单假通过）")

    payload = build_payload(members, list(args.json_files), args.source_version, descriptors)
    args.out.write_text(
        json.dumps(payload, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
    )
    print(f"[cdp] 写出 {args.out}：{len(payload['members'])} 条成员")
    return 0


if __name__ == "__main__":  # pragma: no cover - 由 CLI 测覆盖
    raise SystemExit(main())
