"""`v8_abi` —— engine-v8 类型面的 **Python 契约投影**（ADR-0009 决策 2 / 方案 E）。

本模块**不是**重新实现语义，而是描述 Rust 权威定义
(``crates/engine-v8/src/abi.rs``) 在 Python 侧可见的 **值域与序列化契约**：

- ``RealmId`` / ``JsHandle`` / ``WeakJsHandle`` 在 Python 侧一律是
  **定宽 64 位 ``int``**（``0 <= v < 2**64``），位序与 ``abi.rs`` 完全一致：
  **高 32 位 = isolate 序号 / generation，低 32 位 = realm 序号 / 槽位下标**；
- :class:`HostValueKind` 是 Rust ``enum HostValue`` 的**变体名投影**，成员集合必须与
  Rust 侧**逐项相等**。

两侧的一致性由 ``contracts/tests/test_v8_abi.py`` 读取 ``abi.rs`` **文本**断言
（方向：**Python 读 Rust 文件**，不反向依赖，也不手写第二份期望源码副本）。
"""

from __future__ import annotations

import enum

__all__ = [
    "U64_WIDTH",
    "LOW_HALF_MASK",
    "HIGH_HALF_SHIFT",
    "HostValueKind",
    "HOST_VALUE_VARIANT_NAMES",
    "pack_handle",
    "unpack_handle",
    "pack_realm",
    "unpack_realm",
]

# 序列化契约的固定宽度（位）与位序常量；改动会破坏与既有 Profile 的兼容。
U64_WIDTH = 64
LOW_HALF_MASK = 0xFFFF_FFFF
HIGH_HALF_SHIFT = 32


class HostValueKind(enum.Enum):
    """Rust ``enum HostValue`` 的 8 个变体（成员名与 Rust 侧逐字一致）。"""

    Undefined = "Undefined"
    Null = "Null"
    Bool = "Bool"
    Number = "Number"
    Str = "Str"
    Object = "Object"
    Array = "Array"
    Function = "Function"


#: Rust 侧 ``HostValue`` 变体名的 Python 侧投影集合。
HOST_VALUE_VARIANT_NAMES = frozenset(member.name for member in HostValueKind)


def _require_int(value: object, name: str) -> int:
    # bool 是 int 的子类，但其语义是布尔而非整数，故显式拒绝，避免 True 被当作 1。
    if isinstance(value, bool) or not isinstance(value, int):
        raise TypeError(f"{name} 必须是 int（bool 不算），得到 {type(value).__name__}")
    return value


def _require_u32(value: object, name: str) -> int:
    checked = _require_int(value, name)
    if not 0 <= checked <= LOW_HALF_MASK:
        raise ValueError(f"{name} 超出 u32 值域 [0, 2**32-1]：{checked}")
    return checked


def _require_u64(value: object, name: str) -> int:
    checked = _require_int(value, name)
    if not 0 <= checked < (1 << U64_WIDTH):
        raise ValueError(f"{name} 超出 u64 值域 [0, 2**64-1]：{checked}")
    return checked


def pack_handle(index: int, generation: int) -> int:
    """把 ``(index, generation)`` 打包成句柄 ``u64``（高 32 = generation，低 32 = index）。"""
    checked_index = _require_u32(index, "index")
    checked_generation = _require_u32(generation, "generation")
    return (checked_generation << HIGH_HALF_SHIFT) | checked_index


def unpack_handle(packed: int) -> tuple[int, int]:
    """:func:`pack_handle` 的逆运算，返回 ``(index, generation)``。"""
    checked = _require_u64(packed, "packed")
    return checked & LOW_HALF_MASK, (checked >> HIGH_HALF_SHIFT) & LOW_HALF_MASK


def pack_realm(isolate_seq: int, realm_seq: int) -> int:
    """把 ``(isolate_seq, realm_seq)`` 打包成 ``RealmId`` 的 ``u64``（高 32 = isolate，低 32 = realm）。"""
    checked_isolate = _require_u32(isolate_seq, "isolate_seq")
    checked_realm = _require_u32(realm_seq, "realm_seq")
    return (checked_isolate << HIGH_HALF_SHIFT) | checked_realm


def unpack_realm(packed: int) -> tuple[int, int]:
    """:func:`pack_realm` 的逆运算，返回 ``(isolate_seq, realm_seq)``。"""
    checked = _require_u64(packed, "packed")
    return (checked >> HIGH_HALF_SHIFT) & LOW_HALF_MASK, checked & LOW_HALF_MASK
