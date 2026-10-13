//! Python 回调派发（施工图 §2.3，严守 ADR-0012 + ADR-0014）。
//!
//! # 关键不变量（ADR-0014 决策 1）
//!
//! **回调总在「已持 GIL」的线程上执行；派发路径不发起跨线程阻塞式取锁。**
//! G1 采用「**回调进入前已在解释器内**」的形态——由 Python 侧主动调用 `ctx.eval(...)`
//! （此时本线程**本就持有** GIL），宿主回调在同一线程内直接派发。
//!
//! # 为什么 `Python::attach` 在这里是安全的（不是「跨线程取锁」）
//!
//! 同线程已持锁时 `Python::attach` 是**重入**的（ADR-0014 决策 5；0.29 起 `attach`
//! 即旧版的 `with_gil`）。本派发**从不**在
//! 「另一线程持锁忙等」的前提下调用它，故 ADR-0012 的 GIL 死锁路径**结构性不存在**。
//! （另一条路 `assume_gil_acquired` 是 `unsafe`，在本 crate 被 `unsafe_code = "forbid"`
//! 禁止——U-1 已证 lint 武装，故本模块**不写任何 `unsafe`**。）
//!
//! # 三条补强（ADR-0014 决策 2/3/4）
//!
//! 1. **截止期限**：进入前 / 返回后做**墙钟计量**；超限 ⇒ [`JsErrorKind::CallbackDeadline`]
//!    （**事后可判定，不挂死**；同线程下无法抢占正在运行的 Python 回调）。
//! 2. **重入禁令**：由 `engine-v8` 的 `Realm` 内 `in_callback` RAII 守卫强制
//!    （回调 trampoline 置位）。Python 侧经 `ctx.eval` 重入同一 Context ⇒ 见 `lib.rs` 的
//!    `ReentrantEvalError` 投影。
//! 3. **线程断言**：回调线程必须 == 注册（eval）线程；违反 ⇒ 显式错误（**非 UB**）。

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread::ThreadId;
use std::time::{Duration, Instant};

use pyo3::prelude::*;
use pyo3::types::PyTuple;

use engine_v8::HostValue;
use engine_v8::error::{JsError, JsErrorKind};
use engine_v8::native_fn::NativeFn;
use engine_v8::realm::Realm;

use crate::ReentrantEvalError;
use crate::convert::{host_to_py, py_to_host};

/// 回调截止期限默认值（毫秒）。
///
/// 取值足够大，等价于「G1 不设限」；实际上限由 Python 侧经
/// [`crate::PyContext::set_callback_deadline_ms`] 自持收紧（ADR-0014 决策 2）。
pub const DEFAULT_DEADLINE_MS: u64 = 30_000;

/// 线程不变量断言（ADR-0014 决策 4）：回调线程必须 == 注册（eval）线程。
///
/// 违反 ⇒ [`JsErrorKind::Internal`]（附两线程信息），**不是 UB**。
fn check_thread(owner: ThreadId) -> Result<(), JsError> {
    let current = std::thread::current().id();
    if current == owner {
        Ok(())
    } else {
        Err(JsError::new(
            JsErrorKind::Internal,
            format!(
                "回调线程 {current:?} != eval 线程 {owner:?}（ADR-0014 决策 4：显式错误，非 UB）"
            ),
        ))
    }
}

/// 把 `Py<PyAny>` 包装成 [`NativeFn`]（派发假设「调用线程已持 GIL」）。
///
/// 注册时记录**当前线程**为 `owner`；每次派发先做线程断言，再做墙钟计量。
/// `deadline_ms` 是共享的可变上限（`Arc<AtomicU64>`）⇒ 宿主可随时收紧，已有回调立即生效。
pub fn py_callback(name: String, cb: Py<PyAny>, deadline_ms: Arc<AtomicU64>) -> NativeFn {
    let owner = std::thread::current().id();
    Box::new(move |realm: &mut Realm, args: &[HostValue]| {
        dispatch(&name, &cb, realm, args, owner, &deadline_ms)
    })
}

/// 一次派发的完整流程：线程断言 → 参数转换 → 调用 → 结果回传 → 截止期限判定。
fn dispatch(
    name: &str,
    cb: &Py<PyAny>,
    realm: &mut Realm,
    args: &[HostValue],
    owner: ThreadId,
    deadline_ms: &Arc<AtomicU64>,
) -> Result<HostValue, JsError> {
    let started = Instant::now();

    // 同线程已持 GIL ⇒ `Python::attach` 是重入，不发起跨线程阻塞式取锁（ADR-0014 决策 5）。
    let outcome = Python::attach(|py| -> Result<HostValue, JsError> {
        check_thread(owner)?;

        let mut py_args: Vec<Py<PyAny>> = Vec::with_capacity(args.len());
        for arg in args {
            py_args.push(host_to_py(py, realm, arg).map_err(|err| {
                JsError::new(JsErrorKind::Internal, format!("回调参数转换失败：{err}"))
            })?);
        }
        let tuple = PyTuple::new(py, py_args).map_err(|err| {
            JsError::new(JsErrorKind::Internal, format!("构造参数元组失败：{err}"))
        })?;

        let result = cb
            .bind(py)
            .call1(tuple)
            .map_err(|err| classify_pyerr(py, &err))?;
        py_to_host(realm.id(), &result)
    });

    // 进入前 / 返回后墙钟计量：超限 ⇒ CallbackDeadline（优先于调用结果，事后可判定）。
    let elapsed = started.elapsed();
    let max_ms = deadline_ms.load(Ordering::SeqCst);
    if elapsed > Duration::from_millis(max_ms) {
        return Err(JsError::new(
            JsErrorKind::CallbackDeadline,
            format!(
                "CallbackDeadline：回调 '{name}' 耗时 {}ms 超过墙钟上限 {max_ms}ms\
                 （ADR-0014 决策 2：事后可判定，不挂死）",
                elapsed.as_millis()
            ),
        ));
    }

    outcome
}

/// 把 Python 异常分类成引擎错误。
///
/// - 重入类（[`ReentrantEvalError`]）⇒ [`JsErrorKind::ReentrantEval`]；
/// - 其余 ⇒ [`JsErrorKind::Exception`]，消息含**异常类型名**（供 JS 侧 `e.message` 观测）。
fn classify_pyerr(py: Python<'_>, err: &PyErr) -> JsError {
    if err.is_instance_of::<ReentrantEvalError>(py) {
        return JsError::new(
            JsErrorKind::ReentrantEval,
            format!("ReentrantEval：{}", err.value(py)),
        );
    }
    let type_name = err
        .get_type(py)
        .name()
        .map_or_else(|_| "PythonError".to_owned(), |name| name.to_string());
    JsError::new(
        JsErrorKind::Exception,
        format!("Python 回调异常：{type_name}: {}", err.value(py)),
    )
}

#[cfg(test)]
mod tests {
    use engine_v8::error::JsErrorKind;

    use super::check_thread;

    /// ADR-0014 决策 4 的线程不变量：异线程触发 ⇒ 显式错误（非 UB）。
    ///
    /// G1 单线程下「非 eval 线程触发回调」的**完整集成路径**结构上不可达（`PyContext` 为
    /// `unsendable`，V8 Isolate 为 `!Send`），故本不变量在**机制层**直接断言。
    #[test]
    fn callback_thread_assertion_rejects_foreign_thread() {
        let owner = std::thread::current().id();
        assert!(check_thread(owner).is_ok(), "注册线程自身应通过断言");

        let err = std::thread::spawn(move || check_thread(owner).expect_err("异线程应被拒"))
            .join()
            .expect("子线程应正常结束");
        assert_eq!(err.kind(), JsErrorKind::Internal, "应为显式 Internal 错误");
        assert!(
            err.message().contains("eval 线程"),
            "错误消息应含线程信息，得到：{}",
            err.message()
        );
    }
}
