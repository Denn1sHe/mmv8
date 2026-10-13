//! 布局后端门面 trait（`layout-backend::backend`，施工图 §1.2；接口不随后端变化，ADR-0011·C14）。
//!
//! **上层（`navigation` / `sdk-python`）只依赖 [`LayoutBackend`]**，不依赖 `StyloBackend` 等具体实现。
//! `: std::fmt::Debug` 是**必需**（G3-B6）：`navigation` 的 `BindingInputs`/`PageBinding` 均
//! `#[derive(Debug)]` 且要装 `Box<dyn LayoutBackend>`（`navigation/src/binding.rs:47/66`）。
//!
//! 具体实现（`StyloBackend`）在 **S6** 落地（依赖 R17 unsafe 白名单获批）。

use web_dom::node::NodeId;
use web_dom::tree::NodeTree;
use web_style::SheetOrigin;

use crate::error::LayoutError;
use crate::geometry::{NodeGeometry, Size};

/// 样式表句柄（[`LayoutBackend::add_stylesheet`] 返回；[`LayoutBackend::remove_stylesheet`] 消费）。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct StyleSheetHandle(u64);

impl StyleSheetHandle {
    /// 由不透明原始值构造（实现侧使用）。
    #[must_use]
    pub const fn new(raw: u64) -> Self {
        Self(raw)
    }

    /// 取不透明原始值。
    #[must_use]
    pub const fn raw(self) -> u64 {
        self.0
    }
}

/// 两级失效状态（对齐 T2.4 的 style / layout 双层读数；施工图 §3.3）。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct DirtyState {
    /// 样式级脏（需重算级联 / 计算值）。
    pub style: bool,
    /// 布局级脏（需重排盒树）。
    pub layout: bool,
}

/// 布局后端门面。
///
/// **`Debug` 为 supertrait**（G3-B6）⇒ `Box<dyn LayoutBackend>` 可 `#[derive(Debug)]`。
pub trait LayoutBackend: std::fmt::Debug {
    /// 设视口尺寸（改变 ⇒ 需重排）。
    fn set_viewport(&mut self, viewport: Size);

    /// 登记一张样式表（返回句柄，供 [`Self::remove_stylesheet`]）。
    ///
    /// # Errors
    /// CSS 解析 / 登记失败 ⇒ [`LayoutErrorKind::StyleSheet`](crate::error::LayoutErrorKind::StyleSheet)。
    fn add_stylesheet(
        &mut self,
        origin: SheetOrigin,
        css: &str,
    ) -> Result<StyleSheetHandle, LayoutError>;

    /// 移除一张样式表（`add_stylesheet` 对称面；G3-B11）。
    ///
    /// # Errors
    /// 未知句柄 ⇒ [`LayoutErrorKind::StyleSheet`](crate::error::LayoutErrorKind::StyleSheet)。
    fn remove_stylesheet(&mut self, handle: StyleSheetHandle) -> Result<(), LayoutError>;

    /// **重算**：级联 + 计算值 + 布局（对齐 blitz `resolve` 语义）。
    ///
    /// # Errors
    /// 级联 / 布局失败 ⇒ 对应 [`LayoutErrorKind`](crate::error::LayoutErrorKind)。
    fn resolve(&mut self, tree: &NodeTree, now: f64) -> Result<(), LayoutError>;

    /// 读一个节点的可读几何（`offset*`/`client*`/`getBoundingClientRect` 口径）。
    ///
    /// # Errors
    /// 节点未知 ⇒ `UnknownNode`；未布局 / 不可渲染 ⇒ `NotLaidOut`。
    fn geometry(&self, node: NodeId) -> Result<NodeGeometry, LayoutError>;

    /// 读计算值（`getComputedStyle` 语义；**属性白名单**见 `serialize.rs`，G3-B12③）。
    ///
    /// # Errors
    /// 节点未知 ⇒ `UnknownNode`；未布局 ⇒ `NotLaidOut`；属性不在白名单 ⇒ `UnsupportedProperty`。
    fn computed_value(&self, node: NodeId, prop: &str) -> Result<String, LayoutError>;

    /// 标记样式级脏。
    fn mark_style_dirty(&mut self, node: NodeId);

    /// 标记布局级脏。
    fn mark_layout_dirty(&mut self, node: NodeId);

    /// 当前失效状态。
    #[must_use]
    fn is_dirty(&self) -> DirtyState;

    /// **导航重绑**：清空 opaque 侧表 + 几何 / 样式缓存（ADR-0018·D2 + D8）。
    fn reset(&mut self);
}

#[cfg(test)]
mod tests {
    use super::{DirtyState, LayoutBackend, StyleSheetHandle};
    use crate::error::LayoutError;
    use crate::geometry::{NodeGeometry, Size};
    use web_dom::node::NodeId;
    use web_dom::tree::NodeTree;
    use web_style::SheetOrigin;

    /// 最小 mock（仅用于证明门面 `: Debug` 与 trait object 装配；非生产实现）。
    #[derive(Debug, Default)]
    struct MockBackend {
        viewport: Size,
        dirty: DirtyState,
    }

    impl LayoutBackend for MockBackend {
        fn set_viewport(&mut self, viewport: Size) {
            self.viewport = viewport;
        }

        fn add_stylesheet(
            &mut self,
            _origin: SheetOrigin,
            _css: &str,
        ) -> Result<StyleSheetHandle, LayoutError> {
            Ok(StyleSheetHandle::new(1))
        }

        fn remove_stylesheet(&mut self, handle: StyleSheetHandle) -> Result<(), LayoutError> {
            if handle.raw() == 1 {
                Ok(())
            } else {
                Err(LayoutError::new(
                    crate::error::LayoutErrorKind::StyleSheet,
                    "unknown handle",
                ))
            }
        }

        fn resolve(&mut self, _tree: &NodeTree, _now: f64) -> Result<(), LayoutError> {
            Ok(())
        }

        fn geometry(&self, _node: NodeId) -> Result<NodeGeometry, LayoutError> {
            Err(LayoutError::not_laid_out("mock has no layout"))
        }

        fn computed_value(&self, _node: NodeId, prop: &str) -> Result<String, LayoutError> {
            Err(LayoutError::unsupported_property(prop))
        }

        fn mark_style_dirty(&mut self, _node: NodeId) {
            self.dirty.style = true;
        }

        fn mark_layout_dirty(&mut self, _node: NodeId) {
            self.dirty.layout = true;
        }

        fn is_dirty(&self) -> DirtyState {
            self.dirty
        }

        fn reset(&mut self) {
            self.dirty = DirtyState::default();
        }
    }

    #[test]
    fn facade_trait_object_is_debug() {
        // G3-B6：门面 trait `: Debug` ⇒ `Box<dyn LayoutBackend>` 可被 #[derive(Debug)] 装配。
        let boxed: Box<dyn LayoutBackend> = Box::new(MockBackend::default());
        let rendered = format!("{boxed:?}");
        assert!(rendered.contains("MockBackend"), "Debug 输出应含类型名");
    }

    #[test]
    fn facade_dirty_state_transitions() {
        let mut backend = MockBackend::default();
        assert_eq!(backend.is_dirty(), DirtyState::default());
        backend.mark_style_dirty(NodeId::new(1, 0));
        assert!(backend.is_dirty().style);
        assert!(!backend.is_dirty().layout);
        backend.mark_layout_dirty(NodeId::new(1, 0));
        assert!(backend.is_dirty().layout);
        backend.reset();
        assert_eq!(backend.is_dirty(), DirtyState::default());
    }

    #[test]
    fn facade_error_paths_reachable() {
        let backend = MockBackend::default();
        // 每个返回 Result 的门面方法 ≥1 错误路径（施工图 §4）。
        assert!(backend.geometry(NodeId::new(1, 0)).is_err());
        assert!(backend.computed_value(NodeId::new(1, 0), "width").is_err());
        let mut node = MockBackend::default();
        assert!(node.remove_stylesheet(StyleSheetHandle::new(999)).is_err());
        assert!(node.remove_stylesheet(StyleSheetHandle::new(1)).is_ok());
    }
}
