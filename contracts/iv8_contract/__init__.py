"""iv8 G0 契约测试工具包。

- :mod:`iv8_contract.manifest` —— `compat-manifest.json` 的结构、状态流转与单调性校验（铁律 8）
- :mod:`iv8_contract.cases` —— 探针用例（probe case）的加载与校验
- :mod:`iv8_contract.runner` —— 契约 runner（规范模式 / iv8 兼容模式双 pipeline）
- :mod:`iv8_contract.differential` —— Chromium 差分 harness
- :mod:`iv8_contract.report` —— 报告生成
"""

from __future__ import annotations

__all__ = ["__version__"]

__version__ = "0.0.0"