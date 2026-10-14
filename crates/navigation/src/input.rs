//! 输入分派编排 + JS `Event` 对象桥（`navigation::input`，S12；ADR-0019·D1~D6）。
//!
//! - **显式 target**（命中测试属 G3）：`input.dispatch(kind, Some(node))` 校验节点属于当前文档。
//! - **JS `Event` 桥（ADR-0019·D1）**：每个事件建一个 JS 对象（`type`/`bubbles`/`cancelable`/
//!   `isTrusted`/`timeStamp`），进入每 `DispatchStep` 更新 `currentTarget`/`eventPhase`。
//! - **分派计划** `web-events::plan_dispatch` 为**纯函数**（owned `Vec`），借用在任何调用前释放
//!   （ADR-0016·D2 copy-out）。
//! - **调用走 [`Realm::dispatch_call`]**（ADR-0019·D6 分派专用入口——放开「分派期调用监听器」
//!   的受限重入，使脚本内 `el.click()` 可同步分派）；调用前 `is_live` 查活性，`once` 触发后置 dead。

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use engine_v8::error::JsError;
use engine_v8::node_map::NodeMap;
use engine_v8::realm::Realm;
use engine_v8::{HostValue, JsHandle};

use web_dom::node::NodeId;
use web_dom::tree::NodeTree;
use web_events::{Event, EventInit, EventPhase, ListenerRegistry, UserActivation, plan_dispatch};

use crate::binding::{refcell_conflict, wrapper_for_impl};

/// 输入事件种类（ADR-0017·D7 补齐；G2 仅做最小状态机）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InputKind {
    /// `click`（可信激活类）。
    Click,
    /// `keydown`（可信激活类）。
    KeyDown,
    /// `focus`。
    Focus,
}

impl InputKind {
    /// 对应的事件类型字符串。
    #[must_use]
    pub const fn event_type(self) -> &'static str {
        match self {
            Self::Click => "click",
            Self::KeyDown => "keydown",
            Self::Focus => "focus",
        }
    }

    /// 是否属「激活类」事件（置位 user activation）。
    #[must_use]
    pub const fn is_activation(self) -> bool {
        matches!(self, Self::Click | Self::KeyDown)
    }
}

/// 分派编排所需的共享依赖（各 `Rc` 克隆；见 `PageBinding::deps`）。
#[derive(Debug, Clone)]
pub struct DispatchDeps {
    /// 节点树。
    pub tree: Rc<RefCell<NodeTree>>,
    /// 节点 ↔ wrapper 弱映射（构造 `currentTarget`/`target` wrapper）。
    pub node_map: Rc<RefCell<NodeMap>>,
    /// 监听器注册表。
    pub registry: Rc<RefCell<ListenerRegistry>>,
    /// user activation 状态。
    pub activation: Rc<RefCell<UserActivation>>,
    /// `document.activeElement` 最小态。
    pub active_element: Rc<Cell<Option<NodeId>>>,
}

/// 分派一个**可信**输入事件（宿主 `input.dispatch` / `el.click()`）。
///
/// 激活类事件置位 `UserActivation.sticky`；`click` 默认动作 ⇒ 聚焦 `target`（更新 `activeElement`）。
///
/// # Errors
/// 建 JS 事件对象 / 构造 wrapper 失败时报 [`JsError`]。
pub fn dispatch_trusted(
    realm: &mut Realm,
    deps: &DispatchDeps,
    target: NodeId,
    kind: InputKind,
) -> Result<(), JsError> {
    if kind.is_activation() {
        // `G3X-S3c2`（②，Result 语境）：可判定错误（不 panic）。
        deps.activation
            .try_borrow_mut()
            .map_err(|_| {
                JsError::new(
                    engine_v8::error::JsErrorKind::ReentrantEval,
                    "user activation 借用冲突（宿主重入）⇒ 可判定错误",
                )
            })?
            .activate();
    }
    dispatch_named(realm, deps, target, kind.event_type(), true)?;
    if matches!(kind, InputKind::Click) {
        deps.active_element.set(Some(target));
    }
    Ok(())
}

/// 分派一个具名事件（`DOMContentLoaded` 等）；`is_trusted` 决定 `event.isTrusted`。
///
/// # Errors
/// 建 JS 事件对象 / 构造 wrapper 失败时报 [`JsError`]。
pub fn dispatch_named(
    realm: &mut Realm,
    deps: &DispatchDeps,
    target: NodeId,
    type_: &str,
    is_trusted: bool,
) -> Result<(), JsError> {
    let init = EventInit {
        bubbles: true,
        cancelable: true,
        composed: false,
    };
    let mut event = if is_trusted {
        Event::new_trusted(type_, init)
    } else {
        Event::new(type_, init)
    };
    event.target = Some(target);

    // —— JS Event 对象桥（ADR-0019·D1）——
    let id = realm.id();
    let ev = realm.new_object()?;
    let _ev_lease = realm.root(ev)?;
    realm.set_prop(ev, "type", HostValue::Str(event.type_.clone()))?;
    realm.set_prop(ev, "bubbles", HostValue::Bool(event.bubbles))?;
    realm.set_prop(ev, "cancelable", HostValue::Bool(event.cancelable))?;
    realm.set_prop(ev, "isTrusted", HostValue::Bool(event.is_trusted))?;
    realm.set_prop(ev, "timeStamp", HostValue::Number(event.time_stamp))?;
    realm.set_prop(ev, "defaultPrevented", HostValue::Bool(false))?;
    realm.set_prop(ev, "eventPhase", HostValue::Number(0.0))?;
    let target_wrapper = wrapper_for_impl(realm, &deps.node_map, target)?;
    realm.set_prop(ev, "target", target_wrapper)?;

    // —— 分派计划（owned，纯函数；借用在任何调用前释放）——
    let steps = {
        let tree = deps
            .tree
            .try_borrow()
            .map_err(|_| refcell_conflict("NodeTree"))?;
        let registry = deps
            .registry
            .try_borrow()
            .map_err(|_| refcell_conflict("ListenerRegistry"))?;
        plan_dispatch(&tree, &registry, target, &event)
    };

    let ev_value = HostValue::Object {
        realm: id,
        handle: ev,
    };
    for step in &steps {
        let current = wrapper_for_impl(realm, &deps.node_map, step.target)?;
        realm.set_prop(ev, "currentTarget", current)?;
        realm.set_prop(
            ev,
            "eventPhase",
            HostValue::Number(phase_number(step.phase)),
        )?;
        for &listener_id in &step.listeners {
            let listener = {
                let registry = deps
                    .registry
                    .try_borrow()
                    .map_err(|_| refcell_conflict("ListenerRegistry"))?;
                if registry.is_live(listener_id) {
                    registry.get(listener_id)
                } else {
                    None
                }
            };
            let Some(listener) = listener else {
                continue; // 已 `remove`/`once`（移除即置 dead）或已回收 ⇒ 跳过（ADR-0019·D2/D5）。
            };
            let callee = JsHandle::new(listener.callback.index(), listener.callback.generation());
            let _ = realm.dispatch_call(callee, None, std::slice::from_ref(&ev_value));
            if listener.options.once {
                deps.registry
                    .try_borrow_mut()
                    .map_err(|_| refcell_conflict("ListenerRegistry"))?
                    .mark_dead(listener_id);
            }
        }
    }
    Ok(())
}

/// `EventPhase` → JS `eventPhase` 数值（None=0 / Capturing=1 / AtTarget=2 / Bubbling=3）。
fn phase_number(phase: EventPhase) -> f64 {
    match phase {
        EventPhase::None => 0.0,
        EventPhase::Capturing => 1.0,
        EventPhase::AtTarget => 2.0,
        EventPhase::Bubbling => 3.0,
    }
}

/// `node` 是否属于 `ancestor` 子树（含自反）。
#[must_use]
pub fn is_descendant_or_self(tree: &NodeTree, node: NodeId, ancestor: NodeId) -> bool {
    let mut current = Some(node);
    while let Some(id) = current {
        if id == ancestor {
            return true;
        }
        current = tree.parent(id);
    }
    false
}

#[cfg(test)]
mod tests {
    use super::{InputKind, is_descendant_or_self};
    use web_dom::tree::NodeTree;

    #[test]
    fn input_kind_types_and_activation() {
        assert_eq!(InputKind::Click.event_type(), "click");
        assert_eq!(InputKind::KeyDown.event_type(), "keydown");
        assert_eq!(InputKind::Focus.event_type(), "focus");
        assert!(InputKind::Click.is_activation());
        assert!(InputKind::KeyDown.is_activation());
        assert!(!InputKind::Focus.is_activation());
    }

    #[test]
    fn descendant_helper() {
        let mut tree = NodeTree::new();
        let root = tree.root();
        let body = tree.create_element("body", "html", vec![]);
        let child = tree.create_element("span", "html", vec![]);
        tree.append_child(root, body);
        tree.append_child(body, child);
        assert!(is_descendant_or_self(&tree, child, root));
        assert!(is_descendant_or_self(&tree, root, root));
        assert!(!is_descendant_or_self(&tree, root, child));
    }
}
