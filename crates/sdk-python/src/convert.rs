//! `HostValue` ↔ Python 转换（施工图 §2.2，ADR-0009 决策 2）。
//!
//! 方向唯一：`engine-v8 → sdk-python`（`use engine_v8::{HostValue, JsHandle, RealmId}`）。
//! 转换放这里、**不放** `engine-v8`（避免把 Python 转换点塞进 unsafe 白名单）。
//!
//! # 类型映射（§2.2 表，逐项可测）
//!
//! | Rust `HostValue` | Python 侧 | 说明 |
//! |---|---|---|
//! | `Undefined` | `iv8.undefined`（单例哨兵） | 由 [`PyUndefined`] + [`undefined_singleton`] 保证同一性 |
//! | `Null` | `None` | |
//! | `Bool` | `bool` | |
//! | `Number` | `float` | 统一按 `f64` |
//! | `Str` | `str` | |
//! | `Object`/`Array`/`Function` | [`PyJsRef`] | 内持 `HandleLease`（ADR-0013 决策 2） |
//!
//! # 跨 Realm 摩擦（§2.2 末条，故意为之）
//!
//! [`PyJsRef`] 只能回到**同一 Realm**——否则返回 [`JsErrorKind::WrongRealm`]。
//! 这是**故意制造摩擦**，防止把句柄当普通值跨 Realm 传递。

use pyo3::prelude::*;
use pyo3::sync::PyOnceLock;
use pyo3::types::{PyBool, PyFloat, PyString};

use engine_v8::error::{JsError, JsErrorKind};
use engine_v8::handles::HandleLease;
use engine_v8::realm::Realm;
use engine_v8::{HostValue, JsHandle, RealmId};

/// `iv8.undefined` 单例哨兵类型：Python 侧表示 JS `undefined`。
///
/// 命名加业务前缀 `Py`，且**不叫** `Probe`——规避 pyo3 内部 `trait Probe` 的命名冲突
/// （施工图 §2.5 注意项 2/3）。
#[pyclass(frozen, name = "UndefinedType", module = "iv8")]
#[derive(Debug)]
pub struct PyUndefined;

/// 进程级单例：保证 `iv8.undefined is iv8.undefined` 恒真。
///
/// pyo3 0.29：`GILOnceCell` 已私有化，公开替身为 [`PyOnceLock`]（API 同形）。
static UNDEFINED: PyOnceLock<Py<PyUndefined>> = PyOnceLock::new();

/// 取 `iv8.undefined` 单例（首次调用时创建，其后复用同一对象）。
///
/// # Errors
/// 首次创建 `PyUndefined` 实例失败时报 `PyErr`。
pub fn undefined_singleton(py: Python<'_>) -> PyResult<Py<PyUndefined>> {
    UNDEFINED
        .get_or_try_init(py, || Py::new(py, PyUndefined))
        .map(|cell| cell.clone_ref(py))
}

/// 把 `RealmId` 打包成契约口径的 `u64`（高 32 = isolate 序号，低 32 = realm 序号）。
///
/// 与 `contracts/iv8_contract/v8_abi.py::pack_realm` 的位序**逐位一致**（ADR-0009 决策 2）。
#[must_use]
fn pack_realm(id: RealmId) -> u64 {
    (u64::from(id.isolate_seq()) << 32) | u64::from(id.realm_seq())
}

/// 把 `JsHandle` 打包成契约口径的 `u64`（高 32 = generation，低 32 = 槽位下标）。
#[must_use]
fn pack_handle(handle: JsHandle) -> u64 {
    (u64::from(handle.generation()) << 32) | u64::from(handle.index())
}

/// JS 引用在 Python 侧的载体（ADR-0013 决策 2）。
///
/// **内持 `HandleLease`** ⇒ Python 侧**默认存活且可用**；`Drop` 即取消 rooting。
/// 「句柄非根」仍成立：根在这个 lease 里，**不在** `JsHandle` 本身。
#[pyclass(name = "JsRef", module = "iv8")]
pub struct PyJsRef {
    /// 该引用所属 Realm。
    realm: RealmId,
    /// 该对象在所属 Realm 内的句柄。
    handle: JsHandle,
    /// 强引用本体（存活由它显式承载；只持有、不读取）。
    _lease: HandleLease,
}

impl PyJsRef {
    /// 由 `(realm, handle, lease)` 构造（供 `convert` / `callback` 使用）。
    pub(crate) fn new(realm: RealmId, handle: JsHandle, lease: HandleLease) -> Self {
        Self {
            realm,
            handle,
            _lease: lease,
        }
    }
}

#[pymethods]
impl PyJsRef {
    /// 所属 Realm 的打包整型标识（契约口径 `u64`）。
    #[getter]
    fn realm_id(&self) -> u64 {
        pack_realm(self.realm)
    }

    /// 句柄的打包整型标识（契约口径 `u64`）。
    #[getter]
    fn handle_id(&self) -> u64 {
        pack_handle(self.handle)
    }

    /// 可读表示（调试用）。
    fn __repr__(&self) -> String {
        format!(
            "iv8.JsRef(realm_id={}, handle_id={})",
            pack_realm(self.realm),
            pack_handle(self.handle)
        )
    }
}

/// `HostValue` → Python 对象（§2.2）。
///
/// 对象/数组/函数类值转成 [`PyJsRef`]，并**当场 `root`**（取得 `HandleLease`）
/// 以保证 Python 侧默认存活。
///
/// # Errors
/// - `HostValue` 携带的 realm 与 `realm.id()` 不一致 ⇒ `WrongRealm`（转成 `PyErr`）。
/// - 句柄失效（`StaleHandle` / `Collected`）或创建 Python 对象失败时报错。
pub fn host_to_py(py: Python<'_>, realm: &mut Realm, value: &HostValue) -> PyResult<Py<PyAny>> {
    match value {
        HostValue::Undefined => Ok(undefined_singleton(py)?.into_any()),
        HostValue::Null => Ok(py.None()),
        HostValue::Bool(flag) => Ok(PyBool::new(py, *flag).to_owned().into_any().unbind()),
        HostValue::Number(number) => Ok(PyFloat::new(py, *number).into_any().unbind()),
        HostValue::Str(text) => Ok(PyString::new(py, text).into_any().unbind()),
        HostValue::Object { realm: rid, handle }
        | HostValue::Array { realm: rid, handle }
        | HostValue::Function { realm: rid, handle } => {
            if *rid != realm.id() {
                return Err(crate::js_error_to_pyerr(JsError::new(
                    JsErrorKind::WrongRealm,
                    format!(
                        "HostValue 的 realm {} 与目标 realm {} 不一致",
                        pack_realm(*rid),
                        pack_realm(realm.id())
                    ),
                )));
            }
            let lease = realm.root(*handle).map_err(crate::js_error_to_pyerr)?;
            Ok(Py::new(py, PyJsRef::new(*rid, *handle, lease))?.into_any())
        }
    }
}

/// Python 对象 → `HostValue`（§2.2）。
///
/// 仅接受已登记类型；**不可转换即明确报错**（不静默 `None`）。
///
/// # 与施工图 §2.2 签名的偏差（供文档同步）
/// 施工图写 `py_to_host(py, obj)`，但「`PyJsRef` 只能回到同一 Realm（否则 `WrongRealm`）」
/// 这条语义**必须**有目标 realm 才能判定，且本函数不使用 GIL token，故签名为
/// `py_to_host(target_realm, obj)`（去掉未用参数、补上必需的 realm）。
///
/// # Errors
/// - `PyJsRef` 携带的 realm 与 `target_realm` 不一致 ⇒ [`JsErrorKind::WrongRealm`]。
/// - 对象类型不在映射表内 ⇒ [`JsErrorKind::Internal`]（附 Python 类型名）。
pub fn py_to_host(target_realm: RealmId, obj: &Bound<'_, PyAny>) -> Result<HostValue, JsError> {
    if obj.is_none() {
        return Ok(HostValue::Null);
    }
    if obj.is_instance_of::<PyUndefined>() {
        return Ok(HostValue::Undefined);
    }
    // `bool` 是 `int` 的子类，必须先于数字判定。
    if let Ok(flag) = obj.cast::<PyBool>() {
        return Ok(HostValue::Bool(flag.is_true()));
    }
    if let Ok(number) = obj.extract::<f64>() {
        return Ok(HostValue::Number(number));
    }
    if let Ok(text) = obj.cast::<PyString>() {
        return Ok(HostValue::Str(text.to_string_lossy().into_owned()));
    }
    if let Ok(js_ref) = obj.cast::<PyJsRef>() {
        let borrowed = js_ref.borrow();
        if borrowed.realm != target_realm {
            return Err(JsError::new(
                JsErrorKind::WrongRealm,
                format!(
                    "PyJsRef 的 realm {} 与目标 realm {} 不一致",
                    pack_realm(borrowed.realm),
                    pack_realm(target_realm)
                ),
            ));
        }
        return Ok(HostValue::Object {
            realm: borrowed.realm,
            handle: borrowed.handle,
        });
    }
    let type_name = obj
        .get_type()
        .name()
        .map_or_else(|_| "?".to_owned(), |name| name.to_string());
    Err(JsError::new(
        JsErrorKind::Internal,
        format!("无法把 Python 对象（类型 {type_name}）转换为 HostValue"),
    ))
}
