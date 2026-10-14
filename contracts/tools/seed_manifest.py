#!/usr/bin/env python3
"""`G4-S0b` 种子生成器：`community` 成员集 ⇒ **九字段 manifest 种子**（可复现）。

口径（`ADR-0027 §6` + 计划 `§4`）：

- **种子宁全勿缺**：`members` 里**每个成员一条** ⇒ `status` 起始恒为 ``missing``
  （**未达者显名 `missing`**；**禁止**"不在清单里"）；
- `behavior_level` 起始恒为 ``stub``（**面级**：仅有接口面存在性证据；**不得附行为标记**）；
- `test_ids` 起步 = **具名占位** ``("g4-seed:pending",)``（**必填**；后续按 `cases/` 探针逐条替换）；
- `implementation_owner` 按**族归属**（`§6.2` 六行）取；
- `descriptor` = 从成员名派生的 **WebIDL 构造类**（`attribute`/`method`/`constant` 三选一，
  由抽取器的规则反推：常量名**全大写** ⇒ `constant`；`IDB*`/`URL` 等小写成员 ⇒ `method`；
  其余 ⇒ `attribute`）——**仅作起步分类**，后续按实测证据改判。

## 用法

```bash
uv run python contracts/tools/seed_manifest.py <members.json> --out contracts/compat-manifest.json
```
"""

from __future__ import annotations

import argparse
import json
import sys
from datetime import date
from pathlib import Path

#: 族归属（`ADR-0027 §6.2` 六行）——按接口名前缀匹配（**顺序敏感，先具体后一般**）。
FAMILY_OWNERS: tuple[tuple[tuple[str, ...], str], ...] = (
    (("IDB", "Storage", "Cache", "CookieStore"), "web-dom"),  # 存储
    (("Crypto", "SubtleCrypto", "Algorithm", "Permission"), "engine-v8"),  # 密码 / 安全与权限
    (
        ("Canvas", "OffscreenCanvas", "WebGL", "GPU", "Audio", "Media", "WebSocket"),
        "graphics",
    ),  # 图形媒体
    (("TextEncoder", "TextDecoder", "Blob", "File", "FileReader", "URL", "ReadableStream",
      "WritableStream", "TransformStream"), "bindings"),  # 编码文件（含 ⑤ Streams 归属判）
    (("Geolocation", "Notification", "Clipboard"), "input"),  # 设备
)

#: `test_ids` 起步占位（**必填**；后续按 `cases/` 探针逐条替换为真实 `test_id`）。
SEED_TEST_ID = "g4-seed:pending"

#: 起始版本（社区版冻结目标）。
TARGET_VERSION = "v0.1.4"


def owner_for(member_id: str) -> str:
    """按族归属定 `implementation_owner`（`§6.2`；未命中 ⇒ `uncategorized`，**显名不猜**）。"""
    iface = member_id.split(".", 1)[0]
    for prefixes, owner in FAMILY_OWNERS:
        if iface.startswith(prefixes):
            return owner
    return "uncategorized"


def descriptor_for(member: str) -> str:
    """起步 `descriptor`（由成员名反推；**仅起步**，后续按实测改判）。"""
    name = member.split(".", 1)[1]
    if name.isupper() or (name.isalnum() and name.upper() == name and not name.isdigit()):
        return "constant"
    return "attribute"  # 方法/属性**由抽取器同列**，此处不臆断细别 ⇒ 起步取 attribute 面
    # 注：`method` 与 `attribute` 的细分需**行为证据**（属 `behaviour`），故起步不冒充。


def build_entry(member: str) -> dict[str, object]:
    """造一条**九字段齐**的种子条目（`status=missing`，`behavior_level=stub`）。"""
    return {
        "target_version": TARGET_VERSION,
        "id": member,
        "realm": "main",
        "descriptor": descriptor_for(member),
        "behavior_level": "stub",
        "test_ids": [SEED_TEST_ID],
        "status": "missing",
        "evidence": f"face: {member.split('.', 1)[0]} 见 webref IDL（面级存在性；行为取证待 S1~S5）",
        "implementation_owner": owner_for(member),
    }


def main(argv: list[str] | None = None) -> int:
    """读成员集 ⇒ 写九字段 manifest 种子（**denominator_fixed 保持 false**，待核可）。"""
    parser = argparse.ArgumentParser(description="G4-S0b 种子生成器（成员集 ⇒ manifest 种子）")
    parser.add_argument("members", type=Path, help="community 成员集 JSON（计划 §4 格式）")
    parser.add_argument("--out", type=Path, required=True, help="输出 compat-manifest.json")
    parser.add_argument("--version", type=int, default=1)
    args = parser.parse_args(argv)

    payload = json.loads(args.members.read_text(encoding="utf-8"))
    members = payload["members"]
    if not members:
        raise SystemExit("[seed] 成员集为空 ⇒ 拒绝生成（防空表假通过）")

    manifest = {
        "version": args.version,
        # `M0c`（`denominator_fixed: true`）**须经核可**后另笔设置（本生成器不动它）。
        "denominator_fixed": False,
        "entries": [build_entry(member) for member in members],
    }
    args.out.write_text(
        json.dumps(manifest, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
    )
    print(
        f"[seed] 写出 {args.out}：{len(manifest['entries'])} 条（status=missing；"
        f"behavior_level=stub；source={payload.get('source')}）"
    )
    return 0


if __name__ == "__main__":  # pragma: no cover - 由 CLI 测覆盖
    raise SystemExit(main())
