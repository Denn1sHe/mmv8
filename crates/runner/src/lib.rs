//! runner —— 运行与分发：supervisor、子进程服务、进程池与配额
//!
//! 状态：**T0 骨架占位**。本 crate 目前只提供两项脚手架自检能力，用来证明
//! workspace 的编译、lint 与测试基建（insta / proptest）已经接通；它们**不是**
//! runner 的正式接口，G0 交付后会整体替换。
//!
//! 正式职责（技术方案 §4 / §16 / §19）：
//! - Supervisor 持有进程预算、任务路由与配置版本，不持有页面 JS 指针；
//! - 每个任务设置 CPU、墙钟、总内存、网络与 Worker 数配额；
//! - 异常任务销毁执行单元；复用进程时确保没有旧 Realm、回调、Cookie 或日志引用。

/// T0 骨架登记的 workspace crate 名单。
///
/// 顺序与《iv8 自研运行时完整技术方案》第 16 节的分组顺序一致：
/// engine-v8 / bindings-webidl → web-dom / web-events / navigation →
/// scheduler / profile / policy → web-style / layout-backend / graphics →
/// network / resources / storage / crypto → workers / debug / sdk-python / runner。
///
/// **这是脚手架期的边界副本，不是架构事实源。** 架构事实源是
/// `docs/adr/` 与技术方案第 16 节；本常量只为骨架自检与后续 capability-report
/// 提供一个可被测试断言的名单。
const ROSTER: [&str; 19] = [
    "engine-v8",
    "bindings-webidl",
    "web-dom",
    "web-events",
    "navigation",
    "scheduler",
    "profile",
    "policy",
    "web-style",
    "layout-backend",
    "graphics",
    "network",
    "resources",
    "storage",
    "crypto",
    "workers",
    "debug",
    "sdk-python",
    "runner",
];

/// 返回 T0 骨架登记的 crate 名单（见 crate 内 `ROSTER` 常量）。
#[must_use]
pub fn workspace_crates() -> &'static [&'static str] {
    &ROSTER
}

/// 返回脚手架状态行。
///
/// 内容**刻意保持稳定、不含任何计数或环境信息**，以便作为 insta 快照的基线：
/// 一旦本函数的输出意外漂移，评审 diff 里一眼可见（质量方案 §2「L1 快照测试」）。
#[must_use]
pub fn scaffold_status() -> String {
    "iv8 self-hosted runtime\nscaffold: engine-v8 isolated, workspace compiled\nstatus: skeleton"
        .to_owned()
}
