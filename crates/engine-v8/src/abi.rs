//! 类型面（type surface）—— 跨 crate 安全类型的**唯一 Rust 定义点**。
//!
//! 本模块定义四个可安全跨 crate 传递的类型：`RealmId`、`JsHandle`、`WeakJsHandle`、
//! `HostValue`。依据 ADR-0009 决策 1/2，它们**只在此处定义**；`lib.rs` 通过
//! `pub use abi::{HostValue, JsHandle, RealmId, WeakJsHandle};` 做**唯一**的类型再导出，
//! 其余一律走模块路径。
//!
//! # 硬约束（违反即门禁红）
//!
//! - **本文件不得出现任何 V8 类型路径引用**：类型面不是 FFI 桥。该约束由
//!   `contracts/tests/test_v8_abi.py` 直接读取本文件文本断言（守卫脚本规则 5 只覆盖
//!   白名单**之外**的目录，故对 `abi.rs` 的约束由契约测试承担）。
//! - 三个关键类型的定义**各恰好一处**，由
//!   `workspace_policy.rs::three_safety_types_have_single_definition_point` 断言。
//! - 全部类型均为 `#[repr(transparent)]` 的 `u64`，或纯数据枚举，**不携带任何
//!   GC rooting 语义**（`JsHandle` **不是** GC 根，见 ADR-0013 决策 7 的 C-1）。

/// `RealmId::realm_seq` 与 `JsHandle::index` 所在的**低 32 位**掩码。
const LOW_HALF_MASK: u64 = 0x0000_0000_FFFF_FFFF;

/// 一个 V8 Context 及其宿主身份。
///
/// 打包布局：**高 32 位 = isolate 序号，低 32 位 = realm 序号**（ADR-0009 决策 2）。
/// 不透明：外部只能经 [`RealmId::from_parts`] 构造，经 [`RealmId::isolate_seq`] /
/// [`RealmId::realm_seq`] 读取。`#[repr(transparent)]` 保证其内存布局与内部 `u64`
/// 完全一致，使 Python 契约可按固定 64 位宽度逐项对应。
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct RealmId(u64);

impl RealmId {
    /// 由 isolate 序号与 realm 序号打包构造（高 32 位 = isolate，低 32 位 = realm）。
    #[must_use]
    pub const fn from_parts(isolate_seq: u32, realm_seq: u32) -> Self {
        Self(((isolate_seq as u64) << 32) | (realm_seq as u64))
    }

    /// 取 isolate 序号（高 32 位）。
    #[must_use]
    pub const fn isolate_seq(self) -> u32 {
        (self.0 >> 32) as u32
    }

    /// 取 realm 序号（低 32 位）。
    #[must_use]
    pub const fn realm_seq(self) -> u32 {
        (self.0 & LOW_HALF_MASK) as u32
    }
}

/// 句柄表槽位索引。
///
/// 打包布局：**高 32 位 = generation（复用防呆），低 32 位 = 槽位下标**。
///
/// **不是 GC 根**（ADR-0013 决策 7 的 C-1）：类型本身只是 `u64`，**不携带任何 rooting
/// 语义**；持有它**不等于**让对应 JS 值存活——存活须由 `HandleLease` 显式承载
/// （ADR-0013 决策 1/2）。
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct JsHandle(u64);

impl JsHandle {
    /// 由槽位下标与 generation 打包构造（高 32 位 = generation，低 32 位 = index）。
    #[must_use]
    pub const fn new(index: u32, generation: u32) -> Self {
        Self(((generation as u64) << 32) | (index as u64))
    }

    /// 取槽位下标（低 32 位）。
    #[must_use]
    pub const fn index(self) -> u32 {
        (self.0 & LOW_HALF_MASK) as u32
    }

    /// 取 generation（高 32 位）。
    #[must_use]
    pub const fn generation(self) -> u32 {
        (self.0 >> 32) as u32
    }
}

/// 弱句柄：**不阻止 GC**，供 DOM 侧监听器槽与 `node_map` 使用（ADR-0013 决策 6 的 C-2）。
///
/// 打包布局与 [`JsHandle`] 一致（高 32 位 = generation，低 32 位 = 下标），以便在同一个
/// 弱槽表内定位；但它是**弱**语义：对应 JS 值可被 GC 回收，回收后经弱句柄的解析会报
/// `JsErrorKind::Collected`（见 `error.rs`）。类型本身仍只是 `u64`，不含任何 rooting。
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct WeakJsHandle(u64);

impl WeakJsHandle {
    /// 由槽位下标与 generation 打包构造（高 32 位 = generation，低 32 位 = index）。
    #[must_use]
    pub const fn new(index: u32, generation: u32) -> Self {
        Self(((generation as u64) << 32) | (index as u64))
    }

    /// 取槽位下标（低 32 位）。
    #[must_use]
    pub const fn index(self) -> u32 {
        (self.0 & LOW_HALF_MASK) as u32
    }

    /// 取 generation（高 32 位）。
    #[must_use]
    pub const fn generation(self) -> u32 {
        (self.0 >> 32) as u32
    }
}

/// 跨「JS 引擎 ↔ 宿主」边界的值。
///
/// **有界枚举，8 变体，只增不改语义**（ADR-0009 决策 4 判据 3）：新增变体（如
/// `BigInt` / `Symbol` / `ArrayBuffer`）必须走独立 ADR。变体名集合与 Python 契约
/// `contracts/iv8_contract/v8_abi.py` 的枚举成员**逐项相等**，由
/// `contracts/tests/test_v8_abi.py` 断言。
#[derive(Clone, Debug, PartialEq)]
pub enum HostValue {
    /// `undefined`。
    Undefined,
    /// `null`。
    Null,
    /// 布尔值。
    Bool(bool),
    /// 数字（统一按 `f64`）。
    Number(f64),
    /// 字符串。
    Str(String),
    /// 普通对象，由 `(realm, handle)` 共同定位。
    Object {
        /// 所属 Realm。
        realm: RealmId,
        /// 该对象在所属 Realm 内的句柄。
        handle: JsHandle,
    },
    /// 数组，由 `(realm, handle)` 共同定位。
    Array {
        /// 所属 Realm。
        realm: RealmId,
        /// 该数组在所属 Realm 内的句柄。
        handle: JsHandle,
    },
    /// 函数（裸 function，**非**句柄别名），由 `(realm, handle)` 共同定位。
    Function {
        /// 所属 Realm。
        realm: RealmId,
        /// 该函数在所属 Realm 内的句柄。
        handle: JsHandle,
    },
}

#[cfg(test)]
mod tests {
    use super::{JsHandle, RealmId, WeakJsHandle};

    #[test]
    fn handle_packing_roundtrip() {
        // 边界 0 / u32::MAX 与常规值的打包-解包必须逐位还原。
        for (index, generation) in [
            (0u32, 0u32),
            (u32::MAX, u32::MAX),
            (0, u32::MAX),
            (u32::MAX, 0),
            (7, 3),
        ] {
            let h = JsHandle::new(index, generation);
            assert_eq!(h.index(), index, "index 往返失败：{index}");
            assert_eq!(
                h.generation(),
                generation,
                "generation 往返失败：{generation}"
            );
        }
    }

    #[test]
    fn handle_distinct_pairs_are_not_equal() {
        // 不同 (index, generation) 必须不相等（键空间无别名）。
        assert_ne!(
            JsHandle::new(1, 2),
            JsHandle::new(2, 1),
            "index 与 generation 不得互换"
        );
        assert_ne!(
            JsHandle::new(1, 2),
            JsHandle::new(1, 3),
            "generation 不同应不等"
        );
        assert_ne!(JsHandle::new(1, 2), JsHandle::new(3, 2), "index 不同应不等");
        assert_eq!(
            JsHandle::new(1, 2),
            JsHandle::new(1, 2),
            "相同 (index, generation) 应相等"
        );
    }

    #[test]
    fn realm_id_packing() {
        for (isolate_seq, realm_seq) in [
            (0u32, 0u32),
            (u32::MAX, u32::MAX),
            (1, u32::MAX),
            (u32::MAX, 1),
            (5, 9),
        ] {
            let id = RealmId::from_parts(isolate_seq, realm_seq);
            assert_eq!(id.isolate_seq(), isolate_seq, "isolate 序号往返失败");
            assert_eq!(id.realm_seq(), realm_seq, "realm 序号往返失败");
        }
    }

    #[test]
    fn weak_handle_packing_and_equality() {
        for (index, generation) in [(0u32, 0u32), (u32::MAX, u32::MAX), (7, 3)] {
            let w = WeakJsHandle::new(index, generation);
            assert_eq!(w.index(), index);
            assert_eq!(w.generation(), generation);
            assert_eq!(w, WeakJsHandle::new(index, generation));
        }
        // 相等性对 (index, generation) 的任一维敏感。
        assert_ne!(
            WeakJsHandle::new(7, 3),
            WeakJsHandle::new(7, 4),
            "generation 不同应不等"
        );
        assert_ne!(
            WeakJsHandle::new(7, 3),
            WeakJsHandle::new(8, 3),
            "index 不同应不等"
        );
    }
}
