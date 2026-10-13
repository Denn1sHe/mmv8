//! 节点身份与内容（§1.2 `web-dom::node`）。
//!
//! `NodeId` 是不透明 `u64`：**高 32 位 = generation（复用防呆），低 32 位 = arena 下标**
//! （与 `engine_v8::JsHandle` / `WeakJsHandle` 逐位一致，U-7），故其 [`NodeId::raw`] 可直接
//! 作 `engine_v8::node_map::NodeMap` 的 `u64` key。本 crate **零 unsafe**、类型面**不含任何
//! v8 类型**（C-2）。

use web_style::InlineStyle;

use crate::attr::AttributeMap;

/// `NodeId::index` / `NodeId::generation` 所在的低 32 位掩码。
const LOW_HALF_MASK: u64 = 0x0000_0000_FFFF_FFFF;

/// 节点身份。高 32 位 = generation（复用防呆），低 32 位 = arena 下标。
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct NodeId(u64);

impl NodeId {
    /// 由 arena 下标与 generation 打包构造（高 32 位 = generation，低 32 位 = index）。
    #[must_use]
    pub const fn new(index: u32, generation: u32) -> Self {
        Self(((generation as u64) << 32) | (index as u64))
    }

    /// 取 arena 下标（低 32 位）。
    #[must_use]
    pub const fn index(self) -> u32 {
        (self.0 & LOW_HALF_MASK) as u32
    }

    /// 取 generation（高 32 位）。
    #[must_use]
    pub const fn generation(self) -> u32 {
        (self.0 >> 32) as u32
    }

    /// 取原始 `u64`（供 `engine_v8::node_map::NodeMap` 的 `u64` key）。
    #[must_use]
    pub const fn raw(self) -> u64 {
        self.0
    }
}

impl From<NodeId> for u64 {
    fn from(id: NodeId) -> Self {
        id.0
    }
}

/// 节点种类（G2 不做 `DocumentFragment`；G3 增 `ShadowRoot`）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NodeKind {
    /// 文档根。
    Document,
    /// `<!DOCTYPE ...>`。
    DocumentType,
    /// 元素。
    Element,
    /// 文本节点。
    Text,
    /// 注释节点。
    Comment,
    /// **影子根**（G3 D6：**不是** Element；影子树链**独立于** light DOM）。
    ShadowRoot,
}

/// 影子根模式（`ShadowRoot.mode`，§1.2）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ShadowMode {
    /// `"open"`（`element.shadowRoot` 可见）。
    Open,
    /// `"closed"`（`element.shadowRoot` 返回 `null`）。
    Closed,
}

impl ShadowMode {
    /// `mode` 字符串（JS 面用；`"open"` / `"closed"`）。
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Closed => "closed",
        }
    }
}

/// 影子根节点载荷（仅 [`NodeKind::ShadowRoot`] 有值，§1.2）。
///
/// **mode 语义定死（G3-B13）**：`attach_shadow` 对已存在影子根——
/// 同 `mode` ⇒ 幂等返回既有根；不同 `mode` ⇒ `Err(DomErrorKind::NotSupported)`。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ShadowRootData {
    /// 影子根模式。
    pub mode: ShadowMode,
    /// 宿主元素（light DOM 中的 Element）。
    pub host: NodeId,
}

/// 元素特有数据（最小面；Shadow 支撑见下）。
///
/// ⚠️ ADR-0016·D6：**不设** `class_list` 字段（消除 `attrs`/`class_list` 双真相源）；
/// `class_list` 由 `attrs`（`class` 属性）**惰性派生**（[`ElementData::class_list`]）。
#[derive(Debug)]
pub struct ElementData {
    /// 标签名（小写规范名）。
    pub name: String,
    /// 命名空间：`"html"` / `"svg"` / `"mathml"`。
    pub namespace: String,
    /// 有序属性表（名大小写折叠）。
    pub attrs: AttributeMap,
    /// 内联样式（仅声明值；级联属 G3）。
    pub style: InlineStyle,
    /// 影子根（`NodeKind::ShadowRoot` 节点的 id；无则 `None`）。
    ///
    /// **clone 注记（G3-B15）**：本字段**不随克隆复制**（见下 [`Clone`] 实现）。
    pub shadow: Option<NodeId>,
}

impl Clone for ElementData {
    /// **clone 注记（G3-B15，S2 落定）**：`shadow` **置 `None`**，其余字段深拷贝。
    ///
    /// 理由：`shadow: Option<NodeId>` 指向宿主元素的影子根；若克隆 host 会得到**两个节点
    /// 指向同一影子根**（第二真相源，违反 D6「影子根唯一」）。故约定 = **克隆时置 `None`**：
    /// 克隆体不带影子根关联（`shadow_root(clone) == None`）。
    fn clone(&self) -> Self {
        Self {
            name: self.name.clone(),
            namespace: self.namespace.clone(),
            attrs: self.attrs.clone(),
            style: self.style.clone(),
            shadow: None,
        }
    }
}

impl ElementData {
    /// 建一个空元素数据（无属性、空内联样式、无影子根）。
    #[must_use]
    pub fn new(name: &str, namespace: &str) -> Self {
        Self {
            name: name.to_ascii_lowercase(),
            namespace: namespace.to_owned(),
            attrs: AttributeMap::new(),
            style: InlineStyle::new(),
            shadow: None,
        }
    }

    /// 由 `class` 属性**惰性派生** `class_list`（空白分隔；无第二写点 —— ADR-0016·D6）。
    #[must_use]
    pub fn class_list(&self) -> Vec<&str> {
        self.attrs
            .get("class")
            .map(|value| value.split_ascii_whitespace().collect())
            .unwrap_or_default()
    }
}

/// 一个节点（arena 内一条记录）。树链用**下标**（非指针）⇒ 可序列化、无 unsafe。
#[derive(Clone, Debug)]
pub struct NodeData {
    /// 节点种类。
    pub kind: NodeKind,
    /// 父节点。
    pub parent: Option<NodeId>,
    /// 首子节点。
    pub first_child: Option<NodeId>,
    /// 末子节点。
    pub last_child: Option<NodeId>,
    /// 前一兄弟。
    pub prev_sibling: Option<NodeId>,
    /// 后一兄弟。
    pub next_sibling: Option<NodeId>,
    /// 仅 [`NodeKind::Element`] 有值。
    pub element: Option<ElementData>,
    /// 仅 [`NodeKind::ShadowRoot`] 有值（影子根载荷：mode + 宿主）。
    pub shadow_root: Option<ShadowRootData>,
    /// `Text` / `Comment` / `DocumentType` 的文本。
    pub text: Option<String>,
}

impl NodeData {
    /// 建一个无子、无兄弟、无父的基础节点。
    #[must_use]
    pub fn new(kind: NodeKind) -> Self {
        Self {
            kind,
            parent: None,
            first_child: None,
            last_child: None,
            prev_sibling: None,
            next_sibling: None,
            element: None,
            shadow_root: None,
            text: None,
        }
    }

    /// 该节点是否元素。
    #[must_use]
    pub fn is_element(&self) -> bool {
        self.kind == NodeKind::Element
    }

    /// 该节点是否影子根。
    #[must_use]
    pub fn is_shadow_root(&self) -> bool {
        self.kind == NodeKind::ShadowRoot
    }
}

#[cfg(test)]
mod tests {
    use super::{ElementData, NodeId};

    #[test]
    fn nodeid_packing_roundtrip() {
        for (index, generation) in [(0u32, 0u32), (u32::MAX, u32::MAX), (7, 3)] {
            let id = NodeId::new(index, generation);
            assert_eq!(id.index(), index);
            assert_eq!(id.generation(), generation);
            assert_eq!(u64::from(id), id.raw());
        }
        // 与 `JsHandle` 位序一致：generation 在高位、index 在低位。
        let id = NodeId::new(5, 9);
        assert_eq!(id.raw(), (9u64 << 32) | 5);
    }

    #[test]
    fn class_list_derived_from_attr() {
        let mut el = ElementData::new("DIV", "html");
        assert_eq!(el.name, "div", "标签名应小写");
        assert!(
            el.class_list().is_empty(),
            "无 class 属性时 class_list 应为空"
        );
        el.attrs.set("class", "a  b\tc");
        assert_eq!(el.class_list(), vec!["a", "b", "c"]);
    }

    #[test]
    fn element_clone_drops_shadow() {
        // G3-B15：clone 注记 —— 克隆体不复制 shadow（避免两指针指向同一影子根）。
        let mut el = ElementData::new("div", "html");
        el.attrs.set("id", "host");
        el.shadow = Some(NodeId::new(3, 0));
        let cloned = el.clone();
        assert_eq!(cloned.name, el.name);
        assert_eq!(cloned.attrs.get("id"), Some("host"));
        assert_eq!(cloned.shadow, None, "克隆体不得复制 shadow（G3-B15）");
        assert_eq!(el.shadow, Some(NodeId::new(3, 0)), "原节点 shadow 不受影响");
    }
}
