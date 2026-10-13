//! 控制入口隔离（S11）。
//!
//! # 落定路线（对施工图 §1.2 的偏差 **E**，供文档同步）
//!
//! 施工图 §1.2 设想「**双 Realm**（控制 Realm + 页面 Realm）」，但批次 2B 已验收的 **D1** 定下
//! 「`Realm` 自持 `OwnedIsolate`」——**一个 Isolate 不可能被两个 Realm 同时自持**。
//! 故采用 **路线 (a)：单 Realm 双模式**——同一个 `Realm` 上，用「控制全局 `__iv8__` 的
//! 安装/撤销」区分**控制求值范围**与**页面范围**：
//!
//! - **控制范围**（[`ControlPlane::eval_control`]）：临时安装 `__iv8__`（RAII，退出即撤）；
//! - **页面范围**（[`ControlPlane::eval_page`] / [`ControlPlane::with_page_script`]）：
//!   `__iv8__` 不存在 ⇒ `typeof __iv8__ === "undefined"`；且控制面 API 被撤销（⇒ `ControlRevoked`）。
//!
//! 对 **D1 的影响**：**无**——仍是「单 Isolate 单 Realm 自持」，只是给它加了一层能力开关。

use crate::abi::HostValue;
use crate::error::{JsError, JsErrorKind};
use crate::isolate::AgentRuntime;
use crate::realm::Realm;

/// 控制全局名（控制范围专用）。
pub const CONTROL_GLOBAL: &str = "__iv8__";

/// 控制入口隔离（§5）：页面脚本执行期**撤销**控制能力；只在控制求值范围开放。
pub struct ControlPlane {
    realm: Realm,
    revoked: bool,
}

impl ControlPlane {
    /// 建控制面（内部持有一个 Realm）。
    ///
    /// # Errors
    /// 建 Realm 失败时报错。
    pub fn new(host: &mut AgentRuntime) -> Result<Self, JsError> {
        let realm = host.create_realm()?;
        Ok(Self {
            realm,
            revoked: false,
        })
    }

    /// 控制求值：`__iv8__` 可用（RAII 安装/撤销控制全局）。
    ///
    /// # Errors
    /// 页面脚本期（已撤销）调用 ⇒ [`JsErrorKind::ControlRevoked`]；求值失败按 [`Realm::eval`]。
    pub fn eval_control(&mut self, src: &str) -> Result<HostValue, JsError> {
        if self.revoked {
            return Err(JsError::new(
                JsErrorKind::ControlRevoked,
                "控制能力已在页面脚本期撤销",
            ));
        }
        self.realm.eval("globalThis.__iv8__ = {};")?;
        let result = self.realm.eval(src);
        // 无论成败都撤销控制全局（避免泄漏到页面范围）。
        let _ = self.realm.eval("delete globalThis.__iv8__;");
        result
    }

    /// 页面求值：`__iv8__` 不可见（控制全局只在控制范围临时安装）。
    ///
    /// # Errors
    /// 求值失败按 [`Realm::eval`]。
    pub fn eval_page(&mut self, src: &str) -> Result<HostValue, JsError> {
        self.realm.eval(src)
    }

    /// 页面脚本范围：进入时**撤销**控制能力，退出时**恢复**（RAII，**即便 panic 也恢复**）。
    pub fn with_page_script<R>(&mut self, f: impl FnOnce(&mut ControlPlane) -> R) -> R {
        self.revoked = true;
        // 用 `catch_unwind` 保证 panic 路径也复位（等价于 RAII 守卫，且全程安全代码）。
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(self)));
        self.revoked = false;
        match outcome {
            Ok(value) => value,
            Err(payload) => std::panic::resume_unwind(payload),
        }
    }

    /// 控制能力当前是否可用。
    #[must_use]
    pub fn is_control_available(&self) -> bool {
        !self.revoked
    }

    /// 页面 Realm 的可变引用（供页面侧调用求值等）。
    pub fn realm_mut(&mut self) -> &mut Realm {
        &mut self.realm
    }
}

#[cfg(test)]
mod tests {
    use super::{CONTROL_GLOBAL, ControlPlane};
    use crate::abi::HostValue;
    use crate::error::JsErrorKind;
    use crate::isolate::AgentRuntime;

    fn new_plane() -> ControlPlane {
        let mut runtime = AgentRuntime::new().expect("建 AgentRuntime");
        ControlPlane::new(&mut runtime).expect("建 ControlPlane")
    }

    #[test]
    fn control_available_in_control_scope() {
        let mut plane = new_plane();
        let value = plane
            .eval_control(&format!("typeof {CONTROL_GLOBAL}"))
            .unwrap();
        assert_eq!(value, HostValue::Str("object".to_owned()));
    }

    #[test]
    fn control_invisible_in_page_scope() {
        let mut plane = new_plane();
        let value = plane
            .eval_page(&format!("typeof {CONTROL_GLOBAL}"))
            .unwrap();
        assert_eq!(
            value,
            HostValue::Str("undefined".to_owned()),
            "页面范围不应看到控制入口"
        );
    }

    #[test]
    fn control_revoked_during_page_script() {
        let mut plane = new_plane();
        let outcome = plane.with_page_script(|inner| inner.eval_control("1 + 1"));
        let err = outcome.expect_err("页面脚本期控制入口应被拒");
        assert_eq!(err.kind(), JsErrorKind::ControlRevoked);
    }

    #[test]
    fn capability_restored_after_page() {
        let mut plane = new_plane();
        plane.with_page_script(|inner| {
            assert!(!inner.is_control_available(), "页面脚本期应不可用");
        });
        assert!(plane.is_control_available(), "退出页面脚本后应恢复");

        // panic 路径专测：闭包 panic 后能力仍必须恢复。
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            plane.with_page_script(|_inner| panic!("page panic"));
        }));
        assert!(
            plane.is_control_available(),
            "panic 后能力也必须恢复（RAII）"
        );
    }
}
