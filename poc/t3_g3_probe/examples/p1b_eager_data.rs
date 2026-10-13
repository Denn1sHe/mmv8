//! P-1b 定形探针 —— `TElement` 5 个必需 `unsafe fn` 的**零 unsafe 函数体**验证。
//!
//! ## 结论（先证「零 unsafe 出路」）
//!
//! 以**急切持有** `style::data::ElementDataWrapper`（**非 `Option`、我方无 `UnsafeCell`**）为载荷，
//! 5 个方法的**函数体可全部零 `unsafe`**：
//! - `ensure_data` / `clear_data`：stylo 的 `ElementDataWrapper::borrow`/`borrow_mut`（`data.rs:332/344`）
//!   均为**安全 `pub fn`**（其内部 `unsafe` 属依赖）；`ElementData: Default`（`data.rs:287`）⇒
//!   `clear_data` = `*self.data.borrow_mut() = ElementData::default()`（`DerefMut` 安全写）。
//! - `set_handled_snapshot` / `set_dirty_descendants` / `unset_dirty_descendants`：仅置 `Cell<bool>`。
//!
//! ⇒ **unsafe 面 = 5 个 `unsafe fn` 签名 + 0 个 `unsafe {}` 块**（**无「腔体」**）。
//! 本文件用 `#![deny(unsafe_code)]`（**非 `forbid`**）+ **逐方法** `#[allow(unsafe_code)]`
//! 精确表达该最小面（对应 R17 备选 ②）。
#![deny(unsafe_code)]

use std::cell::Cell;

use style::data::{ElementData, ElementDataMut, ElementDataRef, ElementDataWrapper};

/// 急切载荷：元素样式数据（**非 `Option`** + 两个脏位）。
///
/// ⚠️ 与 blitz 的差异：blitz 用 `UnsafeCell<Option<ElementDataWrapper>>` + `unsafe fn ensure_init/clear`；
/// 本方案**急切持有**（`ElementDataWrapper: Default`）⇒ 无需我方 `UnsafeCell`，`clear` 走 `DerefMut` 覆盖。
#[derive(Debug, Default)]
struct ElementSlot {
    data: ElementDataWrapper,
    snapshot_handled: Cell<bool>,
    dirty_descendants: Cell<bool>,
}

impl ElementSlot {
    /// 对应 `TElement::ensure_data`（trait 强制 `unsafe fn`；**函数体零 unsafe**）。
    // SAFETY: 签名强制 unsafe；实现不 deref 裸指针，内部互斥由 stylo borrow_mut 保证。
    #[allow(unsafe_code)]
    unsafe fn ensure_data(&self) -> ElementDataMut<'_> {
        self.data.borrow_mut()
    }

    /// 对应 `TElement::clear_data`（**函数体零 unsafe**；「清空」= 覆盖为默认值）。
    // SAFETY: 签名强制 unsafe；DerefMut 写 + ElementData: Default，无裸指针运算。
    #[allow(unsafe_code)]
    unsafe fn clear_data(&self) {
        *self.data.borrow_mut() = ElementData::default();
    }

    /// 对应 `TElement::set_handled_snapshot`（**函数体零 unsafe**）。
    // SAFETY: 签名强制 unsafe；仅置 Cell<bool>，无内存风险。
    #[allow(unsafe_code)]
    unsafe fn set_handled_snapshot(&self) {
        self.snapshot_handled.set(true);
    }

    /// 对应 `TElement::set_dirty_descendants`（**函数体零 unsafe**）。
    // SAFETY: 签名强制 unsafe；仅置 Cell<bool>，无内存风险。
    #[allow(unsafe_code)]
    unsafe fn set_dirty_descendants(&self) {
        self.dirty_descendants.set(true);
    }

    /// 对应 `TElement::unset_dirty_descendants`（**函数体零 unsafe**）。
    // SAFETY: 签名强制 unsafe；仅清 Cell<bool>，无内存风险。
    #[allow(unsafe_code)]
    unsafe fn unset_dirty_descendants(&self) {
        self.dirty_descendants.set(false);
    }

    /// 对应 `TElement::borrow_data`（**安全方法**，无 `unsafe` 标记）。
    fn borrow_data(&self) -> ElementDataRef<'_> {
        self.data.borrow()
    }

    /// 对应 `TElement::has_data`（**安全方法**；急切载荷 ⇒ 恒 `true`）。
    fn has_data(&self) -> bool {
        true
    }

    /// 探针用：脏位读回。
    fn is_dirty(&self) -> bool {
        self.dirty_descendants.get()
    }
}

fn main() {
    let slot = ElementSlot::default();

    // 安全方法路径（对照）：无需任何 `unsafe`。
    assert!(slot.has_data(), "急切载荷：has_data 恒真");
    assert!(!slot.borrow_data().has_styles(), "初始无样式");
    assert!(!slot.is_dirty());

    // SAFETY: 探针 demo 调用块（真实 stylo 遍历由依赖内部调用，我方不产生调用点）。
    #[allow(unsafe_code)]
    unsafe {
        slot.set_dirty_descendants();
        assert!(slot.is_dirty(), "set_dirty_descendants 生效");
        slot.unset_dirty_descendants();
        assert!(!slot.is_dirty(), "unset_dirty_descendants 生效");
        slot.set_handled_snapshot();
        assert!(slot.snapshot_handled.get());
        {
            // ensure_data 返回可写借用（零 unsafe 体）；作用域内即释放，避免二次借用在 debug 下 panic。
            let _mutable: ElementDataMut<'_> = slot.ensure_data();
        }
        slot.clear_data();
    }

    assert!(!slot.borrow_data().has_styles(), "clear_data 后无样式");

    println!("P1B_EAGER_DATA_COMPILES_WITHOUT_UNSAFE_BODY=true");
    println!("P1B_UNSAFE_SURFACE=signatures:5 bodies:0");
}
