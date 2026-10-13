//! 页面控制面子对象（`sdk-python::page`，S14；施工图 §2.7 / §3.8，ADR-0018·D5）。
//!
//! `PyPage` / `PyNetwork` / `PyInput` 各持 **`Rc<RefCell<BrowsingContext>>`**（与 `PyContext`
//! 共享同一实例；**不**做 `Py<PyContext>` 反向引用）。所有方法首步 `try_borrow(_mut)` →
//! 投影为 [`crate::reentrant_pyerr`]（可判定错误，非 `RefCell` panic；铁律 10）。

use std::cell::RefCell;
use std::rc::Rc;

use pyo3::prelude::*;

use navigation::input::InputKind;
use navigation::page::{BrowsingContext, OfflineResource, PageSnapshot};
use network::{NetLogEntry, NetLogPhase, RequestId, Response};

/// 页面快照（`PyPage.load` 入参）：URL + HTML + 预置离线资源。
#[pyclass(name = "Snapshot", module = "iv8")]
#[derive(Debug)]
pub struct PySnapshot {
    url: String,
    html: String,
    resources: Vec<OfflineResource>,
}

#[pymethods]
impl PySnapshot {
    /// 建一个页面快照。
    ///
    /// `resources` 为 `(url, status, body)` 序列（`body` 为 `str`，按 UTF-8 作响应体）。
    #[new]
    #[pyo3(signature = (url, html, resources=None))]
    fn new(url: &str, html: &str, resources: Option<Vec<(String, u16, String)>>) -> Self {
        let resources = resources
            .unwrap_or_default()
            .into_iter()
            .map(|(url, status, body)| OfflineResource {
                url,
                status,
                headers: Vec::new(),
                body: body.into_bytes(),
            })
            .collect();
        Self {
            url: url.to_owned(),
            html: html.to_owned(),
            resources,
        }
    }

    /// 快照 URL。
    #[getter]
    fn url(&self) -> &str {
        &self.url
    }

    /// 快照 HTML 源。
    #[getter]
    fn html(&self) -> &str {
        &self.html
    }
}

impl PySnapshot {
    /// 转成引擎侧 [`PageSnapshot`]。
    fn to_snapshot(&self) -> PageSnapshot {
        PageSnapshot {
            url: self.url.clone(),
            html: self.html.clone(),
            headers: Vec::new(),
            resources: self.resources.clone(),
        }
    }
}

/// 宿主回填的 fetch 响应（`PyNetwork.complete` 入参）。
#[pyclass(name = "Response", module = "iv8")]
#[derive(Debug)]
pub struct PyResponse {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

#[pymethods]
impl PyResponse {
    /// 建一个正常响应（`body` 为 `bytes`）。
    #[new]
    #[pyo3(signature = (status=200, body=None, headers=None))]
    fn new(status: u16, body: Option<Vec<u8>>, headers: Option<Vec<(String, String)>>) -> Self {
        Self {
            status,
            headers: headers.unwrap_or_default(),
            body: body.unwrap_or_default(),
        }
    }

    /// HTTP 状态码。
    #[getter]
    fn status(&self) -> u16 {
        self.status
    }
}

impl PyResponse {
    /// 转成引擎侧 [`Response`]。
    fn to_response(&self) -> Response {
        Response::ok(self.status, self.headers.clone(), self.body.clone())
    }
}

/// 一条 netLog 条目（`PyNetwork.export_log` 元素）。
#[pyclass(frozen, name = "NetLogEntry", module = "iv8")]
#[derive(Debug)]
pub struct PyNetLogEntry {
    url: String,
    status: Option<u16>,
    phase: String,
}

#[pymethods]
impl PyNetLogEntry {
    /// 条目 URL。
    #[getter]
    fn url(&self) -> &str {
        &self.url
    }

    /// HTTP 状态码（发起阶段为 `None`）。
    #[getter]
    fn status(&self) -> Option<u16> {
        self.status
    }

    /// 阶段名（`"RequestSent"` / `"ResponseReceived"` / `"Failed"`）。
    #[getter]
    fn phase(&self) -> &str {
        &self.phase
    }
}

impl PyNetLogEntry {
    /// 由引擎 [`NetLogEntry`] 转成投影。
    fn from_entry(entry: &NetLogEntry) -> Self {
        let phase = match entry.phase {
            NetLogPhase::RequestSent => "RequestSent",
            NetLogPhase::ResponseReceived => "ResponseReceived",
            NetLogPhase::Failed => "Failed",
        };
        Self {
            url: entry.url.clone(),
            status: entry.status,
            phase: phase.to_owned(),
        }
    }
}

/// `page` 子对象：加载快照、读就绪状态 / URL / origin。
#[pyclass(unsendable, name = "Page", module = "iv8")]
pub struct PyPage {
    browser: Rc<RefCell<BrowsingContext>>,
}

impl PyPage {
    /// 由共享 `BrowsingContext` 句柄建子对象。
    pub(crate) fn new(browser: Rc<RefCell<BrowsingContext>>) -> Self {
        Self { browser }
    }
}

#[pymethods]
impl PyPage {
    /// `page.load(snapshot)`：建文档 + 流式解析 + 执行 defer/module（§2.7 / §3.8）。
    ///
    /// # Errors
    /// - 回调内重入 ⇒ [`crate::ReentrantEvalError`]；
    /// - URL 非法 / 脚本失败 ⇒ [`crate::Iv8Error`]。
    fn load(&self, snapshot: &PySnapshot) -> PyResult<()> {
        let mut browser = self
            .browser
            .try_borrow_mut()
            .map_err(|_| crate::reentrant_pyerr())?;
        browser
            .load(&snapshot.to_snapshot())
            .map_err(crate::page_error_to_pyerr)?;
        Ok(())
    }

    /// 就绪状态字符串（`"loading"` / `"interactive"` / `"complete"`）。
    ///
    /// # Errors
    /// 回调内重入 ⇒ [`crate::ReentrantEvalError`]。
    #[getter]
    fn ready_state(&self) -> PyResult<String> {
        let browser = self
            .browser
            .try_borrow()
            .map_err(|_| crate::reentrant_pyerr())?;
        Ok(browser.ready_state().as_str().to_owned())
    }

    /// 当前文档 URL。
    ///
    /// # Errors
    /// 回调内重入 ⇒ [`crate::ReentrantEvalError`]。
    #[getter]
    fn url(&self) -> PyResult<String> {
        let browser = self
            .browser
            .try_borrow()
            .map_err(|_| crate::reentrant_pyerr())?;
        Ok(browser.url().to_owned())
    }

    /// 当前 origin（规范化字符串）。
    ///
    /// # Errors
    /// 回调内重入 ⇒ [`crate::ReentrantEvalError`]。
    #[getter]
    fn origin(&self) -> PyResult<String> {
        let browser = self
            .browser
            .try_borrow()
            .map_err(|_| crate::reentrant_pyerr())?;
        Ok(browser.origin_string())
    }

    /// 按 element `id` 读**真实几何**（S15 / §3.7）：
    /// `(offset_width, offset_height, client_width, client_height, top, left)`。
    ///
    /// # Errors
    /// 未找到该 `id` ⇒ `KeyError`；重入 ⇒ [`crate::ReentrantEvalError`]；布局失败 ⇒ [`crate::Iv8Error`]。
    fn geometry(&self, element_id: &str) -> PyResult<(f64, f64, f64, f64, f64, f64)> {
        let mut browser = self
            .browser
            .try_borrow_mut()
            .map_err(|_| crate::reentrant_pyerr())?;
        let node = browser.element_by_id(element_id).ok_or_else(|| {
            pyo3::exceptions::PyKeyError::new_err(format!("未找到 id={element_id} 的元素"))
        })?;
        let g = browser
            .node_geometry(node)
            .map_err(crate::page_error_to_pyerr)?;
        Ok((
            f64::from(g.offset_width),
            f64::from(g.offset_height),
            f64::from(g.client_width()),
            f64::from(g.client_height()),
            f64::from(g.top),
            f64::from(g.left),
        ))
    }
}

/// `network` 子对象：列出 pending、回填响应、导出 netLog。
#[pyclass(unsendable, name = "Network", module = "iv8")]
pub struct PyNetwork {
    browser: Rc<RefCell<BrowsingContext>>,
}

impl PyNetwork {
    /// 由共享 `BrowsingContext` 句柄建子对象。
    pub(crate) fn new(browser: Rc<RefCell<BrowsingContext>>) -> Self {
        Self { browser }
    }
}

#[pymethods]
impl PyNetwork {
    /// 未完成请求 id（`RequestId` 的原始 `u64`）。
    ///
    /// # Errors
    /// 回调内重入 ⇒ [`crate::ReentrantEvalError`]。
    fn pending(&self) -> PyResult<Vec<u64>> {
        let browser = self
            .browser
            .try_borrow()
            .map_err(|_| crate::reentrant_pyerr())?;
        Ok(browser
            .pending_fetches()
            .iter()
            .map(|id| id.get())
            .collect())
    }

    /// 回填一个请求的响应（下次 `run_until` 内 resolve；§2.5）。
    ///
    /// # Errors
    /// - 回调内重入 ⇒ [`crate::ReentrantEvalError`]；
    /// - 已中止 / 代次过期 / 未知 id ⇒ [`crate::Iv8Error`]。
    fn complete(&self, id: u64, resp: &PyResponse) -> PyResult<()> {
        let mut browser = self
            .browser
            .try_borrow_mut()
            .map_err(|_| crate::reentrant_pyerr())?;
        browser
            .complete_fetch(RequestId::new(id), resp.to_response())
            .map_err(crate::page_error_to_pyerr)?;
        Ok(())
    }

    /// 导出 netLog（副本）。
    ///
    /// # Errors
    /// 回调内重入 ⇒ [`crate::ReentrantEvalError`]。
    fn export_log(&self) -> PyResult<Vec<PyNetLogEntry>> {
        let mut browser = self
            .browser
            .try_borrow_mut()
            .map_err(|_| crate::reentrant_pyerr())?;
        let log = browser.network().export_log();
        Ok(log.iter().map(PyNetLogEntry::from_entry).collect())
    }
}

/// `input` 子对象：显式 target 可信输入。
#[pyclass(unsendable, name = "Input", module = "iv8")]
pub struct PyInput {
    browser: Rc<RefCell<BrowsingContext>>,
}

impl PyInput {
    /// 由共享 `BrowsingContext` 句柄建子对象。
    pub(crate) fn new(browser: Rc<RefCell<BrowsingContext>>) -> Self {
        Self { browser }
    }
}

#[pymethods]
impl PyInput {
    /// 分派一个可信输入事件（`kind` ∈ `"click"` / `"keydown"` / `"focus"`；`target` = `NodeId` raw）。
    ///
    /// # Errors
    /// - 回调内重入 ⇒ [`crate::ReentrantEvalError`]；
    /// - 无 target / target 不属于当前文档 ⇒ [`crate::Iv8Error`]。
    #[pyo3(signature = (kind, target=None))]
    fn dispatch(&self, kind: &str, target: Option<u64>) -> PyResult<()> {
        let kind = match kind {
            "click" => InputKind::Click,
            "keydown" => InputKind::KeyDown,
            "focus" => InputKind::Focus,
            other => {
                return Err(PyErr::new::<crate::Iv8Error, _>(format!(
                    "Internal: 未知输入种类 {other:?}（G2 仅 click/keydown/focus）"
                )));
            }
        };
        let mut browser = self
            .browser
            .try_borrow_mut()
            .map_err(|_| crate::reentrant_pyerr())?;
        browser
            .input_dispatch_raw(kind, target)
            .map_err(crate::page_error_to_pyerr)?;
        Ok(())
    }
}
