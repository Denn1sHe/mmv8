//! engine-v8 —— V8 专属句柄、Inspector 宿主、模块宿主与生命周期（唯一 unsafe 白名单 crate）
//!
//! 状态：**T0 骨架占位**。本 crate 目前只声明模块边界，不含实现。
//!
//! # unsafe 白名单
//!
//! 本 crate 是 workspace 中唯一被允许使用 `unsafe` 的成员（AGENTS.md 铁律 4）。
//! 使用规则：
//! 1. 每个 `unsafe` 块必须紧跟 `// SAFETY: <不变量说明>` 注释；
//! 2. `unsafe` 只能出现在与 V8/`rusty_v8` 的 FFI 薄桥处，不得用于业务逻辑；
//! 3. V8 / `rusty_v8` 类型**不得越出本 crate**（AGENTS.md 铁律 5，退出机制见技术方案 §5）。
//!
//! 高风险改动（unsafe 块与 GC 桥）必须先走决策上报协议获批（AGENTS.md 铁律 6）。

// —— 模块接线（ADR-0009 决策 1/2；统一口径：lib.rs 全部 pub mod）——
pub mod abi;
pub mod control;
pub mod error;
pub mod handles;
pub mod isolate;
pub mod microtask;
pub mod module_host;
pub mod native_fn;
pub mod node_map;
pub mod realm;

// —— 唯一的类型再导出（ADR-0009 决策 1 的类型面）——
// 只有 abi 的四个安全类型被再导出；其余类型一律走模块路径（如 `engine_v8::realm::Realm`）。
pub use abi::{HostValue, JsHandle, RealmId, WeakJsHandle};
