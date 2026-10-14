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

import hashlib
import json
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from iv8_contract.manifest import Manifest, ManifestError, load

__all__ = [
    "CompletenessReport",
    "check_completeness",
    "load_members",
    "load_golden",
    "denominator_digest",
    "check_denominator_golden",
    "GOLDEN_LOCK",
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


#: golden 锁文件（`ADR-0028 §3`：分母唯一真源；**唯一允许变更的途径 = ADR 显式**）。
GOLDEN_LOCK = "docs/refs/denominator-golden.txt"


def load_golden(repo_root: str | Path) -> tuple[int, str]:
    """读 golden 锁 ⇒ `(n, sha256)`。

    `D-3`：**类型层强制** —— `denominator_fixed: true` 的分母必须有**外部锚**。
    此前它只是自声明布尔（只能证明"不再改"，不能证明"没被剪"）；golden 把
    「分母成员集」固化成**可证哈希** ⇒ 任何增删都必须在 golden 与 manifest **同时**体现，
    否则本校验红（**声明态 ≠ 实物态 ⇒ 红**）。
    """
    path = Path(repo_root) / GOLDEN_LOCK
    if not path.is_file():
        raise ManifestError(f"golden 锁不存在：{path}（denominator_fixed=true 必须有外部锚）")
    fields: dict[str, str] = {}
    for line in path.read_text(encoding="utf-8").splitlines():
        if "=" in line and not line.lstrip().startswith("#"):
            key, value = line.split("=", 1)
            fields[key.strip()] = value.strip()
    try:
        return int(fields["denominator_n"]), fields["denominator_sha256"]
    except KeyError as exc:  # pragma: no cover - 由坏锁测试覆盖
        raise ManifestError(f"golden 锁缺字段：{exc}") from exc


def denominator_digest(members: frozenset[str]) -> str:
    """成员集合的**稳定摘要**（与 golden 锁同口径：`sha256("\\n".join(sorted(members)))`）。"""
    return hashlib.sha256("\n".join(sorted(members)).encode("utf-8")).hexdigest()


def check_denominator_golden(repo_root: str | Path) -> None:
    """**golden 锁校验**（`D-3`）：manifest 的 `denominator_fixed=true` ⇒ 成员集必须等于 golden。

    三层防线中的**类型层**（另两层：写入侧 = seed 拒缩水；实物态 = 发货态断言）。
    任一层不一致 ⇒ 本函数抛 :class:`ManifestError`（**不静默**）。
    """
    root = Path(repo_root)
    manifest = load(root / "contracts" / "compat-manifest.json")
    if not manifest.denominator_fixed:
        return  # 未冻结 ⇒ 无 golden 义务
    members = load_members(root / "contracts" / "community-v0.1.4-members.json")
    expected_n, expected_sha = load_golden(root)
    actual_ids = frozenset(entry.id for entry in manifest)
    if actual_ids != members:
        raise ManifestError(
            "manifest 条目集与 community 成员集不一致（增删须 ADR 显式 + 同步 golden）"
        )
    digest = denominator_digest(members)
    if len(members) != expected_n or digest != expected_sha:
        raise ManifestError(
            f"分母与 golden 锁不一致：实物 n={len(members)} sha={digest[:12]}… "
            f"vs golden n={expected_n} sha={expected_sha[:12]}… "
            "⇒ 增删分母必须 ADR 显式并同步更新 docs/refs/denominator-golden.txt"
        )
