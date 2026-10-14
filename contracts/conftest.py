"""契约套件全局守卫（conftest）。

## 收集数哨兵（COLLECTION_FLOOR）

「陈旧陷阱」家族的一类：测试**静默不收集**（如漏 `test_` 前缀）——套件从 N 条变 N-1 条，
**不红**、CI 照绿，漏洞永久潜伏（本仓已实证：`capabilities_reports_g3_surface` 漏前缀 ⇒
60→59 而无人察觉，属"永不运行也永不红"的最危险一类）。

故用 `pytest_collection_modifyitems` 断言**收集数下界**：**仅升；降须显式改本常量并说明理由**
（改这个数字 = 一次显式决策，不再是静默漂移）。
"""

from __future__ import annotations

import pytest

#: 契约套件**收集数下界**（具名常量）。
#:
#: 规则：**仅升**；任何下调都必须显式修改本常量**并写明理由**（不接受静默漂移）。
#: 历史：2026-10-14 = 60（G2→G3 更名 + 新增后实测）；
#:       2026-10-15 = 78（**G4-S0a**：manifest 九字段 schema + behavior_level 三态 + 完备性双门禁测试，
#:       新增 18 条 ⇒ **只升**，理由见 docs/design-g4-真身-2026-10-15.md §6；
#:       2026-10-16 = 80（**G4-S0a 补记**：三态错标格全覆盖 —— 补 `CONTRACT`×行为标记 ⇒ 红 + 其合法格正例）；
#:       2026-10-16 = 89（**G4-S0b**：候选源 B 工具 —— WebIDL 成员抽取器自测 9 条，含 constructor 排除 /
#:       继承不误归 / 注释剔除 / partial 合并 / 产出必带 needs_community_review / 可被 load_members 读）。
#:       2026-10-16 = 97（**G4-S0b**：双门禁对接 —— 真集绿 + 漏/造/空三负对照保持红，8 条）。
#:       2026-10-16 = 99（**G4-S0b**：mixin 合入修 —— `X includes M` 合入 + mixin 自身不作分母接口，2 条）。
#:       2026-10-16 = 100（**G4-S0b**：CDP 族另源立项 —— 抽取器 + 并集 + 两源登记 + CDP 覆盖探针）。
#:       2026-10-16 = 101（**G4-S0b**：WebGL 族 Khronos 另源 + merge 续接 provenance 修 + WebGL 覆盖探针）。
#:       2026-10-16 = 103（**G4-S0b / U-6**：逐源锁文件落点 + 无源拒绝负对照，2 条）。
#:       2026-10-16 = 106（**G4-S0b ③**：跨源消歧三举措 —— 唯一性门禁（静默去重专防）/ 共享前缀报告 /
#:       条目来源族承载，3 条）。
COLLECTION_FLOOR = 106


def pytest_collection_modifyitems(config: pytest.Config, items: list[pytest.Item]) -> None:
    """收集后立即断言下界（漏收集 ⇒ 套件**红**，而非静默减量）。"""
    del config  # 钩子签名要求；本守卫不消费配置。
    collected = len(items)
    if collected < COLLECTION_FLOOR:
        pytest.exit(
            f"契约收集数哨兵：仅收集到 {collected} 条 < 下界 {COLLECTION_FLOOR}。\n"
            "疑似「静默不收集」（漏 `test_` 前缀 / 文件名不匹配 / 新目录未纳入 testpaths）。\n"
            "若确为**有意缩减**：请显式下调 conftest.py 的 COLLECTION_FLOOR 并写明理由。",
            returncode=1,
        )
