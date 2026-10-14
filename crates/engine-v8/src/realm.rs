//! 一个 `Context` 及其宿主身份；求值 / 属性 / 调用 / 原生函数 / 微任务 / 关闭（S6+S7+S8）。
//!
//! 强类型出口只有 `HostValue`（abi 四类型之一）——V8 类型不出本模块签名。
//!
//! # 句柄纪律（ADR-0013）
//! `eval` 返回的 `HostValue::Object/Array/Function` 携带的是**弱**句柄（`JsHandle`）；
//! Rust 侧跨调用存活必须显式 [`Realm::root`]。弱引用被 GC 回收 ⇒ [`JsErrorKind::Collected`]；
//! generation 不匹配 ⇒ [`JsErrorKind::StaleHandle`]。
//!
//! # `&mut Realm` 与 trampoline 的别名不变量（S7 的关键）
//! Isolate 字段是 `Box<OwnedIsolate>`（**堆间接**）。运行 JS 期间，本模块的入口只持有
//! **指向堆上 `OwnedIsolate` 的 `&mut`**（经 v8 scope），**不持有指向 `Realm` 结构体的引用**；
//! 因此 `native_fn::native_trampoline` 在回调内物化的 `&mut Realm` 是该结构体的唯一活跃可变
//! 引用（详见 `native_fn.rs` 的模块文档）。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use crate::abi::{HostValue, JsHandle, RealmId, WeakJsHandle};
use crate::error::{JsError, JsErrorKind};
use crate::handles::{HandleLease, HandleTable};
use crate::isolate::TerminateHandle;
use crate::microtask::{DEFAULT_MICROTASK_GRACE, MicrotaskPolicy};
use crate::module_host::ModuleRegistry;
use crate::native_fn::{NativeFn, NativeFnSpec, native_trampoline};

/// isolate data slot：存放「正在执行的 `Realm`」的裸指针（供 `native_fn` trampoline 取回）。
pub(crate) const REALM_DATA_SLOT: u32 = 0;

/// 把 `Local<Value>` 转成 `HostValue`（在调用处的 `scope` / `handles` 上下文中**就地展开**）。
macro_rules! host_from_value {
    ($scope:expr, $handles:expr, $realm_id:expr, $value:expr) => {{
        let value = $value;
        if value.is_undefined() {
            Ok(HostValue::Undefined)
        } else if value.is_null() {
            Ok(HostValue::Null)
        } else if value.is_boolean() {
            Ok(HostValue::Bool(value.boolean_value($scope)))
        } else if value.is_number() {
            Ok(HostValue::Number(value.number_value($scope).unwrap_or(f64::NAN)))
        } else if value.is_string() {
            Ok(HostValue::Str(value.to_rust_string_lossy($scope)))
        } else {
            // 不用 `?`：本宏需能在**返回 `()`** 的回调（native trampoline）中展开。
            match $handles.alloc($scope, value) {
                Err(err) => Err(err),
                Ok(handle) => Ok(if value.is_function() {
                    HostValue::Function {
                        realm: $realm_id,
                        handle,
                    }
                } else if value.is_array() {
                    HostValue::Array {
                        realm: $realm_id,
                        handle,
                    }
                } else {
                    HostValue::Object {
                        realm: $realm_id,
                        handle,
                    }
                }),
            }
        }
    }};
}

/// 把 `HostValue` 转成 `Local<Value>`（对象类经 `handles.resolve`）。
macro_rules! value_from_host {
    ($scope:expr, $handles:expr, $value:expr) => {{
        match $value {
            HostValue::Undefined => Ok(v8::undefined($scope).into()),
            HostValue::Null => Ok(v8::null($scope).into()),
            HostValue::Bool(flag) => Ok(v8::Boolean::new($scope, flag).into()),
            HostValue::Number(number) => Ok(v8::Number::new($scope, number).into()),
            HostValue::Str(text) => v8::String::new($scope, &text)
                .map(Into::into)
                .ok_or_else(|| JsError::new(JsErrorKind::Internal, "无法在 V8 中创建字符串")),
            HostValue::Object { handle, .. }
            | HostValue::Array { handle, .. }
            | HostValue::Function { handle, .. } => $handles.resolve($scope, handle),
        }
    }};
}

/// 从 `TryCatch` 作用域取异常文本，失败则回退到给定的默认串。
macro_rules! caught_text {
    ($scope:expr, $fallback:expr) => {
        $scope
            .exception()
            .or_else(|| $scope.stack_trace())
            .map(|value| value.to_rust_string_lossy($scope))
            .unwrap_or_else(|| ($fallback).to_owned())
    };
}

pub(crate) use host_from_value;
pub(crate) use value_from_host;

/// 一个 V8 Context 及其宿主身份（G1：单 Isolate 单 Realm）。
pub struct Realm {
    /// **堆间接**（S7 别名不变量的前提，见模块文档）。
    isolate: Box<v8::OwnedIsolate>,
    /// 本 Realm 的身份（设计与 ADR 称为 `realm_id`）。
    identity: RealmId,
    context: v8::Global<v8::Context>,
    handles: HandleTable,
    /// 已注册的宿主原生函数（按 id 索引；trampoline 经 v8 function data 取回 id）。
    native_fns: Vec<Option<NativeFn>>,
    /// 模块缓存（`eval_module`：同一 URL 复用同一 namespace）。
    module_registry: ModuleRegistry,
    /// 模块解析/加载钩子（`None` ⇒ 只能求值无 `import` 的模块）。
    module_host: Option<Box<dyn crate::module_host::ModuleHost>>,
    /// 当前模块根 URL（`eval_module` 期间设置，供解析相对 specifier）。
    module_root: String,
    /// 回调重入守卫位（ADR-0014 决策 3；trampoline 置位，RAII 复位）。
    in_callback: bool,
    /// 跨线程终止句柄（微任务墙钟守卫用）。
    terminate: TerminateHandle,
    /// 微任务墙钟守卫宽限（G1 的实际截断判据，见 `microtask::MicrotaskBudget`）。
    microtask_grace: Duration,
    closed: bool,
}

impl Realm {
    /// 用给定的 Isolate 建一个 Realm（建一个 Context 并持其 `Global`）。
    ///
    /// # Errors
    /// 建 Context 失败时返回 [`JsErrorKind::Internal`]。
    pub fn new(isolate: v8::OwnedIsolate) -> Result<Self, JsError> {
        let realm_id = RealmId::from_parts(0, 0);
        let terminate = TerminateHandle::from_isolate(&isolate);
        let mut isolate = Box::new(isolate);
        // 安装动态 `import()` 的宿主回调（K-11）。回调经 `REALM_DATA_SLOT` 取回本 Realm；
        // 是否真能解析取决于宿主是否设置 `set_module_host`。回调内**失败一律拒绝 promise**、
        // 绝不空返回（否则 V8 FATAL，见 [`dynamic_import_callback`]）。
        isolate.set_host_import_module_dynamically_callback(dynamic_import_callback);
        let context = {
            v8::scope!(let handle_scope, &mut *isolate);
            let context = v8::Context::new(handle_scope, v8::ContextOptions::default());
            v8::Global::new(handle_scope, context)
        };
        Ok(Self {
            isolate,
            identity: realm_id,
            context,
            handles: HandleTable::new(realm_id),
            native_fns: Vec::new(),
            module_registry: ModuleRegistry::default(),
            module_host: None,
            module_root: String::new(),
            in_callback: false,
            terminate,
            microtask_grace: DEFAULT_MICROTASK_GRACE,
            closed: false,
        })
    }

    /// 本 Realm 的身份。
    #[must_use]
    pub fn id(&self) -> RealmId {
        self.identity
    }

    /// 句柄表可变引用（供 `native_fn` trampoline 在回调内登记/解析句柄）。
    pub(crate) fn handles_mut(&mut self) -> &mut HandleTable {
        &mut self.handles
    }

    /// 取走 id 处的原生函数（take/put 规避借用冲突，见 `native_fn`）。
    pub(crate) fn take_native(&mut self, id: usize) -> Option<NativeFn> {
        self.native_fns.get_mut(id).and_then(Option::take)
    }

    /// 把原生函数放回 id 处。
    pub(crate) fn put_native(&mut self, id: usize, func: Option<NativeFn>) {
        if let Some(slot) = self.native_fns.get_mut(id) {
            *slot = func;
        }
    }

    /// 强制一次 Full GC（**仅测试 / `test-util` 特性**；依赖 `--expose-gc`，由
    /// `EngineHost::init_once` 设定）。
    ///
    /// 供**跨 crate** 测试（如 `navigation::binding` 的弱引用路径断言）经 dev-dependency
    /// 开启 `test-util` 特性后调用。默认构建**不含**本方法（不扩大稳定类型面）。
    ///
    /// # Errors
    /// 当前实现不产生错误。
    #[cfg(any(test, feature = "test-util"))]
    pub fn collect_garbage_for_testing(&mut self) -> Result<(), JsError> {
        let realm: *mut Self = std::ptr::from_mut(self);
        // SAFETY（别名 + 生命周期）：经裸指针取堆上 Isolate 的 `&mut` 建 scope 触发 Full GC；
        // 此后不再使用 `self`，且 GC 期间无 trampoline 活跃。
        // SAFETY: 别名与生命周期不变量见相邻论证与模块文档。
        #[allow(unsafe_code)]
        unsafe {
            let isolate: &mut v8::OwnedIsolate = &mut (*realm).isolate;
            v8::scope!(let scope, isolate);
            scope.request_garbage_collection_for_testing(v8::GarbageCollectionType::Full);
        }
        Ok(())
    }

    /// 求值一段 JS 源码，返回强类型 `HostValue`。
    ///
    /// # Errors
    /// - [`JsErrorKind::Compile`]：语法 / 编译错误。
    /// - [`JsErrorKind::Exception`]：运行期抛出可捕获异常。
    /// - [`JsErrorKind::Internal`]：Realm 已关闭，或 V8 内部失败。
    pub fn eval(&mut self, src: &str) -> Result<HostValue, JsError> {
        let realm: *mut Self = std::ptr::from_mut(self);
        Self::eval_impl(realm, src)
    }

    /// 带 deadline 的求值（ADR-0017·D2）：复用 G1 `perform_checkpoint` 同款 watchdog
    /// （独立线程 + 超时 `terminate_execution()` + 事后 `cancel_terminate_execution()`）。
    ///
    /// 超过 `deadline` 被打断 ⇒ [`JsErrorKind::Terminated`]（**专有判据**；不依赖偶发 message
    /// `"null"`，P-1a 实测带出）。脚本在 deadline 前正常返回 ⇒ 返回其结果（即便终止请求恰好在
    /// 返回后到达）。
    ///
    /// # Errors
    /// Realm 已关闭；编译 / 运行异常；或超 deadline 被打断（[`JsErrorKind::Terminated`]）。
    pub fn eval_with_deadline(
        &mut self,
        src: &str,
        deadline: Duration,
    ) -> Result<HostValue, JsError> {
        self.ensure_open()?;
        let terminate = self.terminate.clone();
        let fired = std::sync::Arc::new(AtomicBool::new(false));
        let fired_watch = std::sync::Arc::clone(&fired);
        let (tx, rx) = mpsc::channel::<()>();
        let watchdog = std::thread::spawn(move || {
            if rx.recv_timeout(deadline).is_err() {
                fired_watch.store(true, Ordering::SeqCst);
                terminate.terminate_execution();
            }
        });

        let outcome = self.eval(src);
        tx.send(()).ok();
        let _ = watchdog.join();

        if fired.load(Ordering::SeqCst) {
            // 终止是粘性的：复位以便 isolate 之后继续可用（G1 同款）。
            self.isolate.cancel_terminate_execution();
            if outcome.is_err() {
                return Err(JsError::new(
                    JsErrorKind::Terminated,
                    "脚本执行超过 deadline 被 watchdog 截断",
                ));
            }
        }
        outcome
    }

    /// `eval` 的实现：经裸指针工作。
    ///
    /// 运行 JS 期间本帧**只持有堆上 `OwnedIsolate` 的 `&mut`**（Box 间接，与 `Realm` 结构体
    /// 是不同分配），**不持有指向结构体的引用** ⇒ `native_fn::native_trampoline` 在回调内
    /// 物化的 `&mut Realm` 是回调期唯一活跃可变引用（别名不变量见模块文档）。
    fn eval_impl(realm: *mut Self, src: &str) -> Result<HostValue, JsError> {
        // SAFETY（别名）：`realm` 派生自 `eval` 中存活的 `&mut self`；本函数全程只经该裸指针
        // 访问，运行 JS 期间只持堆上 Isolate 的 `&mut`（见上）。运行结束后取 `handles` 的
        // `&mut`（结构体字段）时 trampoline 已不活跃，故不与他引用冲突。
        // SAFETY（生命周期）：`Realm` 被 `eval` 的 `&mut self` 借住、本次调用内必然存活；
        // isolate data slot 在运行 JS 前写入，JS 返回后不再被读取。
        // SAFETY: 别名与生命周期不变量见相邻论证与模块文档。
        #[allow(unsafe_code)]
        unsafe {
            if (*realm).closed {
                return Err(closed_error());
            }
            if (*realm).in_callback {
                return Err(reentrant_error());
            }
            let identity = (*realm).identity;
            let isolate: &mut v8::OwnedIsolate = &mut (*realm).isolate;
            // 安装 Realm 裸指针到 isolate data（在运行任何 JS 之前）。
            isolate.set_data(REALM_DATA_SLOT, realm.cast());
            let context_ref: &v8::Global<v8::Context> = &(*realm).context;

            v8::scope_with_context!(let ctx_scope, isolate, context_ref);
            v8::scope!(let handle_scope, ctx_scope);
            v8::tc_scope!(let try_catch, handle_scope);

            let source = v8::String::new(try_catch, src)
                .ok_or_else(|| JsError::new(JsErrorKind::Internal, "无法创建 JS 源字符串"))?;
            let Some(script) = v8::Script::compile(try_catch, source, None) else {
                return Err(JsError::new(
                    JsErrorKind::Compile,
                    caught_text!(try_catch, src),
                ));
            };
            let Some(result) = script.run(try_catch) else {
                return Err(JsError::new(
                    JsErrorKind::Exception,
                    caught_text!(try_catch, "JS 运行时抛出异常"),
                ));
            };

            let handles: &mut HandleTable = &mut (*realm).handles;
            host_from_value!(try_catch, handles, identity, result)
        }
    }

    /// 读取对象属性。
    ///
    /// # Errors
    /// 句柄失效、句柄非对象、或 V8 失败时报错。
    pub fn get_prop(&mut self, handle: JsHandle, key: &str) -> Result<HostValue, JsError> {
        self.ensure_open()?;
        let identity = self.identity;
        let Self {
            isolate,
            context,
            handles,
            ..
        } = self;

        v8::scope_with_context!(let scope, &mut **isolate, &*context);
        let target = handles.resolve(scope, handle)?;
        let object = target
            .to_object(scope)
            .ok_or_else(|| JsError::new(JsErrorKind::Internal, "句柄不是对象，无法 get_prop"))?;
        let key_local = v8::String::new(scope, key)
            .ok_or_else(|| JsError::new(JsErrorKind::Internal, "无法创建属性名字符串"))?;
        let value = object
            .get(scope, key_local.into())
            .ok_or_else(|| JsError::new(JsErrorKind::Internal, "Object::get 失败"))?;
        host_from_value!(scope, handles, identity, value)
    }

    /// 设置对象属性。
    ///
    /// # Errors
    /// 句柄失效、句柄非对象、`HostValue` 不可转换、或 V8 失败时报错。
    pub fn set_prop(
        &mut self,
        handle: JsHandle,
        key: &str,
        value: HostValue,
    ) -> Result<(), JsError> {
        self.ensure_open()?;
        let Self {
            isolate,
            context,
            handles,
            ..
        } = self;

        v8::scope_with_context!(let scope, &mut **isolate, &*context);
        let target = handles.resolve(scope, handle)?;
        let object = target
            .to_object(scope)
            .ok_or_else(|| JsError::new(JsErrorKind::Internal, "句柄不是对象，无法 set_prop"))?;
        let key_local = v8::String::new(scope, key)
            .ok_or_else(|| JsError::new(JsErrorKind::Internal, "无法创建属性名字符串"))?;
        let v8_value = value_from_host!(scope, handles, value)?;
        object
            .set(scope, key_local.into(), v8_value)
            .ok_or_else(|| JsError::new(JsErrorKind::Internal, "Object::set 失败"))?;
        Ok(())
    }

    /// 调用一个函数句柄。
    ///
    /// # Errors
    /// 句柄失效、句柄非函数、参数不可转换、或调用抛出异常时报错。
    pub fn call(
        &mut self,
        handle: JsHandle,
        this: Option<HostValue>,
        args: &[HostValue],
    ) -> Result<HostValue, JsError> {
        let realm: *mut Self = std::ptr::from_mut(self);
        Self::call_impl(realm, handle, this, args, false)
    }

    /// 事件分派专用调用入口（ADR-0019·D6）：放开「**分派期调用监听器**」这一**有界**受限重入。
    ///
    /// # 何时需要
    /// `el.click()` / 脚本内 `dispatchEvent` 在 JS 执行中**同步**触发监听器调用；此时
    /// `in_callback == true`，普通 [`Realm::call`] 会被重入禁令挡下。本入口跳过该禁令。
    ///
    /// # 深度论证（为什么仅此形状可放开）
    /// 1. **仅放开「调用」**：`eval`/`eval_module`/`eval_with_deadline`（求值类）**仍禁**重入
    ///    （ADR-0014 决策 3 的禁令只对求值入口适用）。
    /// 2. **别名不变量不破**：本入口复用 [`Self::call_impl`] 的裸指针形态——运行 JS 期间本帧
    ///    **只持堆上 `OwnedIsolate` 的 `&mut`**（Box 间接，与 `Realm` 结构体是不同分配），
    ///    **不持指向结构体的引用**（见模块文档）。故从回调（native trampoline 已物化 `&mut Realm`）
    ///    再入时，其 `&mut Realm` 已被**借用挂起**，本帧的堆 Isolate `&mut` 与之不相交。
    /// 3. **有界形状**：重入链仅限「同一次事件分派内逐监听器调用」——`plan_dispatch` 产出
    ///    **owned 快照**、每步调用前 `is_live` 查活性、不存在求值再入。
    ///
    /// # Errors
    /// Realm 已关闭、句柄非函数、或调用抛出异常时报错。
    pub fn dispatch_call(
        &mut self,
        callee: JsHandle,
        this: Option<HostValue>,
        args: &[HostValue],
    ) -> Result<HostValue, JsError> {
        let realm: *mut Self = std::ptr::from_mut(self);
        Self::call_impl(realm, callee, this, args, true)
    }

    /// 解析 + 调用**原子**入口（ADR-0019·D5）：`WeakJsHandle` 与 `JsHandle` 位序一致，
    /// 合成同槽 `JsHandle` 后走 [`Realm::call`] 的**单帧**解析 + 调用 —— 中间**无分配**，
    /// 故**无** `resolve_weak`→`call` 两步间的 GC 回收窗口。
    ///
    /// # Errors
    /// Realm 已关闭、弱引用已空（`Collected`）、非函数、或调用抛出异常时报错。
    pub fn call_weak(
        &mut self,
        weak_handle: WeakJsHandle,
        this: Option<HostValue>,
        args: &[HostValue],
    ) -> Result<HostValue, JsError> {
        self.call(
            JsHandle::new(weak_handle.index(), weak_handle.generation()),
            this,
            args,
        )
    }

    /// `call` 的实现（别名/生命周期论证同 [`Self::eval_impl`]）。
    ///
    /// `from_dispatch` 为真时跳过 `in_callback` 重入禁令（见 [`Realm::dispatch_call`] 的深度论证）；
    /// 其余路径不变。
    fn call_impl(
        realm: *mut Self,
        handle: JsHandle,
        this: Option<HostValue>,
        args: &[HostValue],
        from_dispatch: bool,
    ) -> Result<HostValue, JsError> {
        // SAFETY（别名 + 生命周期）：见 `eval_impl` 与模块文档。
        // SAFETY: 别名与生命周期不变量见相邻论证与模块文档。
        #[allow(unsafe_code)]
        unsafe {
            if (*realm).closed {
                return Err(closed_error());
            }
            if (*realm).in_callback && !from_dispatch {
                return Err(reentrant_error());
            }
            let identity = (*realm).identity;
            let isolate: &mut v8::OwnedIsolate = &mut (*realm).isolate;
            isolate.set_data(REALM_DATA_SLOT, realm.cast());
            let context_ref: &v8::Global<v8::Context> = &(*realm).context;

            v8::scope_with_context!(let scope, isolate, context_ref);
            let handles: &mut HandleTable = &mut (*realm).handles;
            let target = handles.resolve(scope, handle)?;
            let function = target
                .try_cast::<v8::Function>()
                .map_err(|_| JsError::new(JsErrorKind::Internal, "句柄不是函数，无法 call"))?;
            let receiver = match this {
                Some(value) => value_from_host!(scope, handles, value)?,
                None => v8::undefined(scope).into(),
            };
            let mut arguments = Vec::with_capacity(args.len());
            for arg in args {
                arguments.push(value_from_host!(scope, handles, arg.clone())?);
            }
            let result = function.call(scope, receiver, &arguments).ok_or_else(|| {
                JsError::new(JsErrorKind::Exception, "函数调用失败（可能抛出了异常）")
            })?;
            host_from_value!(scope, handles, identity, result)
        }
    }

    /// 显式获得强引用（存活保证）：从弱槽取得 `Global`，包成 [`HandleLease`]。
    ///
    /// # Errors
    /// 句柄失效（`StaleHandle` / `Collected`）时报错。
    pub fn root(&mut self, handle: JsHandle) -> Result<HandleLease, JsError> {
        self.ensure_open()?;
        let Self {
            isolate, handles, ..
        } = self;
        handles.root(isolate, handle)
    }

    /// 生成弱句柄（供 DOM 面 / `node_map` 使用，不阻止 GC）。
    ///
    /// # Errors
    /// 句柄失效时报 [`JsErrorKind::StaleHandle`]。
    pub fn weak(&mut self, handle: JsHandle) -> Result<WeakJsHandle, JsError> {
        self.ensure_open()?;
        self.handles.weak(handle)
    }

    /// 注册一个宿主原生函数（`FunctionTemplate` 路径），返回其句柄。
    ///
    /// # Errors
    /// Realm 已关闭、函数数量超过 `u32`、或创建 v8 `Function` 失败时报错。
    pub fn new_function(
        &mut self,
        name: &str,
        length: u32,
        f: NativeFn,
    ) -> Result<JsHandle, JsError> {
        let id = u32::try_from(self.native_fns.len())
            .map_err(|_| JsError::new(JsErrorKind::Internal, "原生函数数量超过 u32"))?;
        let spec = NativeFnSpec {
            name: name.to_owned(),
            length,
            id,
        };
        self.install_native_at(&spec, f)
    }

    /// 按显式 `id` 注册宿主原生函数（`native_fn::install_native` 的落点）。
    ///
    /// # Errors
    /// Realm 已关闭、id 超出 `usize`、或创建 v8 `Function` 失败时报错。
    pub fn install_native_at(
        &mut self,
        spec: &NativeFnSpec,
        f: NativeFn,
    ) -> Result<JsHandle, JsError> {
        self.ensure_open()?;
        let id = usize::try_from(spec.id)
            .map_err(|_| JsError::new(JsErrorKind::Internal, "原生函数 id 超出 usize"))?;
        if self.native_fns.len() <= id {
            self.native_fns.resize_with(id + 1, || None);
        }
        self.native_fns[id] = Some(f);

        let Self {
            isolate,
            context,
            handles,
            ..
        } = self;
        v8::scope_with_context!(let scope, &mut **isolate, &*context);

        let data: v8::Local<v8::Value> =
            v8::Integer::new(scope, i32::try_from(spec.id).unwrap_or(i32::MAX)).into();
        let function = v8::Function::builder(native_trampoline)
            .length(i32::try_from(spec.length).unwrap_or(0))
            .data(data)
            .build(scope)
            .ok_or_else(|| JsError::new(JsErrorKind::Internal, "Function 创建失败"))?;

        // 设置函数的 `name`（函数自身 name 属 configurable，用 define_property 覆盖）。
        if let (Some(name_key), Some(name_value)) = (
            v8::String::new(scope, "name"),
            v8::String::new(scope, &spec.name),
        ) {
            let descriptor =
                v8::PropertyDescriptor::new_from_value_writable(name_value.into(), false);
            function.define_property(scope, name_key.into(), &descriptor);
        }

        handles.alloc(scope, function.into())
    }

    /// 建一个空普通对象（供 DOM wrapper / JS `Event` 对象）。**安全 API**（`Object::new`）。
    ///
    /// # Errors
    /// Realm 已关闭或登记句柄失败时报错。
    pub fn new_object(&mut self) -> Result<JsHandle, JsError> {
        self.ensure_open()?;
        let Self {
            isolate,
            context,
            handles,
            ..
        } = self;
        v8::scope_with_context!(let scope, &mut **isolate, &*context);
        let object: v8::Local<v8::Value> = v8::Object::new(scope).into();
        handles.alloc(scope, object)
    }

    /// 建一个空数组。
    ///
    /// # Errors
    /// Realm 已关闭或登记句柄失败时报错。
    pub fn new_array(&mut self) -> Result<JsHandle, JsError> {
        self.ensure_open()?;
        let Self {
            isolate,
            context,
            handles,
            ..
        } = self;
        v8::scope_with_context!(let scope, &mut **isolate, &*context);
        let array: v8::Local<v8::Value> = v8::Array::new(scope, 0).into();
        handles.alloc(scope, array)
    }

    /// 把弱句柄解析为 [`HostValue`]；值已被回收 ⇒ `Ok(None)`（**不 root**，ADR-0019·D5）。
    ///
    /// # Errors
    /// Realm 已关闭、弱句柄 generation 不匹配时报错。
    pub fn resolve_weak(
        &mut self,
        weak_handle: WeakJsHandle,
    ) -> Result<Option<HostValue>, JsError> {
        self.ensure_open()?;
        let identity = self.identity;
        let Self {
            isolate,
            context,
            handles,
            ..
        } = self;
        v8::scope_with_context!(let scope, &mut **isolate, &*context);
        match handles.resolve_weak(scope, weak_handle)? {
            None => Ok(None),
            Some(local) => host_from_value!(scope, handles, identity, local).map(Some),
        }
    }

    /// 把句柄转为**弱句柄**（供 `NodeMap`/监听器槽；ADR-0013 决策 6 / C-2）。**不 root**。
    ///
    /// # Errors
    /// Realm 已关闭、句柄 generation 不匹配时报错。
    pub fn weak_handle(&mut self, handle: JsHandle) -> Result<WeakJsHandle, JsError> {
        self.ensure_open()?;
        self.handles.weak(handle)
    }

    /// 设全局属性（供 binding 安装 `document` 等）。
    ///
    /// # Errors
    /// Realm 已关闭、属性名建串失败、值不可转换、或 `set` 失败时报错。
    pub fn set_global(&mut self, name: &str, value: HostValue) -> Result<(), JsError> {
        self.ensure_open()?;
        let Self {
            isolate,
            context,
            handles,
            ..
        } = self;
        v8::scope_with_context!(let scope, &mut **isolate, &*context);
        let context_local = v8::Local::new(scope, &*context);
        let global = context_local.global(scope);
        let key = v8::String::new(scope, name)
            .ok_or_else(|| JsError::new(JsErrorKind::Internal, "无法创建全局属性名字符串"))?;
        let v8_value = value_from_host!(scope, handles, value)?;
        global
            .set(scope, key.into(), v8_value)
            .ok_or_else(|| JsError::new(JsErrorKind::Internal, "全局属性 set 失败"))?;
        Ok(())
    }

    /// 设置微任务策略（G1 默认 `Explicit`）。
    pub fn set_microtasks_policy(&mut self, policy: MicrotaskPolicy) {
        let mapped = match policy {
            MicrotaskPolicy::Explicit => v8::MicrotasksPolicy::Explicit,
            MicrotaskPolicy::Auto => v8::MicrotasksPolicy::Auto,
        };
        self.isolate.set_microtasks_policy(mapped);
    }

    /// 设置微任务墙钟守卫的宽限（供测试缩短；G1 的实际截断判据）。
    pub fn set_microtask_grace(&mut self, grace: Duration) {
        self.microtask_grace = grace;
    }

    /// 跑一次微任务 checkpoint。
    ///
    /// 返回**已执行一次 checkpoint** 的标记（恒 `1`）：V8 的 `perform_microtask_checkpoint`
    /// **不暴露**本次执行的微任务条数，故无法返回真实条数（如实登记，见
    /// [`crate::microtask::MicrotaskBudget`]）。
    ///
    /// 为防「自递归微任务」无界挂起，本方法启动一个**墙钟守卫线程**：超时则调用
    /// `IsolateHandle::terminate_execution` 打断（V8 对正在执行的 JS 会响应终止）；被打断
    /// ⇒ [`JsErrorKind::MicrotaskBudget`]。
    ///
    /// # Errors
    /// Realm 已关闭时报错；微任务超预算被截断时返回 [`JsErrorKind::MicrotaskBudget`]。
    pub fn perform_checkpoint(&mut self) -> Result<usize, JsError> {
        self.ensure_open()?;
        let realm: *mut Self = std::ptr::from_mut(self);
        // SAFETY（别名 + 生命周期）：经裸指针取堆上 Isolate 的 `&mut`；watchdog 线程只经
        // `TerminateHandle`（跨线程安全）在超时后打断；此后不再使用 `self`。
        // SAFETY: 别名与生命周期不变量见相邻论证与模块文档。
        #[allow(unsafe_code)]
        unsafe {
            let isolate: &mut v8::OwnedIsolate = &mut (*realm).isolate;
            // `G3X-S8`（③ / U-8 哨兵）：`perform_checkpoint` **不重写** `REALM_DATA_SLOT`，
            // 只依赖**上次 eval 写入的** Realm 指针 ⇒ G1 单 Realm 下恒真；**多 Realm（G4）化一旦发生
            // ⇒ 本断言 debug 即红**（防「上次 eval 的 Realm ≠ 当前 checkpoint 的 Realm」静默错配）。
            #[cfg(debug_assertions)]
            {
                let slot_realm = isolate.get_data(REALM_DATA_SLOT).cast::<Self>();
                debug_assert!(
                    std::ptr::eq(slot_realm, realm),
                    "REALM_DATA_SLOT 指向的 Realm ≠ 当前 checkpoint 的 Realm（U-8：G4 化前须显式重写该槽）"
                );
            }
            let terminate = (*realm).terminate.clone();
            let grace = (*realm).microtask_grace;

            let fired = std::sync::Arc::new(AtomicBool::new(false));
            let fired_watch = std::sync::Arc::clone(&fired);
            let (tx, rx) = mpsc::channel::<()>();
            let watchdog = std::thread::spawn(move || {
                if rx.recv_timeout(grace).is_err() {
                    fired_watch.store(true, Ordering::SeqCst);
                    terminate.terminate_execution();
                }
            });

            isolate.perform_microtask_checkpoint();
            // 正常完成：唤醒守卫线程（若仍在等待），使其不再触发终止。
            tx.send(()).ok();
            let _ = watchdog.join();

            if fired.load(Ordering::SeqCst) {
                // 恢复 isolate 的可执行状态（terminate 是粘性的）。
                isolate.cancel_terminate_execution();
                Err(JsError::new(
                    JsErrorKind::MicrotaskBudget,
                    "微任务超预算被墙钟守卫截断（V8 无法抢占，改用 watchdog + terminate）",
                ))
            } else {
                Ok(1)
            }
        }
    }

    /// 设置页面/模块根 URL（供动态 `import('./x')` 的相对解析）。
    pub fn set_module_root(&mut self, url: &str) {
        self.module_root.clear();
        self.module_root.push_str(url);
    }

    /// 设置模块宿主（解析/加载钩子）。
    pub fn set_module_host(&mut self, host: Box<dyn crate::module_host::ModuleHost>) {
        self.module_host = Some(host);
    }

    /// 求值一个 ES 模块，返回其命名空间（§3.2）。
    ///
    /// # Errors
    /// - [`JsErrorKind::Compile`]：模块语法 / 实例化失败。
    /// - [`JsErrorKind::Exception`]：模块求值抛出异常。
    /// - [`JsErrorKind::ReentrantEval`]：在回调内调用（重入被拒）。
    /// - [`JsErrorKind::Internal`]：Realm 已关闭。
    pub fn eval_module(&mut self, src: &str, url: &str) -> Result<HostValue, JsError> {
        let realm: *mut Self = std::ptr::from_mut(self);
        Self::eval_module_impl(realm, src, url)
    }

    /// 同 [`Self::eval_module`]，但带 **watchdog 墙钟上限**（ADR-0017·D2；U-12）：
    /// 模块执行超过 `deadline` 被打断 ⇒ [`JsErrorKind::Terminated`]（专有判据）。
    ///
    /// # Errors
    /// 同 [`Self::eval_module`]；或超 deadline 被打断（[`JsErrorKind::Terminated`]）。
    pub fn eval_module_with_deadline(
        &mut self,
        src: &str,
        url: &str,
        deadline: std::time::Duration,
    ) -> Result<HostValue, JsError> {
        self.ensure_open()?;
        let terminate = self.terminate.clone();
        let fired = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let fired_watch = std::sync::Arc::clone(&fired);
        let (tx, rx) = mpsc::channel::<()>();
        let watchdog = std::thread::spawn(move || {
            if rx.recv_timeout(deadline).is_err() {
                fired_watch.store(true, std::sync::atomic::Ordering::SeqCst);
                terminate.terminate_execution();
            }
        });

        let outcome = self.eval_module(src, url);
        tx.send(()).ok();
        let _ = watchdog.join();

        if fired.load(std::sync::atomic::Ordering::SeqCst) {
            // 终止是粘性的：复位以便 isolate 之后继续可用（G1 同款）。
            self.isolate.cancel_terminate_execution();
            if outcome.is_err() {
                return Err(JsError::new(
                    JsErrorKind::Terminated,
                    "模块执行超过 deadline 被 watchdog 截断",
                ));
            }
        }
        outcome
    }

    /// `eval_module` 的实现（别名/生命周期论证同 [`Self::eval_impl`]）。
    fn eval_module_impl(realm: *mut Self, src: &str, url: &str) -> Result<HostValue, JsError> {
        // SAFETY（别名 + 生命周期）：同 `eval_impl`。
        // SAFETY: 别名与生命周期不变量见相邻论证与模块文档。
        #[allow(unsafe_code)]
        unsafe {
            if (*realm).closed {
                return Err(closed_error());
            }
            if (*realm).in_callback {
                return Err(reentrant_error());
            }
            let identity = (*realm).identity;
            (*realm).module_root.clear();
            (*realm).module_root.push_str(url);
            let isolate: &mut v8::OwnedIsolate = &mut (*realm).isolate;
            isolate.set_data(REALM_DATA_SLOT, realm.cast());
            let context_ref: &v8::Global<v8::Context> = &(*realm).context;

            v8::scope_with_context!(let scope, isolate, context_ref);
            v8::tc_scope!(let try_catch, scope);

            // 命中缓存 ⇒ 直接复用（同一 URL 同一 namespace）。
            if let Some(cached) = (*realm).module_registry.get(url) {
                let module = v8::Local::new(try_catch, cached);
                let namespace = module.get_module_namespace();
                return host_from_value!(try_catch, &mut (*realm).handles, identity, namespace);
            }

            let source_string = v8::String::new(try_catch, src)
                .ok_or_else(|| JsError::new(JsErrorKind::Internal, "无法创建模块源字符串"))?;
            let resource_name: v8::Local<v8::Value> = v8::String::new(try_catch, url)
                .ok_or_else(|| JsError::new(JsErrorKind::Internal, "无法创建模块 URL 字符串"))?
                .into();
            // `is_module = true` 是硬要求（否则 `CompileModule` 直接 FATAL）。
            let origin = v8::ScriptOrigin::new(
                try_catch,
                resource_name,
                0,
                0,
                false,
                -1,
                None,
                false,
                false,
                true,
                None,
            );
            let mut source = v8::script_compiler::Source::new(source_string, Some(&origin));
            let Some(module) = v8::script_compiler::compile_module(try_catch, &mut source) else {
                return Err(JsError::new(
                    JsErrorKind::Compile,
                    caught_text!(try_catch, src),
                ));
            };
            let Some(true) = module.instantiate_module(try_catch, module_resolver) else {
                return Err(JsError::new(
                    JsErrorKind::Compile,
                    caught_text!(try_catch, "模块实例化失败"),
                ));
            };
            if module.evaluate(try_catch).is_none() {
                return Err(JsError::new(
                    JsErrorKind::Exception,
                    caught_text!(try_catch, "模块求值失败"),
                ));
            }
            let namespace = module.get_module_namespace();
            (*realm)
                .module_registry
                .insert(url.to_owned(), v8::Global::new(try_catch, module));
            host_from_value!(try_catch, &mut (*realm).handles, identity, namespace)
        }
    }

    /// 关闭：排空句柄表与原生函数注册表；**幂等**（重复调用无副作用）。
    ///
    /// # Errors
    /// 当前实现不产生错误。
    pub fn close(&mut self) -> Result<(), JsError> {
        if self.closed {
            return Ok(());
        }
        self.handles.drain();
        self.native_fns.clear();
        self.module_registry.clear();
        self.closed = true;
        Ok(())
    }

    /// 关闭后所有入口一律报错。
    fn ensure_open(&self) -> Result<(), JsError> {
        if self.closed {
            Err(closed_error())
        } else {
            Ok(())
        }
    }
}

/// 「回调内重入」错误（ADR-0014 决策 3）。
fn reentrant_error() -> JsError {
    JsError::new(
        JsErrorKind::ReentrantEval,
        "回调内重入被拒（ADR-0014 决策 3：可预测性，不依赖 v8『能跑通』行为）",
    )
}

/// 回调重入守卫：RAII，`Drop` **无条件**复位（含 panic 展开路径）。
pub(crate) struct CallbackGuard {
    realm: *mut Realm,
}

impl Drop for CallbackGuard {
    fn drop(&mut self) {
        // SAFETY: `self.realm` 来自 `enter_callback` 的裸指针；守卫存活期内 Realm 必然存活。
        // SAFETY: 复位只是一个 bool 写，无别名冲突（回调已退出）。
        #[allow(unsafe_code)]
        unsafe {
            (*self.realm).in_callback = false;
        }
    }
}

/// 置位重入守卫；已在回调中 ⇒ [`JsErrorKind::ReentrantEval`]。
pub(crate) fn enter_callback(realm: *mut Realm) -> Result<CallbackGuard, JsError> {
    // SAFETY: `realm` 由 trampoline 保证指向存活且唯一的 Realm（见 native_fn 文档）。
    // SAFETY: 仅读写 `in_callback: bool`，无别名冲突。
    #[allow(unsafe_code)]
    unsafe {
        if (*realm).in_callback {
            return Err(reentrant_error());
        }
        (*realm).in_callback = true;
    }
    Ok(CallbackGuard { realm })
}

/// 模块解析回调（`instantiate_module`）。
///
/// **静态 import 已实现**：v8 152 的 `ResolveModuleCallback` 适配器**不提供 scope**，故本回调
/// 用 v8 crate 自带的作用域桥 `CallbackScope::new(context)`（`isolate.rs:299` 同款）在回调栈上
/// 重建 scope，从而支持**静态 import 解析**（§3.2 `module_static_import_resolves`，已绿）。
///
/// **动态 import 见 [`dynamic_import_callback`]**：动态 `import()` 走 isolate 级回调
/// （`set_host_import_module_dynamically_callback`），与本静态回调各自独立。
#[allow(clippy::explicit_auto_deref, clippy::assigning_clones)]
fn module_resolver<'s>(
    context: v8::Local<'s, v8::Context>,
    specifier: v8::Local<'s, v8::String>,
    _import_attributes: v8::Local<'s, v8::FixedArray>,
    _referrer: v8::Local<'s, v8::Module>,
) -> Option<v8::Local<'s, v8::Module>> {
    // SAFETY（桥）：`CallbackScope::new` 是 v8 crate 自己在同类回调里用的作用域桥
    // （isolate.rs:299 同款）。本回调处于 V8 模块解析回调栈上、单线程，`context` 的 `Local`
    // 在本回调内有效，故由此构造 scope 安全。
    // SAFETY: 处于 V8 模块解析回调栈上、单线程；`context` 的 `Local` 在本回调内有效。
    #[allow(unsafe_code)]
    let storage = std::pin::pin!(unsafe { v8::CallbackScope::new(context) });
    let scope = storage.init();

    let realm_ptr = scope.get_data(REALM_DATA_SLOT).cast::<Realm>();
    if realm_ptr.is_null() {
        return None;
    }
    // SAFETY: `realm_ptr` 由 `eval_module_impl` 在实例化前写入，指向存活的 Realm。
    #[allow(unsafe_code)]
    let realm: &Realm = unsafe { &*realm_ptr };
    let host = realm.module_host.as_deref()?;

    let spec = specifier.to_rust_string_lossy(&*scope);
    let url = host.resolve(&realm.module_root, &spec)?;
    let text = host.load(&url).ok()?;

    let source_string = v8::String::new(&*scope, &text)?;
    let resource_name: v8::Local<'s, v8::Value> = v8::String::new(&*scope, &url)?.into();
    let origin = v8::ScriptOrigin::new(
        &*scope,
        resource_name,
        0,
        0,
        false,
        -1,
        None,
        false,
        false,
        true,
        None,
    );
    let mut source = v8::script_compiler::Source::new(source_string, Some(&origin));
    v8::script_compiler::compile_module(&*scope, &mut source)
}

/// 动态 `import()` 的宿主回调（`set_host_import_module_dynamically_callback`，K-11）。
///
/// # 不变量（K-11 根因根治）
///
/// V8 要求宿主回调返回**空 `MaybeLocal`**（`None`）**当且仅当存在可传播的挂起异常**；
/// 否则 `Isolate::UnwindAndFindHandler` 会以 `Check failed: has_exception()` **FATAL**
/// （V8 引擎级致命错误，非 Rust panic，进程 `SIGTRAP`）。因此本回调在**任何宿主侧失败**
/// （无宿主 / 解析失败 / 加载失败 / 建串失败）时，都**拒绝**本回调创建的那个 promise
/// （[`reject_import`]），**绝不**空返回；只有 V8 自身已挂起异常（模块编译 / 实例化 / 求值
/// 失败）时才返回 `None` 令其自然传播。
///
/// # 成功路径
///
/// 读穿模块缓存 → 未命中则编译 → 实例化 → 求值目标模块，以其 namespace 解析返回的
/// promise；并写回缓存（同一 URL 复用同一 namespace，与 `eval_module` 一致）。§3.2
/// `dynamic_import_specifier`。
fn dynamic_import_callback<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    _host_defined_options: v8::Local<'s, v8::Data>,
    resource_name: v8::Local<'s, v8::Value>,
    specifier: v8::Local<'s, v8::String>,
    _import_attributes: v8::Local<'s, v8::FixedArray>,
) -> Option<v8::Local<'s, v8::Promise>> {
    // 先建 resolver：此后每个失败路径都能「拒绝而非空返回」。
    let Some(resolver) = v8::PromiseResolver::new(scope) else {
        // 连 resolver 都建不出 ⇒ 只可能是进程级 OOM，V8 必已挂起异常 ⇒ 空返回是合法传播。
        return None;
    };
    let promise = resolver.get_promise(scope);

    // SAFETY（桥 + 生命周期）：本回调处于 V8 回调栈上、单线程；`REALM_DATA_SLOT` 由
    // `eval_impl` / `call_impl` / `eval_module_impl` 在运行 JS 前写入，指向存活且唯一的 Realm。
    // 本帧仅**只读**借用该 Realm（与静态解析回调 `module_resolver` 同构）。
    // SAFETY: 见相邻论证；`&*realm_ptr` 仅在下列只读访问期间存活（至 `host.load` 之后结束）。
    #[allow(unsafe_code)]
    let realm_ptr = scope.get_data(REALM_DATA_SLOT).cast::<Realm>();
    if realm_ptr.is_null() {
        return reject_import(scope, &resolver, promise, "动态 import：Realm 不可用");
    }
    // SAFETY: `realm_ptr` 指向存活 Realm；只读借用，无别名冲突（见上）。
    #[allow(unsafe_code)]
    let realm: &Realm = unsafe { &*realm_ptr };

    let Some(host) = realm.module_host.as_deref() else {
        return reject_import(scope, &resolver, promise, "动态 import：未设置模块宿主");
    };
    // referrer：优先取 V8 给的 `resource_name`（发起 import 的模块/脚本 URL），为空则回退根 URL。
    let referrer = if resource_name.is_string() {
        let text = resource_name.to_rust_string_lossy(scope);
        if text.is_empty() {
            realm.module_root.clone()
        } else {
            text
        }
    } else {
        realm.module_root.clone()
    };
    let spec = specifier.to_rust_string_lossy(scope);
    let Some(url) = host.resolve(&referrer, &spec) else {
        return reject_import(
            scope,
            &resolver,
            promise,
            &format!("动态 import：无法解析模块 '{spec}'"),
        );
    };

    // 读穿缓存：命中 ⇒ 直接以缓存命名空间解析（同一 URL 同一 namespace）。
    if let Some(cached) = realm.module_registry.get(&url) {
        let module = v8::Local::new(scope, cached);
        let namespace = module.get_module_namespace();
        let _ = resolver.resolve(scope, namespace);
        return Some(promise);
    }

    let text = match host.load(&url) {
        Ok(text) => text,
        Err(err) => return reject_import(scope, &resolver, promise, err.message()),
    };
    let Some(source_string) = v8::String::new(scope, &text) else {
        return reject_import(
            scope,
            &resolver,
            promise,
            "动态 import：无法创建模块源字符串",
        );
    };
    let Some(url_string) = v8::String::new(scope, &url) else {
        return reject_import(
            scope,
            &resolver,
            promise,
            "动态 import：无法创建模块 URL 字符串",
        );
    };
    let resource: v8::Local<'s, v8::Value> = url_string.into();
    let origin = v8::ScriptOrigin::new(
        scope, resource, 0, 0, false, -1, None, false, false, true, None,
    );
    let mut source = v8::script_compiler::Source::new(source_string, Some(&origin));

    // 以下任何一步失败 ⇒ V8 已挂起异常 ⇒ `?` 返回 `None` 令其传播（合法：has_exception() == true）。
    let module = v8::script_compiler::compile_module(scope, &mut source)?;
    let Some(true) = module.instantiate_module(scope, module_resolver) else {
        return None;
    };
    module.evaluate(scope)?;
    let namespace = module.get_module_namespace();
    let _ = resolver.resolve(scope, namespace);

    // 写回模块缓存：同一 URL 复用同一 namespace（与 `eval_module` 一致）。
    // SAFETY（别名）：上面的只读借用 `realm`（及派生的 `host`）最后一次使用是 `host.load`，
    // 至此已结束（NLL）；运行 JS 期间本帧不持有指向 Realm 结构体的引用（只持堆上 Isolate 的
    // `&mut`，见模块文档），故此处对 `module_registry` **字段**的 `&mut` 是唯一活跃借用，
    // 与 `eval_impl` 持有的 `context` 字段 `&` 亦不相交（不同字段），无别名冲突。
    // SAFETY: `realm_ptr` 指向存活 Realm；只写 `module_registry` 字段。
    #[allow(unsafe_code)]
    unsafe {
        (*realm_ptr)
            .module_registry
            .insert(url, v8::Global::new(scope, module));
    }
    Some(promise)
}

/// 以 `TypeError` **拒绝** `promise`（宿主侧失败的统一出口；K-11：绝不空返回）。
///
/// 返回 `Some(promise)`。仅在连消息字符串都建不出（进程级 OOM，V8 必已挂起异常）时返回
/// `None` 令 V8 传播。
fn reject_import<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    resolver: &v8::PromiseResolver,
    promise: v8::Local<'s, v8::Promise>,
    message: &str,
) -> Option<v8::Local<'s, v8::Promise>> {
    let Some(text) = v8::String::new(scope, message) else {
        // 连消息字符串都建不出 ⇒ 进程级 OOM，V8 必已挂起异常 ⇒ 空返回合法。
        return None;
    };
    let exception = v8::Exception::type_error(scope, text);
    let _ = resolver.reject(scope, exception);
    Some(promise)
}

/// 「Realm 已关闭」错误。
fn closed_error() -> JsError {
    JsError::new(JsErrorKind::Internal, "Realm 已关闭")
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::Realm;
    use crate::abi::HostValue;
    use crate::error::{JsError, JsErrorKind};
    use crate::isolate::AgentRuntime;
    use crate::microtask::MicrotaskPolicy;
    use crate::native_fn::NativeFn;
    use proptest::prelude::*;

    fn new_realm() -> Realm {
        let mut runtime = AgentRuntime::new().expect("建 AgentRuntime");
        runtime.create_realm().expect("建 Realm")
    }

    #[test]
    fn eval_basic_values() {
        let mut realm = new_realm();
        assert_eq!(realm.eval("1 + 2").unwrap(), HostValue::Number(3.0));
        assert_eq!(realm.eval("'a'").unwrap(), HostValue::Str("a".to_owned()));
        assert_eq!(realm.eval("null").unwrap(), HostValue::Null);
        assert_eq!(realm.eval("undefined").unwrap(), HostValue::Undefined);
        assert_eq!(realm.eval("true").unwrap(), HostValue::Bool(true));
    }

    #[test]
    fn eval_error_kind() {
        let mut realm = new_realm();
        let compile = realm.eval("(((").expect_err("语法错应报错");
        assert_eq!(compile.kind(), JsErrorKind::Compile);
        let thrown = realm
            .eval("throw new Error('boom')")
            .expect_err("抛异常应报错");
        assert_eq!(thrown.kind(), JsErrorKind::Exception);
        assert!(
            thrown.message().contains("boom"),
            "异常消息应含 'boom'，实际：{}",
            thrown.message()
        );
    }

    #[test]
    fn close_idempotent() {
        let mut realm = new_realm();
        realm.close().expect("首次 close 应成功");
        realm.close().expect("重复 close 应成功（幂等）");
        let after = realm.eval("1").expect_err("关闭后 eval 应报错");
        assert_eq!(after.kind(), JsErrorKind::Internal);
    }

    #[test]
    fn get_set_prop_roundtrip_sample() {
        let mut realm = new_realm();
        let object = realm.eval("({})").unwrap();
        let handle = match object {
            HostValue::Object { handle, .. } => handle,
            other => panic!("期望 Object，得到 {other:?}"),
        };
        let _lease = realm.root(handle).expect("root");
        realm
            .set_prop(handle, "answer", HostValue::Number(42.0))
            .expect("set_prop");
        assert_eq!(
            realm.get_prop(handle, "answer").unwrap(),
            HostValue::Number(42.0)
        );
    }

    #[test]
    fn call_returns_value() {
        let mut realm = new_realm();
        let function = realm.eval("(function (a, b) { return a + b; })").unwrap();
        let handle = match function {
            HostValue::Function { handle, .. } => handle,
            other => panic!("期望 Function，得到 {other:?}"),
        };
        let _lease = realm.root(handle).expect("root");
        let result = realm
            .call(
                handle,
                None,
                &[HostValue::Number(2.0), HostValue::Number(3.0)],
            )
            .expect("call");
        assert_eq!(result, HostValue::Number(5.0));
    }

    // ———————————————————————— S7 原生绑定 ————————————————————————

    /// 装 `__t` 对象并在其上放原生函数 `add` / `boom`。
    fn setup_native_object(realm: &mut Realm) {
        let adder: NativeFn = Box::new(|_realm: &mut Realm, args: &[HostValue]| {
            let sum = args
                .iter()
                .filter_map(|v| match v {
                    HostValue::Number(n) => Some(*n),
                    _ => None,
                })
                .sum();
            Ok(HostValue::Number(sum))
        });
        let handle = realm.new_function("add", 2, adder).expect("new add");

        let boom: NativeFn = Box::new(|_realm: &mut Realm, _args: &[HostValue]| {
            Err(crate::error::JsError::new(
                JsErrorKind::Exception,
                "boom from host",
            ))
        });
        let boom_handle = realm.new_function("boom", 0, boom).expect("new boom");

        let target = realm.eval("globalThis.__t = {}; __t").unwrap();
        let target_handle = match target {
            HostValue::Object { handle, .. } => handle,
            other => panic!("期望 Object，得到 {other:?}"),
        };
        let _lease = realm.root(target_handle).expect("root __t");
        let _add_lease = realm.root(handle).expect("root add");
        let _boom_lease = realm.root(boom_handle).expect("root boom");
        realm
            .set_prop(
                target_handle,
                "add",
                HostValue::Object {
                    realm: realm.id(),
                    handle,
                },
            )
            .expect("set __t.add");
        realm
            .set_prop(
                target_handle,
                "boom",
                HostValue::Object {
                    realm: realm.id(),
                    handle: boom_handle,
                },
            )
            .expect("set __t.boom");
    }

    #[test]
    fn native_fn_callable_and_returns() {
        let mut realm = new_realm();
        setup_native_object(&mut realm);
        assert_eq!(
            realm.eval("__t.add(2, 3)").unwrap(),
            HostValue::Number(5.0),
            "跨边界返回值应正确"
        );
    }

    #[test]
    fn native_fn_name_length_descriptor() {
        let mut realm = new_realm();
        setup_native_object(&mut realm);
        assert_eq!(
            realm.eval("__t.add.name").unwrap(),
            HostValue::Str("add".to_owned())
        );
        assert_eq!(
            realm.eval("__t.add.length").unwrap(),
            HostValue::Number(2.0)
        );
        assert_eq!(
            realm
                .eval("Object.getOwnPropertyDescriptor(__t,'add').writable")
                .unwrap(),
            HostValue::Bool(true)
        );
    }

    #[test]
    fn native_fn_throw_propagates() {
        let mut realm = new_realm();
        setup_native_object(&mut realm);
        assert_eq!(
            realm
                .eval("try { __t.boom(); 'no' } catch (e) { e.message }")
                .unwrap(),
            HostValue::Str("boom from host".to_owned()),
            "宿主异常应可被 JS 侧捕获并取回 message"
        );
    }

    #[test]
    fn object_identity_stable_within_realm() {
        // 同一 JS 对象两次取回 ⇒ 经任一 wrapper 的修改对另一 wrapper 可见（同一对象身份稳定）。
        //
        // ⚠️ 说明：G1 的弱槽表**不对普通对象做句柄去重**（每次取回分配新句柄）；「同一 wrapper」
        // 的复用由 `node_map`（C-3）承担，非本表职责。本测试证的是**对象身份**稳定。
        let mut realm = new_realm();
        let first = realm.eval("globalThis.__obj = {}; __obj").unwrap();
        let h1 = match first {
            HostValue::Object { handle, .. } => handle,
            other => panic!("期望 Object，得到 {other:?}"),
        };
        let _lease = realm.root(h1).expect("root h1");
        realm
            .set_prop(h1, "marker", HostValue::Number(7.0))
            .expect("set_prop");
        // 再次取回同一对象（新句柄）应看到同一次修改 ⇒ 是同一对象。
        let second = realm.eval("__obj").unwrap();
        let h2 = match second {
            HostValue::Object { handle, .. } => handle,
            other => panic!("期望 Object，得到 {other:?}"),
        };
        let _lease2 = realm.root(h2).expect("root h2");
        assert_eq!(
            realm.get_prop(h2, "marker").unwrap(),
            HostValue::Number(7.0),
            "两次取回应指向同一对象"
        );
    }

    #[test]
    fn native_tostring_contains_native_code() {
        let mut realm = new_realm();
        setup_native_object(&mut realm);
        let text = match realm.eval("__t.add.toString()").unwrap() {
            HostValue::Str(s) => s,
            other => panic!("期望 Str，得到 {other:?}"),
        };
        assert!(
            text.contains("native code"),
            "原生函数 toString 应含 'native code'，实际：{text}"
        );
    }

    #[test]
    fn native_tostring_snapshot() {
        // 钉 v8 152 实测串（新快照 = 新增 golden，允许；不改既有 golden）。
        let mut realm = new_realm();
        setup_native_object(&mut realm);
        let text = match realm.eval("__t.add.toString()").unwrap() {
            HostValue::Str(s) => s,
            other => panic!("期望 Str，得到 {other:?}"),
        };
        insta::assert_snapshot!("native_tostring", text);
    }

    // ———————————————————————— S8 微任务 ————————————————————————

    #[test]
    fn explicit_policy_defers_microtask() {
        let mut realm = new_realm();
        realm.set_microtasks_policy(MicrotaskPolicy::Explicit);
        // 挂一个 then，未 checkpoint 前 flag 应仍为 undefined。
        realm
            .eval("globalThis.flag = undefined; Promise.resolve().then(() => { globalThis.flag = 1; });")
            .unwrap();
        assert_eq!(
            realm.eval("globalThis.flag").unwrap(),
            HostValue::Undefined,
            "Explicit 下未 checkpoint 前不应跑微任务"
        );
        let count = realm.perform_checkpoint().expect("checkpoint");
        assert!(count >= 1, "checkpoint 应报告已执行");
        assert_eq!(
            realm.eval("globalThis.flag").unwrap(),
            HostValue::Number(1.0)
        );
    }

    #[test]
    fn no_auto_checkpoint_after_dom_op() {
        let mut realm = new_realm();
        realm.set_microtasks_policy(MicrotaskPolicy::Explicit);
        realm
            .eval("globalThis.flag2 = undefined; Promise.resolve().then(() => { globalThis.flag2 = 1; });")
            .unwrap();
        // 一个普通 eval 返回后**不**自动跑微任务。
        assert_eq!(
            realm.eval("globalThis.flag2").unwrap(),
            HostValue::Undefined
        );
    }

    #[test]
    fn checkpoint_runs_pending_microtasks() {
        let mut realm = new_realm();
        realm.set_microtasks_policy(MicrotaskPolicy::Explicit);
        realm
            .eval("globalThis.flag3 = undefined; Promise.resolve().then(() => { globalThis.flag3 = 1; });")
            .unwrap();
        let count = realm.perform_checkpoint().expect("checkpoint");
        assert!(count >= 1, "checkpoint 返回计数应 ≥1，实际 {count}");
        assert_eq!(
            realm.eval("globalThis.flag3").unwrap(),
            HostValue::Number(1.0)
        );
    }

    // ———————————————————————— S9 弱句柄 GC ————————————————————————

    #[test]
    fn weak_handle_empties_after_gc() {
        // WeakJsHandle 在值被 GC 后失效（C-2 的弱引用成立）：宿主侧只持弱引用。
        let mut realm = new_realm();
        let value = realm.eval("({})").unwrap();
        let handle = match value {
            HostValue::Object { handle, .. } => handle,
            other => panic!("期望 Object，得到 {other:?}"),
        };
        let weak = realm.weak(handle).expect("weak");
        assert_eq!(
            weak,
            crate::abi::WeakJsHandle::new(handle.index(), handle.generation())
        );
        // 宿主只持弱句柄（WeakJsHandle 是 u64，不保活）⇒ 强引用为空 ⇒ GC 可回收。
        realm.collect_garbage_for_testing().expect("gc");
        let resolved = realm.root(handle);
        assert!(
            matches!(resolved, Err(ref e) if e.kind() == JsErrorKind::Collected),
            "弱句柄在值被 GC 后应失效；得到 {resolved:?}"
        );
    }

    #[test]
    fn t2_1_leak_shape_avoided() {
        // 复刻 T2.1 exp3 的「宿主对象 ↔ 捕获它的闭包」环，但**宿主侧只走弱引用** ⇒ `alive === false`
        // （对照 T2.1 的 `CYCLE_LEAK=true`）。
        //
        // ⚠️ 判据边界（防误读）：本测试**只证**「弱引用路径不产生泄漏」，**不得**读成
        // 「跨语言环已能回收」——真正的跨堆追踪（ephemeron）本轮不做（ADR-0007 遗留）。
        let mut realm = new_realm();
        let value = realm
            .eval(
                "globalThis.__rc = (() => { const o = { tag: 'node' }; o.cb = () => o; return o; })(); __rc",
            )
            .unwrap();
        let handle = match value {
            HostValue::Object { handle, .. } => handle,
            other => panic!("期望 Object，得到 {other:?}"),
        };
        let _weak = realm.weak(handle).expect("weak");
        // 切断 JS 侧唯一外部可达（留下闭包→对象、对象→闭包的环）。
        realm.eval("delete globalThis.__rc").unwrap();
        // 宿主侧只持弱引用 ⇒ 环可被回收。
        realm.collect_garbage_for_testing().expect("gc");
        let resolved = realm.root(handle);
        assert!(
            matches!(resolved, Err(ref e) if e.kind() == JsErrorKind::Collected),
            "弱引用路径应可回收；得到 {resolved:?}"
        );
    }

    #[test]
    fn microtask_budget_exhaustion() {
        let mut realm = new_realm();
        realm.set_microtasks_policy(MicrotaskPolicy::Explicit);
        realm.set_microtask_grace(Duration::from_millis(200));
        // 自递归微任务（Promise 链）：无墙钟守卫会无界挂起；守卫应截断并报 MicrotaskBudget。
        realm
            .eval(
                "globalThis.__spin = function spin() { Promise.resolve().then(spin); }; Promise.resolve().then(__spin);",
            )
            .unwrap();
        let err = realm.perform_checkpoint().expect_err("无界微任务应被截断");
        assert_eq!(err.kind(), JsErrorKind::MicrotaskBudget);
        // 截断后 isolate 应恢复可用（cancel_terminate_execution）。
        assert!(realm.eval("1 + 1").is_ok(), "截断后 isolate 应恢复可执行");
    }

    // ———————————————— P-1a：watchdog 能否截断 `eval` 内的纯计算死循环 ————————————————

    /// 复用 `perform_checkpoint` 的墙钟守卫现形包裹 `eval`：返回（结果, 是否触发终止, 用时）。
    ///
    /// 现形与 `perform_checkpoint`（`realm.rs:501-518`）一致：`TerminateHandle` + grace +
    /// 独立线程 + `recv_timeout` + `terminate_execution` + `join`；结束若触发过则
    /// `cancel_terminate_execution` 复位（terminate 是粘性的）。**仅测试内辅助**——公开的
    /// `eval_with_deadline` 形态待 ADR 定。
    fn eval_with_watchdog(
        realm: &mut Realm,
        src: &str,
        grace: Duration,
    ) -> (Result<HostValue, JsError>, bool, Duration) {
        let terminate = realm.terminate.clone();
        let fired = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let fired_watch = std::sync::Arc::clone(&fired);
        let (tx, rx) = std::sync::mpsc::channel::<()>();
        let watchdog = std::thread::spawn(move || {
            if rx.recv_timeout(grace).is_err() {
                fired_watch.store(true, std::sync::atomic::Ordering::SeqCst);
                terminate.terminate_execution();
            }
        });
        let start = std::time::Instant::now();
        let outcome = realm.eval(src);
        let elapsed = start.elapsed();
        tx.send(()).ok();
        let _ = watchdog.join();
        let was_fired = fired.load(std::sync::atomic::Ordering::SeqCst);
        if was_fired {
            realm.isolate.cancel_terminate_execution();
        }
        (outcome, was_fired, elapsed)
    }

    #[test]
    fn eval_watchdog_terminates_infinite_loop() {
        // 防 flaky（对齐 `microtask_budget_exhaustion` 先例）：200ms 宽限 + 稳定复跑 3 轮，
        // 每轮断言「终止触发 + 用时 >= 宽限 + 截断后 isolate 恢复可用」。
        let mut realm = new_realm();
        let grace = Duration::from_millis(200);
        for round in 0..3 {
            let (outcome, fired, elapsed) =
                eval_with_watchdog(&mut realm, "while (true) {}", grace);
            assert!(fired, "第 {round} 轮：watchdog 应触发终止");
            let err = outcome.expect_err("纯计算死循环应被截断（不得正常返回）");
            eprintln!(
                "P-1a round {round}: kind={:?} msg={} elapsed={elapsed:?}",
                err.kind(),
                err.message()
            );
            assert!(
                elapsed >= grace,
                "第 {round} 轮：用时 {elapsed:?} 应 >= grace {grace:?}"
            );
            // 截断后隔离状态复位（cancel_terminate_execution）⇒ 可继续执行。
            assert!(
                realm.eval("1 + 1").is_ok(),
                "第 {round} 轮：截断后 isolate 应恢复可执行"
            );
        }
    }

    /// 取全局对象句柄。
    fn global_handle(realm: &mut Realm) -> crate::abi::JsHandle {
        match realm.eval("globalThis").unwrap() {
            HostValue::Object { handle, .. } => handle,
            other => panic!("期望 Object，得到 {other:?}"),
        }
    }

    /// 取对象句柄。
    fn as_object(value: HostValue) -> crate::abi::JsHandle {
        match value {
            HostValue::Object { handle, .. } => handle,
            other => panic!("期望 Object，得到 {other:?}"),
        }
    }

    #[test]
    fn reentrant_native_to_native_rejected() {
        // native fn 内 eval ⇒ 重入被守卫拒绝（返回其 kind 供观测）。
        let mut realm = new_realm();
        let probe: NativeFn =
            Box::new(
                |realm: &mut Realm, _args: &[HostValue]| match realm.eval("1 + 1") {
                    Err(err) => Ok(HostValue::Str(format!("{:?}", err.kind()))),
                    Ok(_) => Ok(HostValue::Str("no-error".to_owned())),
                },
            );
        let handle = realm.new_function("probe", 0, probe).unwrap();
        let global = global_handle(&mut realm);
        let _lg = realm.root(global).unwrap();
        realm
            .set_prop(
                global,
                "probe",
                HostValue::Object {
                    realm: realm.id(),
                    handle,
                },
            )
            .unwrap();
        let result = realm.eval("probe()").unwrap();
        assert!(
            matches!(result, HostValue::Str(ref s) if s.contains("ReentrantEval")),
            "回调内 eval 应被拒为 ReentrantEval，得到 {result:?}"
        );
    }

    #[test]
    fn reentrant_in_microtask_rejected() {
        // 微任务期间隐式重入：微任务 → native fn → eval ⇒ 被拒。
        let mut realm = new_realm();
        realm.set_microtasks_policy(MicrotaskPolicy::Explicit);
        let probe: NativeFn =
            Box::new(
                |realm: &mut Realm, _args: &[HostValue]| match realm.eval("1 + 1") {
                    Err(err) => Ok(HostValue::Str(format!("{:?}", err.kind()))),
                    Ok(_) => Ok(HostValue::Str("no-error".to_owned())),
                },
            );
        let handle = realm.new_function("probe", 0, probe).unwrap();
        let global = global_handle(&mut realm);
        let _lg = realm.root(global).unwrap();
        realm
            .set_prop(
                global,
                "probe",
                HostValue::Object {
                    realm: realm.id(),
                    handle,
                },
            )
            .unwrap();
        realm
            .eval("globalThis.__res = 'pending'; Promise.resolve().then(() => { globalThis.__res = probe(); });")
            .unwrap();
        realm.perform_checkpoint().unwrap();
        let result = realm.eval("globalThis.__res").unwrap();
        assert!(
            matches!(result, HostValue::Str(ref s) if s.contains("ReentrantEval")),
            "微任务内 native 重入应被拒，得到 {result:?}"
        );
    }

    // ———————————————————————— S10 模块 / WASM ————————————————————————

    #[test]
    fn module_export_value() {
        let mut realm = new_realm();
        let namespace = realm
            .eval_module("export const x = 42;", "mem://m.js")
            .unwrap();
        let handle = as_object(namespace);
        let _lease = realm.root(handle).unwrap();
        assert_eq!(
            realm.get_prop(handle, "x").unwrap(),
            HostValue::Number(42.0)
        );
    }

    #[test]
    fn module_syntax_error_kind() {
        let mut realm = new_realm();
        let err = realm
            .eval_module("export const x = ;", "mem://bad.js")
            .expect_err("模块语法错应报错");
        assert_eq!(
            err.kind(),
            JsErrorKind::Compile,
            "坏模块应报 Compile（非 panic）"
        );
    }

    #[test]
    fn module_static_import_resolves() {
        let mut realm = new_realm();
        let mut host = crate::module_host::MemoryModuleHost::new();
        host.insert("mem://dep.js", "export const v = 7;");
        host.insert(
            "mem://root.js",
            "import { v } from './dep.js'; export const w = v + 1;",
        );
        realm.set_module_host(Box::new(host));
        let namespace = realm
            .eval_module(
                "import { v } from './dep.js'; export const w = v + 1;",
                "mem://root.js",
            )
            .unwrap();
        let handle = as_object(namespace);
        let _lease = realm.root(handle).unwrap();
        assert_eq!(
            realm.get_prop(handle, "w").unwrap(),
            HostValue::Number(8.0),
            "静态 import 应经宿主解析器命中并求值"
        );
    }

    #[test]
    fn module_cache_identity() {
        let mut realm = new_realm();
        let src = "export const y = 1;";
        let url = "mem://c.js";
        let first = as_object(realm.eval_module(src, url).unwrap());
        let global = global_handle(&mut realm);
        let _lg = realm.root(global).unwrap();
        let _l1 = realm.root(first).unwrap();
        realm
            .set_prop(
                global,
                "__ns1",
                HostValue::Object {
                    realm: realm.id(),
                    handle: first,
                },
            )
            .unwrap();
        let second = as_object(realm.eval_module(src, url).unwrap());
        let _l2 = realm.root(second).unwrap();
        realm
            .set_prop(
                global,
                "__ns2",
                HostValue::Object {
                    realm: realm.id(),
                    handle: second,
                },
            )
            .unwrap();
        assert_eq!(
            realm.eval("globalThis.__ns1 === globalThis.__ns2").unwrap(),
            HostValue::Bool(true),
            "同一 URL 两次加载应复用同一 namespace"
        );
    }

    #[test]
    fn wasm_instantiate_and_call() {
        // 内置最小 wasm 字节码（add:i32,i32->i32）；经 JS `WebAssembly` 走真实路径。
        let mut realm = new_realm();
        let js = r"
          const bytes = new Uint8Array([0,97,115,109,1,0,0,0,1,7,1,96,2,127,127,1,127,3,2,1,0,7,7,1,3,97,100,100,0,0,10,9,1,7,0,32,0,32,1,106,11]);
          const mod = new WebAssembly.Module(bytes);
          const inst = new WebAssembly.Instance(mod);
          inst.exports.add(2, 3);
        ";
        assert_eq!(
            realm.eval(js).unwrap(),
            HostValue::Number(5.0),
            "wasm instance.exports.add(2,3) 应为 5"
        );
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(24))]

        #[test]
        fn get_set_prop_roundtrip(values in proptest::collection::vec(proptest::num::f64::NORMAL, 1..6)) {
            let mut realm = new_realm();
            let object = realm.eval("({})").unwrap();
            let handle = match object {
                HostValue::Object { handle, .. } => handle,
                other => panic!("期望 Object，得到 {other:?}"),
            };
            let _lease = realm.root(handle).expect("root");
            for (index, value) in values.iter().enumerate() {
                let key = format!("k{index}");
                realm.set_prop(handle, &key, HostValue::Number(*value)).expect("set_prop");
                match realm.get_prop(handle, &key).expect("get_prop") {
                    HostValue::Number(got) => prop_assert_eq!(got.to_bits(), value.to_bits()),
                    other => panic!("期望 Number，得到 {other:?}"),
                }
            }
        }
    }

    // ———————————————————————— S10 动态 import（K-11） ————————————————————————

    /// 建一个带模块宿主的 Realm（模块图：`mem://m.js`）。
    fn realm_with_module_host() -> Realm {
        let mut realm = new_realm();
        realm.set_microtasks_policy(MicrotaskPolicy::Explicit);
        let mut host = crate::module_host::MemoryModuleHost::new();
        host.insert("mem://m.js", "export const v = 7;");
        realm.set_module_host(Box::new(host));
        realm.set_module_root("mem://root.js");
        realm
    }

    #[test]
    fn dynamic_import_specifier() {
        // §3.2：`import('./m.js')` 经宿主解析器命中并 resolve 为命名空间（K-11 验收）。
        let mut realm = realm_with_module_host();
        realm
            .eval(
                "globalThis.__dyn = 'unset'; import('./m.js').then(ns => { globalThis.__dyn = ns.v; });",
            )
            .expect("触发 import() 应不报错");
        realm.perform_checkpoint().expect("跑微任务");
        assert_eq!(
            realm.eval("globalThis.__dyn").unwrap(),
            HostValue::Number(7.0),
            "import('./m.js') 应经宿主解析器命中并 resolve 为命名空间"
        );
    }

    #[test]
    fn dynamic_import_unresolvable_rejects_without_fatal() {
        // K-11 回归护栏：宿主侧失败时回调**必须拒绝 promise**，绝不空返回（空返回 ⇒ V8 FATAL）。
        let mut realm = realm_with_module_host();
        realm
            .eval(
                "globalThis.__err = 'unset'; import('./missing.js').catch(e => { globalThis.__err = String(e); });",
            )
            .expect("解析失败应表现为 promise 拒绝（非同步抛出、更非 FATAL）");
        realm.perform_checkpoint().expect("跑微任务");
        let text = match realm.eval("globalThis.__err").unwrap() {
            HostValue::Str(text) => text,
            other => panic!("期望 Str，得到 {other:?}"),
        };
        assert!(
            text.contains("missing.js"),
            "拒绝原因应含 specifier，实际：{text}"
        );
    }

    #[test]
    fn dynamic_import_caches_namespace_identity() {
        // 动态 import 与 `eval_module` 共用模块缓存：同一 URL 同一 namespace。
        let mut realm = realm_with_module_host();
        let namespace = as_object(
            realm
                .eval_module("export const v = 7;", "mem://m.js")
                .unwrap(),
        );
        let _lease = realm.root(namespace).unwrap();
        let global = global_handle(&mut realm);
        let _lg = realm.root(global).unwrap();
        realm
            .set_prop(
                global,
                "__ns",
                HostValue::Object {
                    realm: realm.id(),
                    handle: namespace,
                },
            )
            .unwrap();
        realm
            .eval(
                "globalThis.__same = 'unset'; import('./m.js').then(ns => { globalThis.__same = (ns === globalThis.__ns); });",
            )
            .unwrap();
        realm.perform_checkpoint().unwrap();
        assert_eq!(
            realm.eval("globalThis.__same").unwrap(),
            HostValue::Bool(true),
            "同一 URL 的动态 import 与 eval_module 应复用同一 namespace"
        );
    }

    // ———————————————— S7：engine-v8 增量（new_object/resolve_weak/call_weak/…） ————————————————

    #[test]
    fn new_object_roundtrip() {
        let mut realm = new_realm();
        let handle = realm.new_object().expect("new_object");
        let _lease = realm.root(handle).expect("root");
        realm
            .set_prop(handle, "x", HostValue::Number(3.0))
            .expect("set_prop");
        assert_eq!(realm.get_prop(handle, "x").unwrap(), HostValue::Number(3.0));
        assert!(realm.new_array().is_ok(), "new_array 应成功");
        // set_global：脚本可见。
        realm
            .set_global("__g", HostValue::Number(7.0))
            .expect("set_global");
        assert_eq!(realm.eval("__g").unwrap(), HostValue::Number(7.0));
    }

    #[test]
    fn resolve_weak_collected() {
        // ADR-0019·D5：解析弱句柄；失效（槽释放）⇒ StaleHandle。
        let mut realm = new_realm();
        let handle = realm.new_object().expect("obj");
        let weak = realm.handles.weak(handle).expect("weak");
        assert!(
            realm.resolve_weak(weak).unwrap().is_some(),
            "存活时应可解析"
        );
        realm.handles.free(handle).expect("free");
        assert_eq!(
            realm.resolve_weak(weak).expect_err("失效").kind(),
            JsErrorKind::StaleHandle
        );
    }

    #[test]
    fn call_weak_atomic() {
        // 解析 + 调用原子入口（无两步回收窗口）。
        let mut realm = new_realm();
        let function = match realm.eval("(() => 42)").unwrap() {
            HostValue::Function { handle, .. } => handle,
            other => panic!("期望 Function，得到 {other:?}"),
        };
        let _lease = realm.root(function).expect("root");
        let weak = realm.handles.weak(function).expect("weak");
        assert!(realm.resolve_weak(weak).unwrap().is_some(), "存活");
        assert_eq!(
            realm.call_weak(weak, None, &[]).unwrap(),
            HostValue::Number(42.0)
        );
        // 分派专用调用入口亦可用（常规路径一致）。
        assert_eq!(
            realm.dispatch_call(function, None, &[]).unwrap(),
            HostValue::Number(42.0)
        );
    }

    #[test]
    fn eval_with_deadline_terminates() {
        // ADR-0017·D2：watchdog 截断 ⇒ `Terminated`（专有判据）。
        let mut realm = new_realm();
        assert_eq!(
            realm
                .eval_with_deadline("1 + 1", Duration::from_secs(5))
                .unwrap(),
            HostValue::Number(2.0)
        );
        let err = realm
            .eval_with_deadline("while (true) {}", Duration::from_millis(200))
            .expect_err("死循环应被 watchdog 截断");
        assert_eq!(
            err.kind(),
            JsErrorKind::Terminated,
            "应为 Terminated 专有判据，实得 {err:?}"
        );
        // 截断后 isolate 恢复可用。
        assert!(realm.eval("1 + 1").is_ok(), "截断后应恢复可执行");
    }
}
