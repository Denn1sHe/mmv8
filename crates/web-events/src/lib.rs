//! web-events —— 事件模型：捕获/冒泡、监听器变更与可信模拟输入（G2 最小面）。
//!
//! 状态：**G2 落地**（施工图 §2.3 / §3.6，ADR-0019）。
//! - 分派算法为**纯函数** [`plan_dispatch`]（owned `Vec<DispatchStep>`，不调 v8）。
//! - 监听器槽**只存弱句柄** [`ListenerRef`]（C-2）——本 crate **仅依赖** `engine_v8::WeakJsHandle`
//!   这一 abi 类型，不出现 `JsHandle`/`HandleLease`。
//! - 本 crate **零 unsafe**。

pub mod dispatch;
pub mod error;
pub mod event;
pub mod listener;
pub mod target;
pub mod trusted;

pub use dispatch::{DispatchStep, plan_dispatch};
pub use error::{EventError, EventErrorKind};
pub use event::{Event, EventInit, EventPhase};
pub use listener::{ListenerId, ListenerOptions, ListenerRef};
pub use target::ListenerRegistry;
pub use trusted::{TrustedEventKind, UserActivation};
