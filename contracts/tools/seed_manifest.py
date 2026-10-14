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
    (("Crypto", "SubtleCrypto", "Algorithm", "Permission", "Credential", "CredentialsContainer",
      "PasswordCredential", "FederatedCredential"),
     "engine-v8"),  # 密码 / 安全与权限（Credentials 家族：`D-S2-1` 扩源）
    (
        ("Canvas", "OffscreenCanvas", "WebGL", "GPU", "Audio", "Media", "HTMLMediaElement",
         "MediaStream", "MediaStreamTrack", "WebSocket"),
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


def owner_for(member_id: str, source_slug: str = "") -> str:
    """按族归属定 `implementation_owner`（`§6.2`；未命中 ⇒ `uncategorized`，**显名不猜**）。

    `F10`：**来源族优先于接口名前缀** —— CDP `Storage.*`（宿主调试域方法）若只按前缀匹配
    会误挂 `web-dom`（Web 存储 API），而它其实属**宿主调试面**（`host`）；`HTMLMediaElement.*`
    前缀表只有 `Media` ⇒ 漏挂 `uncategorized`（应 `graphics`）。⇒ 先判来源族，再回落前缀。
    """
    if source_slug.startswith("cdp"):
        return "host"  # 宿主 CDP 域方法/事件 ⇒ 宿主调试面（与 Web 存储 API 不同族）
    if source_slug.startswith("khronos"):
        return "graphics"  # WebGL/WebGL2 ⇒ 图形媒体
    iface = member_id.split(".", 1)[0]
    for prefixes, owner in FAMILY_OWNERS:
        if iface.startswith(prefixes):
            return owner
    return "uncategorized"


def build_entry(member: str, source_slug: str, descriptor: str | None = None) -> dict[str, object]:
    """造一条**九字段齐**的种子条目（`status=missing`，`behavior_level=stub`）。

    `F6`：`descriptor` **缺失 ⇒ 报错**（由调用方捕获并指名成员），**不得**静默回退
    `descriptor_for` 的旧分类 —— 静默回退正是 `G4-S0b` ② 修真过的"善意的谎"。
    """
    if not descriptor:
        raise SystemExit(
            f"[seed] 成员 {member} 缺 `descriptor`（member_descriptors 未收录）"
            "⇒ 拒绝静默回退旧分类（`F6`）：请先在抽取器侧补四类 descriptor 再重跑"
        )
    return {
        "target_version": TARGET_VERSION,
        "id": member,
        "realm": "main",
        "descriptor": descriptor,
        "behavior_level": "stub",
        "test_ids": [SEED_TEST_ID],
        "status": "missing",
        "evidence": f"face: {member.split('.', 1)[0]} 见 webref IDL（面级存在性；行为取证待 S1~S5）",
        "implementation_owner": owner_for(member, source_slug),
        # `G4-S0b` ③-(3)：**来源族**随条目承载（同名前缀双源时防误读；见 refs/shared-prefix-report.md）。
        "category": source_slug,
    }


def freeze_state_of(path: Path) -> tuple[bool, int]:
    """读既有 manifest 的**冻结态** = `(denominator_fixed, 条目数)`；文件不存在 ⇒ `(False, 0)`。

    `F1`：本函数存在的唯一理由 —— **重跑 seed 不得静默打回冻结位**
    （`93ae891`/`28da287` 重跑时无条件写 `False`，把 `M0c` 已置的 `true` 抹掉了）。
    """
    if not path.exists():
        return False, 0
    payload = json.loads(path.read_text(encoding="utf-8"))
    entries = payload.get("entries") or []
    return bool(payload.get("denominator_fixed", False)), len(entries)


def main(argv: list[str] | None = None) -> int:
    """读成员集 ⇒ 写九字段 manifest 种子。

    **分母冻结纪律（`F1`）**：
    - 输出文件**已冻结** ⇒ **保留** `denominator_fixed: true`（**绝不**静默打回 `false`）；
    - 已冻结且新成员集**条目更少** ⇒ **拒绝写入**（分母只防删；缩水须 `ADR` 显式）；
    - 未冻结（首次生成）⇒ 写 `false`，冻结由 `M0c` 另笔显式置真。
    """
    parser = argparse.ArgumentParser(description="G4-S0b 种子生成器（成员集 ⇒ manifest 种子）")
    parser.add_argument("members", type=Path, help="community 成员集 JSON（计划 §4 格式）")
    parser.add_argument("--out", type=Path, required=True, help="输出 compat-manifest.json")
    parser.add_argument("--version", type=int, default=1)
    args = parser.parse_args(argv)

    payload = json.loads(args.members.read_text(encoding="utf-8"))
    members = payload["members"]
    if not members:
        raise SystemExit("[seed] 成员集为空 ⇒ 拒绝生成（防空表假通过）")

    provenance = payload.get("member_sources") or {}
    descriptors = payload.get("member_descriptors") or {}
    frozen_before, frozen_entries = freeze_state_of(args.out)
    if frozen_before and len(members) < frozen_entries:
        raise SystemExit(
            f"[seed] 拒绝写入 {args.out}：分母已冻结（{frozen_entries} 条），而本次成员集只有 "
            f"{len(members)} 条 ⇒ **缩减分母须 `ADR` 显式**（禁静默删分母）"
        )
    manifest = {
        "version": args.version,
        # `M0c` 的冻结位由**另笔显式**置真；本生成器**只保留、不覆写**（`F1`：
        # 无条件写 `False` 会把已冻结的分母静默打回未冻结 —— 静默家族原型）。
        "denominator_fixed": frozen_before,
        "entries": [
            build_entry(
                member,
                str(provenance.get(member, "unknown")),
                descriptors.get(member),
            )
            for member in members
        ],    }
    args.out.write_text(
        json.dumps(manifest, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
    )
    print(
        f"[seed] 写出 {args.out}：{len(manifest['entries'])} 条（status=missing；"
        f"behavior_level=stub；source={payload.get('source')}）"
    )
    if frozen_before:
        # **显名**（不静默）：冻结位被保留这件事必须出现在输出里，供人复核。
        print(
            f"[seed] ⚠️ 分母已冻结（既有 {frozen_entries} 条）⇒ **保留 "
            f"denominator_fixed=true**；本次仅重写 entries（{len(manifest['entries'])} 条）"
        )
    else:
        print("[seed] 分母未冻结（首次生成）⇒ `denominator_fixed` 保持 false（待 `M0c` 显式置真）")
    return 0


if __name__ == "__main__":  # pragma: no cover - 由 CLI 测覆盖
    raise SystemExit(main())
