//! # G3 S1 探针（`poc/t3_g3_probe`）
//!
//! 验证 ADR-0020/0021 定下的**直连路线**（`layout-backend` 内建直接 Stylo + Taffy + Parley
//! 适配器，不依赖 `blitz-dom`）在 S1 阶段的四项前置探针：
//!
//! - **P-1（首要门）**：`NodeRef<'a>` 可实现 `style::dom::{TNode,TElement,TShadowRoot}` +
//!   `selectors::Element`，且 **opaque/rehydrate 经线程局部侧表**可行、**零 unsafe**。
//! - **P-2**：经 stylo→taffy 桥（`to_taffy_style`）「样式改 ⇒ 布局盒变」可判。
//! - **P-8**：ADR-0021 精确锁版整组编译通过。
//! - **P-11**：stylo prefs 初始化在**无 GUI** 环境可运行。
//!
//! ## 本 crate 的 unsafe 证词
//!
//! 本 crate 根为 `#![forbid(unsafe_code)]`（见下）。它证明：
//!
//! 1. [`opaque`] 侧表机制 —— **零 unsafe**（把 `OpaqueElement` 当不透明键用，
//!    从不调 `as_const_ptr`，见 `selectors-0.40.0/tree.rs:41`）。
//! 2. [`prefs`] / 依赖整组 —— 安全 API。
//!
//! ⚠️ **P-1 阻断发现**：`style::dom::TElement` 的**必需方法**含 **5 个 `unsafe fn`**
//! （`set_handled_snapshot` / `set_dirty_descendants` / `unset_dirty_descendants` /
//! `ensure_data` / `clear_data`；`stylo-0.20.0/dom.rs:648/675/680/739/744`，均**无默认实现**）。
//! Rust 要求其实现将它们也声明为 `unsafe fn`，而 `#![forbid(unsafe_code)]` / 全仓
//! `unsafe_code = "forbid"` 会**拒绝** `unsafe fn` 的**声明与实现**（实测见
//! 仓库根 `.workbuddy/refs/g3-probe-evidence/forbid_unsafe_fn_repro.txt`）⇒ **`layout-backend`
//! （白名单外 + 继承 `forbid`）无法实现 `TElement`**。这与施工图 §1「G3 新增代码零 unsafe 为准」
//! 直接冲突，属 **P-1 阻断项**，须停上报（ADR-0020 决策 2 / 施工图 §7 P-1 铁律）。详见 [`report`]。
#![forbid(unsafe_code)]

pub mod opaque;
pub mod pins;
pub mod prefs;
pub mod report;
