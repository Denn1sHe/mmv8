//! 宿主原生函数：`FunctionTemplate` 路径 + trampoline + 表征（S7）。
//!
//! # trampoline 如何取得 `&mut Realm`（本模块的核心 `unsafe` 桥）
//!
//! V8 在 JS 调起宿主函数时，Rust 侧需要「**正在执行中的那个 Realm**」的引用。沿用
//! ADR-0008 的「薄桥 + 逐块 SAFETY」形态，本模块用 **isolate data slot** 存放
//! `*mut Realm`：入口（`Realm::eval` / `Realm::call`）在**运行任何 JS 之前**把自身地址写入
//! `REALM_DATA_SLOT`，trampoline 在回调内取回并物化 `&mut Realm`。
//!
//! ## 别名不变量（为什么与 eval 帧的借用不冲突）
//!
//! `Realm` 的 Isolate 字段是 `Box<OwnedIsolate>`（**堆间接**）。运行 JS 期间，`eval` / `call`
//! 帧持有的唯一活跃引用是**指向堆上 `OwnedIsolate` 的 `&mut`**（v8 scope 借用它），
//! **不持有任何指向 `Realm` 结构体本身的引用**。因此 trampoline 物化的 `&mut Realm`
//! 在回调执行期间是**指向该结构体的唯一活跃可变引用**（堆内 Isolate 与栈上/内联的 Realm
//! 结构体是两个不同分配，互不重叠）。
//!
//! ## 生命周期不变量（为什么不悬垂）
//!
//! `REALM_DATA_SLOT` 中的指针**仅在** `eval` / `call` 的同步调用栈内被写入并立即使用：
//! trampoline 只可能在 `script.run` / `function.call` **同步**触发的 JS 执行中被调用，而此刻
//! `Realm` 正被 `eval` / `call` 的 `&mut self` 借住、必然存活。JS 返回后 `eval` / `call`
//! 结束，slot 不再被读取（下次入口会覆写）。
//!
//! ## 已知边界（如实登记，防误读）
//!
//! 若某宿主函数在回调内**再次**进入 `eval` / `call`（重入），本桥不会阻止它——那是
//! **重入禁令**的职责（ADR-0014 决策 3 的 RAII `ReentrantEval`，属批 2D / S10+）。
//! 本模块不宣称已闭合重入，只保证「单次回调内 `&mut Realm` 唯一」。

use crate::abi::{HostValue, JsHandle};
use crate::error::{JsError, JsErrorKind};
use crate::realm::{REALM_DATA_SLOT, Realm, host_from_value, value_from_host};

/// 宿主函数的类型擦除签名：入参是只读 `HostValue` 切片，返回一个 `HostValue`。
///
/// P-1b：已移除 `+ Send`——`NativeFn` 只存于 `Realm.native_fns`，而 `Realm` 内含
/// `v8::OwnedIsolate`（`!Send`），故整条链路本就不要求 `Send`；移除后允许非 `Send`
/// 闭包（如 `Rc` 捕获），扩大可注册面。
pub type NativeFn = Box<dyn FnMut(&mut Realm, &[HostValue]) -> Result<HostValue, JsError>>;

/// 一个原生函数的注册描述（name / length / id）。
#[derive(Clone, Debug)]
pub struct NativeFnSpec {
    /// 函数名（决定 `fn.name`）。
    pub name: String,
    /// 函数形参个数（决定 `fn.length`）。
    pub length: u32,
    /// 注册槽位 id（trampoline 经 v8 function data 取回）。
    pub id: u32,
}

/// 把原生函数注册到 Realm（`FunctionTemplate` 路径，§5「特殊表征」）。
///
/// # Errors
/// Realm 已关闭，或创建 v8 `Function` 失败时报错。
pub fn install_native(
    realm: &mut Realm,
    spec: &NativeFnSpec,
    f: NativeFn,
) -> Result<JsHandle, JsError> {
    realm.install_native_at(spec, f)
}

/// trampoline：**非捕获 `fn`**（v8 的 `MapFnTo` 要求回调是 `UnitType`；捕获闭包不满足）。
///
/// `#[allow(clippy::needless_pass_by_value)]`：`args` / `rv` 的**按值签名由 v8 的
/// `FunctionCallback` ABI 固定**，不可改为引用。
#[allow(clippy::needless_pass_by_value)]
pub(crate) fn native_trampoline(
    scope: &mut v8::PinScope<'_, '_>,
    args: v8::FunctionCallbackArguments,
    mut rv: v8::ReturnValue,
) {
    // 1) 取回注册 id（存于 v8 function 的 data）。
    let Some(id) = args
        .data()
        .int32_value(scope)
        .and_then(|raw| usize::try_from(raw).ok())
    else {
        throw_message(scope, "原生函数 data 缺失或非法");
        return;
    };

    // 2) 取回 Realm 裸指针（存于 isolate data slot）。
    let realm_ptr = scope.get_data(REALM_DATA_SLOT).cast::<Realm>();
    if realm_ptr.is_null() {
        throw_message(scope, "Realm 上下文缺失");
        return;
    }
    // SAFETY（别名）：`realm_ptr` 由 `Realm::eval` / `Realm::call` 在运行 JS 前写入，指向
    // 正在执行的 Realm；这些入口在运行期间**不持有指向 Realm 结构体的引用**（只持堆上
    // Isolate 的 &mut），故此处物化的 `&mut Realm` 是该结构体的唯一活跃可变引用。
    // SAFETY（生命周期）：trampoline 只在 `script.run` / `function.call` 同步触发的 JS 执行中
    // 被调用，此刻 Realm 被入口的 `&mut self` 借住、必然存活，指针不悬垂。
    // SAFETY: 别名与生命周期不变量见相邻论证与模块文档。
    // 置位重入守卫（RAII；Drop 无条件复位）：回调内再 eval/call ⇒ ReentrantEval。
    let _guard = match crate::realm::enter_callback(realm_ptr) {
        Ok(guard) => guard,
        Err(err) => {
            throw_message(scope, err.message());
            return;
        }
    };
    // SAFETY: 别名与生命周期不变量见相邻论证与模块文档。
    #[allow(unsafe_code)]
    let realm: &mut Realm = unsafe { &mut *realm_ptr };

    // 3) 收集宿主参数（对象类经弱槽登记为句柄）。
    let realm_id = realm.id();
    let argument_count = args.length();
    let mut host_args: Vec<HostValue> =
        Vec::with_capacity(usize::try_from(argument_count).unwrap_or(0));
    for index in 0..argument_count {
        let value = args.get(index);
        match host_from_value!(scope, realm.handles_mut(), realm_id, value) {
            Ok(host) => host_args.push(host),
            Err(err) => {
                throw_message(scope, err.message());
                return;
            }
        }
    }

    // 4) 取出宿主函数（take/put 规避「借 `native_fns` 与借 `realm` 并存」的借用冲突），
    //    调用后将函数放回注册表。
    let mut slot = realm.take_native(id);
    let outcome = match slot.as_mut() {
        Some(func) => func(realm, &host_args),
        None => Err(JsError::new(JsErrorKind::Internal, "原生函数 id 无效")),
    };
    realm.put_native(id, slot);

    // 5) 结果回填或抛出（异常经 TryCatch 传播，见 `native_fn_throw_propagates`）。
    match outcome {
        Ok(host) => match value_from_host!(scope, realm.handles_mut(), host) {
            Ok(local) => rv.set(local),
            Err(err) => throw_message(scope, err.message()),
        },
        Err(err) => throw_message(scope, err.message()),
    }
}

/// 抛出一个带 `message` 属性的对象（JS 侧 `catch(e){ e.message }` 可取回）。
fn throw_message(scope: &mut v8::PinScope<'_, '_>, message: &str) {
    if let Some(text) = v8::String::new(scope, message) {
        let error = v8::Object::new(scope);
        if let Some(key) = v8::String::new(scope, "message") {
            error.set(scope, key.into(), text.into());
        }
        scope.throw_exception(error.into());
    } else {
        scope.throw_exception(v8::undefined(scope).into());
    }
}
