//! `prefs` —— **P-11**：stylo prefs 初始化在**无 GUI** 环境可运行。
//!
//! ## 读原文得到的事实
//!
//! - **prefs 是编译期静态**：`stylo_static_prefs`（stylo 的 `static_prefs` 依赖）
//!   用 `include!(concat!(env!("OUT_DIR"), "/generated.rs"))` 生成带**默认值**的原子量；
//!   `static_prefs::pref!("...")` 直接读该原子量 ⇒ **无运行期 init 调用**
//!   （证据：`stylo_static_prefs-0.20.0/src/lib.rs:7`；其自带测试 `test_basic_preferences`
//!   不调用任何 init 即可读写 pref）。
//! - **`GLOBAL_STYLE_DATA` / `STYLE_THREAD_POOL` 均为 `std::sync::LazyLock`**，首次使用即
//!   初始化（`stylo-0.20.0/global_style_data.rs:221` / `:172`）。stylo 内**唯一**的 pref 读取在
//!   `stylo_threads_pref()`（`global_style_data.rs:158`，读 `layout.threads`），
//!   亦为静态 ⇒ **不依赖 GUI / 无窗口系统**。
//! - 构建期依赖：`stylo/build.rs` 需 **`python3`** 跑属性生成器（`properties/build.py`），
//!   属**构建**要求，不是运行期 GUI 要求。
//!
//! 本模块在 `cargo test`（headless，无窗口系统）中**强制触发**上述 LazyLock 初始化，
//! 以证明「无 GUI 环境可运行」。

use style::global_style_data::{GLOBAL_STYLE_DATA, STYLE_THREAD_POOL};

/// P-11 运行报告。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HeadlessReport {
    /// `GLOBAL_STYLE_DATA` 是否成功初始化（强制 LazyLock）。
    pub global_style_data_init: bool,
    /// 样式线程池线程数（`None` = 单线程串行路径；见 `StyleThreadPool::num_threads`）。
    pub style_threads: Option<usize>,
    /// 运行期是否检测到 GUI 依赖（本探针恒 `false`；数据来源 = 依赖树黑名单，见 [`crate::pins`]）。
    pub gui_present: bool,
}

/// 在**无 GUI** 环境强制初始化 stylo 的全局样式数据，返回报告。
#[must_use]
pub fn headless_init_probe() -> HeadlessReport {
    // 强制 `LazyLock` 首次解引用 ⇒ 触发初始化（生成 `SharedRwLock` + `StyleSystemOptions`）。
    let _global: &style::global_style_data::GlobalStyleData = &GLOBAL_STYLE_DATA;
    let _thread_pool: &style::global_style_data::StyleThreadPool = &STYLE_THREAD_POOL;

    HeadlessReport {
        global_style_data_init: true,
        style_threads: STYLE_THREAD_POOL.num_threads,
        // GUI 检测交由 `pins::check_pins`（读锁文件黑名单）；此处不重复。
        gui_present: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// P-11：无 GUI 环境可初始化 stylo 全局样式数据（不 panic / 不依赖窗口系统）。
    #[test]
    fn prefs_init_runs_headless() {
        let r = headless_init_probe();
        assert!(r.global_style_data_init, "GLOBAL_STYLE_DATA 必须可初始化");
        // 线程池可为 None（= 串行；`layout.threads` 默认 -1 ⇒ 视 CPU 而定），
        // 但初始化本身不得 panic —— 这正是 P-11 的判据（无 GUI 可运行）。
        let _ = r.style_threads;
    }

    /// 幂等：重复初始化不 panic（`LazyLock` 语义）。
    #[test]
    fn prefs_init_is_idempotent() {
        let a = headless_init_probe();
        let b = headless_init_probe();
        assert_eq!(a.shape(), b.shape(), "重复初始化读数形状必须一致");
    }

    impl HeadlessReport {
        /// 仅取「可比较形状」（线程数可能随环境不同，但同一进程内应稳定）。
        fn shape(self) -> (bool, Option<usize>) {
            (self.global_style_data_init, self.style_threads)
        }
    }
}
