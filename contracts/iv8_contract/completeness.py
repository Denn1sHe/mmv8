"""分母**完备性双门禁**（`G4` `R-3` / `§4.7`）—— 「社区清单 ⊆ manifest」∧「无杜撰 id」。

## 口径（`§6` `R-3`，最高优先）

- **分母定义源** = `docs/refs/iv8-docx-G4-平台能力-提取-2026-10-15.md`（`§18` 接口清单，**已机读落库**）；
  社区版 `v0.1.4` 双模式实测 = **证据来源，非定义源**。
- **完备性 = 双向**：
  1. **不漏**：`社区清单 ⊆ manifest`（**逐成员**，非族级）；
  2. **不造**：`manifest ⊆ 社区清单`（**无杜撰 `id`**）。
- **分母冻结时点** = `S0` 结束（`Manifest.denominator_fixed` 置真；**只防删**）。

## 用法

```python
community = load_members("community-v0.1.4.json")   # 由 `S0b` 种子填充产出
report = check_completeness(manifest, community)    # 双向核
report.assert_ok()                                  # 违反 ⇒ ManifestError
```

本模块**只做核验**（不产生分母）；分母由 `S0b` 种子填充落码。
"""

from __future__ import annotations

import json
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from iv8_contract.manifest import Manifest, ManifestError

__all__ = [
    "CompletenessReport",
    "check_completeness",
    "load_members",
]


@dataclass(frozen=True, slots=True)
class CompletenessReport:
    """双向核结果（**可读报告**，便于进闸门文书）。"""

    #: 社区清单有、而 manifest **缺**的成员（**漏列** ⇒ 必须为空）。
    missing_from_manifest: frozenset[str]
    #: manifest 有、而社区清单**无**的成员（**杜撰** ⇒ 必须为空）。
    fabricated_in_manifest: frozenset[str]
    #: 两侧共有成员数（分母规模的直接读数）。
    common: int

    def assert_ok(self) -> None:
        """双向核**均须为空**；任一非空 ⇒ :class:`ManifestError`（**含明细**）。"""
        problems: list[str] = []
        if self.missing_from_manifest:
            sample = sorted(self.missing_from_manifest)[:5]
            problems.append(
                f"社区清单有 {len(self.missing_from_manifest)} 个成员未进 manifest"
                f"（漏列，示例：{sample}）"
            )
        if self.fabricated_in_manifest:
            sample = sorted(self.fabricated_in_manifest)[:5]
            problems.append(
                f"manifest 有 {len(self.fabricated_in_manifest)} 个成员不在社区清单"
                f"（**杜撰 id**，示例：{sample}）"
            )
        if problems:
            raise ManifestError("；".join(problems))

    def as_row(self) -> dict[str, Any]:
        """完备性行（`§4.7`）—— 进闸门文书用。"""
        return {
            "common": self.common,
            "missing_from_manifest": len(self.missing_from_manifest),
            "fabricated_in_manifest": len(self.fabricated_in_manifest),
            "ok": not self.missing_from_manifest and not self.fabricated_in_manifest,
        }


def check_completeness(manifest: Manifest, community_ids: frozenset[str]) -> CompletenessReport:
    """**双向核**：`社区 ⊆ manifest` ∧ `manifest ⊆ 社区`（成员级）。"""
    manifest_ids = frozenset(entry.id for entry in manifest.entries)
    return CompletenessReport(
        missing_from_manifest=frozenset(community_ids - manifest_ids),
        fabricated_in_manifest=frozenset(manifest_ids - community_ids),
        common=len(community_ids & manifest_ids),
    )


def load_members(path: str | Path) -> frozenset[str]:
    """读取社区清单成员集合（`S0b` 种子填充的产物）。

    格式（**最小**）：``{"members": ["Window.setTimeout", ...]}``；亦接受裸数组。
    缺文件 / 结构非法 ⇒ :class:`ManifestError`（**不静默为空集**——空集会让门禁**假通过**）。
    """
    target = Path(path)
    if not target.is_file():
        raise ManifestError(f"社区清单不存在：{target}（完备性门禁不得以空集代替）")
    raw = json.loads(target.read_text(encoding="utf-8"))
    members = raw.get("members") if isinstance(raw, dict) else raw
    if not isinstance(members, list) or not members:
        raise ManifestError(f"社区清单 {target} 的 members 必须是非空数组")
    return frozenset(str(m) for m in members)
