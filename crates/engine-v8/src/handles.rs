//! 句柄表（slot table）：`JsHandle ↔ JS 值` 的**唯一实现点**（ADR-0013，S5）。
//!
//! # 形态（ADR-0013 决策 1 / 3 / 4）
//!
//! - 槽位存 **弱引用**（`v8::Weak`）：持有一个 `JsHandle` **不等于**持有 GC 根（C-1 的属性面阅读）。
//! - `HandleLease` **自持 `Global`（强引用本体）**、**不借用整表** —— 存活只能由显式租借承载。
//! - 失效分两类：弱引用空 ⇒ [`JsErrorKind::Collected`]；generation 不匹配 ⇒ [`JsErrorKind::StaleHandle`]。
//! - `free` 清空弱引用 + `generation += 1`；generation 回绕到 0 时**退役该槽**（消除 ABA）。
//!
//! # 零 unsafe
//!
//! 本模块只调用 `v8::Weak::new` / `to_local` / `to_global` / drop（**均为安全 API**），
//! **不含任何 `unsafe`** —— 兑现 ADR-0013 决策 9 的「很可能零 unsafe」，故 SAFETY 面
//! 收敛到 scope / trampoline（`isolate.rs` / `realm.rs`）。

use crate::abi::{JsHandle, RealmId, WeakJsHandle};
use crate::error::{JsError, JsErrorKind};

/// 一个槽位：`generation` 单调递增（复用防呆）；`weak` 是**弱引用**（不阻止 GC）。
///
/// `weak` 为 `None` 表示该槽**已释放**（`free` / `drain` 之后）。用 `Option<Weak>` 而非裸
/// `Weak`：`v8::Weak::empty(..)` 需要 `&mut Isolate`，而设计契约的 `free(&mut self, h)`
/// **不带 isolate 参数**；置 `None`（丢弃 `Weak`）即等价于「清空 weak」（ADR-0013 决策 4），
/// 且不会泄漏。
struct Slot {
    generation: u32,
    weak: Option<v8::Weak<v8::Value>>,
}

/// 句柄表。**这是 `JsHandle` 语义的唯一实现**（C-1）：`JsHandle` 只是本表的 `u64` 下标，
/// 持句柄不 root（ADR-0013）。
pub struct HandleTable {
    slots: Vec<Slot>,
    /// 空闲槽位下标栈（`free` 后入栈、`alloc` 时出栈复用）。
    free: Vec<u32>,
    realm: RealmId,
}

impl HandleTable {
    /// 为一个 Realm 建空表。
    #[must_use]
    pub fn new(realm: RealmId) -> Self {
        Self {
            slots: Vec::new(),
            free: Vec::new(),
            realm,
        }
    }

    /// 登记一个值：建槽、存**弱引用**。
    ///
    /// `isolate` 参数必需（`v8::Weak::new` 需 `&mut Isolate`，ADR-0013 决策 5）。
    ///
    /// # Errors
    /// 槽位下标超过 `u32` 上限时返回 [`JsErrorKind::Internal`]。
    pub fn alloc(
        &mut self,
        isolate: &mut v8::Isolate,
        local: v8::Local<v8::Value>,
    ) -> Result<JsHandle, JsError> {
        let weak = v8::Weak::new(isolate, local);
        let (index, generation) = if let Some(index) = self.free.pop() {
            let slot = &mut self.slots[index as usize];
            slot.weak = Some(weak);
            (index, slot.generation)
        } else {
            let index = u32::try_from(self.slots.len())
                .map_err(|_| JsError::new(JsErrorKind::Internal, "句柄表槽位索引超过 u32 上限"))?;
            // 新槽 generation 从 1 起：0 保留为「已退役」哨兵（见 `free` 的回绕处理）。
            self.slots.push(Slot {
                generation: 1,
                weak: Some(weak),
            });
            (index, 1)
        };
        Ok(JsHandle::new(index, generation))
    }

    /// 解析：比对**完整** `(index, generation)`；弱已空 ⇒ `Collected`；generation 不匹配 ⇒ `StaleHandle`。
    ///
    /// # Errors
    /// - [`JsErrorKind::StaleHandle`]：下标越界或 generation 不匹配（已释放 / 槽位被复用）。
    /// - [`JsErrorKind::Collected`]：弱引用已空，对应值已被 GC 回收。
    pub fn resolve<'s>(
        &self,
        scope: &v8::PinScope<'s, '_>,
        handle: JsHandle,
    ) -> Result<v8::Local<'s, v8::Value>, JsError> {
        let slot = self.slot_for(handle)?;
        let weak = slot
            .weak
            .as_ref()
            .ok_or_else(|| stale(handle, "所在槽位已释放"))?;
        weak.to_local(scope).ok_or_else(|| {
            JsError::new(
                JsErrorKind::Collected,
                format!("句柄 {handle:?} 的值已被 GC 回收"),
            )
        })
    }

    /// 解析**弱句柄**（ADR-0019·D5 的原子解析入口）。
    ///
    /// `WeakJsHandle` 与 `JsHandle` 位序一致（高 32 = generation / 低 32 = 下标），故合成同槽
    /// `JsHandle` 后按完整 `(index, generation)` 校验。
    ///
    /// # Errors
    /// - [`JsErrorKind::StaleHandle`]：下标越界、generation 不匹配或槽位已释放。
    ///
    /// 弱引用已空（值被 GC 回收）**不是错误**：返回 `Ok(None)`（调用方可判定跳过）。
    pub fn resolve_weak<'s>(
        &self,
        scope: &v8::PinScope<'s, '_>,
        weak_handle: WeakJsHandle,
    ) -> Result<Option<v8::Local<'s, v8::Value>>, JsError> {
        let handle = JsHandle::new(weak_handle.index(), weak_handle.generation());
        let slot = self.slot_for(handle)?;
        let weak = slot
            .weak
            .as_ref()
            .ok_or_else(|| stale(handle, "所在槽位已释放"))?;
        Ok(weak.to_local(scope))
    }

    /// 释放：清空 `weak` + `generation += 1`；generation 回绕到 0 ⇒ **退役该槽**（ADR-0013 决策 4）。
    ///
    /// # Errors
    /// 下标越界或 generation 不匹配时返回 [`JsErrorKind::StaleHandle`]。
    pub fn free(&mut self, handle: JsHandle) -> Result<(), JsError> {
        let index = handle.index() as usize;
        let slot = self
            .slots
            .get_mut(index)
            .ok_or_else(|| stale(handle, "下标越界"))?;
        if slot.generation != handle.generation() {
            return Err(stale(handle, "generation 不匹配"));
        }
        slot.weak = None;
        slot.generation = slot.generation.wrapping_add(1);
        // generation 回绕到 0（退役哨兵）：不再入空闲栈，该下标永久退役 ⇒ 彻底消除 ABA。
        if slot.generation != 0 {
            self.free.push(handle.index());
        }
        Ok(())
    }

    /// close 时排空全部槽位（§4：释放宿主回调与句柄）。
    pub fn drain(&mut self) {
        self.slots.clear();
        self.free.clear();
    }

    /// 显式租借：从弱槽 `to_global` 得到 `Global`（强引用本体），包成 [`HandleLease`]。
    ///
    /// # Errors
    /// - [`JsErrorKind::StaleHandle`]：下标越界或 generation 不匹配。
    /// - [`JsErrorKind::Collected`]：弱引用已空，无法取得强引用。
    pub fn root(
        &mut self,
        isolate: &mut v8::Isolate,
        handle: JsHandle,
    ) -> Result<HandleLease, JsError> {
        let slot = self.slot_for(handle)?;
        let weak = slot
            .weak
            .as_ref()
            .ok_or_else(|| stale(handle, "所在槽位已释放"))?;
        let global = weak.to_global(isolate).ok_or_else(|| {
            JsError::new(
                JsErrorKind::Collected,
                format!("句柄 {handle:?} 的值已被 GC 回收，无法 root"),
            )
        })?;
        Ok(HandleLease {
            realm: self.realm,
            handle,
            global,
        })
    }

    /// 生成弱句柄（供 DOM 面 / `node_map` 使用，**不阻止 GC**；ADR-0013 决策 6 / C-2）。
    ///
    /// # Errors
    /// 下标越界或 generation 不匹配时返回 [`JsErrorKind::StaleHandle`]。
    pub fn weak(&self, handle: JsHandle) -> Result<WeakJsHandle, JsError> {
        self.slot_for(handle)?;
        Ok(WeakJsHandle::new(handle.index(), handle.generation()))
    }

    /// 本表所属 Realm。
    #[must_use]
    pub fn realm(&self) -> RealmId {
        self.realm
    }

    /// 取（并校验 generation 的）槽位。
    fn slot_for(&self, handle: JsHandle) -> Result<&Slot, JsError> {
        let slot = self
            .slots
            .get(handle.index() as usize)
            .ok_or_else(|| stale(handle, "下标越界"))?;
        if slot.generation != handle.generation() {
            return Err(stale(handle, "generation 不匹配"));
        }
        Ok(slot)
    }
}

/// 构造一个 [`JsErrorKind::StaleHandle`] 错误。
fn stale(handle: JsHandle, why: &str) -> JsError {
    JsError::new(
        JsErrorKind::StaleHandle,
        format!("句柄 {handle:?} 已失效（{why}）"),
    )
}

/// 显式租借：**自己拥有该值的 `Global`（强引用本体）**，**不借用表**（ADR-0013 决策 1）。
///
/// 只有持有一个 `HandleLease` 才保证对应 JS 值存活；`Drop` = 取消 rooting（RAII）。
/// **注**：防跨语言环**不由**「lease 不 Clone」承担（跨层错配，ADR-0013 决策 6）——
/// 改由 C-2 类型面禁令承担。
#[derive(Debug)]
pub struct HandleLease {
    realm: RealmId,
    handle: JsHandle,
    /// 强引用本体（`Global`）。**只持有、不读取**：本字段的全部作用是把 `Global` 的存活期
    /// 绑定到 lease 上（RAII rooting）。rustc 的 dead-code 分析**刻意忽略**派生 Debug 的读取，
    /// 故需显式 `allow`（这是预期内的「只持有」字段）。
    #[allow(dead_code)]
    global: v8::Global<v8::Value>,
}

impl HandleLease {
    /// 该租借所属 Realm。
    #[must_use]
    pub fn realm(&self) -> RealmId {
        self.realm
    }

    /// 该租借对应的句柄。
    #[must_use]
    pub fn handle(&self) -> JsHandle {
        self.handle
    }
}

impl Drop for HandleLease {
    fn drop(&mut self) {
        // `v8::Global` 自身的 `Drop` 会 reset 存储单元 ⇒ 取消 rooting。
        // 本 impl 显式存在，用于集中说明「lease 生命期 == rooting 生命期」这一不变量。
    }
}

#[cfg(test)]
mod tests {
    use super::HandleTable;
    use crate::abi::{JsHandle, RealmId};
    use crate::error::JsErrorKind;
    use crate::isolate::EngineHost;
    use proptest::prelude::*;

    /// 建一个隔离的 Isolate + Context + ContextScope，执行 `body`。
    ///
    /// `body` 拿到 `scope`（ContextScope）与一张空 `HandleTable`。每个测试自建 Isolate
    /// （V8 支持跨线程并发建/销 Isolate；同线程串行建销亦无碍，见 ADR-0010 的实测约束）。
    fn with_table<R>(body: impl FnOnce(&mut v8::PinScope<'_, '_>, &mut HandleTable) -> R) -> R {
        EngineHost::init_once().expect("V8 初始化");
        let isolate = &mut v8::Isolate::new(v8::CreateParams::default());
        v8::scope!(let handle_scope, isolate);
        let context = v8::Context::new(handle_scope, v8::ContextOptions::default());
        let scope = &mut v8::ContextScope::new(handle_scope, context);
        let mut table = HandleTable::new(RealmId::from_parts(0, 0));
        body(scope, &mut table)
    }

    /// 在**独立 `HandleScope`** 内造一个普通对象并登记弱句柄（离开块后对象仅由弱槽引用）。
    fn alloc_orphan(table: &mut HandleTable, scope: &mut v8::PinScope<'_, '_>) -> JsHandle {
        v8::scope!(let inner, scope);
        let object: v8::Local<v8::Value> = v8::Object::new(inner).into();
        table.alloc(inner, object).expect("alloc")
    }

    #[test]
    fn handle_not_a_gc_root() {
        // C-1（属性面）：只持 `JsHandle`（弱槽）不足以保活 ⇒ GC 后可回收。
        with_table(|scope, table| {
            let handle = alloc_orphan(table, scope);
            scope.request_garbage_collection_for_testing(v8::GarbageCollectionType::Full);
            let resolved = table.resolve(scope, handle);
            assert!(
                matches!(resolved, Err(ref e) if e.kind() == JsErrorKind::Collected),
                "仅持 JsHandle 不应保活；期望 Collected，得到 {resolved:?}"
            );
        });
    }

    #[test]
    fn collected_detected() {
        // 弱引用空 ⇒ Collected（与 StaleHandle 分开，ADR-0013 决策 3）。
        // **同时兑现 ADR-0013 的 T1 待实测项**：`Weak` 回收后 `to_local` 与 `to_global`
        // **都**返回 `None`（可判定、非 UB）——分别经 `resolve`（`to_local` 侧）与
        // `root`（`to_global` 侧）观测。
        with_table(|scope, table| {
            let handle = alloc_orphan(table, scope);
            scope.request_garbage_collection_for_testing(v8::GarbageCollectionType::Full);

            let via_local = table.resolve(scope, handle).expect_err("应被回收");
            assert_eq!(
                via_local.kind(),
                JsErrorKind::Collected,
                "to_local 侧应为 Collected（非 StaleHandle）"
            );

            let via_global = table.root(scope, handle).expect_err("应无法 root");
            assert_eq!(
                via_global.kind(),
                JsErrorKind::Collected,
                "to_global 侧应为 Collected（T1：Weak 回收后 to_global → None）"
            );
        });
    }

    #[test]
    fn lease_keeps_value_alive() {
        // 显式 rooting 语义：持 `HandleLease` 时存活；drop 后 GC 可回收。
        with_table(|scope, table| {
            let handle = alloc_orphan(table, scope);
            let lease = table.root(scope, handle).expect("root 应成功");

            // 「保活」检查放进**独立 HandleScope**：`resolve` 产出的 `Local` 是 GC 根，
            // 若留在外层作用域会**人为保活**，使 drop(lease) 后的回收判定失真。
            {
                v8::scope!(let check, scope);
                check.request_garbage_collection_for_testing(v8::GarbageCollectionType::Full);
                assert!(
                    table.resolve(check, handle).is_ok(),
                    "持 lease 时应保活（弱槽仍指向活值）"
                );
            }

            drop(lease);
            scope.request_garbage_collection_for_testing(v8::GarbageCollectionType::Full);
            let resolved = table.resolve(scope, handle);
            assert!(
                matches!(resolved, Err(ref e) if e.kind() == JsErrorKind::Collected),
                "drop(lease) 后应可回收；得到 {resolved:?}"
            );
        });
    }

    #[test]
    fn stale_handle_detected() {
        // 释放后复用槽位 ⇒ 旧句柄解析报 StaleHandle（复用防呆，不是静默指向新值）。
        with_table(|scope, table| {
            let first = alloc_orphan(table, scope);
            let other = alloc_orphan(table, scope);
            assert_ne!(first.index(), other.index(), "两次 alloc 应落在不同槽位");

            table.free(first).expect("free 应成功");
            let reused = alloc_orphan(table, scope);
            assert_eq!(reused.index(), first.index(), "槽位应被复用（同一下标）");
            assert_ne!(
                reused.generation(),
                first.generation(),
                "复用后 generation 必须递增"
            );

            let err = table.resolve(scope, first).expect_err("旧句柄应失效");
            assert_eq!(err.kind(), JsErrorKind::StaleHandle);
            assert!(table.resolve(scope, reused).is_ok(), "新句柄应有效");
        });
    }

    #[derive(Debug, Clone)]
    enum Op {
        Alloc,
        Free(usize),
    }

    fn op_strategy() -> impl Strategy<Value = Op> {
        prop_oneof![3 => Just(Op::Alloc), 2 => any::<usize>().prop_map(Op::Free)]
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(24))]

        #[test]
        fn alloc_free_reuse_no_alias(ops in proptest::collection::vec(op_strategy(), 1..40)) {
            // 随机 alloc/free 序列后，任何**存活**句柄仍指向其原始值（无别名到新值）。
            let observed = with_table(|scope, table| {
                let mut live: Vec<(JsHandle, f64)> = Vec::new();
                let mut counter = 0.0_f64;

                for op in ops {
                    match op {
                        Op::Alloc => {
                            counter += 1.0;
                            let value: v8::Local<v8::Value> =
                                v8::Number::new(scope, counter).into();
                            let handle = table.alloc(scope, value).expect("alloc");
                            live.push((handle, counter));
                        }
                        Op::Free(_) if live.is_empty() => {}
                        Op::Free(k) => {
                            let index = k % live.len();
                            let (handle, _) = live.remove(index);
                            table.free(handle).expect("free");
                        }
                    }
                }

                live.into_iter()
                    .map(|(handle, expected)| {
                        let value = table.resolve(scope, handle).expect("存活句柄应可解析");
                        (expected, value.number_value(scope))
                    })
                    .collect::<Vec<_>>()
            });

            for (expected, got) in observed {
                prop_assert_eq!(got, Some(expected));
            }
        }
    }
}
