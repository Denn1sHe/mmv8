//! `BrowsingContext`：页面全部状态 + `ControlPlane`（内含 `Realm`）+ `RunLoop`（技术方案 §4；ADR-0017·D7 / ADR-0018·D2）。
//!
//! **本批（批 2F / S13 余项）**：`__iv8_fetch` 绑定 + `FetchTable` 接线（闭包直接捕获 `Rc`）、
//! `MutationObserver` 交付（copy-out `mem::take`、checkpoint 前段）、`PageBinding`（S11）与
//! input（S12）接线、模块脚本最小纳入（S9b）。
//!
//! # `PageShared` 持有形态（批 2E 精化）
//! `BrowsingContext.shared` 为**值**（非 `Rc<RefCell<PageShared>>`）；**需跨 V8 回调共享的子单元**
//! （`tree`/`registry`/`activation`/`mutations`/`observers`/`writes`/`fetch_table`/…）各自以
//! `Rc<RefCell<…>>` 字段，供原生闭包**直接捕获**（`network` 保持 `&mut NetworkService`）。

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::rc::Rc;
use std::time::Duration;

use engine_v8::control::ControlPlane;
use engine_v8::error::{JsError, JsErrorKind};
use engine_v8::handles::HandleLease;
use engine_v8::isolate::AgentRuntime;
use engine_v8::microtask::MicrotaskPolicy;
use engine_v8::native_fn::NativeFn;
use engine_v8::realm::Realm;
use engine_v8::{HostValue, WeakJsHandle};

use network::{
    FetchOutcome, NetworkError, NetworkService, OfflineBundle, RequestId, RequestSpec, Response,
    ResponseKind, Transport,
};
use policy::NavigationEpoch;
use scheduler::{
    LogicalClock, RunBudget, RunLoop, RunOutcome, SchedulerError, SchedulerErrorKind, TaskExecutor,
    TaskSource, TaskToken, TaskVerdict,
};
use web_dom::mutation::{
    MutationBuffer, MutationKind, MutationRecord, ObserveOptions, ObserverRegistry,
};
use web_dom::node::NodeId;
use web_dom::tree::NodeTree;
use web_events::{ListenerRegistry, UserActivation};
use web_style::InlineStyle;

use crate::binding::{BindingInputs, PageBinding};
use crate::controller::{LoadBlockerKind, NavigationController, ReadyState};
use crate::error::{PageError, PageErrorKind};
use crate::input::{self, InputKind};
use crate::layout::LayoutFacade;
use crate::parser::{ParseOutcome, ParserHost};

/// 一个预置离线资源（供解析后 `fetch`/外链脚本命中）。
#[derive(Clone, Debug)]
pub struct OfflineResource {
    /// 资源 URL。
    pub url: String,
    /// HTTP 状态码。
    pub status: u16,
    /// 响应头。
    pub headers: Vec<(String, String)>,
    /// 响应体。
    pub body: Vec<u8>,
}

/// 页面快照（`page.load(snapshot)` 入参）。
#[derive(Clone, Debug)]
pub struct PageSnapshot {
    /// 文档 URL。
    pub url: String,
    /// HTML 源。
    pub html: String,
    /// 响应头。
    pub headers: Vec<(String, String)>,
    /// 预置离线资源。
    pub resources: Vec<OfflineResource>,
}

/// `__iv8_fetch` 关闭捕获的 `res`/`rej` 表（ADR-0018·D3；`HandleLease` 只在 navigation 类型面）。
#[derive(Debug, Default)]
pub struct FetchTable {
    entries: HashMap<RequestId, (HandleLease, HandleLease)>,
}

impl FetchTable {
    /// 建空表。
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// 登记一个 `res`/`rej` 对。
    pub fn insert(&mut self, id: RequestId, resolve: HandleLease, reject: HandleLease) {
        self.entries.insert(id, (resolve, reject));
    }

    /// 取出并移除一个 `res`/`rej` 对（`complete` 时释放租借）。
    pub fn remove(&mut self, id: RequestId) -> Option<(HandleLease, HandleLease)> {
        self.entries.remove(&id)
    }

    /// 表内条数。
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否为空。
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// 一条待处理的 fetch 票（`__iv8_fetch` 未命中时登记；由 `run_until` 泵入 `NetworkService`）。
#[derive(Debug)]
pub struct FetchTicket {
    url: String,
    resolve: HandleLease,
    reject: HandleLease,
}

/// 一条延后执行的脚本（classic `defer` / `type=module`；S9b 模块条款）。
#[derive(Clone, Copy, Debug)]
pub struct DeferredScript {
    node: NodeId,
    module: bool,
}

/// 页面共享可变状态（**单线程**；跨回调子单元为 `Rc<RefCell<…>>`）。别名单元清单见 ADR-0016·D2。
#[derive(Debug)]
pub struct PageShared {
    /// 导航协调器（URL/origin/readyState/epoch/blockers）。
    pub controller: NavigationController,
    /// 节点树（导航重绑时整体重建；`Rc` 以便 parser / 闭包共享）。
    pub tree: Rc<RefCell<NodeTree>>,
    /// 页面级内联样式（极简）。
    pub style: InlineStyle,
    /// 监听器注册表。
    pub registry: Rc<RefCell<ListenerRegistry>>,
    /// user activation 状态。
    pub activation: Rc<RefCell<UserActivation>>,
    /// `document.activeElement` 最小态（ADR-0019·D7）。
    pub active_element: Rc<Cell<Option<NodeId>>>,
    /// parser 是否活跃（`readyState == Loading`）；`document.write` post-load 据此呈具名错误。
    pub parser_active: Rc<Cell<bool>>,
    /// 网络服务（**值持有**；`network()` 返回 `&mut`）。
    pub network: NetworkService,
    /// 离线资源束（与网络 transport **共享**，供 fetch 就地解析）。
    pub offline: Rc<RefCell<OfflineBundle>>,
    /// 变更记录缓冲。
    pub mutations: Rc<RefCell<MutationBuffer>>,
    /// `MutationObserver` 注册表（回调槽 `WeakJsHandle`）。
    pub observers: Rc<RefCell<ObserverRegistry<WeakJsHandle>>>,
    /// `document.write` 缓冲（**单一真相源**）。
    pub writes: Rc<RefCell<Vec<String>>>,
    /// `res`/`rej` 表（`__iv8_fetch` 关闭捕获）。
    pub fetch_table: Rc<RefCell<FetchTable>>,
    /// fetch 票队列（`__iv8_fetch` 未命中入队；`run_until` 泵入网络）。
    pub fetch_queue: Rc<RefCell<VecDeque<FetchTicket>>>,
    /// 宿主已 `complete` 的响应（供下一次 `run_until` resolve）。
    pub completed: Rc<RefCell<HashMap<RequestId, Response>>>,
    /// 延后脚本队列（classic `defer` + `type=module`，文档序）。
    pub deferred: Rc<RefCell<Vec<DeferredScript>>>,
    /// 是否存在**未解析到的外链脚本**（`<script src>` 离线未命中 ⇒ 停泊；§3.2）。
    pub pending_external: Rc<Cell<bool>>,
    /// 布局门面（S13：宿主持 `LayoutBackend`；与 binding 共享；导航 `reset` 时整体替换）。
    pub layout: Rc<RefCell<LayoutFacade>>,
}

/// 建一个已装 UA 表的布局门面（S13；导航 `reset` 亦复用）。
fn new_layout_facade() -> Rc<RefCell<LayoutFacade>> {
    let mut facade = LayoutFacade::new(layout_backend::geometry::Size::new(800.0, 600.0));
    facade
        .register_sheet(web_style::SheetOrigin::UserAgent, layout_backend::UA_CSS)
        .expect("UA 表应可登记");
    Rc::new(RefCell::new(facade))
}

/// G2 能力面口径（§3.8；`geometry` = stub 为**明确登记**）。
#[allow(clippy::struct_excessive_bools)] // 能力面本质是布尔开关集合。
#[derive(Clone, Debug)]
pub struct Capabilities {
    /// 解析形态。
    pub parsing: &'static str,
    /// 是否支持 `document.write`。
    pub document_write: bool,
    /// 是否支持导航。
    pub navigation: bool,
    /// 网络形态。
    pub network: &'static str,
    /// 输入形态（显式 target）。
    pub input: &'static str,
    /// 几何口径（G2 = stub）。
    pub geometry: &'static str,
    /// 是否支持 Shadow DOM（G2 = false）。
    pub shadow_dom: bool,
    /// 脚本是否可被 watchdog 截断（P-1a 已证可靠 ⇒ true）。
    pub script_interruptible: bool,
    /// user activation 口径（G2 = `sticky_only`）。
    pub input_user_activation: &'static str,
    /// 单 Realm 导航（旧 JS 全局残留 = **具名登记偏差**，ADR-0018·D2）。
    pub single_realm_navigation: bool,
}

/// 页面浏览上下文。
pub struct BrowsingContext {
    plane: ControlPlane,
    shared: PageShared,
    runloop: RunLoop,
    parser: Option<ParserHost>,
    binding: Option<PageBinding>,
    /// 脚本 watchdog 墙钟上限（`eval_with_deadline`；ADR-0017·D2）。默认 2000ms。
    script_deadline: Duration,
}

impl fmt::Debug for BrowsingContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BrowsingContext")
            .field("url", &self.shared.controller.url())
            .field("epoch", &self.shared.controller.epoch())
            .finish_non_exhaustive()
    }
}

impl BrowsingContext {
    /// 建浏览上下文（单 Realm 单线程；安装页面桥）。
    ///
    /// # Errors
    /// URL 非法 ⇒ [`PageErrorKind::InvalidUrl`]；建 Realm / 安装页面桥失败 ⇒ [`PageErrorKind::Internal`]。
    pub fn new(url: &str) -> Result<Self, PageError> {
        let controller = NavigationController::new(url)?;
        let mut runtime = AgentRuntime::new()
            .map_err(|err| PageError::new(PageErrorKind::Internal, err.message().to_owned()))?;
        let mut plane = ControlPlane::new(&mut runtime)
            .map_err(|err| PageError::new(PageErrorKind::Internal, err.message().to_owned()))?;
        // §6：G2 默认 `Explicit`——微任务只在宿主显式 `perform_checkpoint` 时跑。
        plane
            .realm_mut()
            .set_microtasks_policy(MicrotaskPolicy::Explicit);
        let shared = PageShared::new(controller);
        let mut context = Self {
            plane,
            shared,
            runloop: RunLoop::new(Box::new(LogicalClock::new())),
            parser: None,
            binding: None,
            script_deadline: Duration::from_millis(2000),
        };
        context.install_page_bridge()?;
        Ok(context)
    }

    /// `page.load`：`begin_navigation`（epoch+1）+ **重建文档/树/绑定/ParserHost**（ADR-0018·D2）
    /// + 执行 defer/module 脚本 + 派发 `DOMContentLoaded`。
    ///
    /// # Errors
    /// URL 非法，或脚本 / 模块执行失败（含 TLA 具名错误）时报错。
    pub fn load(&mut self, snapshot: &PageSnapshot) -> Result<NavigationEpoch, PageError> {
        let previous_epoch = self.shared.controller.epoch();
        let epoch = self.shared.controller.begin_navigation(&snapshot.url)?;
        // 旧代次网络请求作废（不回写新文档；ADR-0018·D3）。
        self.shared.network.abort_epoch(previous_epoch);
        let base = u32::try_from(epoch.get()).unwrap_or(u32::MAX);
        let tree = Rc::new(RefCell::new(NodeTree::with_generation_base(base)));
        let document = tree.borrow().root();
        // 重绑：清各别名单元（`Rc` 整体替换 ⇒ 旧闭包捕获的旧单元被丢弃）。
        self.shared.tree = Rc::clone(&tree);
        self.shared.registry = Rc::new(RefCell::new(ListenerRegistry::new()));
        self.shared.activation = Rc::new(RefCell::new(UserActivation::default()));
        self.shared.active_element = Rc::new(Cell::new(None));
        self.shared.mutations = Rc::new(RefCell::new(MutationBuffer::new()));
        self.shared.observers = Rc::new(RefCell::new(ObserverRegistry::new()));
        self.shared.writes = Rc::new(RefCell::new(Vec::new()));
        self.shared.fetch_table = Rc::new(RefCell::new(FetchTable::new()));
        self.shared.fetch_queue = Rc::new(RefCell::new(VecDeque::new()));
        self.shared.completed = Rc::new(RefCell::new(HashMap::new()));
        self.shared.deferred = Rc::new(RefCell::new(Vec::new()));
        self.shared.pending_external = Rc::new(Cell::new(false));
        // 布局 reset：新文档 ⇒ 门面整体重建（脏位归零，UA 表重装）。
        self.shared.layout = new_layout_facade();
        self.shared.controller.set_document(document);
        // 装离线资源（与网络 transport 共享，供 fetch 就地解析）。
        let mut bundle = OfflineBundle::new();
        for resource in &snapshot.resources {
            bundle.insert(
                &resource.url,
                resource.status,
                resource.headers.clone(),
                resource.body.clone(),
            );
        }
        let offline = Rc::new(RefCell::new(bundle));
        self.shared.offline = Rc::clone(&offline);
        self.shared.network = NetworkService::new(Box::new(SharedBundle(Rc::clone(&offline))));
        // 重建页面桥（新树 / 新子单元）。
        self.install_page_bridge()?;
        // 重建 ParserHost 并解析（阻塞脚本就地执行；defer/module 入队）。
        let mut parser = ParserHost::new(Rc::clone(&tree), html5ever::ParseOpts::default());
        let outcome = parser.feed(&snapshot.html);
        self.parser = Some(parser);
        self.shared.parser_active.set(true);
        let parsed_done = self.drain_parse(outcome)?;
        self.shared.parser_active.set(false);
        if parsed_done {
            self.shared.controller.mark_parse_finished();
            self.run_deferred_scripts(&snapshot.url)?;
            self.fire_dom_content_loaded()?;
            self.shared.controller.try_finish();
        }
        Ok(epoch)
    }

    /// 有界推进（ADR-0017·D8：解构字段 + 独立 executor）；先泵 fetch 票并解析已完成响应。
    ///
    /// # Errors
    /// 运行循环错误（如重入）或响应 resolve 失败 ⇒ [`PageError`]。
    pub fn run_until(&mut self, budget: RunBudget) -> Result<RunOutcome, PageError> {
        self.pump_fetch()?;
        self.resolve_completed()?;
        let Self {
            runloop, shared, ..
        } = self;
        let mut exec = PageExec { shared };
        runloop
            .run_until(budget, &mut exec)
            .map_err(PageError::from)
    }

    /// 网络服务（可变）。
    pub fn network(&mut self) -> &mut NetworkService {
        &mut self.shared.network
    }

    /// 转发 `plane.realm_mut()`（ADR-0017·D7）。
    pub fn realm_mut(&mut self) -> &mut Realm {
        self.plane.realm_mut()
    }

    /// 页面范围求值（G1 `eval` 语义；`__iv8__` 不可见，§2.4）。
    ///
    /// # Errors
    /// 求值失败时报 [`JsError`]。
    pub fn eval(&mut self, src: &str) -> Result<HostValue, JsError> {
        self.plane.realm_mut().eval(src)
    }

    /// 页面范围求值一个 ES 模块（G1 `eval_module` 语义）。
    ///
    /// # Errors
    /// 求值失败时报 [`JsError`]。
    pub fn eval_module(&mut self, src: &str, url: &str) -> Result<HostValue, JsError> {
        self.plane.realm_mut().eval_module(src, url)
    }

    /// 控制范围求值（G1 `control_eval` 语义；`__iv8__` 临时可用，RAII）。
    ///
    /// # Errors
    /// 页面脚本期调用 ⇒ `ControlRevoked`；其余求值失败时报 [`JsError`]。
    pub fn control_eval(&mut self, src: &str) -> Result<HostValue, JsError> {
        self.plane.eval_control(src)
    }

    /// 当前就绪状态（导航生命周期）。
    #[must_use]
    pub fn ready_state(&self) -> ReadyState {
        self.shared.controller.ready_state()
    }

    /// 当前文档 URL。
    #[must_use]
    pub fn url(&self) -> &str {
        self.shared.controller.url()
    }

    /// 当前 origin（规范化字符串，如 `https://a.example`）。
    #[must_use]
    pub fn origin_string(&self) -> String {
        self.shared.controller.origin().to_string()
    }

    /// 当前文档根节点。
    #[must_use]
    pub fn document(&self) -> NodeId {
        self.shared.controller.document()
    }

    /// 节点树（只读借用）。
    #[must_use]
    pub fn tree(&self) -> Rc<RefCell<NodeTree>> {
        Rc::clone(&self.shared.tree)
    }

    /// user activation 状态（G2 只置位 `sticky`）。
    #[must_use]
    pub fn user_activation(&self) -> UserActivation {
        *self.shared.activation.borrow()
    }

    /// `document.activeElement` 最小态。
    #[must_use]
    pub fn active_element(&self) -> Option<NodeId> {
        self.shared.active_element.get()
    }

    /// 注册一个 `MutationObserver`（回调槽 `WeakJsHandle`），返回其 id。
    pub fn observe(
        &mut self,
        target: NodeId,
        options: ObserveOptions,
        callback: WeakJsHandle,
    ) -> u64 {
        self.shared
            .observers
            .borrow_mut()
            .observe(target, options, callback)
    }

    /// 交付微任务 checkpoint：**宿主前段**先交付 `MutationObserver`（copy-out），再跑 V8 微任务
    /// （ADR-0017·D5：一处定死 + 登记规范偏差）。
    ///
    /// # Errors
    /// 交付 MO / V8 checkpoint 失败时报错。
    pub fn perform_checkpoint(&mut self) -> Result<usize, PageError> {
        self.deliver_mutations()?;
        self.plane.realm_mut().perform_checkpoint().map_err(map_js)
    }

    /// 显式 target 可信输入（ADR-0019：目标须属于当前文档）。
    ///
    /// # Errors
    /// 无 target / target 已失效 ⇒ `TargetNotInDocument` / `StaleDocumentNode`；分派失败 ⇒ [`PageError`]。
    pub fn input_dispatch(
        &mut self,
        kind: InputKind,
        target: Option<NodeId>,
    ) -> Result<(), PageError> {
        let target = target.ok_or_else(|| {
            PageError::new(
                PageErrorKind::TargetNotInDocument,
                "G2 输入需显式 target（命中测试属 G3）",
            )
        })?;
        let document = self.shared.controller.document();
        let (valid, in_document) = {
            let tree = self.shared.tree.borrow();
            (
                tree.is_valid(target),
                input::is_descendant_or_self(&tree, target, document),
            )
        };
        if !valid {
            return Err(PageError::new(
                PageErrorKind::StaleDocumentNode,
                "target 节点已失效（可能属旧文档）",
            ));
        }
        if !in_document {
            return Err(PageError::new(
                PageErrorKind::TargetNotInDocument,
                "target 不属于当前文档",
            ));
        }
        let deps = self
            .binding
            .as_ref()
            .ok_or_else(|| PageError::new(PageErrorKind::Internal, "缺少页面绑定"))?
            .deps();
        input::dispatch_trusted(self.plane.realm_mut(), &deps, target, kind).map_err(map_js)
    }

    /// 显式 target 可信输入（`target_raw` = `NodeId::raw()` 的 `u64`；供 Python 控制面免依赖 `web-dom`）。
    ///
    /// # Errors
    /// 同 [`BrowsingContext::input_dispatch`]。
    pub fn input_dispatch_raw(
        &mut self,
        kind: InputKind,
        target_raw: Option<u64>,
    ) -> Result<(), PageError> {
        let target = target_raw.map(|raw| {
            let index = u32::try_from(raw & u64::from(u32::MAX)).unwrap_or(u32::MAX);
            let generation = u32::try_from(raw >> 32).unwrap_or(u32::MAX);
            NodeId::new(index, generation)
        });
        self.input_dispatch(kind, target)
    }

    /// 宿主回填一个 fetch 响应（`FetchOutcome::Pending` 的那条；下次 `run_until` 内 resolve）。
    ///
    /// # Errors
    /// 已中止 / 代次过期 / 未知 id ⇒ [`PageErrorKind::NavigationAborted`]。
    pub fn complete_fetch(&mut self, id: RequestId, response: Response) -> Result<(), PageError> {
        self.shared
            .network
            .complete(id, response.clone())
            .map_err(|err| {
                PageError::new(PageErrorKind::NavigationAborted, err.message().to_owned())
            })?;
        self.shared.completed.borrow_mut().insert(id, response);
        Ok(())
    }

    /// 未完成 fetch 的 id（供宿主 `complete`）。
    #[must_use]
    pub fn pending_fetches(&self) -> Vec<RequestId> {
        self.shared.network.pending()
    }

    /// G2 能力面。
    /// **宿主驱动路径**（S13）：脏 ⇒ 隐式同步重建布局（§2.5）。
    ///
    /// # Errors
    /// `RefCell` 冲突（可判定，不 panic）或级联 / 布局失败 ⇒ [`PageErrorKind::Internal`]。
    pub fn relayout_if_dirty(&mut self) -> Result<(), PageError> {
        let tree = Rc::clone(&self.shared.tree);
        let tree = tree
            .try_borrow()
            .map_err(|_| PageError::new(PageErrorKind::Internal, "NodeTree 借用冲突"))?;
        let mut layout = self
            .shared
            .layout
            .try_borrow_mut()
            .map_err(|_| PageError::new(PageErrorKind::Internal, "LayoutFacade 借用冲突"))?;
        layout.relayout_if_dirty(&tree)
    }

    /// G3 能力面口径（§3.8）：`geometry = "computed"`、`shadow_dom = true`。
    #[must_use]
    pub fn capabilities(&self) -> Capabilities {
        Capabilities {
            parsing: "streaming",
            document_write: true,
            navigation: true,
            network: "offline",
            input: "explicit_target",
            geometry: "computed",
            shadow_dom: true,
            script_interruptible: true,
            input_user_activation: "sticky_only",
            single_realm_navigation: true,
        }
    }

    /// 关闭页面（幂等）：作废当前代次网络请求并清缓冲。
    ///
    /// # Errors
    /// 当前不产生错误（保留 `Result` 以便将来上报）。
    pub fn close(&mut self) -> Result<(), PageError> {
        let epoch = self.shared.controller.epoch();
        self.shared.network.abort_epoch(epoch);
        self.shared.writes.borrow_mut().clear();
        self.shared.fetch_table.borrow_mut().entries.clear();
        self.shared.fetch_queue.borrow_mut().clear();
        self.shared.completed.borrow_mut().clear();
        // 关闭底层 Realm（G1 `close` 语义：此后 eval 报明确错误；幂等）。
        self.plane.realm_mut().close().map_err(map_js)?;
        Ok(())
    }

    /// 安装页面桥（DOM/事件 binding + `__iv8_fetch` + JS shim）。
    fn install_page_bridge(&mut self) -> Result<(), PageError> {
        let inputs = BindingInputs {
            tree: Rc::clone(&self.shared.tree),
            mutations: Rc::clone(&self.shared.mutations),
            writes: Rc::clone(&self.shared.writes),
            registry: Rc::clone(&self.shared.registry),
            activation: Rc::clone(&self.shared.activation),
            active_element: Rc::clone(&self.shared.active_element),
            parser_active: Rc::clone(&self.shared.parser_active),
            layout: Rc::clone(&self.shared.layout),
        };
        let binding = PageBinding::install(self.plane.realm_mut(), inputs).map_err(map_js)?;
        self.binding = Some(binding);
        install_fetch(self.plane.realm_mut(), Rc::clone(&self.shared.fetch_queue))
            .map_err(map_js)?;
        Ok(())
    }

    /// 就地执行阻塞脚本直至解析完成（`</script>` → 求值 → `resume`）；module/defer 入队。
    ///
    /// 返回 `true` = 解析已到底（可推进 readyState）；`false` = 因外链脚本未命中而**停泊**（§3.2）。
    fn drain_parse(&mut self, mut outcome: ParseOutcome) -> Result<bool, PageError> {
        loop {
            match outcome {
                ParseOutcome::Done => return Ok(true),
                ParseOutcome::BlockedOnScript(node) => {
                    // 外链脚本（`<script src>`）：经 OfflineBundle 同步解析（§3.2 / §2.1）。
                    let src = self
                        .shared
                        .tree
                        .borrow()
                        .get_attribute(node, "src")
                        .map(str::to_owned);
                    if let Some(src) = src {
                        let url = resolve_script_src(self.shared.controller.url(), &src);
                        let response = self.shared.offline.borrow().resolve(&url);
                        if let Some(response) = response {
                            let body = String::from_utf8_lossy(&response.body).into_owned();
                            self.eval_page_script(&body)?;
                            self.flush_writes();
                        } else {
                            // 未命中 ⇒ 登记 load blocker 并停泊（非 Requeue 自旋 ⇒ WaitingIo）。
                            self.shared
                                .controller
                                .add_blocker(LoadBlockerKind::ExternalScript);
                            self.shared.pending_external.set(true);
                            return Ok(false);
                        }
                    } else {
                        let kind = script_kind(&self.shared.tree.borrow(), node);
                        match kind {
                            ScriptKind::Module | ScriptKind::DeferredClassic => {
                                let module = kind == ScriptKind::Module;
                                self.shared
                                    .deferred
                                    .borrow_mut()
                                    .push(DeferredScript { node, module });
                            }
                            ScriptKind::BlockingClassic => {
                                let text = self.shared.tree.borrow().text_content(node);
                                self.eval_page_script(&text)?;
                                self.flush_writes();
                            }
                        }
                    }
                    let Some(parser) = self.parser.as_mut() else {
                        return Err(PageError::new(PageErrorKind::Internal, "解析器缺失"));
                    };
                    outcome = parser.resume();
                }
            }
        }
    }

    /// 解析完成后、`DOMContentLoaded` 前，按文档序执行 defer/module 脚本（S9b）。
    fn run_deferred_scripts(&mut self, base_url: &str) -> Result<(), PageError> {
        let deferred: Vec<DeferredScript> = self.shared.deferred.borrow_mut().drain(..).collect();
        for (index, script) in deferred.into_iter().enumerate() {
            let text = self.shared.tree.borrow().text_content(script.node);
            if script.module {
                if contains_top_level_await(&text) {
                    return Err(PageError::new(
                        PageErrorKind::ModuleTopLevelAwaitUnsupported,
                        "G2 不支持模块顶层 await（ADR-0017 模块条款 ④）",
                    ));
                }
                let url = format!("{base_url}#module-{index}");
                self.plane
                    .with_page_script(|plane| plane.realm_mut().eval_module(&text, &url))
                    .map_err(map_js)?;
            } else {
                self.eval_page_script(&text)?;
            }
            self.flush_writes();
        }
        Ok(())
    }

    /// 派发 `DOMContentLoaded`（target = 文档根；文档序末）。
    fn fire_dom_content_loaded(&mut self) -> Result<(), PageError> {
        let document = self.shared.controller.document();
        let deps = self
            .binding
            .as_ref()
            .ok_or_else(|| PageError::new(PageErrorKind::Internal, "缺少页面绑定"))?
            .deps();
        input::dispatch_named(
            self.plane.realm_mut(),
            &deps,
            document,
            "DOMContentLoaded",
            true,
        )
        .map_err(map_js)
    }

    /// 在**页面 scope**下求值一段 classic 脚本（带 watchdog 截断，ADR-0017·D2）。
    fn eval_page_script(&mut self, src: &str) -> Result<(), PageError> {
        let deadline = self.script_deadline;
        self.plane
            .with_page_script(|plane| plane.realm_mut().eval_with_deadline(src, deadline))
            .map_err(map_js)?;
        Ok(())
    }

    /// 设置脚本 watchdog 墙钟上限（毫秒；默认 2000ms）——供测试缩短截断等待。
    pub fn set_script_deadline_ms(&mut self, ms: u64) {
        self.script_deadline = Duration::from_millis(ms);
    }

    /// 把 `document.write` 写缓冲交给 parser（保序）。
    fn flush_writes(&mut self) {
        let texts: Vec<String> = self.shared.writes.borrow_mut().drain(..).collect();
        if texts.is_empty() {
            return;
        }
        if let Some(parser) = self.parser.as_mut() {
            for text in &texts {
                parser.enqueue_document_write(text);
            }
        }
    }

    /// 泵 fetch 票：把 `__iv8_fetch` 登记项喂入 `NetworkService`（命中 ⇒ 就地 resolve）。
    fn pump_fetch(&mut self) -> Result<(), PageError> {
        loop {
            let ticket = self.shared.fetch_queue.borrow_mut().pop_front();
            let Some(ticket) = ticket else {
                return Ok(());
            };
            let id = self.shared.network.next_request_id();
            let epoch = self.shared.controller.epoch();
            let spec = RequestSpec::simple(id, &ticket.url, epoch);
            match self.shared.network.fetch(spec) {
                Ok(FetchOutcome::Ready(_)) => {
                    let response = self.shared.offline.borrow().resolve(&ticket.url);
                    let object = build_response_object(self.plane.realm_mut(), response.as_ref())
                        .map_err(map_js)?;
                    let handle = ticket.resolve.handle();
                    self.plane
                        .realm_mut()
                        .call(handle, None, &[object])
                        .map_err(map_js)?;
                }
                Ok(FetchOutcome::Pending(_)) => {
                    self.shared
                        .fetch_table
                        .borrow_mut()
                        .insert(id, ticket.resolve, ticket.reject);
                }
                Err(err) => {
                    let message = HostValue::Str(err.message().to_owned());
                    let handle = ticket.reject.handle();
                    let _ = self.plane.realm_mut().call(handle, None, &[message]);
                }
            }
        }
    }

    /// 解析宿主已 `complete` 的 fetch 响应（promise resolve）。
    fn resolve_completed(&mut self) -> Result<(), PageError> {
        let done: Vec<(RequestId, Response)> = self.shared.completed.borrow_mut().drain().collect();
        for (id, response) in done {
            let entry = self.shared.fetch_table.borrow_mut().remove(id);
            if let Some((resolve, _reject)) = entry {
                let object = build_response_object(self.plane.realm_mut(), Some(&response))
                    .map_err(map_js)?;
                let handle = resolve.handle();
                self.plane
                    .realm_mut()
                    .call(handle, None, &[object])
                    .map_err(map_js)?;
            }
        }
        Ok(())
    }

    /// 交付待发 `MutationObserver` 记录（**copy-out**：`mem::take` 后释放借用再回调）。
    fn deliver_mutations(&mut self) -> Result<(), PageError> {
        let records = self
            .shared
            .mutations
            .try_borrow_mut()
            .map_err(|_| page_refcell_conflict("MutationBuffer"))?
            .take_records();
        if records.is_empty() {
            return Ok(());
        }
        let ids = self
            .shared
            .observers
            .try_borrow()
            .map_err(|_| page_refcell_conflict("ObserverRegistry"))?
            .ids();
        let mut deliveries: Vec<(WeakJsHandle, Vec<MutationRecord>)> = Vec::new();
        for id in ids {
            let (target, options, callback) = {
                let registry = self
                    .shared
                    .observers
                    .try_borrow()
                    .map_err(|_| page_refcell_conflict("ObserverRegistry"))?;
                let (Some(target), Some(options), Some(callback)) = (
                    registry.target_of(id),
                    registry.options_of(id),
                    registry.callback_of(id).copied(),
                ) else {
                    continue;
                };
                (target, options, callback)
            };
            let matching: Vec<MutationRecord> = {
                let tree = self
                    .shared
                    .tree
                    .try_borrow()
                    .map_err(|_| page_refcell_conflict("NodeTree"))?;
                records
                    .iter()
                    .filter(|record| record_matches(record, target, options, &tree))
                    .cloned()
                    .collect()
            };
            if !matching.is_empty() {
                deliveries.push((callback, matching));
            }
        }
        for (callback, records) in deliveries {
            let array = build_records_array(self.plane.realm_mut(), &records).map_err(map_js)?;
            let _ = self.plane.realm_mut().call_weak(callback, None, &[array]);
        }
        Ok(())
    }
}

impl PageShared {
    /// 建初始共享状态（字段 `new`）。
    fn new(controller: NavigationController) -> Self {
        Self {
            controller,
            tree: Rc::new(RefCell::new(NodeTree::new())),
            style: InlineStyle::new(),
            registry: Rc::new(RefCell::new(ListenerRegistry::new())),
            activation: Rc::new(RefCell::new(UserActivation::default())),
            active_element: Rc::new(Cell::new(None)),
            parser_active: Rc::new(Cell::new(false)),
            network: NetworkService::new(Box::new(OfflineBundle::new())),
            offline: Rc::new(RefCell::new(OfflineBundle::new())),
            mutations: Rc::new(RefCell::new(MutationBuffer::new())),
            observers: Rc::new(RefCell::new(ObserverRegistry::new())),
            writes: Rc::new(RefCell::new(Vec::new())),
            fetch_table: Rc::new(RefCell::new(FetchTable::new())),
            fetch_queue: Rc::new(RefCell::new(VecDeque::new())),
            completed: Rc::new(RefCell::new(HashMap::new())),
            deferred: Rc::new(RefCell::new(Vec::new())),
            pending_external: Rc::new(Cell::new(false)),
            layout: new_layout_facade(),
        }
    }
}

/// 与 `NetworkService` 共享同一离线资源束的 transport（`network` crate 保持零 `engine-v8`）。
#[derive(Debug)]
struct SharedBundle(Rc<RefCell<OfflineBundle>>);

impl Transport for SharedBundle {
    fn send(&mut self, spec: &RequestSpec) -> Result<Response, NetworkError> {
        self.0
            .borrow()
            .resolve(&spec.url)
            .ok_or_else(|| NetworkError::not_found(&spec.url))
    }
}

/// 脚本分流种类（暂停点对 `type`/`defer` 无感 ⇒ 调用方判；ADR-0017 模块条款 ①）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ScriptKind {
    /// 阻塞式 classic（就地执行）。
    BlockingClassic,
    /// classic `defer`（解析后执行）。
    DeferredClassic,
    /// `type=module`（解析后执行，永不在暂停点执行）。
    Module,
}

/// 解析外链脚本 URL：绝对 URL 原样；`/path` 取 base origin；其余按 base 目录拼接（G2 最小面）。
fn resolve_script_src(base: &str, src: &str) -> String {
    if src.starts_with("http://") || src.starts_with("https://") {
        return src.to_owned();
    }
    let origin = match base.find("://").and_then(|scheme_end| {
        base[scheme_end + 3..]
            .find('/')
            .map(|slash| &base[..scheme_end + 3 + slash])
    }) {
        Some(origin) => origin,
        None => base,
    };
    if src.starts_with('/') {
        return format!("{origin}{src}");
    }
    format!("{origin}/{src}")
}

/// 由 `<script>` 元素的 `type`/`defer` 属性分流。
fn script_kind(tree: &NodeTree, node: NodeId) -> ScriptKind {
    let type_ = tree
        .get_attribute(node, "type")
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    if type_ == "module" {
        return ScriptKind::Module;
    }
    if tree.get_attribute(node, "defer").is_some() {
        return ScriptKind::DeferredClassic;
    }
    ScriptKind::BlockingClassic
}

/// 粗略检测**顶层 `await`**（brace 深度 0 处的 `await` 标识符）⇒ TLA（S9b ④）。
fn contains_top_level_await(src: &str) -> bool {
    let bytes = src.as_bytes();
    let n = bytes.len();
    let mut index = 0;
    let mut depth: i32 = 0;
    while index < n {
        let current = bytes[index] as char;
        match current {
            '{' => {
                depth += 1;
                index += 1;
            }
            '}' => {
                depth -= 1;
                index += 1;
            }
            _ if current.is_ascii_alphabetic() || current == '_' || current == '$' => {
                let start = index;
                while index < n {
                    let ch = bytes[index] as char;
                    if ch.is_ascii_alphanumeric() || ch == '_' || ch == '$' {
                        index += 1;
                    } else {
                        break;
                    }
                }
                if depth <= 0 && &src[start..index] == "await" {
                    return true;
                }
            }
            _ => index += 1,
        }
    }
    false
}

/// 建一个 JS `Response` 最小对象（`status` / `ok` / `bodyText`）。
fn build_response_object(
    realm: &mut Realm,
    response: Option<&Response>,
) -> Result<HostValue, JsError> {
    let handle = realm.new_object()?;
    let _lease = realm.root(handle)?;
    let (status, ok, body) = match response {
        Some(response) => (
            f64::from(response.status),
            response.kind == ResponseKind::Ok,
            String::from_utf8_lossy(&response.body).into_owned(),
        ),
        None => (0.0, false, String::new()),
    };
    realm.set_prop(handle, "status", HostValue::Number(status))?;
    realm.set_prop(handle, "ok", HostValue::Bool(ok))?;
    realm.set_prop(handle, "bodyText", HostValue::Str(body))?;
    Ok(HostValue::Object {
        realm: realm.id(),
        handle,
    })
}

/// 建一个 JS 记录数组（`{type}` 每项）。
fn build_records_array(
    realm: &mut Realm,
    records: &[MutationRecord],
) -> Result<HostValue, JsError> {
    let handle = realm.new_array()?;
    let _lease = realm.root(handle)?;
    for (index, record) in records.iter().enumerate() {
        let kind = match record.kind {
            MutationKind::ChildList => "childList",
            MutationKind::Attributes => "attributes",
            MutationKind::CharacterData => "characterData",
        };
        let object = realm.new_object()?;
        realm.set_prop(object, "type", HostValue::Str(kind.to_owned()))?;
        realm.set_prop(
            handle,
            &index.to_string(),
            HostValue::Object {
                realm: realm.id(),
                handle: object,
            },
        )?;
    }
    realm.set_prop(
        handle,
        "length",
        HostValue::Number(f64::from(u32::try_from(records.len()).unwrap_or(u32::MAX))),
    )?;
    Ok(HostValue::Object {
        realm: realm.id(),
        handle,
    })
}

/// 一条记录是否匹配某观察器（目标 + 选项；`subtree` 含后代）。
fn record_matches(
    record: &MutationRecord,
    target: NodeId,
    options: ObserveOptions,
    tree: &NodeTree,
) -> bool {
    let kind_ok = match record.kind {
        MutationKind::ChildList => options.child_list,
        MutationKind::Attributes => options.attributes,
        MutationKind::CharacterData => false,
    };
    if !kind_ok {
        return false;
    }
    if record.target == target {
        return true;
    }
    options.subtree && input::is_descendant_or_self(tree, record.target, target)
}

/// 注册 `__iv8_fetch` 原生函数 + `fetch` JS shim。
fn install_fetch(
    realm: &mut Realm,
    queue: Rc<RefCell<VecDeque<FetchTicket>>>,
) -> Result<(), JsError> {
    let function: NativeFn = Box::new(move |realm, args| {
        let url = match args.first() {
            Some(HostValue::Str(text)) => text.clone(),
            _ => String::new(),
        };
        let Some(HostValue::Function {
            handle: resolve, ..
        }) = args.get(1)
        else {
            return Err(JsError::new(
                JsErrorKind::Internal,
                "fetch 需要 resolve 函数",
            ));
        };
        let Some(HostValue::Function { handle: reject, .. }) = args.get(2) else {
            return Err(JsError::new(
                JsErrorKind::Internal,
                "fetch 需要 reject 函数",
            ));
        };
        let resolve = realm.root(*resolve)?;
        let reject = realm.root(*reject)?;
        queue.borrow_mut().push_back(FetchTicket {
            url,
            resolve,
            reject,
        });
        Ok(HostValue::Undefined)
    });
    let id = realm.id();
    let handle = realm.new_function("__iv8_fetch", 3, function)?;
    realm.set_global("__iv8_fetch", HostValue::Function { realm: id, handle })?;
    realm.eval(FETCH_SHIM)?;
    Ok(())
}

/// `fetch` JS shim（捕获原语后删除全局，收窄页面可见面）。
const FETCH_SHIM: &str = r"
(function () {
  'use strict';
  var g = globalThis;
  var nFetch = g.__iv8_fetch;
  delete g.__iv8_fetch;
  g.fetch = function (u) {
    return new Promise(function (resolve, reject) { nFetch(String(u), resolve, reject); });
  };
})();
";

/// 构造「共享单元借用冲突」可判定错误（ADR-0016·D2：`try_borrow` ⇒ 可判定错误，非 panic）。
fn page_refcell_conflict(what: &str) -> PageError {
    PageError::new(
        PageErrorKind::Internal,
        format!("RefCell 重入：{what} 已被借用（ADR-0016·D2，可判定错误）"),
    )
}

/// 把 `JsError` 映射为 [`PageError`]（ADR-0017·D11 映射表）。
#[allow(clippy::needless_pass_by_value)] // 作为 `.map_err(map_js)` 适配器，须按值接住错误。
fn map_js(err: JsError) -> PageError {
    let kind = match err.kind() {
        JsErrorKind::Terminated => PageErrorKind::Terminated,
        JsErrorKind::ReentrantEval => PageErrorKind::Reentrancy,
        _ => PageErrorKind::Internal,
    };
    PageError::new(kind, err.message().to_owned())
}

/// `run_until` 的独立 executor（ADR-0017·D8）。
struct PageExec<'a> {
    shared: &'a PageShared,
}

impl TaskExecutor for PageExec<'_> {
    fn run_task(
        &mut self,
        _source: TaskSource,
        _token: TaskToken,
    ) -> Result<TaskVerdict, SchedulerError> {
        Ok(TaskVerdict::Done)
    }

    fn has_pending_io(&self) -> bool {
        !self.shared.network.pending().is_empty()
            || !self.shared.fetch_queue.borrow().is_empty()
            || !self.shared.fetch_table.borrow().is_empty()
            || self.shared.pending_external.get()
    }
}

impl From<SchedulerError> for PageError {
    fn from(err: SchedulerError) -> Self {
        let kind = match err.kind() {
            SchedulerErrorKind::Reentrancy => PageErrorKind::Reentrancy,
            SchedulerErrorKind::Closed | SchedulerErrorKind::Internal => PageErrorKind::Internal,
        };
        PageError::new(kind, err.message().to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::{BrowsingContext, OfflineResource, PageSnapshot};
    use crate::error::PageErrorKind;
    use crate::input::InputKind;
    use engine_v8::HostValue;
    use engine_v8::native_fn::NativeFn;
    use network::{RequestId, Response};
    use scheduler::{RunBudget, RunOutcome};
    use std::cell::Cell;
    use std::rc::Rc;
    use web_dom::mutation::ObserveOptions;
    use web_dom::node::NodeId;
    use web_dom::tree::NodeTree;

    fn snapshot(url: &str, html: &str) -> PageSnapshot {
        PageSnapshot {
            url: url.to_owned(),
            html: html.to_owned(),
            headers: Vec::new(),
            resources: Vec::new(),
        }
    }

    fn snapshot_with(url: &str, html: &str, resources: Vec<OfflineResource>) -> PageSnapshot {
        PageSnapshot {
            url: url.to_owned(),
            html: html.to_owned(),
            headers: Vec::new(),
            resources,
        }
    }

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

    /// 按**文档序**（前序 DFS）收集元素标签名。
    fn element_names_in_order(tree: &NodeTree, root: NodeId) -> Vec<String> {
        let mut names = Vec::new();
        for child in tree.children(root) {
            if let Some(element) = tree.get(child).and_then(|node| node.element.as_ref()) {
                names.push(element.name.clone());
            }
            names.extend(element_names_in_order(tree, child));
        }
        names
    }

    fn page_with_button() -> BrowsingContext {
        let mut page = BrowsingContext::new("https://a.example/").expect("new");
        page.load(&snapshot(
            "https://a.example/doc",
            "<html><body><button id='b'>x</button></body></html>",
        ))
        .expect("load");
        page
    }

    #[test]
    fn page_load_streams_snapshot() {
        // §3.8：page.load 建文档并流式解析。
        let mut page = BrowsingContext::new("https://a.example/").expect("new");
        let epoch = page
            .load(&snapshot(
                "https://a.example/doc",
                "<html><body><p>x</p></body></html>",
            ))
            .expect("load");
        assert_eq!(epoch.get(), 1, "load 应 epoch+1");
        let tree = page.tree();
        let root = tree.borrow().root();
        assert!(tree.borrow().len() > 1, "应有节点");
        assert_eq!(page.document(), root);
        assert_eq!(page.capabilities().geometry, "computed");
    }

    #[test]
    fn capabilities_reports_g3_surface() {
        // §4 page：G3 能力面 —— geometry 由 stub 升为 computed；shadow_dom 开启。
        let page = BrowsingContext::new("https://a.example/").expect("new");
        let caps = page.capabilities();
        assert_eq!(caps.geometry, "computed", "G3 几何面应为 computed");
        assert!(caps.shadow_dom, "G3 应开启 Shadow DOM 面");
    }

    #[test]
    fn node_id_not_aliased_and_stale_document_rejected() {
        // ADR-0018·D2：重绑新建 NodeTree（generation 基线单调）⇒ 旧文档根 id 不别名。
        let mut page = BrowsingContext::new("https://a.example/").expect("new");
        page.load(&snapshot("https://a.example/one", "<body>"))
            .expect("load1");
        let old_doc = page.document();
        page.load(&snapshot("https://a.example/two", "<body>"))
            .expect("load2");
        let new_doc = page.document();
        assert_ne!(old_doc, new_doc, "跨导航文档根 id 不得别名");
        assert!(
            !page.tree().borrow().is_valid(old_doc),
            "旧文档节点在新树中应失效"
        );
    }

    #[test]
    fn inline_script_executes_during_load() {
        let mut page = BrowsingContext::new("https://a.example/").expect("new");
        page.load(&snapshot(
            "https://a.example/s",
            "<body><script>globalThis.__ran = 41 + 1;</script>",
        ))
        .expect("load");
        let value = page.realm_mut().eval("globalThis.__ran").expect("eval");
        assert_eq!(value, HostValue::Number(42.0));
    }

    #[test]
    fn run_until_returns_outcome() {
        let mut page = BrowsingContext::new("https://a.example/").expect("new");
        let outcome = page.run_until(RunBudget::tasks(4)).expect("run_until");
        assert_eq!(outcome, RunOutcome::Idle);
    }

    #[test]
    fn from_scheduler_error_maps_reentrancy() {
        use scheduler::{SchedulerError, SchedulerErrorKind};
        let err = super::PageError::from(SchedulerError::new(SchedulerErrorKind::Reentrancy, "x"));
        assert_eq!(err.kind(), PageErrorKind::Reentrancy);
    }

    // ———————————————————————— ① fetch ————————————————————————

    #[test]
    fn fetch_offline_hit_is_ready() {
        // §3.5：OfflineBundle 命中 ⇒ promise 可解（run_until 泵入网络就地 resolve）。
        let mut page = BrowsingContext::new("https://a.example/").expect("new");
        page.load(&snapshot_with(
            "https://a.example/",
            "<body></body>",
            vec![OfflineResource {
                url: "https://a.example/data".to_owned(),
                status: 200,
                headers: Vec::new(),
                body: b"hi".to_vec(),
            }],
        ))
        .expect("load");
        page.realm_mut()
            .eval("globalThis.__got = 'unset'; fetch('https://a.example/data').then(function(r){ globalThis.__got = r.status; });")
            .expect("fetch");
        page.run_until(RunBudget::unlimited()).expect("run");
        page.perform_checkpoint().expect("checkpoint");
        assert_eq!(
            page.realm_mut().eval("globalThis.__got").unwrap(),
            HostValue::Number(200.0),
            "命中应 resolve 为 200"
        );
    }

    #[test]
    fn fetch_miss_is_pending_then_waiting_io() {
        // §3.5：未命中 ⇒ Pending；run_until 返回 WaitingIo（不凭空完成）。
        let mut page = BrowsingContext::new("https://a.example/").expect("new");
        page.load(&snapshot("https://a.example/", "<body></body>"))
            .expect("load");
        page.realm_mut()
            .eval("fetch('https://a.example/missing');")
            .expect("fetch");
        let outcome = page.run_until(RunBudget::unlimited()).expect("run");
        assert_eq!(outcome, RunOutcome::WaitingIo, "未命中应停泊为 WaitingIo");
        assert_eq!(page.pending_fetches().len(), 1, "应有一条 pending");
    }

    #[test]
    fn network_complete_resolves_promise() {
        // §3.5：host complete ⇒ promise resolve ⇒ then 在 checkpoint 执行。
        let mut page = BrowsingContext::new("https://a.example/").expect("new");
        page.load(&snapshot("https://a.example/", "<body></body>"))
            .expect("load");
        page.realm_mut()
            .eval("globalThis.__hit = 0; fetch('https://a.example/late').then(function(){ globalThis.__hit = 1; });")
            .expect("fetch");
        page.run_until(RunBudget::unlimited()).expect("run"); // 登记 pending
        let id = page.pending_fetches()[0];
        page.complete_fetch(id, Response::ok(201, Vec::new(), b"body".to_vec()))
            .expect("complete");
        page.run_until(RunBudget::unlimited()).expect("run2"); // resolve
        page.perform_checkpoint().expect("checkpoint");
        assert_eq!(
            page.realm_mut().eval("globalThis.__hit").unwrap(),
            HostValue::Number(1.0),
            "complete 后 then 应执行"
        );
    }

    #[test]
    fn complete_unknown_id_is_error() {
        let mut page = BrowsingContext::new("https://a.example/").expect("new");
        page.load(&snapshot("https://a.example/", "<body></body>"))
            .expect("load");
        let err = page
            .complete_fetch(
                RequestId::new(99),
                Response::ok(200, Vec::new(), Vec::new()),
            )
            .expect_err("未知 id 应报错");
        assert_eq!(err.kind(), PageErrorKind::NavigationAborted);
    }

    // ———————————————————————— ① MutationObserver 交付 ————————————————————————

    #[test]
    fn mutation_observer_delivered_at_checkpoint() {
        // §3.7：变更在微任务 checkpoint（宿主前段）交付 MO 回调。
        let mut page = page_with_button();
        let fired = Rc::new(Cell::new(false));
        let flag = Rc::clone(&fired);
        let callback: NativeFn = Box::new(move |_realm, _args| {
            flag.set(true);
            Ok(HostValue::Undefined)
        });
        let handle = page
            .realm_mut()
            .new_function("mo", 1, callback)
            .expect("mo fn");
        let _lease = page.realm_mut().root(handle).expect("root mo");
        let weak = page.realm_mut().weak_handle(handle).expect("weak mo");
        let document = page.document();
        page.observe(
            document,
            ObserveOptions {
                child_list: true,
                attributes: false,
                subtree: false,
                attribute_old_value: false,
            },
            weak,
        );
        page.realm_mut()
            .eval("document.appendChild(document.createElement('div'));")
            .expect("mutate");
        page.perform_checkpoint().expect("checkpoint");
        assert!(fired.get(), "MO 回调应在 checkpoint 前段被交付");
    }

    // ———————————————————————— ② DOM 内核最小面 ————————————————————————

    #[test]
    fn js_create_element_and_append_reflects() {
        let mut page = page_with_button();
        page.realm_mut()
            .eval("var d = document.createElement('div'); d.id = 'made'; document.body.appendChild(d);")
            .expect("eval");
        let tree = page.tree();
        let made = find_by_attr(&tree.borrow(), tree.borrow().root(), "id", "made");
        assert!(
            made.is_some(),
            "createElement/appendChild 应反映到 NodeTree"
        );
    }

    #[test]
    fn js_set_classname_and_style_reflects() {
        let mut page = page_with_button();
        page.realm_mut()
            .eval("var e = document.createElement('div'); e.className = 'a b'; e.style.color = 'red'; document.body.appendChild(e);")
            .expect("eval");
        let tree = page.tree();
        let node = find_by_attr(&tree.borrow(), tree.borrow().root(), "class", "a b")
            .expect("应有 class='a b' 的节点");
        let element = tree
            .borrow()
            .get(node)
            .and_then(|data| data.element.as_ref())
            .cloned();
        assert_eq!(
            element.and_then(|e| e.style.get_property("color").map(str::to_owned)),
            Some("red".to_owned())
        );
    }

    #[test]
    fn js_get_element_by_id_hits() {
        let mut page = page_with_button();
        assert_eq!(
            page.realm_mut()
                .eval("document.getElementById('b') !== null")
                .unwrap(),
            HostValue::Bool(true)
        );
        assert_eq!(
            page.realm_mut()
                .eval("document.getElementById('nope') === null")
                .unwrap(),
            HostValue::Bool(true)
        );
    }

    // ———————————————————————— ③ input ————————————————————————

    #[test]
    fn trusted_click_dispatches_to_target() {
        // §3.6：显式 target 收到 click；JS event.isTrusted === true。
        let mut page = page_with_button();
        page.realm_mut()
            .eval("globalThis.__trusted = null; document.getElementById('b').addEventListener('click', function(ev){ globalThis.__trusted = ev.isTrusted; });")
            .expect("listener");
        let button = find_by_attr(
            &page.tree().borrow(),
            page.tree().borrow().root(),
            "id",
            "b",
        )
        .expect("button");
        page.input_dispatch(InputKind::Click, Some(button))
            .expect("dispatch");
        assert_eq!(
            page.realm_mut().eval("globalThis.__trusted").unwrap(),
            HostValue::Bool(true),
            "可信 click 应送达且 isTrusted=true"
        );
    }

    #[test]
    fn listener_phase_order_capture_target_bubble() {
        // §3.6：capture（root→…→target）→ target → bubble（target→…→root）。
        let mut page = page_with_button();
        page.realm_mut()
            .eval(
                "globalThis.__order = []; \
                 document.addEventListener('click', function(){ globalThis.__order.push('root-cap'); }, true); \
                 document.body.addEventListener('click', function(){ globalThis.__order.push('body-cap'); }, true); \
                 var b = document.getElementById('b'); \
                 b.addEventListener('click', function(){ globalThis.__order.push('bubble'); }); \
                 document.body.addEventListener('click', function(){ globalThis.__order.push('body-bub'); }); \
                 document.addEventListener('click', function(){ globalThis.__order.push('root-bub'); });",
            )
            .expect("listeners");
        let button = find_by_attr(
            &page.tree().borrow(),
            page.tree().borrow().root(),
            "id",
            "b",
        )
        .expect("button");
        page.input_dispatch(InputKind::Click, Some(button))
            .expect("dispatch");
        let order = match page
            .realm_mut()
            .eval("globalThis.__order.join(',')")
            .unwrap()
        {
            HostValue::Str(text) => text,
            other => panic!("期望 Str，得到 {other:?}"),
        };
        assert_eq!(
            order, "root-cap,body-cap,bubble,body-bub,root-bub",
            "捕获→目标→冒泡顺序应正确"
        );
    }

    #[test]
    fn untrusted_event_not_trusted() {
        // §3.6：new Event('click') 的 isTrusted === false（依赖 Event 桥）。
        let mut page = page_with_button();
        assert_eq!(
            page.realm_mut()
                .eval("new Event('click').isTrusted")
                .unwrap(),
            HostValue::Bool(false)
        );
    }

    #[test]
    fn trusted_activation_sets_user_activation() {
        // §3.6：可信 click 后 sticky == true（transient 不置位）。
        let mut page = page_with_button();
        assert!(!page.user_activation().sticky);
        let button = find_by_attr(
            &page.tree().borrow(),
            page.tree().borrow().root(),
            "id",
            "b",
        )
        .expect("button");
        page.input_dispatch(InputKind::Click, Some(button))
            .expect("dispatch");
        let activation = page.user_activation();
        assert!(activation.sticky, "可信 click 后应置位 sticky");
        assert!(!activation.transient, "G2 不置位 transient");
        assert_eq!(
            page.active_element(),
            Some(button),
            "click 默认动作应聚焦 target"
        );
    }

    #[test]
    fn once_listener_removed_after_fire() {
        // §3.6：{once:true} 监听器第二次不再触发。
        let mut page = page_with_button();
        page.realm_mut()
            .eval("globalThis.__n = 0; document.getElementById('b').addEventListener('click', function(){ globalThis.__n += 1; }, { once: true });")
            .expect("listener");
        let button = find_by_attr(
            &page.tree().borrow(),
            page.tree().borrow().root(),
            "id",
            "b",
        )
        .expect("button");
        page.input_dispatch(InputKind::Click, Some(button))
            .expect("d1");
        page.input_dispatch(InputKind::Click, Some(button))
            .expect("d2");
        assert_eq!(
            page.realm_mut().eval("globalThis.__n").unwrap(),
            HostValue::Number(1.0),
            "once 监听器应仅触发一次"
        );
    }

    #[test]
    fn el_click_dispatches_from_script() {
        // ADR-0019·D6：脚本内 el.click() 经 dispatch 专用入口同步分派。
        let mut page = page_with_button();
        page.realm_mut()
            .eval(
                "globalThis.__clicked = 0; \
                 var b = document.getElementById('b'); \
                 b.addEventListener('click', function(){ globalThis.__clicked += 1; }); \
                 b.click();",
            )
            .expect("script click");
        assert_eq!(
            page.realm_mut().eval("globalThis.__clicked").unwrap(),
            HostValue::Number(1.0),
            "脚本内 el.click() 应同步分派监听器"
        );
    }

    #[test]
    fn explicit_target_must_belong_to_document() {
        // §3.6：target 不属于当前 Document ⇒ TargetNotInDocument。
        let mut page = page_with_button();
        // 建一个未挂树的游离元素。
        let detached = {
            let tree = page.tree();
            let mut tree = tree.borrow_mut();
            tree.create_element("div", "html", Vec::new())
        };
        let err = page
            .input_dispatch(InputKind::Click, Some(detached))
            .expect_err("游离 target 应被拒");
        assert_eq!(err.kind(), PageErrorKind::TargetNotInDocument);

        // 无 target ⇒ 亦拒。
        let err = page
            .input_dispatch(InputKind::Click, None)
            .expect_err("无 target 应被拒");
        assert_eq!(err.kind(), PageErrorKind::TargetNotInDocument);
    }

    // ———————————————————————— ④ 模块脚本（S9b） ————————————————————————

    #[test]
    fn module_script_deferred_and_executes() {
        // §3.2：module 不在暂停点执行、按 defer 在解析后执行。
        let mut page = BrowsingContext::new("https://a.example/").expect("new");
        page.load(&snapshot(
            "https://a.example/m",
            "<body><div id='late'></div><script type='module'>globalThis.__m = document.getElementById('late') !== null;</script></body>",
        ))
        .expect("load");
        assert_eq!(
            page.realm_mut().eval("globalThis.__m").unwrap(),
            HostValue::Bool(true),
            "module 应看到其后的 DOM（解析后执行）"
        );
    }

    #[test]
    fn classic_defer_module_domcontentloaded_order() {
        // §3.2：classic-defer / module / DOMContentLoaded 三条相对顺序。
        let mut page = BrowsingContext::new("https://a.example/").expect("new");
        page.load(&snapshot(
            "https://a.example/order",
            "<body>\
             <script>globalThis.__order = ['inline'];</script>\
             <script defer>globalThis.__order.push('defer');</script>\
             <script type='module'>globalThis.__order.push('module');</script>\
             <script>document.addEventListener('DOMContentLoaded', function(){ globalThis.__order.push('dcl'); });</script>\
             </body>",
        ))
        .expect("load");
        let order = match page
            .realm_mut()
            .eval("globalThis.__order.join(',')")
            .unwrap()
        {
            HostValue::Str(text) => text,
            other => panic!("期望 Str，得到 {other:?}"),
        };
        assert_eq!(
            order, "inline,defer,module,dcl",
            "顺序应为 inline→defer→module→DOMContentLoaded"
        );
    }

    #[test]
    fn module_top_level_await_unsupported() {
        // §3.2 / ADR-0017 模块条款 ④：TLA ⇒ 具名错误。
        let mut page = BrowsingContext::new("https://a.example/").expect("new");
        let err = page
            .load(&snapshot(
                "https://a.example/tla",
                "<body><script type='module'>export const x = await 1;</script></body>",
            ))
            .expect_err("TLA 应具名报错");
        assert_eq!(err.kind(), PageErrorKind::ModuleTopLevelAwaitUnsupported);
    }

    // ———————————————————————— document.write（JS 路径，ADR-0017·D4） ————————————————————————

    #[test]
    fn document_write_parses_as_html_js_path() {
        // §3.3：解析期 document.write（JS 路径）注入的 HTML 应产生节点。
        let mut page = BrowsingContext::new("https://a.example/").expect("new");
        page.load(&snapshot(
            "https://a.example/w",
            "<body><script>document.write('<u>x</u>');</script></body>",
        ))
        .expect("load");
        let tree = page.tree();
        let found = {
            let tree = tree.borrow();
            find_by_tag(&tree, tree.root(), "u")
        };
        assert!(found.is_some(), "document.write 注入的 <u> 应入树");
    }

    #[test]
    fn document_write_after_load_js_path() {
        // §3.3 / ADR-0017·D4：post-load document.write（JS 路径）⇒ 具名错误。
        let mut page = BrowsingContext::new("https://a.example/").expect("new");
        page.load(&snapshot("https://a.example/", "<body></body>"))
            .expect("load");
        let message = page
            .realm_mut()
            .eval("globalThis.__e = null; try { document.write('x'); } catch (e) { globalThis.__e = e.message; } globalThis.__e")
            .expect("eval");
        let message = match message {
            HostValue::Str(text) => text,
            other => panic!("期望 Str，得到 {other:?}"),
        };
        assert!(
            message.contains("DocumentWriteAfterLoad"),
            "post-load write 应呈具名错误，实得 {message:?}"
        );
    }

    #[test]
    fn script_interruptible_by_deadline() {
        // §3.2 / §4：死循环脚本经 `eval_with_deadline` watchdog 截断 ⇒ `Terminated`（有界端到端证据）。
        let mut page = BrowsingContext::new("https://a.example/").expect("new");
        page.set_script_deadline_ms(50);
        let err = page
            .load(&snapshot(
                "https://a.example/spin",
                "<body><script>while (true) {}</script></body>",
            ))
            .expect_err("死循环脚本应被 watchdog 截断");
        assert_eq!(err.kind(), PageErrorKind::Terminated);
    }

    // ———————————————————————— §3.2 外链脚本 ————————————————————————

    #[test]
    fn external_script_blocks_until_loaded() {
        // §3.2：`<script src>` 经 OfflineBundle 同步解析；命中 ⇒ 执行；未命中 ⇒ 登记 blocker 并停泊。
        // 命中分支。
        let mut hit = BrowsingContext::new("https://a.example/").expect("new");
        hit.load(&snapshot_with(
            "https://a.example/hit",
            "<body><script src='https://a.example/lib.js'></script></body>",
            vec![OfflineResource {
                url: "https://a.example/lib.js".to_owned(),
                status: 200,
                headers: Vec::new(),
                body: b"globalThis.__ext = 7;".to_vec(),
            }],
        ))
        .expect("load hit");
        assert_eq!(
            hit.realm_mut().eval("globalThis.__ext").unwrap(),
            HostValue::Number(7.0),
            "外链脚本命中后应执行"
        );
        assert_eq!(hit.ready_state(), crate::controller::ReadyState::Complete);

        // 未命中分支：登记 load blocker、停在 blocker（readyState 非 Complete）、run_until ⇒ WaitingIo。
        let mut miss = BrowsingContext::new("https://a.example/").expect("new");
        miss.load(&snapshot(
            "https://a.example/miss",
            "<body><script src='https://a.example/missing.js'></script></body>",
        ))
        .expect("load miss");
        assert_ne!(miss.ready_state(), crate::controller::ReadyState::Complete);
        assert_eq!(
            miss.run_until(RunBudget::unlimited()).expect("run"),
            RunOutcome::WaitingIo,
            "外链未命中应停泊为 WaitingIo"
        );
    }

    // ———————————————————————— §3.3 document.write 注入序（DOM） ————————————————————————

    #[test]
    fn document_write_multiple_in_order_js_path() {
        // §3.3：JS 路径多条 write 注入序 == 调用序（**断言 DOM 实际顺序**，倒置必红）。
        let mut page = BrowsingContext::new("https://a.example/").expect("new");
        page.load(&snapshot(
            "https://a.example/w2",
            "<body><script>document.write('<i></i>');document.write('<b></b>');</script></body>",
        ))
        .expect("load");
        let tree = page.tree();
        let names = element_names_in_order(&tree.borrow(), tree.borrow().root());
        let i_index = names.iter().position(|name| name == "i").expect("应有 <i>");
        let b_index = names.iter().position(|name| name == "b").expect("应有 <b>");
        assert!(
            i_index < b_index,
            "注入序应为 i 先于 b（实得文档序 {names:?}）"
        );
    }

    // ———————————————————————— §3.5 导航作废旧响应 ————————————————————————

    #[test]
    fn fetch_aborted_on_navigation() {
        // §3.5：导航后旧代次请求作废，不再 resolve。
        let mut page = BrowsingContext::new("https://a.example/").expect("new");
        page.load(&snapshot(
            "https://a.example/one",
            "<body><script>fetch('https://a.example/slow');</script></body>",
        ))
        .expect("load1");
        page.run_until(RunBudget::unlimited()).expect("run");
        let id = page
            .pending_fetches()
            .first()
            .copied()
            .expect("应有一条 pending");
        // 导航离开。
        page.load(&snapshot("https://a.example/two", "<body></body>"))
            .expect("load2");
        let err = page
            .complete_fetch(id, Response::ok(200, Vec::new(), Vec::new()))
            .expect_err("旧代次请求应被作废");
        assert_eq!(err.kind(), PageErrorKind::NavigationAborted);
    }
}
