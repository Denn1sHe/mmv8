#!/usr/bin/env python3
"""`G4-S0b` 候选源 B 工具：**WebIDL 成员抽取器**（规范面 · 兜底源）。

口径见 `docs/decision-briefs/2026-10-16-G4-S0b-成员枚举计划.md` `§3`（范围收窄 + `§4` 产出格式）。

**只做解析**（**不落 seed**；落 seed 待定义源裁定）。

## 成员语义（`ADR-0027 §6`：成员 = WebIDL 语义的 `.member`）

| IDL 构造 | 计入成员 | 形式 |
|---|---|---|
| `attribute` / `readonly attribute` | ✅ | `Iface.name` |
| 方法（有名 + 返回类型） | ✅ | `Iface.name` |
| `const` | ✅ | `Iface.NAME` |
| `constructor(...)` | ❌（非成员名） | — |
| 继承（`: Base`） | ❌（**基类成员归其自身接口**） | — |

## 用法

```bash
# 单文件或目录（目录 ⇒ 递归收 *.idl/*.webidl/*.txt）
uv run python contracts/tools/extract_webidl_members.py <path> [--out FILE]

# 输出（stdout 或 --out）：
{ "members": [...], "source": "spec-webidl", "method": "<本命令>",
  "extracted_at": "…", "source_version": "<webref-commit 或 未锁定>",
  "source_sha256": "<输入内容哈希>", "needs_community_review": true }
```

**纪律**：产出**必带** `needs_community_review: true`（`R-3`：规范面**非定义源**，待社区实物复核）；
`source_sha256` = 全部输入文件内容的稳定哈希（**可证复现**）。
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import sys
from dataclasses import dataclass
from datetime import date
from pathlib import Path

#: 可识别的 IDL 文件后缀（目录模式递归收）。
IDL_SUFFIXES: tuple[str, ...] = (".idl", ".webidl", ".txt")

#: 注释（块 + 行）——先剔除，再解析。
_BLOCK_COMMENT = re.compile(r"/\*.*?\*/", re.S)
_LINE_COMMENT = re.compile(r"//[^\n]*")

#: `(partial )?interface <Name>( : <Base>)? { <body> }`（**不嵌套**；body 由花括号配平取）。
_INTERFACE_HEAD = re.compile(
    r"\b(?:partial\s+)?(?:callback\s+)?interface\s+([A-Za-z_]\w*)(?:\s*:\s*[A-Za-z_]\w*)?\s*\{"
)

#: body 内：属性（含 `readonly`；**类型允许多词**，如 `unsigned long long`）。
_ATTRIBUTE = re.compile(
    r"\b(?:readonly\s+)?attribute\s+(?:[\w.<>?\[\]]+\s+)+([A-Za-z_]\w*)\s*;"
)
#: body 内：常量（**类型允许多词**，如 `unsigned short`）。
_CONST = re.compile(r"\bconst\s+(?:[\w.<>?\[\]]+\s+)+([A-Za-z_]\w*)\s*=")
#: body 内：有名方法（**排除** `constructor`，并避开 `attribute`/`const` 误配）。
_METHOD = re.compile(r"\b([A-Za-z_]\w*)\s*\([^;{]*\)\s*;")


@dataclass(frozen=True, slots=True)
class Extraction:
    """一次抽取的结果（成员集 + 可追性元数据）。"""

    members: tuple[str, ...]
    source_sha256: str
    files: tuple[str, ...]


def _strip_comments(text: str) -> str:
    """剔除块注释与行注释（**字符串字面量内的 `//` 可忽略**——IDL 中罕见且不影响成员名）。"""
    return _LINE_COMMENT.sub("", _BLOCK_COMMENT.sub("", text))


def _interface_bodies(text: str) -> dict[str, str]:
    """抽出 `{ 接口名: body }`（花括号配平；同名的 partial 块**合并**）。"""
    bodies: dict[str, list[str]] = {}
    for head in _INTERFACE_HEAD.finditer(text):
        name = head.group(1)
        depth = 1
        index = head.end()
        while index < len(text) and depth > 0:
            char = text[index]
            if char == "{":
                depth += 1
            elif char == "}":
                depth -= 1
            index += 1
        bodies.setdefault(name, []).append(text[head.end() : index - 1])
    return {name: "\n".join(chunks) for name, chunks in bodies.items()}


def extract_members(idl_text: str) -> tuple[str, ...]:
    """从 IDL 文本抽出**成员级** id（`Iface.member`；属性/方法/常量；排序去重）。"""
    members: set[str] = set()
    for iface, body in _interface_bodies(_strip_comments(idl_text)).items():
        for match in _ATTRIBUTE.finditer(body):
            members.add(f"{iface}.{match.group(1)}")
        for match in _CONST.finditer(body):
            members.add(f"{iface}.{match.group(1)}")
        for match in _METHOD.finditer(body):
            name = match.group(1)
            # `constructor` 不是成员名；`attribute`/`const` 由上面正则各管，不会落到此分支。
            if name not in {"constructor", "attribute", "const"}:
                members.add(f"{iface}.{name}")
    return tuple(sorted(members))


def extract_path(path: Path) -> Extraction:
    """文件或目录（递归）⇒ 成员集 + 输入内容哈希。"""
    files = (
        [path]
        if path.is_file()
        else sorted(p for p in path.rglob("*") if p.suffix.lower() in IDL_SUFFIXES)
    )
    if not files:
        raise SystemExit(f"[extract] 未找到 IDL 输入：{path}")
    digest = hashlib.sha256()
    members: set[str] = set()
    names: list[str] = []
    for file in files:
        payload = file.read_bytes()
        digest.update(file.name.encode("utf-8"))
        digest.update(payload)
        members.update(extract_members(payload.decode("utf-8")))
        names.append(str(file))
    return Extraction(tuple(sorted(members)), digest.hexdigest(), tuple(names))


def build_payload(extraction: Extraction, source_version: str) -> dict[str, object]:
    """按计划 `§4` 造产出（**必带** `needs_community_review: true`）。"""
    return {
        "members": list(extraction.members),
        "source": "spec-webidl",
        "method": " ".join(sys.argv),
        "extracted_at": date.today().isoformat(),
        "source_version": source_version,
        "source_sha256": extraction.source_sha256,
        "needs_community_review": True,
    }


def main(argv: list[str] | None = None) -> int:
    """命令行入口（**只产清单，不落 seed**）。"""
    parser = argparse.ArgumentParser(description="WebIDL 成员抽取器（G4-S0b 候选源 B）")
    parser.add_argument("path", type=Path, help="IDL 文件或目录（目录 ⇒ 递归）")
    parser.add_argument("--out", type=Path, default=None, help="输出文件（默认 stdout）")
    parser.add_argument(
        "--source-version",
        default="未锁定（w3c/webref commit 待填）",
        help="规范快照版本（w3c/webref commit SHA）",
    )
    args = parser.parse_args(argv)

    payload = build_payload(extract_path(args.path), args.source_version)
    text = json.dumps(payload, ensure_ascii=False, indent=2) + "\n"
    if args.out is None:
        sys.stdout.write(text)
    else:
        args.out.write_text(text, encoding="utf-8")
        print(f"[extract] 写出 {args.out}（成员 {len(payload['members'])} 条）")
    return 0


if __name__ == "__main__":  # pragma: no cover - 由 CLI 测覆盖
    raise SystemExit(main())
