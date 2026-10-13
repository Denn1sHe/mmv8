//! web-dom —— 规范 DOM：节点树、属性反射、Live 集合（G2 最小面）。
//!
//! 状态：**G2 内核落地**（施工图 `docs/design-g2-page-2026-10-13.md` §2.2，ADR-0016）。
//! 承载「带 generation 的 [`NodeId`](node::NodeId) + 单线程 arena」（[`tree::NodeTree`]）。
//!
//! # 纪律
//! - **零 `unsafe`**（全部按下标索引，无自引用指针）。
//! - **类型面不含任何 v8 类型**（C-2）：只出现 [`NodeId`](node::NodeId)（`u64` 语义）；
//!   DOM 侧**不持任何** `JsHandle`/`Global`。
//! - `class_list` 由 `attrs` **惰性派生**（ADR-0016·D6，无第二写点）。
//!
//! G2 **不做**：`DocumentFragment`、Shadow tree、`Attr` 节点对象、Range、完整 `HTMLCollection`
//! （均属 G3）。

pub mod attr;
pub mod collection;
pub mod document;
pub mod error;
pub mod mutation;
pub mod node;
pub mod tree;
