"""`page_control` —— G2 页面控制面的 **Python 契约投影**（ADR-0017·D11 / 施工图 §2.7 / §3.8 / §4）。

本模块**不是**重新实现语义，而是描述 Rust 权威定义在 Python 侧可见的**枚举值域与能力面口径**：

- :class:`RunOutcome` —— ``crates/scheduler/src/runloop.rs`` 的 ``enum RunOutcome`` 变体名投影；
- :class:`ReadyState` —— ``crates/navigation/src/controller.rs`` 的 ``enum ReadyState`` 变体名投影；
- :class:`JsErrorKind` —— ``crates/engine-v8/src/error.rs`` 的 ``enum JsErrorKind`` **既有 12 变体集合**
  （含 ``Terminated``）；这是 **G2 新增**面（既有 ``v8_abi.py`` 只覆盖 ``HostValue``）；
- :data:`CAPABILITY_KEYS` / :data:`CAPABILITY_VALUES` —— ``navigation::page::Capabilities`` 的键与 G2 口径。

两侧一致性由 ``contracts/tests/test_page_control.py`` 读取 Rust 文件**文本**断言
（方向：**Python 读 Rust 文件**，不手写第二份期望源码副本）。
"""

from __future__ import annotations

import enum

__all__ = [
    "RunOutcome",
    "RUN_OUTCOME_NAMES",
    "ReadyState",
    "READY_STATE_OUTCOME_NAMES",
    "JsErrorKind",
    "JS_ERROR_KIND_NAMES",
    "CAPABILITY_KEYS",
    "CAPABILITY_VALUES",
    "STORAGE_QUOTA_BYTES",
]


class RunOutcome(enum.Enum):
    """Rust ``enum RunOutcome`` 的 5 个变体（成员名与 Rust 侧逐字一致）。"""

    Idle = "Idle"
    BudgetExhausted = "BudgetExhausted"
    DeadlineExceeded = "DeadlineExceeded"
    WaitingIo = "WaitingIo"
    Closed = "Closed"


#: Rust 侧 ``RunOutcome`` 变体名的投影集合（亦为 ``PyContext.run_until`` 返回的字符串集合）。
RUN_OUTCOME_NAMES = frozenset(member.name for member in RunOutcome)


class ReadyState(enum.Enum):
    """Rust ``enum ReadyState`` 的 3 个变体；值 = ``document.readyState`` 字符串。"""

    Loading = "loading"
    Interactive = "interactive"
    Complete = "complete"


#: Rust 侧 ``ReadyState`` 变体名的投影集合。
READY_STATE_OUTCOME_NAMES = frozenset(member.name for member in ReadyState)

#: Rust ``ReadyState`` 变体的 ``document.readyState`` 字符串值集合。
READY_STATE_VALUES = frozenset(member.value for member in ReadyState)


class JsErrorKind(enum.Enum):
    """Rust ``enum JsErrorKind`` 的**既有 12 变体集合**（G2 新增投影，**无常量新增**）。

    ``Terminated`` 为既有变体（watchdog 截断专有判据），harness 可按稳定前缀断言「被打断」。
    """

    Compile = "Compile"
    Exception = "Exception"
    Collected = "Collected"
    StaleHandle = "StaleHandle"
    WrongRealm = "WrongRealm"
    Terminated = "Terminated"
    MicrotaskBudget = "MicrotaskBudget"
    CallbackDeadline = "CallbackDeadline"
    GcCycleForbidden = "GcCycleForbidden"
    ControlRevoked = "ControlRevoked"
    ReentrantEval = "ReentrantEval"
    Internal = "Internal"


#: Rust 侧 ``JsErrorKind`` 变体名的投影集合（12 个，含 ``Terminated``）。
JS_ERROR_KIND_NAMES = frozenset(member.name for member in JsErrorKind)

#: ``navigation::page::Capabilities`` 的字段名（Python ``capabilities()`` 的键集合）。
CAPABILITY_KEYS = frozenset(
    {
        "parsing",
        "document_write",
        "navigation",
        "network",
        "input",
        "geometry",
        "shadow_dom",
        "script_interruptible",
        "input_user_activation",
        "single_realm_navigation",
        "storage_quota_bytes",
    }
)

#: 存储配额申报值（`G4-S1` `⑥`）：**5 MiB，per-bucket**。
#:
#: 与 Rust ``storage::STORAGE_QUOTA_BYTES`` **同值**（由 ``contracts/tests/test_page_control.py``
#: 读 Rust 源文件文本断言，非手写副本）；``§4.0`` 口径：**外部 golden 基线 = 本申报值**
#: （**待 A 路社区实物对账**），**禁止**以实现读回当外部基线。
STORAGE_QUOTA_BYTES = 5 * 1024 * 1024

#: G3 能力面口径（施工图 §3.8；G3-B16：geometry 升为 computed、shadow_dom 开启）。
CAPABILITY_VALUES = {
    "parsing": "streaming",
    "document_write": True,
    "navigation": True,
    "network": "offline",
    "input": "explicit_target",
    "geometry": "computed",
    "shadow_dom": True,
    "script_interruptible": True,
    "input_user_activation": "sticky_only",
    "single_realm_navigation": True,
    "storage_quota_bytes": STORAGE_QUOTA_BYTES,
}
