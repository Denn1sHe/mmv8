#!/usr/bin/env python3
"""`G4` F7：**manifest 条目流转**（`missing` ⇒ `verified` 等）+ **测试名真实性校验**。

## 为什么单独一个工具（而不是手改 JSON）

1. **机器生成物禁手改**（`ADR-0027 §8`：要改 = 改生成器）；
2. **假流转防护**：`verified` 的 `test_ids` 必须指向**真实存在的测试** ——
   在 `crates/**/*.rs`（`fn <name>`）与 `contracts/tests/*.py`（`def test_<name>`）里查证，
   **找不到即拒写**。这是「声明态 ≠ 实物态」家族在 manifest 面的变体：
   若允许写不存在的测试名，`verified` 就是一张空头支票（观测量不变 = 空门禁）。

## 用法

```bash
uv run python contracts/tools/promote_manifest.py \
  --id Storage.getItem --id Storage.setItem \
  --status verified --behavior-level behaviour \
  --test-id storage_roundtrip_and_key_order \
  --test-id storage_surface_roundtrip_order_and_quota \
  --evidence "S1-a：读写回环 + 插入序 + ToString 归一 + per-bucket 配额无半态"
```
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
MANIFEST = REPO_ROOT / "contracts" / "compat-manifest.json"

#: 合法 `status`（与 `iv8_contract.manifest` 的 `EntryStatus` 一致）。
VALID_STATUS = {"verified", "failed", "missing", "unknown"}
#: 合法 `behavior_level`（三态；ADR-0027 §6）。
VALID_LEVEL = {"stub", "contract", "behaviour"}

_RUST_TEST = r"\bfn\s+{name}\s*\("
_PY_TEST = r"\bdef\s+test_{name}\s*\("


def _test_sources() -> list[Path]:
    """测试证据的搜索面：Rust 产品码（`#[cfg(test)]` 与集成测试）+ 契约 pytest。"""
    sources = sorted((REPO_ROOT / "crates").rglob("*.rs"))
    sources += sorted((REPO_ROOT / "contracts" / "tests").glob("test_*.py"))
    return sources


def _test_exists(name: str, sources: list[Path]) -> bool:
    """测试名是否真实存在（Rust `fn <name>(` 或 pytest `def test_<name>(`）。"""
    rust = _RUST_TEST.format(name=re.escape(name))
    python = _PY_TEST.format(name=re.escape(name))
    for path in sources:
        try:
            text = path.read_text(encoding="utf-8")
        except OSError:  # pragma: no cover - 不可读文件跳过
            continue
        if re.search(rust, text) or re.search(python, text):
            return True
    return False


def main(argv: list[str] | None = None) -> int:
    """按 id 流转 manifest 条目（**测试名不真实 ⇒ 拒写**）。"""
    parser = argparse.ArgumentParser(description="manifest 条目流转（G4 F7）")
    parser.add_argument("--id", action="append", required=True, help="成员 id（可多次）")
    parser.add_argument("--status", required=True, choices=sorted(VALID_STATUS))
    parser.add_argument("--behavior-level", required=True, choices=sorted(VALID_LEVEL))
    parser.add_argument("--test-id", action="append", default=[], help="真实测试名（可多次）")
    parser.add_argument("--evidence", required=True, help="证据描述（具名、可复核）")
    args = parser.parse_args(argv)

    if args.status == "verified" and not args.test_id:
        raise SystemExit(
            "[promote] `verified` 必须附**真实测试名**（无 test_ids 的 verified = 空头支票）"
        )

    # **假流转防护**：先全网查证，再决定是否落笔。
    sources = _test_sources()
    unknown = [name for name in args.test_id if not _test_exists(name, sources)]
    if unknown:
        raise SystemExit(
            f"[promote] 拒写：测试名不存在 {unknown}（在 crates/**/*.rs 与 "
            "contracts/tests/test_*.py 中均未找到 `fn <name>(` / `def test_<name>(`）"
            "⇒ **不得假流转**"
        )

    manifest = json.loads(MANIFEST.read_text(encoding="utf-8"))
    by_id = {str(e["id"]): e for e in manifest["entries"]}
    missing = [i for i in args.id if i not in by_id]
    if missing:
        raise SystemExit(f"[promote] 拒写：id 不在分母中 {missing}（增删分母须 ADR 显式）")

    for member_id in args.id:
        entry = by_id[member_id]
        entry["status"] = args.status
        entry["behavior_level"] = args.behavior_level
        entry["test_ids"] = list(args.test_id)
        entry["evidence"] = args.evidence
    MANIFEST.write_text(
        json.dumps(manifest, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
    )
    print(
        f"[promote] 流转 {len(args.id)} 条 ⇒ status={args.status} / "
        f"behavior_level={args.behavior_level} / test_ids={args.test_id}"
    )
    return 0


if __name__ == "__main__":  # pragma: no cover - 由 CLI 测覆盖
    try:
        raise SystemExit(main())
    except SystemExit as exc:
        if exc.code not in (0, None):
            print(f"[promote] {exc.code}", file=sys.stderr)
        raise
