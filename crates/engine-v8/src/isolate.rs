//! 进程级 V8 初始化 + Isolate 生命周期（S6）。
//!
//! `EngineHost` 用 `Once` 保证「平台创建 + `initialize_platform` + `initialize`」全局只做一次；
//! `AgentRuntime` 持有 G1 的单个 Isolate（`CreateParams`，不挂 cppgc 堆——那属 G2 的 DOM）；
//! `TerminateHandle` 是 `v8::IsolateHandle` 的**安全影子**（对外不暴露 V8 类型）。

use std::sync::Once;

use crate::error::{JsError, JsErrorKind};
use crate::realm::Realm;

/// 进程级初始化守卫（首次真正初始化，其后为 no-op）。
static V8_INIT: Once = Once::new();

/// 进程级 V8 平台初始化守卫。
pub struct EngineHost;

impl EngineHost {
    /// 幂等：首次真正初始化 V8，其后返回 `Ok(())`。
    ///
    /// 初始化前设置 `--expose-gc`（令 `request_garbage_collection_for_testing` 可用，
    /// ADR-0013 的 T2 待实测项）与 `--no_freeze_flags_after_init`（允许之后再设 flag）。
    ///
    /// # Errors
    /// 当前实现不产生错误（保留 `Result` 以便将来平台初始化失败可上报）。
    pub fn init_once() -> Result<(), JsError> {
        V8_INIT.call_once(|| {
            v8::V8::set_flags_from_string("--no_freeze_flags_after_init --expose-gc");
            let platform = v8::new_default_platform(0, false).make_shared();
            v8::V8::initialize_platform(platform);
            v8::V8::initialize();
        });
        Ok(())
    }

    /// 进程是否已完成 V8 初始化（供测试断言，**不触发**初始化）。
    #[must_use]
    pub fn is_initialized() -> bool {
        V8_INIT.is_completed()
    }
}

/// 同一 Agent 的 Isolate（G1：单 Isolate）。
pub struct AgentRuntime {
    /// Isolate 的所有权；`create_realm` 会把它移交给 `Realm`（G1 单 Realm 语义）。
    isolate: Option<v8::OwnedIsolate>,
    terminate: TerminateHandle,
}

impl AgentRuntime {
    /// 建 Isolate（`CreateParams`；G1 不挂 cppgc 堆）。
    ///
    /// # Errors
    /// 仅在 V8 平台初始化失败时报错（当前实现恒 `Ok`）。
    pub fn new() -> Result<Self, JsError> {
        EngineHost::init_once()?;
        let isolate = v8::Isolate::new(v8::CreateParams::default());
        let terminate = TerminateHandle(isolate.thread_safe_handle());
        Ok(Self {
            isolate: Some(isolate),
            terminate,
        })
    }

    /// 在同一个 Isolate 上开一个 Realm。
    ///
    /// **G1 单 Realm 语义**：`Realm` **拥有**该 `OwnedIsolate` 的所有权（否则
    /// `PyContext` 只持 `Realm` 时会悬垂），故本方法把 Isolate 移交给 `Realm`；
    /// 同一 `AgentRuntime` 再次调用会返回 `Internal` 错误。
    ///
    /// # Errors
    /// - [`JsErrorKind::Internal`]：Isolate 已移交给前一个 Realm。
    /// - [`JsErrorKind::Internal`]：创建 Context 失败。
    pub fn create_realm(&mut self) -> Result<Realm, JsError> {
        let isolate = self.isolate.take().ok_or_else(|| {
            JsError::new(
                JsErrorKind::Internal,
                "AgentRuntime 的 Isolate 已移交给前一个 Realm（G1 单 Realm）",
            )
        })?;
        Realm::new(isolate)
    }

    /// 供 watchdog 使用的跨线程终止句柄。
    #[must_use]
    pub fn thread_safe_handle(&self) -> TerminateHandle {
        self.terminate.clone()
    }
}

/// 终止句柄的**安全影子**（对外只暴露方法，不暴露 V8 的 `IsolateHandle`）。
#[derive(Clone)]
pub struct TerminateHandle(v8::IsolateHandle);

impl TerminateHandle {
    /// 从 Isolate 取跨线程终止句柄（供 `Realm` 持有，做微任务墙钟守卫）。
    #[must_use]
    pub(crate) fn from_isolate(isolate: &v8::Isolate) -> Self {
        Self(isolate.thread_safe_handle())
    }

    /// 强制中断该 Isolate 上正在运行的 JS 执行（跨线程安全）。
    ///
    /// **注**：`v8` 152 的 `IsolateHandle` 未暴露 `is_executing` 类查询 API，故本类型
    /// **不提供**该查询（不构造无真实依据的假 API）。
    pub fn terminate_execution(&self) {
        self.0.terminate_execution();
    }
}

#[cfg(test)]
mod tests {
    use super::EngineHost;

    #[test]
    fn init_once_idempotent() {
        EngineHost::init_once().expect("首次初始化应成功");
        assert!(
            EngineHost::is_initialized(),
            "初始化后 is_initialized 应为真"
        );
        // 再次调用是 no-op，且仍成功。
        EngineHost::init_once().expect("重复初始化应成功（幂等）");
        assert!(EngineHost::is_initialized());
    }
}
