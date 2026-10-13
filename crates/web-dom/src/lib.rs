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
//! G2 **不做**：`DocumentFragment`、`Attr` 节点对象、Range、完整 `HTMLCollection`。
//!
//! # G3 S2 增量：Shadow DOM **数据模型最小面**（D6 / 施工图 §1.2；**不含 JS 面**）
//! - `NodeKind::ShadowRoot`（**独立节点**，不是 Element）+ [`node::ShadowMode`] / [`node::ShadowRootData`]；
//! - [`tree::NodeTree::attach_shadow`]（**mode 语义定死**：同 mode 幂等；不同 mode ⇒ `Err(NotSupported)`）
//!   / `shadow_root` / `assigned_nodes`（slot 分发，**按需计算**）/ `flattened_children`（跨 shadow）；
//! - 影子树链**独立于** light DOM ⇒ light 遍历天然不含影子内容（作用域隔离）。
//!
//! JS 面（`attachShadow`/`shadowRoot`）与 `querySelector`/事件 retargeting 属后续步（S10 / G4）。

pub mod attr;
pub mod collection;
pub mod document;
pub mod error;
pub mod mutation;
pub mod node;
pub mod tree;
