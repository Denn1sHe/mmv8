"""`G4-S1` 存储的**契约侧**锚定（`F12`：JS 面具名错误的证据不得只在 Rust 侧）。

`contracts/` 侧**不跑 JS 运行时**，故以**源级对撞**（读 Rust 源文本）锚定契约关注面 ——
即「**JS 可见的错误名**」：

1. 宿主以 `JsError::named(.., "QuotaExceededError", ..)` **具名直抛**（`D4-M` 修法）；
2. **哨兵绕行零残留** —— shim 里「解析 `QuotaExceededError:` 前缀串再 `new Error` 重抛」
   的旧协议已作废；**一旦有人把它加回来 ⇒ 本测红**（防绕行回归）。
"""

from __future__ import annotations

import re
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
#: 存储面接线（宿主原生函数 + JS shim 同在此文件）。
BINDING_RS = REPO_ROOT / "crates" / "navigation" / "src" / "binding.rs"

#: 具名直抛的**成对**形态（`JsError::named` × 具名常量）——单看任一侧都不够。
NAMED_THROW = re.compile(r"JsError::named\(\s*JsErrorKind::\w+,\s*\"QuotaExceededError\"")

#: 旧「哨兵串 + shim 重抛」绕行的**代码特征**（shim 侧解析哨兵前缀）。
SENTINEL_WORKAROUND = "indexOf('QuotaExceededError:')"


def test_storage_quota_error_is_named_at_host() -> None:
    """`F12`：`QuotaExceededError` 在宿主侧**具名直抛**，且**哨兵绕行零残留**。

    **负对照**：① 把常量改成别的名（或改回 `JsError::new`）⇒ 正则不命中 ⇒ 红；
    ② 把 shim 的哨兵解析代码加回来 ⇒ 残留断言红。
    """
    text = BINDING_RS.read_text(encoding="utf-8")
    assert NAMED_THROW.search(text) is not None, (
        f"{BINDING_RS} 中未找到 `JsError::named(.., \"QuotaExceededError\", ..)` 的具名直抛；"
        "JS 面具名错误必须由宿主承载（不得退回 shim 侧重抛）"
    )
    assert SENTINEL_WORKAROUND not in text, (
        "shim 里残留哨兵串解析（旧绕行协议已作废，`D4-M` 后具名错误原生可达）⇒ 绕行回归"
    )
