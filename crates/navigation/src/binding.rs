//! 手写最小 JS `document` / 元素桥（`navigation::binding`，S11；ADR-0016·D3 / ADR-0019·D1）。
//!
//! G3 将由 `bindings-webidl` 的生成物取代；本模块是**非生成物**（不触铁律 7）。
//!
//! # 形态（说明为什么是「手写 JS 桥 + 原生原语」）
//!
//! `engine-v8` 的安全类型面**不提供**「定义属性访问器」的 API（只提供 `new_object`/`set_prop`/
//! `new_function`/`call` 等），而 `el.className` / `el.style.color` 需要 **JS 访问器**。故本模块：
//! 1. 注册**原生原语**（`__iv8_dom_*` / `__iv8_ev_*`，经 `Realm::new_function`+`set_global`）；
//! 2. `eval` 一段**手写 JS shim**：以 `Object.defineProperty` 在节点 wrapper 上安装
//!    `className`/`style` 等访问器与方法，随后**删除**原语全局（收窄页面可见面）。
//!
//! # 弱引用纪律（C-2）
//!
//! 节点 ↔ wrapper 复用由 [`engine_v8::node_map::NodeMap`]（**只存弱句柄**）承担：同一 node
//! 两次取回 ⇒ 同一 wrapper（`node_wrapper_identity_stable`）；弱句柄被回收 ⇒ **重建** wrapper
//! （复用前 `resolve_weak` 存活性校验，ADR-0016·D3）。DOM 侧**不持任何强句柄**。

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use engine_v8::error::{JsError, JsErrorKind};
use engine_v8::native_fn::NativeFn;
use engine_v8::node_map::NodeMap;
use engine_v8::realm::Realm;
use engine_v8::{HostValue, JsHandle};

use storage::StorageKind;
use web_dom::mutation::{MutationBuffer, MutationRecord};
use web_dom::node::{NodeId, ShadowMode};
use web_dom::tree::NodeTree;
use web_events::{ListenerOptions, ListenerRegistry, UserActivation};

use crate::input::{self, DispatchDeps, InputKind};
use crate::layout::LayoutFacade;
use layout_backend::invalidate::DomChange;

/// wrapper 对象上存放「node 原始 `u64`」的隐藏属性名（原生原语据此反查 `NodeId`）。
const NODE_KEY: &str = "__iv8_node";

/// 构造「共享单元借用冲突」可判定错误（ADR-0016·D2：`try_borrow` ⇒ 可判定错误，非 panic）。
pub(crate) fn refcell_conflict(what: &str) -> JsError {
    JsError::new(
        JsErrorKind::Internal,
        format!("RefCell 重入：{what} 已被借用（ADR-0016·D2，可判定错误）"),
    )
}

/// `IdbEvent::Error` ⇒ 具名 `JsError`（`D4-M`：原生具名可达 ⇒ JS 面 `e.name` 可见）。
fn idb_event_error(event: &storage::idb::IdbEvent) -> JsError {
    match event {
        storage::idb::IdbEvent::Error { name, message, .. } => {
            JsError::named(JsErrorKind::Exception, name, message.clone())
        }
        other => JsError::new(
            JsErrorKind::Internal,
            format!("IdbSurface 返回了非错误事件：{other:?}"),
        ),
    }
}

/// 建 `PageBinding` 所需的共享子单元（**均由 `BrowsingContext` 持有、克隆共享**）。
#[derive(Debug, Clone)]
pub struct BindingInputs {
    /// 节点树（与 parser 共享）。
    pub tree: Rc<RefCell<NodeTree>>,
    /// 变更记录缓冲（DOM 操作写入）。
    pub mutations: Rc<RefCell<MutationBuffer>>,
    /// 单一 `document.write` 写缓冲。
    pub writes: Rc<RefCell<Vec<String>>>,
    /// 监听器注册表。
    pub registry: Rc<RefCell<ListenerRegistry>>,
    /// user activation 状态。
    pub activation: Rc<RefCell<UserActivation>>,
    /// `document.activeElement` 最小态。
    pub active_element: Rc<Cell<Option<NodeId>>>,
    /// parser 是否活跃（`readyState == Loading`）；`document.write` post-load 据此呈具名错误。
    pub parser_active: Rc<Cell<bool>>,
    /// 布局门面（S12：几何真值 + `getComputedStyle`；§2.5 同步读）。
    pub layout: Rc<RefCell<LayoutFacade>>,
    /// **存储服务**（`G4-S1`：`StorageService` origin 分区；跨导航持久）。
    pub storage: Rc<RefCell<storage::StorageService>>,
    /// `IndexedDB` 面级表面（与 `PageShared` 共享）。
    pub idb: Rc<RefCell<storage::idb::IdbSurface>>,
    /// **当前文档 origin**（存储分区键；随导航更新）。
    pub origin: Rc<RefCell<String>>,
}

/// 手写最小 JS `document` / 元素桥。
#[derive(Debug)]
pub struct PageBinding {
    inputs: BindingInputs,
    node_map: Rc<RefCell<NodeMap>>,
}

/// `G4-S1`：把 shim 传入的 kind 串解析为 [`StorageKind`]（非法 ⇒ 具名错误）。
fn storage_kind(args: &[HostValue]) -> Result<StorageKind, JsError> {
    match arg_str(args, 0).as_deref() {
        Some("local") => Ok(StorageKind::Local),
        Some("session") => Ok(StorageKind::Session),
        other => Err(JsError::new(
            JsErrorKind::Exception,
            format!("storage kind 非法：{other:?}（应为 local/session）"),
        )),
    }
}

impl PageBinding {
    /// 安装原生原语 + JS shim；返回可复用的绑定句柄。
    ///
    /// # Errors
    /// 注册原生函数 / 求值 shim 失败时报 [`JsError`]。
    #[allow(clippy::too_many_lines)] // 逐条注册 13 个原生原语 + shim，属同一「安装」关注点。
    pub fn install(realm: &mut Realm, inputs: BindingInputs) -> Result<Self, JsError> {
        let node_map = Rc::new(RefCell::new(NodeMap::new()));

        // —— 原生原语：DOM ——
        {
            let tree = Rc::clone(&inputs.tree);
            let node_map = Rc::clone(&node_map);
            install_global_fn(
                realm,
                "__iv8_dom_create",
                1,
                Box::new(move |realm, args| {
                    let tag = arg_str(args, 0).unwrap_or_else(|| "div".to_owned());
                    let node = tree
                        .try_borrow_mut()
                        .map_err(|_| refcell_conflict("NodeTree"))?
                        .create_element(&tag, "html", Vec::new());
                    wrapper_for_impl(realm, &node_map, node)
                }),
            )?;
        }
        {
            let tree = Rc::clone(&inputs.tree);
            let mutations = Rc::clone(&inputs.mutations);
            install_global_fn(
                realm,
                "__iv8_dom_append",
                2,
                Box::new(move |realm, args| {
                    let parent = node_of(realm, args.first())?;
                    let child = node_of(realm, args.get(1))?;
                    if let (Some(parent), Some(child)) = (parent, child) {
                        let valid = {
                            let tree = tree
                                .try_borrow()
                                .map_err(|_| refcell_conflict("NodeTree"))?;
                            tree.is_valid(parent) && tree.is_valid(child)
                        };
                        if valid {
                            tree.try_borrow_mut()
                                .map_err(|_| refcell_conflict("NodeTree"))?
                                .append_child(parent, child);
                            mutations
                                .try_borrow_mut()
                                .map_err(|_| refcell_conflict("MutationBuffer"))?
                                .record(MutationRecord::child_list(
                                    parent,
                                    vec![child],
                                    Vec::new(),
                                ));
                        }
                    }
                    Ok(HostValue::Undefined)
                }),
            )?;
        }
        {
            let tree = Rc::clone(&inputs.tree);
            let node_map = Rc::clone(&node_map);
            install_global_fn(
                realm,
                "__iv8_dom_get_by_id",
                1,
                Box::new(move |realm, args| {
                    let id = arg_str(args, 0).unwrap_or_default();
                    let found = {
                        let tree = tree
                            .try_borrow()
                            .map_err(|_| refcell_conflict("NodeTree"))?;
                        find_by_attr(&tree, tree.root(), "id", &id)
                    };
                    match found {
                        Some(node) => wrapper_for_impl(realm, &node_map, node),
                        None => Ok(HostValue::Null),
                    }
                }),
            )?;
        }
        {
            let tree = Rc::clone(&inputs.tree);
            let node_map = Rc::clone(&node_map);
            install_global_fn(
                realm,
                "__iv8_dom_document",
                0,
                Box::new(move |realm, _args| {
                    let root = tree
                        .try_borrow()
                        .map_err(|_| refcell_conflict("NodeTree"))?
                        .root();
                    wrapper_for_impl(realm, &node_map, root)
                }),
            )?;
        }
        {
            let tree = Rc::clone(&inputs.tree);
            let node_map = Rc::clone(&node_map);
            install_global_fn(
                realm,
                "__iv8_dom_body",
                0,
                Box::new(move |realm, _args| {
                    let found = {
                        let tree = tree
                            .try_borrow()
                            .map_err(|_| refcell_conflict("NodeTree"))?;
                        find_by_tag(&tree, tree.root(), "body")
                    };
                    match found {
                        Some(node) => wrapper_for_impl(realm, &node_map, node),
                        None => Ok(HostValue::Null),
                    }
                }),
            )?;
        }
        {
            let tree = Rc::clone(&inputs.tree);
            install_global_fn(
                realm,
                "__iv8_dom_get_attr",
                2,
                Box::new(move |realm, args| {
                    let node = node_of(realm, args.first())?;
                    let name = arg_str(args, 1).unwrap_or_default();
                    let value = if let Some(node) = node {
                        tree.try_borrow()
                            .map_err(|_| refcell_conflict("NodeTree"))?
                            .get_attribute(node, &name)
                            .map(str::to_owned)
                    } else {
                        None
                    };
                    Ok(value.map_or(HostValue::Null, HostValue::Str))
                }),
            )?;
        }
        {
            let tree = Rc::clone(&inputs.tree);
            let mutations = Rc::clone(&inputs.mutations);
            install_global_fn(
                realm,
                "__iv8_dom_set_attr",
                3,
                Box::new(move |realm, args| {
                    let node = node_of(realm, args.first())?;
                    let name = arg_str(args, 1);
                    let value = arg_str(args, 2).unwrap_or_default();
                    if let (Some(node), Some(name)) = (node, name) {
                        if tree
                            .try_borrow()
                            .map_err(|_| refcell_conflict("NodeTree"))?
                            .is_valid(node)
                        {
                            tree.try_borrow_mut()
                                .map_err(|_| refcell_conflict("NodeTree"))?
                                .set_attribute(node, &name, &value);
                            mutations
                                .try_borrow_mut()
                                .map_err(|_| refcell_conflict("MutationBuffer"))?
                                .record(MutationRecord::attributes(node, &name, None));
                        }
                    }
                    Ok(HostValue::Undefined)
                }),
            )?;
        }
        {
            let tree = Rc::clone(&inputs.tree);
            install_global_fn(
                realm,
                "__iv8_dom_get_style",
                2,
                Box::new(move |realm, args| {
                    let node = node_of(realm, args.first())?;
                    let prop = arg_str(args, 1).unwrap_or_default();
                    let value = if let Some(node) = node {
                        tree.try_borrow()
                            .map_err(|_| refcell_conflict("NodeTree"))?
                            .get(node)
                            .and_then(|data| data.element.as_ref())
                            .and_then(|element| element.style.get_property(&prop))
                            .map(str::to_owned)
                    } else {
                        None
                    };
                    Ok(value.map_or(HostValue::Null, HostValue::Str))
                }),
            )?;
        }
        {
            let tree = Rc::clone(&inputs.tree);
            install_global_fn(
                realm,
                "__iv8_dom_set_style",
                3,
                Box::new(move |realm, args| {
                    let node = node_of(realm, args.first())?;
                    let prop = arg_str(args, 1).unwrap_or_default();
                    let value = arg_str(args, 2).unwrap_or_default();
                    if let Some(node) = node {
                        let mut tree = tree
                            .try_borrow_mut()
                            .map_err(|_| refcell_conflict("NodeTree"))?;
                        if let Some(element) =
                            tree.get_mut(node).and_then(|data| data.element.as_mut())
                        {
                            element.style.set_property(&prop, &value);
                        }
                    }
                    Ok(HostValue::Undefined)
                }),
            )?;
        }
        {
            let writes = Rc::clone(&inputs.writes);
            let parser_active = Rc::clone(&inputs.parser_active);
            install_global_fn(
                realm,
                "__iv8_dom_write",
                1,
                Box::new(move |_realm, args| {
                    if !parser_active.get() {
                        // ADR-0017·D4：post-load `write` ⇒ 具名错误（消息前缀即错误名，便于契约断言）。
                        return Err(JsError::new(
                            engine_v8::error::JsErrorKind::Internal,
                            "DocumentWriteAfterLoad: load 完成后 document.write 被拒（ADR-0017·D4）",
                        ));
                    }
                    if let Some(text) = arg_str(args, 0) {
                        writes
                            .try_borrow_mut()
                            .map_err(|_| refcell_conflict("writes"))?
                            .push(text);
                    }
                    Ok(HostValue::Undefined)
                }),
            )?;
        }

        // —— 原生原语：事件 ——
        {
            let registry = Rc::clone(&inputs.registry);
            install_global_fn(
                realm,
                "__iv8_ev_add",
                5,
                Box::new(move |realm, args| {
                    let node = node_of(realm, args.first())?;
                    let type_ = arg_str(args, 1).unwrap_or_default();
                    let callback = arg_handle(args.get(2));
                    let capture = matches!(args.get(3), Some(HostValue::Bool(true)));
                    let once = matches!(args.get(4), Some(HostValue::Bool(true)));
                    if let (Some(node), Some(callback)) = (node, callback) {
                        let weak = realm.weak_handle(callback)?;
                        registry
                            .try_borrow_mut()
                            .map_err(|_| refcell_conflict("ListenerRegistry"))?
                            .add(
                                node,
                                &type_,
                                weak,
                                ListenerOptions {
                                    capture,
                                    passive: false,
                                    once,
                                },
                            );
                    }
                    Ok(HostValue::Undefined)
                }),
            )?;
        }
        {
            let registry = Rc::clone(&inputs.registry);
            install_global_fn(
                realm,
                "__iv8_ev_remove",
                4,
                Box::new(move |realm, args| {
                    let node = node_of(realm, args.first())?;
                    let type_ = arg_str(args, 1).unwrap_or_default();
                    let callback = arg_handle(args.get(2));
                    let capture = matches!(args.get(3), Some(HostValue::Bool(true)));
                    if let (Some(node), Some(callback)) = (node, callback) {
                        let weak = realm.weak_handle(callback)?;
                        registry
                            .try_borrow_mut()
                            .map_err(|_| refcell_conflict("ListenerRegistry"))?
                            .remove(node, &type_, weak, capture);
                    }
                    Ok(HostValue::Undefined)
                }),
            )?;
        }
        {
            let deps = dispatch_deps(&inputs, &node_map);
            install_global_fn(
                realm,
                "__iv8_ev_click",
                1,
                Box::new(move |realm, args| {
                    let node = node_of(realm, args.first())?;
                    if let Some(node) = node {
                        let _ = input::dispatch_trusted(realm, &deps, node, InputKind::Click);
                    }
                    Ok(HostValue::Undefined)
                }),
            )?;
        }
        {
            let deps = dispatch_deps(&inputs, &node_map);
            install_global_fn(
                realm,
                "__iv8_ev_dispatch",
                2,
                Box::new(move |realm, args| {
                    let node = node_of(realm, args.first())?;
                    let type_ = arg_str(args, 1).unwrap_or_default();
                    if let Some(node) = node {
                        let _ = input::dispatch_named(realm, &deps, node, &type_, false);
                    }
                    Ok(HostValue::Undefined)
                }),
            )?;
        }

        // —— 原生原语：Shadow 面（S10-b；G3-B8 边界：仅 attachShadow / shadowRoot，无 querySelector）——
        {
            let tree = Rc::clone(&inputs.tree);
            let node_map = Rc::clone(&node_map);
            install_global_fn(
                realm,
                "__iv8_dom_attach_shadow",
                2,
                Box::new(move |realm, args| {
                    let host = node_of(realm, args.first())?.ok_or_else(|| {
                        JsError::new(JsErrorKind::Exception, "attachShadow: 非法 host")
                    })?;
                    let mode = match arg_str(args, 1).as_deref() {
                        Some("open") => ShadowMode::Open,
                        Some("closed") => ShadowMode::Closed,
                        _ => {
                            return Err(JsError::new(
                                JsErrorKind::Exception,
                                "attachShadow: mode 必须为 open/closed",
                            ));
                        }
                    };
                    let root = tree
                        .try_borrow_mut()
                        .map_err(|_| refcell_conflict("NodeTree"))?
                        .attach_shadow(host, mode)
                        .map_err(|err| {
                            JsError::new(JsErrorKind::Exception, err.message().to_owned())
                        })?;
                    wrapper_for_impl(realm, &node_map, root)
                }),
            )?;
        }
        {
            let tree = Rc::clone(&inputs.tree);
            let node_map = Rc::clone(&node_map);
            install_global_fn(
                realm,
                "__iv8_dom_shadow_root",
                1,
                Box::new(move |realm, args| {
                    let found = match node_of(realm, args.first())? {
                        Some(host) => tree
                            .try_borrow()
                            .map_err(|_| refcell_conflict("NodeTree"))?
                            .shadow_root(host)
                            .filter(|(_, mode)| *mode == ShadowMode::Open)
                            .map(|(root, _)| root),
                        None => None,
                    };
                    match found {
                        Some(root) => wrapper_for_impl(realm, &node_map, root),
                        None => Ok(HostValue::Null),
                    }
                }),
            )?;
        }

        // —— 原生原语：几何真值 + 计算值（S12-b；§1.3 口径 / §2.5 同步读）——
        {
            let tree = Rc::clone(&inputs.tree);
            let layout = Rc::clone(&inputs.layout);
            install_global_fn(
                realm,
                "__iv8_geom",
                2,
                Box::new(move |realm, args| {
                    let node = node_of(realm, args.first())?
                        .ok_or_else(|| JsError::new(JsErrorKind::Exception, "geom: 非法节点"))?;
                    let prop = arg_str(args, 1).unwrap_or_default();
                    let value = {
                        let tree = tree
                            .try_borrow()
                            .map_err(|_| refcell_conflict("NodeTree"))?;
                        let mut layout = layout
                            .try_borrow_mut()
                            .map_err(|_| refcell_conflict("LayoutFacade"))?;
                        layout.geometry(&tree, node).map_err(|err| {
                            JsError::new(JsErrorKind::Exception, err.message().to_owned())
                        })?
                    };
                    let number = match prop.as_str() {
                        // §1.3：border-box 取整（81.5 => 82）。
                        "offsetWidth" => value.offset_width.round(),
                        "offsetHeight" => value.offset_height.round(),
                        "clientWidth" => value.client_width().round(),
                        "clientHeight" => value.client_height().round(),
                        "rectTop" => value.top,
                        "rectLeft" => value.left,
                        "rectWidth" => value.offset_width,
                        "rectHeight" => value.offset_height,
                        _ => {
                            return Err(JsError::new(JsErrorKind::Exception, "geom: 未知属性"));
                        }
                    };
                    Ok(HostValue::Number(f64::from(number)))
                }),
            )?;
        }
        {
            let tree = Rc::clone(&inputs.tree);
            let layout = Rc::clone(&inputs.layout);
            install_global_fn(
                realm,
                "__iv8_get_computed",
                2,
                Box::new(move |realm, args| {
                    let node = node_of(realm, args.first())?.ok_or_else(|| {
                        JsError::new(JsErrorKind::Exception, "getComputedStyle: 非法节点")
                    })?;
                    let prop = arg_str(args, 1).unwrap_or_default();
                    let value = {
                        let tree = tree
                            .try_borrow()
                            .map_err(|_| refcell_conflict("NodeTree"))?;
                        let mut layout = layout
                            .try_borrow_mut()
                            .map_err(|_| refcell_conflict("LayoutFacade"))?;
                        layout.computed_value(&tree, node, &prop).map_err(|err| {
                            JsError::new(JsErrorKind::Exception, err.message().to_owned())
                        })?
                    };
                    Ok(HostValue::Str(value))
                }),
            )?;
        }

        // —— 原生原语：标脏（§2.5；DOM 变更 ⇒ 布局脏）——
        {
            let layout = Rc::clone(&inputs.layout);
            install_global_fn(
                realm,
                "__iv8_mark_dirty",
                2,
                Box::new(move |_realm, args| {
                    let kind = arg_str(args, 0).unwrap_or_default();
                    let property = arg_str(args, 1).unwrap_or_default();
                    let mut layout = layout
                        .try_borrow_mut()
                        .map_err(|_| refcell_conflict("LayoutFacade"))?;
                    match kind.as_str() {
                        "attribute" => layout.mark_dirty(DomChange::Attribute),
                        "style" => layout.mark_dirty(DomChange::InlineStyle {
                            property: &property,
                        }),
                        _ => layout.mark_dirty(DomChange::ChildList),
                    }
                    Ok(HostValue::Undefined)
                }),
            )?;
        }
        // —— 原生原语：IndexedDB 面级（G4-S1-d）——
        {
            let idb = Rc::clone(&inputs.idb);
            install_global_fn(
                realm,
                "__iv8_idb_open",
                2,
                Box::new(move |_realm, args| {
                    let name = arg_str(args, 0).unwrap_or_default();
                    let version = arg_str(args, 1)
                        .and_then(|v| v.parse::<u32>().ok())
                        .unwrap_or(1);
                    let request = idb
                        .try_borrow_mut()
                        .map_err(|_| refcell_conflict("IdbSurface"))?
                        .open(&name, version);
                    Ok(HostValue::Str(request.to_string()))
                }),
            )?;
            let idb = Rc::clone(&inputs.idb);
            install_global_fn(
                realm,
                "__iv8_idb_create_store",
                2,
                Box::new(move |_realm, args| {
                    let request = arg_str(args, 0)
                        .and_then(|v| v.parse::<u64>().ok())
                        .unwrap_or(0);
                    let name = arg_str(args, 1).unwrap_or_default();
                    let store = idb
                        .try_borrow_mut()
                        .map_err(|_| refcell_conflict("IdbSurface"))?
                        .create_object_store(request, &name)
                        .map_err(|event| idb_event_error(&event))?;
                    Ok(HostValue::Str(store.to_string()))
                }),
            )?;
            let idb = Rc::clone(&inputs.idb);
            install_global_fn(
                realm,
                "__iv8_idb_add",
                3,
                Box::new(move |_realm, args| {
                    let store = arg_str(args, 0)
                        .and_then(|v| v.parse::<u64>().ok())
                        .unwrap_or(0);
                    let value = arg_str(args, 1).unwrap_or_default();
                    let key = arg_str(args, 2);
                    let request = idb
                        .try_borrow_mut()
                        .map_err(|_| refcell_conflict("IdbSurface"))?
                        .add(store, &value, key.as_deref())
                        .map_err(|event| idb_event_error(&event))?;
                    Ok(HostValue::Str(request.to_string()))
                }),
            )?;
            let idb = Rc::clone(&inputs.idb);
            install_global_fn(
                realm,
                "__iv8_idb_pump",
                1,
                Box::new(move |_realm, args| {
                    let request = arg_str(args, 0)
                        .and_then(|v| v.parse::<u64>().ok())
                        .unwrap_or(0);
                    // **事件编码**：`name|key|version|database`（shim 解析后派发）。
                    let events = idb
                        .try_borrow_mut()
                        .map_err(|_| refcell_conflict("IdbSurface"))?
                        .pump(request);
                    let encoded = events
                        .iter()
                        .map(|event| match event {
                            storage::idb::IdbEvent::UpgradeNeeded { version, .. } => {
                                format!("upgradeneeded|{version}")
                            }
                            storage::idb::IdbEvent::OpenSuccess { database, .. } => {
                                format!("success|{database}")
                            }
                            storage::idb::IdbEvent::AddSuccess { key, .. } => {
                                format!("addsuccess|{key}")
                            }
                            storage::idb::IdbEvent::Error { name, message, .. } => {
                                format!("error|{name}|{message}")
                            }
                        })
                        .collect::<Vec<_>>()
                        .join(";");
                    Ok(HostValue::Str(encoded))
                }),
            )?;
        }
        // —— 原生原语：存储（G4-S1）——
        {
            let (storage, origin_ref) = (Rc::clone(&inputs.storage), Rc::clone(&inputs.origin));
            install_global_fn(
                realm,
                "__iv8_storage_get",
                2,
                Box::new(move |_realm, args| {
                    let kind = storage_kind(args)?;
                    let key = arg_str(args, 1).unwrap_or_default();
                    let origin = origin_ref.borrow().clone();
                    let mut service = storage
                        .try_borrow_mut()
                        .map_err(|_| refcell_conflict("StorageService"))?;
                    Ok(service
                        .storage(&origin, kind)
                        .get_item(&key)
                        .map_or(HostValue::Null, HostValue::Str))
                }),
            )?;
            // `setItem`：**超配额 ⇒ 具名错误**（消息前缀 = `QuotaExceededError`；与 D4 同约定）。
            let (storage, origin_ref) = (Rc::clone(&inputs.storage), Rc::clone(&inputs.origin));
            install_global_fn(
                realm,
                "__iv8_storage_set",
                3,
                Box::new(move |_realm, args| {
                    let kind = storage_kind(args)?;
                    let key = arg_str(args, 1).unwrap_or_default();
                    let value = arg_str(args, 2).unwrap_or_default();
                    let origin = origin_ref.borrow().clone();
                    let mut service = storage
                        .try_borrow_mut()
                        .map_err(|_| refcell_conflict("StorageService"))?;
                    // **超配额 ⇒ 具名错误直抛**（`D4-M` 已修）：`JsError::named` 使 JS 面
                    // `e.name === 'QuotaExceededError'`、`String(e)` 与 Rust 面 `message` **均具名**
                    // ⇒ 原「哨兵串 + shim 重抛」绕行**已撤销**（宿主不再返回哨兵串）。
                    match service.storage(&origin, kind).set_item(&key, &value) {
                        Ok(()) => Ok(HostValue::Str("ok".to_owned())),
                        Err(storage::StorageError::QuotaExceeded { quota, attempted }) => {
                            Err(JsError::named(
                                JsErrorKind::Exception,
                                "QuotaExceededError",
                                format!("超出存储配额（quota={quota}, attempted={attempted}）"),
                            ))
                        }
                    }
                }),
            )?;
            let (storage, origin_ref) = (Rc::clone(&inputs.storage), Rc::clone(&inputs.origin));
            install_global_fn(
                realm,
                "__iv8_storage_remove",
                2,
                Box::new(move |_realm, args| {
                    let kind = storage_kind(args)?;
                    let key = arg_str(args, 1).unwrap_or_default();
                    let origin = origin_ref.borrow().clone();
                    let mut service = storage
                        .try_borrow_mut()
                        .map_err(|_| refcell_conflict("StorageService"))?;
                    service.storage(&origin, kind).remove_item(&key);
                    Ok(HostValue::Undefined)
                }),
            )?;
            let (storage, origin_ref) = (Rc::clone(&inputs.storage), Rc::clone(&inputs.origin));
            install_global_fn(
                realm,
                "__iv8_storage_clear",
                1,
                Box::new(move |_realm, args| {
                    let kind = storage_kind(args)?;
                    let origin = origin_ref.borrow().clone();
                    let mut service = storage
                        .try_borrow_mut()
                        .map_err(|_| refcell_conflict("StorageService"))?;
                    service.storage(&origin, kind).clear();
                    Ok(HostValue::Undefined)
                }),
            )?;
            let (storage, origin_ref) = (Rc::clone(&inputs.storage), Rc::clone(&inputs.origin));
            install_global_fn(
                realm,
                "__iv8_storage_key",
                2,
                Box::new(move |_realm, args| {
                    let kind = storage_kind(args)?;
                    let index = arg_str(args, 1)
                        .and_then(|raw| raw.parse::<usize>().ok())
                        .unwrap_or(usize::MAX);
                    let origin = origin_ref.borrow().clone();
                    let mut service = storage
                        .try_borrow_mut()
                        .map_err(|_| refcell_conflict("StorageService"))?;
                    Ok(service
                        .storage(&origin, kind)
                        .key(index)
                        .map_or(HostValue::Null, HostValue::Str))
                }),
            )?;
            let (storage, origin_ref) = (Rc::clone(&inputs.storage), Rc::clone(&inputs.origin));
            install_global_fn(
                realm,
                "__iv8_storage_length",
                1,
                Box::new(move |_realm, args| {
                    let kind = storage_kind(args)?;
                    let origin = origin_ref.borrow().clone();
                    let mut service = storage
                        .try_borrow_mut()
                        .map_err(|_| refcell_conflict("StorageService"))?;
                    let length = service.storage(&origin, kind).length();
                    // `length` 为 `usize`；JS 面为 `f64` —— 经 `u32` 转换避免精度损失告警。
                    let length = f64::from(u32::try_from(length).unwrap_or(u32::MAX));
                    Ok(HostValue::Number(length))
                }),
            )?;
        }

        realm.eval(SHIM)?;
        Ok(Self { inputs, node_map })
    }

    /// 节点 → wrapper（复用前存活性校验；`Collected` ⇒ 重建并覆盖绑定）。
    ///
    /// # Errors
    /// 建对象 / 登记句柄失败时报 [`JsError`]。
    pub fn wrapper_for(&mut self, realm: &mut Realm, node: NodeId) -> Result<HostValue, JsError> {
        wrapper_for_impl(realm, &self.node_map, node)
    }

    /// 供分派编排使用的共享依赖（克隆各 `Rc`）。
    #[must_use]
    pub fn deps(&self) -> DispatchDeps {
        dispatch_deps(&self.inputs, &self.node_map)
    }
}

/// 由 `BindingInputs` + `node_map` 组装分派依赖。
fn dispatch_deps(inputs: &BindingInputs, node_map: &Rc<RefCell<NodeMap>>) -> DispatchDeps {
    DispatchDeps {
        tree: Rc::clone(&inputs.tree),
        node_map: Rc::clone(node_map),
        registry: Rc::clone(&inputs.registry),
        activation: Rc::clone(&inputs.activation),
        active_element: Rc::clone(&inputs.active_element),
    }
}

/// 节点 → wrapper 的落点实现（原生原语与 `wrapper_for` 共用）。
///
/// **存活性校验（ADR-0016·D3）**：`node_map` 命中后先 `resolve_weak`；已回收 ⇒ `unbind` 后重建。
pub(crate) fn wrapper_for_impl(
    realm: &mut Realm,
    node_map: &Rc<RefCell<NodeMap>>,
    node: NodeId,
) -> Result<HostValue, JsError> {
    let key = node.raw();
    let existing = node_map
        .try_borrow()
        .map_err(|_| refcell_conflict("NodeMap"))?
        .handle_for(key);
    if let Some(weak) = existing {
        if let Ok(Some(value)) = realm.resolve_weak(weak) {
            return Ok(value);
        }
        node_map
            .try_borrow_mut()
            .map_err(|_| refcell_conflict("NodeMap"))?
            .unbind(key);
    }
    let handle = realm.new_object()?;
    let _lease = realm.root(handle)?;
    realm.set_prop(handle, NODE_KEY, HostValue::Str(node.raw().to_string()))?;
    let weak = realm.weak_handle(handle)?;
    node_map
        .try_borrow_mut()
        .map_err(|_| refcell_conflict("NodeMap"))?
        .bind_force(key, weak);
    Ok(HostValue::Object {
        realm: realm.id(),
        handle,
    })
}

/// 注册一个原生函数到全局（`FunctionTemplate` 路径）。
fn install_global_fn(
    realm: &mut Realm,
    name: &str,
    length: u32,
    f: NativeFn,
) -> Result<(), JsError> {
    let id = realm.id();
    let handle = realm.new_function(name, length, f)?;
    realm.set_global(name, HostValue::Function { realm: id, handle })
}

/// 取字符串参数（数字/布尔转字符串；其余 ⇒ `None`）。
fn arg_str(args: &[HostValue], index: usize) -> Option<String> {
    match args.get(index) {
        Some(HostValue::Str(text)) => Some(text.clone()),
        Some(HostValue::Number(number)) => Some(number.to_string()),
        Some(HostValue::Bool(flag)) => Some(flag.to_string()),
        _ => None,
    }
}

/// 取函数参数对应的句柄。
fn arg_handle(value: Option<&HostValue>) -> Option<JsHandle> {
    match value {
        Some(HostValue::Function { handle, .. } | HostValue::Object { handle, .. }) => {
            Some(*handle)
        }
        _ => None,
    }
}

/// 从 wrapper 反查 `NodeId`（读隐藏属性 `__iv8_node`）。
fn node_of(realm: &mut Realm, value: Option<&HostValue>) -> Result<Option<NodeId>, JsError> {
    let Some(HostValue::Object { handle, .. }) = value else {
        return Ok(None);
    };
    match realm.get_prop(*handle, NODE_KEY)? {
        HostValue::Str(raw) => Ok(raw.parse::<u64>().ok().map(raw_to_node)),
        _ => Ok(None),
    }
}

/// `u64`（高 32 = generation，低 32 = index）→ `NodeId`。
fn raw_to_node(raw: u64) -> NodeId {
    let index = u32::try_from(raw & u64::from(u32::MAX)).unwrap_or(u32::MAX);
    let generation = u32::try_from(raw >> 32).unwrap_or(u32::MAX);
    NodeId::new(index, generation)
}

/// 深度优先查找首个具 `attr == value` 的元素。
fn find_by_attr(tree: &NodeTree, root: NodeId, attr: &str, value: &str) -> Option<NodeId> {
    for child in tree.children(root) {
        if tree.get_attribute(child, attr) == Some(value) {
            return Some(child);
        }
        if let Some(found) = find_by_attr(tree, child, attr, value) {
            return Some(found);
        }
    }
    None
}

/// 深度优先查找首个标签名为 `tag` 的元素。
fn find_by_tag(tree: &NodeTree, root: NodeId, tag: &str) -> Option<NodeId> {
    for child in tree.children(root) {
        if tree
            .get(child)
            .and_then(|node| node.element.as_ref())
            .is_some_and(|element| element.name == tag)
        {
            return Some(child);
        }
        if let Some(found) = find_by_tag(tree, child, tag) {
            return Some(found);
        }
    }
    None
}

/// 手写 JS shim：安装 `document` / 元素访问器与方法，随即删除原生原语全局。
const SHIM: &str = r"
(function () {
  'use strict';
  var g = globalThis;
  var nCreate = g.__iv8_dom_create;
  var nAppend = g.__iv8_dom_append;
  var nGetById = g.__iv8_dom_get_by_id;
  var nDocument = g.__iv8_dom_document;
  var nBody = g.__iv8_dom_body;
  var nGetAttr = g.__iv8_dom_get_attr;
  var nSetAttr = g.__iv8_dom_set_attr;
  var nGetStyle = g.__iv8_dom_get_style;
  var nSetStyle = g.__iv8_dom_set_style;
  var nWrite = g.__iv8_dom_write;
  var nAttachShadow = g.__iv8_dom_attach_shadow;
  var nShadowRoot = g.__iv8_dom_shadow_root;
  var nGeom = g.__iv8_geom;
  var nGetComputed = g.__iv8_get_computed;
  var nDirty = g.__iv8_mark_dirty;
  var sGet = g.__iv8_storage_get, sSet = g.__iv8_storage_set;
  var sRemove = g.__iv8_storage_remove, sClear = g.__iv8_storage_clear;
  var sKey = g.__iv8_storage_key, sLength = g.__iv8_storage_length;
  var idbOpen = g.__iv8_idb_open, idbCreateStore = g.__iv8_idb_create_store;
  var idbAdd = g.__iv8_idb_add, idbPump = g.__iv8_idb_pump;
  var nEvAdd = g.__iv8_ev_add;
  var nEvRemove = g.__iv8_ev_remove;
  var nEvClick = g.__iv8_ev_click;
  var nEvDispatch = g.__iv8_ev_dispatch;
  delete g.__iv8_dom_create;
  delete g.__iv8_dom_append;
  delete g.__iv8_dom_get_by_id;
  delete g.__iv8_dom_document;
  delete g.__iv8_dom_body;
  delete g.__iv8_dom_get_attr;
  delete g.__iv8_dom_set_attr;
  delete g.__iv8_dom_get_style;
  delete g.__iv8_dom_set_style;
  delete g.__iv8_dom_write;
  delete g.__iv8_dom_attach_shadow;
  delete g.__iv8_dom_shadow_root;
  delete g.__iv8_geom;
  delete g.__iv8_get_computed;
  delete g.__iv8_mark_dirty;
  delete g.__iv8_storage_get; delete g.__iv8_storage_set;
  delete g.__iv8_storage_remove; delete g.__iv8_storage_clear;
  delete g.__iv8_storage_key; delete g.__iv8_storage_length;
  delete g.__iv8_idb_open; delete g.__iv8_idb_create_store;
  delete g.__iv8_idb_add; delete g.__iv8_idb_pump;
  delete g.__iv8_ev_add;
  delete g.__iv8_ev_remove;
  delete g.__iv8_ev_click;
  delete g.__iv8_ev_dispatch;

  function makeStorage(kind) {
    var s = {};
    Object.defineProperty(s, 'length', { get: function () { return sLength(kind); } });
    s.getItem = function (k) { return sGet(kind, String(k)); };
    s.setItem = function (k, v) {
      // `D4-M` 修后：宿主具名错误**原生可达**（`e.name` = 'QuotaExceededError'）⇒
      // 不再需要「哨兵串 + shim 重抛」绕行；超配额由 `sSet` **直接抛出**并传播给 JS。
      sSet(kind, String(k), String(v));
      return undefined;
    };
    s.removeItem = function (k) { return sRemove(kind, String(k)); };
    s.clear = function () { return sClear(kind); };
    s.key = function (i) { return sKey(kind, String(Number(i) >>> 0)); };
    return s;
  }
  g.localStorage = makeStorage('local');
  g.sessionStorage = makeStorage('session');

  // —— IndexedDB **面级** shim（G4-S1-d / §4.1c + §4.1c′）——
  // 序列：open ⇒ pump ⇒ onupgradeneeded ⇒ createObjectStore ⇒ pump ⇒ onsuccess ⇒ add ⇒ pump ⇒ onsuccess(key)。
  // **未达成员**（Transaction/Index/Cursor/getAll/…）⇒ 具名 `NotSupportedError`（逐员 missing，不吞成员）。
  function idbUnsupported(member) {
    var e = new Error('IndexedDB 成员 `' + member + '` 属未达面级（manifest 中逐员 missing；真实现须铁律 6 上报）');
    e.name = 'NotSupportedError';
    return e;
  }
  function makeIdbRequest(kind, handle) {
    var req = { __handle: handle, __kind: kind, result: undefined, error: null,
                onsuccess: null, onerror: null, onupgradeneeded: null };
    req.__dispatch = function (encoded) {
      if (!encoded) { return; }
      var parts = encoded.split(';');
      for (var i = 0; i < parts.length; i++) {
        var f = parts[i].split('|');
        var name = f[0];
        if (name === 'upgradeneeded') {
          if (typeof req.onupgradeneeded === 'function') { req.onupgradeneeded({ target: req, type: 'upgradeneeded' }); }
        } else if (name === 'success') {
          req.result = f[1];
          if (typeof req.onsuccess === 'function') { req.onsuccess({ target: req, type: 'success' }); }
        } else if (name === 'addsuccess') {
          req.result = f[1];
          if (typeof req.onsuccess === 'function') { req.onsuccess({ target: req, type: 'success' }); }
        } else if (name === 'error') {
          var err = new Error(f.slice(2).join('|'));
          err.name = f[1];
          req.error = err;
          if (typeof req.onerror === 'function') { req.onerror({ target: req, type: 'error' }); }
        }
      }
    };
    return req;
  }
  var __idb_store_counter = 0;
  var __idb_stores = {};
  var indexedDB = {
    open: function (name, version) {
      var req = makeIdbRequest('open', idbOpen(String(name), String(Number(version) || 1)));
      req.__pump = function () { req.__dispatch(idbPump(req.__handle)); };
      g.__iv8_idb_last_request = req;
      return req;
    },
    deleteDatabase: function () { throw idbUnsupported('IDBFactory.deleteDatabase'); },
    cmp: function () { throw idbUnsupported('IDBFactory.cmp'); },
    databases: function () { throw idbUnsupported('IDBFactory.databases'); }
  };
  g.indexedDB = indexedDB;
  // 打开请求的数据库面（仅在 onupgradeneeded/onsuccess 期间可建 store）。
  function makeIdbDatabase(request) {
    return {
      createObjectStore: function (name) {
        var handle = idbCreateStore(String(request.__handle), String(name));
        __idb_store_counter += 1;
        var store = {
          __handle: handle,
          add: function (value, key) {
            var r = makeIdbRequest('add', idbAdd(String(handle), String(value), key === undefined ? '' : String(key)));
            r.__pump = function () { r.__dispatch(idbPump(r.__handle)); };
            return r;
          },
          put: function () { throw idbUnsupported('IDBObjectStore.put'); },
          get: function () { throw idbUnsupported('IDBObjectStore.get'); },
          getAll: function () { throw idbUnsupported('IDBObjectStore.getAll'); },
          delete: function () { throw idbUnsupported('IDBObjectStore.delete'); },
          count: function () { throw idbUnsupported('IDBObjectStore.count'); },
          openCursor: function () { throw idbUnsupported('IDBObjectStore.openCursor'); },
          createIndex: function () { throw idbUnsupported('IDBObjectStore.createIndex'); },
          transaction: function () { throw idbUnsupported('IDBObjectStore.transaction'); }
        };
        return store;
      },
      transaction: function () { throw idbUnsupported('IDBDatabase.transaction'); },
      close: function () { throw idbUnsupported('IDBDatabase.close'); },
      deleteObjectStore: function () { throw idbUnsupported('IDBDatabase.deleteObjectStore'); }
    };
  }
  // `onupgradeneeded`/`onsuccess` 期间把 `transaction.source` 语义面接到数据库面。
  var __idb_orig_dispatch = makeIdbRequest;
  g.__iv8_idb_make_db = makeIdbDatabase;

  function decorate(node) {
    if (node === null || typeof node !== 'object') { return node; }
    Object.defineProperty(node, 'className', {
      configurable: true, enumerable: true,
      get: function () { var v = nGetAttr(this, 'class'); return v === null ? '' : v; },
      set: function (v) { nSetAttr(this, 'class', String(v)); }
    });
    Object.defineProperty(node, 'id', {
      configurable: true, enumerable: true,
      get: function () { var v = nGetAttr(this, 'id'); return v === null ? '' : v; },
      set: function (v) { nSetAttr(this, 'id', String(v)); }
    });
    Object.defineProperty(node, 'style', {
      configurable: true, enumerable: true,
      get: function () {
        var self = this;
        var proxy = new Proxy({}, {
          get: function (_t, prop) {
            if (typeof prop !== 'string') { return undefined; }
            var v = nGetStyle(self, prop);
            return v === null ? '' : v;
          },
          set: function (_t, prop, val) {
            if (typeof prop === 'string') { nSetStyle(self, prop, String(val)); nDirty('style', String(prop)); }
            return true;
          }
        });
        Object.defineProperty(this, 'style', { configurable: true, enumerable: true, value: proxy });
        return proxy;
      }
    });
    node.appendChild = function (child) { nAppend(this, child); nDirty('child', ''); return child; };
    node.setAttribute = function (name, value) { nSetAttr(this, String(name), String(value)); nDirty('attribute', ''); };
    node.getAttribute = function (name) { return nGetAttr(this, String(name)); };
    node.attachShadow = function (init) {
      var mode = (init && typeof init.mode === 'string') ? init.mode : '';
      var root = decorate(nAttachShadow(this, mode));
      nDirty('child', '');
      return root;
    };
    Object.defineProperty(node, 'shadowRoot', {
      configurable: true, enumerable: true,
      get: function () { var r = nShadowRoot(this); return r === null ? null : decorate(r); }
    });
    node.addEventListener = function (type, cb, opts) {
      var isObj = (opts && typeof opts === 'object');
      var cap = isObj ? !!opts.capture : !!opts;
      var once = isObj ? !!opts.once : false;
      nEvAdd(this, String(type), cb, cap, once);
    };
    node.removeEventListener = function (type, cb, opts) {
      var cap = (opts && typeof opts === 'object') ? !!opts.capture : !!opts;
      nEvRemove(this, String(type), cb, cap);
    };
    node.click = function () { nEvClick(this); };
    node.dispatchEvent = function (ev) {
      if (ev && typeof ev.type === 'string') { nEvDispatch(this, ev.type); }
      return true;
    };
    // 几何真值（S12；border-box 取整口径 §1.3；§2.5 同步读由宿主门面保证）。
    Object.defineProperty(node, 'offsetWidth', { configurable: true, enumerable: true, get: function () { return nGeom(this, 'offsetWidth'); } });
    Object.defineProperty(node, 'offsetHeight', { configurable: true, enumerable: true, get: function () { return nGeom(this, 'offsetHeight'); } });
    Object.defineProperty(node, 'clientWidth', { configurable: true, enumerable: true, get: function () { return nGeom(this, 'clientWidth'); } });
    Object.defineProperty(node, 'clientHeight', { configurable: true, enumerable: true, get: function () { return nGeom(this, 'clientHeight'); } });
    node.getBoundingClientRect = function () {
      var x = nGeom(this, 'rectLeft'); var y = nGeom(this, 'rectTop');
      var w = nGeom(this, 'rectWidth'); var h = nGeom(this, 'rectHeight');
      return { x: x, y: y, width: w, height: h, top: y, right: x + w, bottom: y + h, left: x };
    };
    return node;
  }

  var doc = decorate(nDocument());
  doc.createElement = function (tag) { return decorate(nCreate(String(tag))); };
  doc.getElementById = function (id) {
    var node = nGetById(String(id));
    return node === null ? null : decorate(node);
  };
  doc.write = function (text) { nWrite(String(text)); };
  Object.defineProperty(doc, 'body', {
    configurable: true, enumerable: true,
    get: function () { var b = nBody(); return b === null ? null : decorate(b); }
  });
  g.getComputedStyle = function (el, pseudo) {
    if (pseudo) { return null; }
    return {
      getPropertyValue: function (prop) { return nGetComputed(el, String(prop)); }
    };
  };
  g.document = doc;

  g.Event = function Event(type, init) {
    this.type = String(type);
    this.bubbles = !!(init && init.bubbles);
    this.cancelable = !!(init && init.cancelable);
    this.isTrusted = false;
    this.defaultPrevented = false;
    this.currentTarget = null;
    this.target = null;
  };
})();
";

#[cfg(test)]
mod tests {
    use super::{BindingInputs, PageBinding};
    use engine_v8::HostValue;
    use engine_v8::isolate::AgentRuntime;
    use engine_v8::realm::Realm;
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;
    use web_dom::mutation::MutationBuffer;
    use web_dom::node::{NodeId, NodeKind};
    use web_dom::tree::NodeTree;
    use web_events::{ListenerRegistry, UserActivation};

    /// 建 Realm + binding（含空树）。
    use crate::layout::LayoutFacade;

    fn setup() -> (Realm, PageBinding, Rc<RefCell<NodeTree>>) {
        let mut realm = AgentRuntime::new()
            .expect("runtime")
            .create_realm()
            .expect("realm");
        let tree = Rc::new(RefCell::new(NodeTree::new()));
        let inputs = BindingInputs {
            storage: Rc::new(RefCell::new(storage::StorageService::new())),
            idb: Rc::new(RefCell::new(storage::idb::IdbSurface::new())),
            origin: Rc::new(RefCell::new("https://a.example".to_owned())),
            tree: Rc::clone(&tree),
            mutations: Rc::new(RefCell::new(MutationBuffer::new())),
            writes: Rc::new(RefCell::new(Vec::new())),
            registry: Rc::new(RefCell::new(ListenerRegistry::new())),
            activation: Rc::new(RefCell::new(UserActivation::default())),
            active_element: Rc::new(Cell::new(None)),
            parser_active: Rc::new(Cell::new(true)),
            layout: {
                let mut facade =
                    LayoutFacade::new(layout_backend::geometry::Size::new(800.0, 600.0));
                facade
                    .register_sheet(web_style::SheetOrigin::UserAgent, layout_backend::UA_CSS)
                    .expect("ua sheet");
                Rc::new(RefCell::new(facade))
            },
        };
        let binding = PageBinding::install(&mut realm, inputs).expect("install");
        (realm, binding, tree)
    }

    fn find_by_tag(tree: &NodeTree, root: NodeId, tag: &str) -> Option<NodeId> {
        for child in tree.children(root) {
            if let Some(element) = tree.get(child).and_then(|node| node.element.as_ref()) {
                if element.name == tag {
                    return Some(child);
                }
            }
            if let Some(found) = find_by_tag(tree, child, tag) {
                return Some(found);
            }
        }
        None
    }

    #[test]
    fn js_create_element_and_append() {
        // §3.7：document.createElement/appendChild 反映到 NodeTree。
        let (mut realm, _binding, tree) = setup();
        realm
            .eval("var d = document.createElement('div'); d.id = 'made'; document.appendChild(d);")
            .expect("eval");
        let tree = tree.borrow();
        let div = find_by_tag(&tree, tree.root(), "div").expect("应建出 <div>");
        assert_eq!(tree.get_attribute(div, "id"), Some("made"));
        assert_eq!(tree.parent(div), Some(tree.root()), "应挂到文档根");
    }

    #[test]
    fn js_set_classname_and_style() {
        // §3.7：el.className / el.style.color 反映到属性 / 内联声明。
        let (mut realm, _binding, tree) = setup();
        realm
            .eval(
                "var e = document.createElement('div'); e.className = 'a b'; e.style.color = 'red'; document.appendChild(e);",
            )
            .expect("eval");
        let tree = tree.borrow();
        let div = find_by_tag(&tree, tree.root(), "div").expect("应有 div");
        assert_eq!(
            tree.get_attribute(div, "class"),
            Some("a b"),
            "className 应写入 class 属性"
        );
        let element = tree
            .get(div)
            .and_then(|node| node.element.as_ref())
            .unwrap();
        assert_eq!(
            element.style.get_property("color"),
            Some("red"),
            "style.color 应写入内联声明"
        );
    }

    #[test]
    fn shadow_root_open_only_via_js() {
        // §3.5：JS 层 shadowRoot 仅 open 返回；closed => null。
        let (mut realm, _binding, _tree) = setup();
        realm
            .eval(
                "var host = document.createElement('x-host'); document.appendChild(host);\
                 var shadow = host.attachShadow({ mode: 'open' });\
                 var got = host.shadowRoot;\
                 var closed = document.createElement('x-closed'); document.appendChild(closed);\
                 closed.attachShadow({ mode: 'closed' });\
                 globalThis.__openOk = (got !== null) && (got === shadow);\
                 globalThis.__closedOk = (closed.shadowRoot === null);",
            )
            .expect("eval");
        assert_eq!(
            realm.eval("globalThis.__openOk").unwrap(),
            HostValue::Bool(true),
            "open 应返回可操作影子根"
        );
        assert_eq!(
            realm.eval("globalThis.__closedOk").unwrap(),
            HostValue::Bool(true),
            "closed 应返回 null（G3-B14）"
        );
    }

    #[test]
    fn offset_size_is_border_box_int() {
        // §3.4：border-box 取整（81.5 => 82）。
        let (mut realm, _binding, _tree) = setup();
        realm
            .eval("var d = document.createElement('div'); d.setAttribute('style','width:81.5px;height:12px'); document.appendChild(d); globalThis.__w = d.offsetWidth;")
            .expect("eval");
        assert_eq!(
            realm.eval("globalThis.__w").unwrap(),
            HostValue::Number(82.0),
            "81.5px 应取整为 82"
        );
    }

    #[test]
    fn client_size_excludes_border() {
        // §3.4：clientWidth == offsetWidth - 左右 border。
        let (mut realm, _binding, _tree) = setup();
        realm
            .eval("var d = document.createElement('div'); d.setAttribute('style','width:100px;height:50px;border:2px solid'); document.appendChild(d); globalThis.__c = d.clientWidth;")
            .expect("eval");
        assert_eq!(
            realm.eval("globalThis.__c").unwrap(),
            HostValue::Number(96.0),
            "clientWidth = 100 - 2*2"
        );
    }

    #[test]
    fn computed_style_via_js() {
        // §3.4：getComputedStyle 经 JS 桥返回真值。
        let (mut realm, _binding, _tree) = setup();
        realm
            .eval("var d = document.createElement('div'); d.setAttribute('style','width:120px'); document.appendChild(d); globalThis.__cv = getComputedStyle(d).getPropertyValue('width');")
            .expect("eval");
        assert_eq!(
            realm.eval("globalThis.__cv").unwrap(),
            HostValue::Str("120px".to_owned()),
            "getComputedStyle width 应为 120px"
        );
    }

    #[test]
    fn geometry_after_mutation_updated() {
        // §3.4/§2.5：改 style => 读几何同步反映新值（非陈旧）。
        let (mut realm, _binding, _tree) = setup();
        realm
            .eval("var d = document.createElement('div'); d.setAttribute('style','width:100px;height:10px'); document.appendChild(d); globalThis.__before = d.offsetWidth; d.setAttribute('style','width:180px;height:10px'); globalThis.__after = d.offsetWidth;")
            .expect("eval");
        assert_eq!(
            realm.eval("globalThis.__before").unwrap(),
            HostValue::Number(100.0)
        );
        assert_eq!(
            realm.eval("globalThis.__after").unwrap(),
            HostValue::Number(180.0),
            "变更后同步读应为 180（§2.5）"
        );
    }

    #[test]
    fn bounding_rect_viewport_coords() {
        // §3.4：gBCR 的 x/y 视口坐标；right == x + width。
        let (mut realm, _binding, _tree) = setup();
        realm
            .eval("var a = document.createElement('div'); a.setAttribute('style','height:50px'); document.appendChild(a); var b = document.createElement('div'); b.setAttribute('style','height:10px;width:30px'); document.appendChild(b); var r = b.getBoundingClientRect(); globalThis.__ok = (r.top === 50 && r.left === 0 && r.right === r.x + r.width && r.bottom === r.y + r.height);")
            .expect("eval");
        assert_eq!(
            realm.eval("globalThis.__ok").unwrap(),
            HostValue::Bool(true),
            "gBCR 视口坐标 + right==x+width"
        );
    }

    #[test]
    fn computed_value_unsupported_property_errors() {
        // §3.4：白名单外属性 => 错误路径（JS 抛异常）。
        let (mut realm, _binding, _tree) = setup();
        let result = realm
            .eval("var d = document.createElement('div'); document.appendChild(d); var threw = false; try { getComputedStyle(d).getPropertyValue('z-index'); } catch (e) { threw = true; } globalThis.__threw = threw;")
            .expect("eval 应返回（不 panic）");
        let _ = result;
        assert_eq!(
            realm.eval("globalThis.__threw").unwrap(),
            HostValue::Bool(true),
            "白名单外属性应抛错（G3-B12③）"
        );
    }

    #[test]
    fn js_get_element_by_id() {
        // §3.7：document.getElementById 命中正确节点。
        let (mut realm, _binding, tree) = setup();
        realm
            .eval(
                "var e = document.createElement('span'); e.id = 'tgt'; document.appendChild(e); globalThis.__found = document.getElementById('tgt');",
            )
            .expect("eval");
        assert_eq!(
            realm.eval("globalThis.__found === e").unwrap(),
            HostValue::Bool(true),
            "getElementById 应返回同一 wrapper"
        );
        let tree = tree.borrow();
        let span = find_by_tag(&tree, tree.root(), "span").expect("应有 span");
        assert_eq!(
            tree.get(span).map(|node| node.kind),
            Some(NodeKind::Element)
        );
    }

    #[test]
    fn node_wrapper_identity_stable() {
        // §3.7：同一节点两次取回（不强制 GC）⇒ 同一 wrapper（node_map 复用）。
        let (mut realm, _binding, _tree) = setup();
        realm
            .eval(
                "var e = document.createElement('p'); e.id = 'same'; document.appendChild(e); globalThis.__a = document.getElementById('same'); globalThis.__b = document.getElementById('same');",
            )
            .expect("eval");
        assert_eq!(
            realm.eval("globalThis.__a === globalThis.__b").unwrap(),
            HostValue::Bool(true),
            "同一 node 两次取回应为同一 wrapper"
        );
    }

    #[test]
    fn node_wrapper_recreated_after_collection() {
        // §3.7 / ADR-0016·D3：wrapper 被回收后，同节点再次取回应**重建**（新 wrapper）。
        let (mut realm, mut binding, tree) = setup();
        let node = tree.borrow_mut().create_element("div", "html", Vec::new());
        let first = binding.wrapper_for(&mut realm, node).expect("first");
        let first_handle = match first {
            HostValue::Object { handle, .. } => handle,
            other => panic!("期望 Object，得到 {other:?}"),
        };
        // 宿主侧只持弱句柄 ⇒ 切断可达后 GC 回收 wrapper。
        realm.collect_garbage_for_testing().expect("gc");
        let second = binding.wrapper_for(&mut realm, node).expect("second");
        let second_handle = match second {
            HostValue::Object { handle, .. } => handle,
            other => panic!("期望 Object，得到 {other:?}"),
        };
        assert_ne!(
            first_handle, second_handle,
            "回收后同节点取回应重建 wrapper（新句柄）"
        );
    }

    #[test]
    fn refcell_reentry_is_error_not_panic() {
        // ADR-0016·D2 / §3.7：native fn 借树期间再借同树 ⇒ **可判定错误**（非 `RefCell` panic）。
        // 构造「native fn A 持 NodeTree 借用 → JS 触发 native fn B（createElement）再借树」：
        // 由测试持住树借用以模拟 A；native B 经 `try_borrow_mut` 失败 ⇒ 抛出可捕获错误（含 message）。
        let (mut realm, _binding, tree) = setup();
        let held = tree.borrow_mut();
        let message = match realm
            .eval(
                "(function () { try { document.createElement('div'); return 'no-error'; } catch (e) { return e.message; } })()",
            )
            .expect("eval 应返回（不 panic）")
        {
            HostValue::Str(text) => text,
            other => panic!("期望 Str，得到 {other:?}"),
        };
        drop(held);
        assert!(
            message.contains("RefCell 重入"),
            "重入借树应返回可判定错误，实得 {message:?}"
        );
    }

    #[test]
    fn dom_wrapper_not_a_cycle_leak() {
        // §3.7：只走弱引用路径 ⇒ 宿主侧不持强引用，切断 JS 可达后 GC 可回收（t2_1 形状）。
        let (mut realm, binding, _tree) = setup();
        realm
            .eval("globalThis.__node = document.createElement('section');")
            .expect("eval");
        let weak = match realm.eval("globalThis.__node").unwrap() {
            HostValue::Object { handle, .. } => {
                // 经 binding 的 node_map 反查该 node 的弱句柄。
                let raw = match realm.get_prop(handle, super::NODE_KEY).unwrap() {
                    HostValue::Str(raw) => raw.parse::<u64>().unwrap(),
                    other => panic!("期望 Str，得到 {other:?}"),
                };
                let node = super::raw_to_node(raw);
                binding
                    .node_map
                    .borrow()
                    .handle_for(node.raw())
                    .expect("node_map 应有弱绑定")
            }
            other => panic!("期望 Object，得到 {other:?}"),
        };
        // 宿主侧只持弱句柄 ⇒ 切断 JS 可达后应可回收（无强引用钉住）。
        realm.eval("delete globalThis.__node;").unwrap();
        realm.collect_garbage_for_testing().expect("gc");
        assert!(
            matches!(realm.resolve_weak(weak), Ok(None) | Err(_)),
            "弱引用路径不应钉住 wrapper（应已回收）"
        );
    }
}
