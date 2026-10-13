//! sdk-python —— `PyO3` Python 接口层：iv8 兼容层与原生控制层（S12 + G2 页面控制面 S14）。
//!
//! # 本 crate 的边界
//!
//! - 只做「**值转换**（[`convert`]）+ **回调派发**（[`callback`]）+ **页面控制面投影**（[`page`]）」，
//!   不做 V8 专属逻辑；
//! - 继承全仓 lints（含 `unsafe_code = "forbid"`）——**不写任何手工 `unsafe`**（含
//!   `assume_gil_acquired`）。U-1 已实证：`forbid` 下 `PyO3` 宏编译通过、手工 `unsafe` 确凿报错；
//! - `v8::` 路径零出现（铁律 5）：只用 `engine_v8::` 的安全类型面。
//!
//! # G2：`PyContext` 改持 `BrowsingContext`（ADR-0018·D5）
//!
//! `PyContext` / `PyPage` / `PyNetwork` / `PyInput` **共享同一个 `Rc<RefCell<BrowsingContext>>`**
//! （**不用** `Py<PyContext>` 反向引用，避免 Python 级环 + 借用交织）。G2 新增入口一律
//! `try_borrow(_mut)` → 投影为 [`ReentrantEvalError`]（**可判定错误，非 `RefCell` panic**）。
//! G1 的 `eval`/`eval_module`/`expose`/`perform_checkpoint`/`control_eval`/`close` 语义不变。

pub mod callback;
pub mod convert;
pub mod page;

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use pyo3::create_exception;
use pyo3::exceptions::PyException;
use pyo3::prelude::*;
use pyo3::types::PyDict;

use engine_v8::HostValue;
use engine_v8::error::JsError;

use navigation::error::PageError;
use navigation::page::BrowsingContext;
use scheduler::{RunBudget, RunOutcome};

use crate::callback::DEFAULT_DEADLINE_MS;
use crate::convert::{PyJsRef, PyUndefined, host_to_py, undefined_singleton};
use crate::page::{PyInput, PyNetLogEntry, PyNetwork, PyPage, PyResponse, PySnapshot};

create_exception!(
    iv8,
    Iv8Error,
    PyException,
    "iv8 引擎错误（消息前缀携带 JsErrorKind 名，便于契约断言）"
);
create_exception!(
    iv8,
    ReentrantEvalError,
    PyException,
    "回调内对同一 Context 的重入被拒（ADR-0014 决策 3）"
);

/// `PyContext` 建页时使用的默认 URL（G1 兼容层不下发文档，仍需一个可解析 origin）。
const DEFAULT_URL: &str = "https://localhost/";

/// 把引擎 [`JsError`] 转成 Python 异常。
///
/// 消息格式为 `"{JsErrorKind:?}: {message}"`，使 Python / JS 侧都能按**稳定前缀**断言失败类别。
// 按值签名是有意为之：调用点统一写作 `.map_err(js_error_to_pyerr)`，最简洁。
#[allow(clippy::needless_pass_by_value)]
pub(crate) fn js_error_to_pyerr(err: JsError) -> PyErr {
    PyErr::new::<Iv8Error, _>(format!("{:?}: {}", err.kind(), err.message()))
}

/// 把页面层 [`PageError`] 转成 Python 异常（消息前缀 = `PageErrorKind` 名，便于契约断言）。
#[allow(clippy::needless_pass_by_value)]
pub(crate) fn page_error_to_pyerr(err: PageError) -> PyErr {
    PyErr::new::<Iv8Error, _>(format!("{:?}: {}", err.kind(), err.message()))
}

/// 构造「回调内重入」Python 异常（[`ReentrantEvalError`]）。
pub(crate) fn reentrant_pyerr() -> PyErr {
    PyErr::new::<ReentrantEvalError, _>(
        "ReentrantEval：回调内不得重入同一 Context（ADR-0014 决策 3）",
    )
}

/// `RunOutcome` → Python 侧稳定字符串名。
fn outcome_name(outcome: RunOutcome) -> &'static str {
    match outcome {
        RunOutcome::Idle => "Idle",
        RunOutcome::BudgetExhausted => "BudgetExhausted",
        RunOutcome::DeadlineExceeded => "DeadlineExceeded",
        RunOutcome::WaitingIo => "WaitingIo",
        RunOutcome::Closed => "Closed",
    }
}

/// G1/G2 最小可用 JS 上下文（Python 侧入口）。
///
/// 内部持一个 [`BrowsingContext`]（内含 `ControlPlane`→`Realm`），以 `Rc<RefCell<…>>` 包裹：
/// `&self` 方法 + 显式重入探测。因内含 V8 Isolate（`!Send`），本类型为 `unsendable`。
#[pyclass(unsendable, name = "Context", module = "iv8")]
pub struct PyContext {
    /// 页面浏览上下文（与 `PyPage`/`PyNetwork`/`PyInput` 共享）。
    browser: Rc<RefCell<BrowsingContext>>,
    /// 回调截止期限（毫秒；宿主动态自持，见 [`PyContext::set_callback_deadline_ms`]）。
    callback_deadline_ms: std::sync::Arc<std::sync::atomic::AtomicU64>,
    /// 兼容层模式（记录用；完整 §16 兼容层属 G4）。
    mode: String,
    /// 时钟模式（记录用；完整 §16 兼容层属 G4）。
    time_mode: String,
}

#[pymethods]
impl PyContext {
    /// 建一个 JS 上下文（单 Realm；内含一个默认 `BrowsingContext`）。
    ///
    /// `mode` / `time_mode` 目前**仅记录**（§16 完整兼容层属 G4）。
    ///
    /// # Errors
    /// 建 Isolate / Context / 页面桥失败时抛 [`Iv8Error`]。
    #[new]
    #[pyo3(signature = (mode=None, time_mode=None))]
    fn new(mode: Option<&str>, time_mode: Option<&str>) -> PyResult<Self> {
        let browser = BrowsingContext::new(DEFAULT_URL).map_err(page_error_to_pyerr)?;
        Ok(Self {
            browser: Rc::new(RefCell::new(browser)),
            callback_deadline_ms: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(
                DEFAULT_DEADLINE_MS,
            )),
            mode: mode.unwrap_or("default").to_owned(),
            time_mode: time_mode.unwrap_or("default").to_owned(),
        })
    }

    /// 页面范围求值：`__iv8__` 不可见（§2.4）。
    ///
    /// # Errors
    /// - 回调内重入 ⇒ [`ReentrantEvalError`]；
    /// - 其余引擎错误 ⇒ [`Iv8Error`]（消息前缀携带 `JsErrorKind`）。
    fn eval(&self, py: Python<'_>, src: &str) -> PyResult<Py<PyAny>> {
        let mut browser = self
            .browser
            .try_borrow_mut()
            .map_err(|_| reentrant_pyerr())?;
        let value = browser.eval(src).map_err(js_error_to_pyerr)?;
        host_to_py(py, browser.realm_mut(), &value)
    }

    /// 求值一个 ES 模块，返回其命名空间（§3.2）。
    ///
    /// # Errors
    /// - 回调内重入 ⇒ [`ReentrantEvalError`]；
    /// - 其余引擎错误 ⇒ [`Iv8Error`]。
    fn eval_module(&self, py: Python<'_>, src: &str, url: &str) -> PyResult<Py<PyAny>> {
        let mut browser = self
            .browser
            .try_borrow_mut()
            .map_err(|_| reentrant_pyerr())?;
        let value = browser.eval_module(src, url).map_err(js_error_to_pyerr)?;
        host_to_py(py, browser.realm_mut(), &value)
    }

    /// 注册 Python 回调为 JS 全局原生函数（§5 原生绑定 + §16 expose）。
    ///
    /// # Errors
    /// - 回调内重入 ⇒ [`ReentrantEvalError`]；
    /// - 其余引擎错误 ⇒ [`Iv8Error`]。
    fn expose(&self, name: &str, cb: Py<PyAny>) -> PyResult<()> {
        let mut browser = self
            .browser
            .try_borrow_mut()
            .map_err(|_| reentrant_pyerr())?;
        let realm = browser.realm_mut();
        let native = callback::py_callback(
            name.to_owned(),
            cb,
            std::sync::Arc::clone(&self.callback_deadline_ms),
        );
        let handle = realm
            .new_function(name, 0, native)
            .map_err(js_error_to_pyerr)?;
        let realm_id = realm.id();
        realm
            .set_global(
                name,
                HostValue::Function {
                    realm: realm_id,
                    handle,
                },
            )
            .map_err(js_error_to_pyerr)?;
        Ok(())
    }

    /// 显式推进微任务 checkpoint（§6：宿主显式管理）；同时交付 `MutationObserver`（ADR-0017·D5）。
    ///
    /// # Errors
    /// - 回调内重入 ⇒ [`ReentrantEvalError`]；
    /// - 微任务超预算被截断 / Realm 已关闭 ⇒ [`Iv8Error`]。
    fn perform_checkpoint(&self) -> PyResult<usize> {
        let mut browser = self
            .browser
            .try_borrow_mut()
            .map_err(|_| reentrant_pyerr())?;
        browser.perform_checkpoint().map_err(page_error_to_pyerr)
    }

    /// 控制范围求值：`__iv8__` 临时可用（退出即撤，RAII；§2.4）。
    ///
    /// # Errors
    /// - 回调内重入 ⇒ [`ReentrantEvalError`]；
    /// - 其余引擎错误 ⇒ [`Iv8Error`]。
    fn control_eval(&self, py: Python<'_>, src: &str) -> PyResult<Py<PyAny>> {
        let mut browser = self
            .browser
            .try_borrow_mut()
            .map_err(|_| reentrant_pyerr())?;
        let value = browser.control_eval(src).map_err(js_error_to_pyerr)?;
        host_to_py(py, browser.realm_mut(), &value)
    }

    /// 有界推进（§2.7）：返回 `RunOutcome` 名（`"Idle"`/`"WaitingIo"`/`"BudgetExhausted"`/
    /// `"DeadlineExceeded"`/`"Closed"`）。
    ///
    /// `deadline_ms == 0` ⇒ 不设墙钟上限。
    ///
    /// # Errors
    /// - 回调内重入 ⇒ [`ReentrantEvalError`]；
    /// - 运行循环错误 / 响应 resolve 失败 ⇒ [`Iv8Error`]。
    #[pyo3(signature = (max_tasks=0, deadline_ms=0))]
    fn run_until(&self, max_tasks: u64, deadline_ms: u64) -> PyResult<String> {
        let mut browser = self
            .browser
            .try_borrow_mut()
            .map_err(|_| reentrant_pyerr())?;
        let budget = if deadline_ms == 0 {
            RunBudget::tasks(max_tasks)
        } else {
            RunBudget::with_deadline(max_tasks, Duration::from_millis(deadline_ms))
        };
        let outcome = browser.run_until(budget).map_err(page_error_to_pyerr)?;
        Ok(outcome_name(outcome).to_owned())
    }

    /// 关闭上下文（**幂等**；§4）：作废网络请求并关闭底层 Realm。
    ///
    /// # Errors
    /// 回调内重入 ⇒ [`ReentrantEvalError`]；关闭失败 ⇒ [`Iv8Error`]。
    fn close(&self) -> PyResult<()> {
        let mut browser = self
            .browser
            .try_borrow_mut()
            .map_err(|_| reentrant_pyerr())?;
        browser.close().map_err(page_error_to_pyerr)
    }

    /// 设置 Python 回调的墙钟截止期限（毫秒；ADR-0014 决策 2）。
    fn set_callback_deadline_ms(&self, ms: u64) {
        self.callback_deadline_ms
            .store(ms, std::sync::atomic::Ordering::SeqCst);
    }

    /// 页面控制面子对象（§2.7）。
    #[getter]
    fn page(&self, py: Python<'_>) -> PyResult<Py<PyPage>> {
        Py::new(py, PyPage::new(Rc::clone(&self.browser)))
    }

    /// 网络控制面子对象（§2.7）。
    #[getter]
    fn network(&self, py: Python<'_>) -> PyResult<Py<PyNetwork>> {
        Py::new(py, PyNetwork::new(Rc::clone(&self.browser)))
    }

    /// 输入控制面子对象（§2.7）。
    #[getter]
    fn input(&self, py: Python<'_>) -> PyResult<Py<PyInput>> {
        Py::new(py, PyInput::new(Rc::clone(&self.browser)))
    }

    /// G2 能力面（§3.8）投影为 `dict`。
    ///
    /// # Errors
    /// - 回调内重入 ⇒ [`ReentrantEvalError`]；
    /// - 建 `dict` 失败 ⇒ `PyErr`。
    fn capabilities(&self, py: Python<'_>) -> PyResult<Py<PyDict>> {
        let browser = self.browser.try_borrow().map_err(|_| reentrant_pyerr())?;
        let caps = browser.capabilities();
        let dict = PyDict::new(py);
        dict.set_item("parsing", caps.parsing)?;
        dict.set_item("document_write", caps.document_write)?;
        dict.set_item("navigation", caps.navigation)?;
        dict.set_item("network", caps.network)?;
        dict.set_item("input", caps.input)?;
        dict.set_item("geometry", caps.geometry)?;
        dict.set_item("shadow_dom", caps.shadow_dom)?;
        dict.set_item("script_interruptible", caps.script_interruptible)?;
        dict.set_item("input_user_activation", caps.input_user_activation)?;
        dict.set_item("single_realm_navigation", caps.single_realm_navigation)?;
        Ok(dict.unbind())
    }

    /// 兼容层模式（记录值）。
    #[getter]
    fn mode(&self) -> &str {
        &self.mode
    }

    /// 时钟模式（记录值）。
    #[getter]
    fn time_mode(&self) -> &str {
        &self.time_mode
    }
}

/// Python 扩展模块入口（`import iv8`）。
///
/// # Errors
/// 注册类 / 常量失败时返回 `PyErr`。
#[pymodule]
fn iv8(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyContext>()?;
    m.add_class::<PyPage>()?;
    m.add_class::<PyNetwork>()?;
    m.add_class::<PyInput>()?;
    m.add_class::<PySnapshot>()?;
    m.add_class::<PyResponse>()?;
    m.add_class::<PyNetLogEntry>()?;
    m.add_class::<PyJsRef>()?;
    m.add_class::<PyUndefined>()?;

    let py = m.py();
    m.add("undefined", undefined_singleton(py)?)?;
    m.add("Iv8Error", py.get_type::<Iv8Error>())?;
    m.add("ReentrantEvalError", py.get_type::<ReentrantEvalError>())?;
    Ok(())
}
