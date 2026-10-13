//! 监听器条目与选项（`web-events::listener`）。
//!
//! **只存弱句柄**（C-2 / ADR-0013 决策 6）：回调的调用由编排层经 `engine-v8` 的
//! `call_weak` 原子入口完成（ADR-0019·D5）。本模块**不出现** `JsHandle`/`HandleLease`。

use engine_v8::WeakJsHandle;

/// 监听器选项。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct ListenerOptions {
    /// 是否在**捕获**阶段触发。
    pub capture: bool,
    /// 被动监听（`preventDefault` 无效；G2 仅记录，不参与匹配）。
    pub passive: bool,
    /// 触发一次后自动移除。
    pub once: bool,
}

/// 监听器条目：**弱引用槽** + 选项。
#[derive(Clone, Copy, Debug)]
pub struct ListenerRef {
    /// 回调（弱句柄；不阻止 GC）。
    pub callback: WeakJsHandle,
    /// 选项。
    pub options: ListenerOptions,
}

/// 稳定注册 id：分派期监听器变更靠它查活性（ADR-0019·D2）。
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ListenerId(u64);

impl ListenerId {
    /// 由原始 `u64` 构造（注册表内部使用）。
    pub(crate) const fn from_raw(raw: u64) -> Self {
        Self(raw)
    }

    /// 取原始 `u64`。
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}
