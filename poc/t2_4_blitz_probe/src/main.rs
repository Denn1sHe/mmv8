//! T2.4 探针：`blitz-dom 0.3.0-beta.2` 的**动态 DOM / 样式失效 / NodeId 映射 / Shadow DOM**
//! 能力实测，产出 T2.4 的**路线裁决**（走 Blitz，还是按技术方案 §8 切 Stylo + Taffy + Parley）。
//!
//! 施工图：`docs/design-remaining-tasks-2026-10-12.md` §2.3~§2.7。
//! 验收判据：`docs/remaining-scope-2026-10-12.md` §3 的 C1~C17。
//!
//! ## 本探针的形态由架构师决策 Q-3 决定
//!
//! `markup5ever 0.12`（html5ever 0.27 线）与 `markup5ever 0.39`（blitz 线）**源码级不兼容**
//! （`Token` 枚举变体、`TreeSink` 签名、`TokenizerResult` 位置都变了），所以
//! **T2.4 不与 html5ever 联调**：不引入 `html5ever`，初始 DOM 一律用
//! `DocumentMutator::create_element` / `append_children` 直接构造，
//! `DocumentConfig::html_parser_provider` 保持 `None`（= `DummyHtmlParserProvider`，no-op）。
//! 目的是把「blitz-dom 能不能用」与「html5ever 0.39 好不好用」两个问题**解耦**。
//!
//! ## 依赖纪律（技术方案 §3「不整包依赖 GUI」/ 判据 C2）
//!
//! - 只依赖 `blitz-dom`（`default-features = false`）+ `blitz-traits` + `markup5ever`。
//! - **不引入** `blitz` 顶层 crate、`blitz-shell`、`blitz-paint`、`blitz-html`、`anyrender`。
//! - 额外引入 `stylo` / `stylo_traits` / `selectors` **仅用于只读地读计算样式、
//!   以及直接调用 `TShadowRoot` / `selectors::Element` 的 shadow 相关方法**。
//!   这三个 crate 已经在 `blitz-dom` 的依赖树里（版本逐一对齐），**不新增任何传递包**。
//! - EXP0 会**读 `Cargo.lock` 逐条核对**上述 GUI 包确实不在树里 ⇒ `NO_GUI_DEP`。
//!
//! ## unsafe 纪律（AGENTS.md 铁律 4/ ADR-0008 决策 4）
//!
//! **本文件零 `unsafe`**。所有能力都用 `blitz-dom` 的安全 API 达成；
//! EXP4 的 Shadow DOM 探测用 `std::panic::catch_unwind` 捕获上游 `todo!()`，
//! 不需要自己写 `unsafe`，也不需要 crate 级 / mod 级 `allow(unsafe_code)` 开口。

use std::collections::BTreeMap;
use std::panic::{catch_unwind, AssertUnwindSafe};

use blitz_dom::{
    ns, Attribute, BaseDocument, DocumentConfig, LocalName, Node, NodeId, QualName, StyleThreading,
};
use blitz_traits::shell::{ColorScheme, Viewport};
use style::dom::{TNode, TShadowRoot};
use style_traits::ToCss;

/// 探针用的固定动画时刻。取 0 表示「无动画进行中」，让读数完全确定
/// （若用「现在」，不同机器上 transition/animation 可能让读数漂移）。
const NOW: f64 = 0.0;

/// 视口尺寸。必须非零，否则 taffy 的 root available space 为 0，
/// 子元素 `width:100px` 之类的读数会被压成 0，令断言变成假失败。
const VIEWPORT_W: u32 = 800;
const VIEWPORT_H: u32 = 600;

/// EXP1 里被观测的三个宽度（px）。
const W_INLINE_BASE: f32 = 100.0;
const W_INLINE_MUTATED: f32 = 200.0;
const W_CLASS_RULE: f32 = 300.0;

// ===========================================================================
// 断言骨架（照抄 `poc/t2_3_html5ever_pause/src/main.rs` 的形态，见设计文档 §4.1）
// ===========================================================================

/// 断言的层次。
#[derive(Clone, Copy, PartialEq, Eq)]
enum Level {
    /// 方法学断言：失败 ⇒ 整份结论作废（对齐 ADR-0007 的 `BASELINE_VALID` 范式）。
    Method,
    /// 能力断言：失败只是**结论**，不是探针坏了。
    Capability,
}

/// 探针自身的有效性（**不是**被测能力的有效性）。
///
/// 关键语义（设计文档 C17）：`T2_4_PROBE_OK` 指「探针全部断言跑完且判定方法有效」，
/// 与 `T2_4_VERDICT` 是两件事。`PROBE_OK` + `VERDICT=SWITCH_TO_STYLO` 是**合法且预期**的组合。
#[derive(Default)]
struct Probe {
    method_ok: bool,
    checks: usize,
}

impl Probe {
    fn new() -> Self {
        Probe {
            method_ok: true,
            checks: 0,
        }
    }

    fn record(&mut self, level: Level, cond: bool, tag: &str, pass: &str, fail: &str) {
        self.checks += 1;
        if cond {
            println!("ASSERT_PASS({tag}): {pass}");
        } else {
            println!("ASSERT_FAIL({tag}): {fail}");
        }
        if level == Level::Method && !cond {
            self.method_ok = false;
        }
    }
}

/// 能力结论的**单一真相源**。
///
/// 每个字段由对应的 EXP 实测填充；裁决函数只读这里，
/// 保证「断言输出的结论」与「裁决用的结论」不会不一致
/// （避免出现「打印 false 但裁决按 true 算」这类自相矛盾）。
#[derive(Default)]
struct Outcomes {
    /// C1：Linux 可用性。**由平台判定**（`OS == "linux" && ARCH == "x86_64"`），
    /// 不是手写常量；非 x86_64 Linux 上为 `false` 并标记 `PENDING_REMOTE`
    /// （照实，不是「实测为否」）。
    target_linux_ok: bool,
    /// C6：动态 DOM。
    dynamic_dom_works: bool,
    /// C7 的一半：style 级失效（class 变更 → 重算样式）。
    style_invalidation_works: bool,
    /// C7 的另一半：layout 级失效（width 变更 → 重算布局）。
    layout_invalidation_works: bool,
    /// C8：NodeId 映射一致。
    node_id_map_consistent: bool,
    /// C9：Shadow DOM。
    shadow_dom_works: bool,
    /// C10：后端树只是缓存。
    backend_is_cache: bool,
}

impl Outcomes {
    /// C11：不通过判据。任一为 false ⇒ `SWITCH_TO_STYLO`。
    fn keep_blitz(&self) -> bool {
        self.target_linux_ok
            && self.dynamic_dom_works
            && self.style_invalidation_works
            && self.layout_invalidation_works
            && self.node_id_map_consistent
            && self.shadow_dom_works
    }
}

// ===========================================================================
// 小工具
// ===========================================================================

/// 构造一个 HTML 命名空间下的 `QualName`。
fn html_name(local: &str) -> QualName {
    QualName::new(None, ns!(html), LocalName::from(local))
}

/// 构造无命名空间的 `QualName`（HTML 里 `class` / `id` / `style` 这类属性就是无命名空间）。
fn attr_name(local: &str) -> QualName {
    QualName::new(None, ns!(), LocalName::from(local))
}

fn attr(name: &str, value: &str) -> Attribute {
    Attribute {
        name: attr_name(name),
        value: value.to_string(),
    }
}

/// 读一个节点的**布局宽度**（CSS px）。
///
/// 走 `Node::final_layout()` 而不是 `get_client_bounding_rect`：后者先试
/// `inline_fragment_rects`，对 inline 元素语义不同；我们要的是盒模型宽度的稳定读数。
fn layout_width(doc: &BaseDocument, id: NodeId) -> Option<f32> {
    doc.get_node(id).map(|n| n.final_layout().size.width)
}

/// 读一个节点的**计算样式**中 `width` 的 CSS 文本形式（如 `"100px"` / `"300px"` / `"auto"`）。
///
/// 这是「样式层」的读数，与布局层的 [`layout_width`] 相互独立：
/// 两者都随变更而变 ⇒ 不只是重排，而是**样式重算 + 布局重排**都成立；
/// 只有布局变、计算样式不变 ⇒ 只证明了 layout 失效。
fn computed_width_css(doc: &BaseDocument, id: NodeId) -> Option<String> {
    let node = doc.get_node(id)?;
    let styles = node.primary_styles()?;
    Some(styles.clone_width().to_css_string())
}

/// 浮点读数的相等判定（布局引擎有小数误差，不用 `==`）。
fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 0.5
}

/// 从 `catch_unwind` 的 `Err(payload)` 里取出 panic 消息文本。
fn panic_message(payload: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&'static str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "<非字符串 panic payload>".to_string()
    }
}

// ===========================================================================
// EXP0 —— 依赖形态
// ===========================================================================

/// GUI 栈黑名单（技术方案 §3 + 设计文档 C2 的完整清单）。
///
/// `blitz` 是顶层 crate 的**精确**包名；`blitz-dom` / `blitz-traits` 是**允许**的，
/// 所以这里必须用精确匹配而不是前缀匹配，否则会把 `blitz-dom` 误判成违规。
const FORBIDDEN_PACKAGES: &[&str] = &[
    "blitz",
    "blitz-html",
    "blitz-shell",
    "blitz-paint",
    "blitz-net",
    "anyrender",
    "anyrender_vello",
    "winit",
    "accesskit",
    "usvg",
    "wuff",
    "tracing",
];

/// 需要在 `Cargo.lock` 里报告版本的包。
const REPORTED_PACKAGES: &[&str] = &[
    "blitz-dom",
    "blitz-traits",
    "stylo",
    "stylo_dom",
    "stylo_traits",
    "stylo_taffy",
    "taffy",
    "parley",
    "markup5ever",
];

/// 从 `Cargo.lock` 里抽出 `包名 -> 版本`。
///
/// `Cargo.lock` 是 TOML，但探针**不引入** `toml` 依赖（不值得为读一个锁文件多拉一个 crate）。
/// 锁文件的 `[[package]]` 区块是 cargo 自己生成的、高度规整的固定形状，行扫描足够；
/// 解析失败会被方法学断言 `m1` 抓住，不会静默给出假绿。
fn parse_lock_versions(lock: &str) -> BTreeMap<String, String> {
    fn unquote(s: &str) -> String {
        s.trim().trim_matches('"').to_string()
    }

    let mut out: BTreeMap<String, String> = BTreeMap::new();
    let mut name: Option<String> = None;
    let mut version: Option<String> = None;
    let mut in_package = false;

    for raw in lock.lines() {
        let line = raw.trim();

        if line == "[[package]]" {
            // 收尾上一个 package
            if let (Some(n), Some(v)) = (name.take(), version.take()) {
                out.insert(n, v);
            }
            name = None;
            version = None;
            in_package = true;
            continue;
        }
        // 任何其它区块标题都结束当前 package
        if line.starts_with('[') {
            if let (Some(n), Some(v)) = (name.take(), version.take()) {
                out.insert(n, v);
            }
            name = None;
            version = None;
            in_package = false;
            continue;
        }
        if !in_package {
            continue;
        }
        if let Some(rest) = line.strip_prefix("name = ") {
            name = Some(unquote(rest));
        } else if let Some(rest) = line.strip_prefix("version = ") {
            version = Some(unquote(rest));
        }
    }
    if let (Some(n), Some(v)) = (name, version) {
        out.insert(n, v);
    }
    out
}

fn exp0(probe: &mut Probe, out: &mut Outcomes) {
    println!("=== EXP0 依赖形态 ===");

    // `CARGO_MANIFEST_DIR` 由 cargo 在编译期注入，指向探针自己的目录
    // （独立 workspace ⇒ 锁文件就在探针目录下）。
    let lock_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.lock");
    let lock = std::fs::read_to_string(&lock_path).unwrap_or_else(|e| {
        println!("LOCK_READ_ERROR={e}");
        String::new()
    });

    probe.record(
        Level::Method,
        !lock.is_empty(),
        "m0",
        "Cargo.lock 可读 —— 依赖形态断言的前提成立",
        "读不到 Cargo.lock，EXP0 的结论不可信",
    );
    if lock.is_empty() {
        println!("NO_GUI_DEP=false");
        return;
    }

    let versions = parse_lock_versions(&lock);
    probe.record(
        Level::Method,
        !versions.is_empty(),
        "m1",
        "Cargo.lock 解析出包清单 —— 锁文件形状符合预期",
        "Cargo.lock 解析结果为空，EXP0 的结论不可信",
    );

    // ---- 报告锁定版本 ----
    let mut parts = Vec::new();
    let mut all_present = true;
    for pkg in REPORTED_PACKAGES {
        match versions.get(*pkg) {
            Some(v) => parts.push(format!("{pkg}={v}")),
            None => {
                all_present = false;
                parts.push(format!("{pkg}=<absent>"));
            }
        }
    }
    println!("LOCKED {}", parts.join(" "));
    probe.record(
        Level::Method,
        all_present,
        "m2",
        "全部被报告的依赖都在锁文件里 —— 版本基线可核对",
        "有被报告的依赖不在锁文件里（见 LOCKED 行的 <absent>）",
    );

    // ---- C2：GUI 栈黑名单 ----
    let found: Vec<&str> = FORBIDDEN_PACKAGES
        .iter()
        .copied()
        .filter(|p| versions.contains_key(*p))
        .collect();
    let no_gui = found.is_empty();
    println!("NO_GUI_DEP={no_gui}");
    if !no_gui {
        println!("FORBIDDEN_FOUND={found:?}");
    }
    probe.record(
        Level::Capability,
        no_gui,
        "c2",
        &format!(
            "依赖树不含 {} —— 满足技术方案 §3「不整包依赖 GUI」",
            FORBIDDEN_PACKAGES.join("/")
        ),
        &format!("依赖树含 GUI 栈包：{found:?} —— 违反技术方案 §3"),
    );

    // ---- 决策 Q-3 的机械验证：html5ever 不在树里 ----
    let has_html5ever = versions.contains_key("html5ever");
    println!("HTML5EVER_IN_TREE={has_html5ever}");
    probe.record(
        Level::Method,
        !has_html5ever,
        "m3",
        "探针未引入 html5ever —— 符合决策 Q-3「T2.4 不与 html5ever 联调」",
        "探针引入了 html5ever，实验形态被污染",
    );

    // ---- 决策 Q-3 的机械验证：markup5ever 落在 0.39 线 ----
    // `HTML5EVER_VERSION_FORK_IMPACT=BREAKING` 由架构师源码级定案（三处证据，见设计文档 §0 Q-3）。
    // 探针**不重跑**该结论（重跑需在 0.39 上复跑 T2.3 的 a~e，属独立任务 T2.3-bis）。
    // 这里只做一件机械可查的事：确认本探针锁定的 markup5ever 与 architect 的核实同线。
    let m5v = versions.get("markup5ever").cloned().unwrap_or_default();
    let m5_on_039 = m5v.starts_with("0.39.");
    println!("MARKUP5EVER_LINE={m5v}");
    probe.record(
        Level::Method,
        m5_on_039,
        "m4",
        "markup5ever 锁定在 0.39 线 —— 与决策 Q-3 的源码级核实一致",
        "markup5ever 不在 0.39 线，Q-3 的版本前提需重新核实",
    );

    // ---- C1：平台可用性 ----
    // 本地只跑 macOS；Linux 侧由主理人在远程 `~/mmv8` 复跑（AGENTS.md:72 跨平台纪律第 4 条）。
    // 这里**照实**输出，绝不用「本机跑通了」暗示 Linux 也行。
    let target_os = std::env::consts::OS;
    let target_arch = std::env::consts::ARCH;
    println!("TARGET_HOST_OS={target_os}");
    println!("TARGET_HOST_ARCH={target_arch}");
    println!("TARGET_MAC_OK={}", target_os == "macos");
    // C1 由**平台判定**得出，不是手写常量：只有真正跑在 x86_64 Linux 上才为 true。
    // 本地 macOS 上它为 false 是**照实**，不是「实测为否」；状态串同步反映这一点。
    out.target_linux_ok = target_os == "linux" && target_arch == "x86_64";
    println!("TARGET_LINUX_OK={}", out.target_linux_ok);
    if out.target_linux_ok {
        println!(
            "TARGET_LINUX_STATUS=VERIFIED_ON_LINUX (本探针正运行于 x86_64-unknown-linux-gnu，C1 就地验证)"
        );
    } else {
        println!("TARGET_LINUX_STATUS=PENDING_REMOTE (需在 x86_64-unknown-linux-gnu 上复跑)");
    }

    // 平台相关依赖面：blitz-dom 唯一的平台相关依赖是 objc2，target 为 apple
    // （`cfg(any(target_vendor = "apple"))`）。它出现只说明当前是 apple 平台，不是违规。
    println!("APPLE_ONLY_DEP_PRESENT={}", versions.contains_key("objc2"));

    // 字体栈：本探针只到「布局数值可读」，不涉及字形 shaping。
    // 但 C1 的 Linux 复跑必须记录字体依赖情况，故在此报告字体相关包是否在树里。
    println!("FONT_STACK_IN_TREE={}", versions.contains_key("parley"));
}

// ===========================================================================
// EXP1 / EXP2 / EXP3 —— 动态 DOM / NodeId 稳定性 / 后端树只是缓存
// ===========================================================================

/// EXP1~EXP3 共用的实验装置。
struct Fixture {
    doc: BaseDocument,
    root: NodeId,
    html: NodeId,
    body: NodeId,
    /// 带 inline `width` 的被观测节点（测 layout 级失效）。
    inline_box: NodeId,
    /// 不带 inline `width` 的被观测节点（测 style 级失效：class 规则）。
    class_box: NodeId,
}

/// 供全部实验复用的 UA 样式表。
///
/// `.big { width: 300px }` 挂在**没有 inline width** 的 `#clsbox` 上：
/// 若挂在带 inline `width` 的节点上，inline 声明的优先级高于类选择器，
/// 类规则永远不生效 ⇒ style 失效的断言会变成恒假门禁。
const PROBE_UA_CSS: &str = ".big { width: 300px; }";

/// 建一个最小文档：
/// `<html><body><div id=box style="width:100px;height:50px"><div id=clsbox style="height:50px">`
fn build_fixture() -> Fixture {
    let config = DocumentConfig {
        viewport: Some(Viewport::new(
            VIEWPORT_W,
            VIEWPORT_H,
            1.0,
            ColorScheme::Light,
        )),
        // 串行样式遍历：绕过 Stylo 的全局 rayon 线程池。
        // 这条同时回答许清楚 §3 待确认 5（rayon 与单一页面线程是否冲突）：
        // 用 `Sequential` 即不碰全局线程池；`Parallel` 的文档明说
        // 「panics if another Parallel resolve is in flight on a different thread」。
        style_threading: StyleThreading::Sequential,
        // html_parser_provider 保持 None —— 决策 Q-3：不走 HTML 解析路径。
        ..Default::default()
    };
    let mut doc = BaseDocument::new(config);
    doc.add_user_agent_stylesheet(PROBE_UA_CSS);

    let root = doc.root_node().id;
    let (html, body, inline_box, class_box) = {
        let mut m = doc.mutate();
        let html = m.create_element(html_name("html"), vec![]);
        let body = m.create_element(html_name("body"), vec![]);
        let inline_box = m.create_element(
            html_name("div"),
            vec![attr("id", "box"), attr("style", "width:100px;height:50px")],
        );
        let class_box = m.create_element(
            html_name("div"),
            vec![attr("id", "clsbox"), attr("style", "height:50px")],
        );
        m.append_children(root, &[html]);
        m.append_children(html, &[body]);
        m.append_children(body, &[inline_box, class_box]);
        (html, body, inline_box, class_box)
    };

    // 关键：`resolve()` 在 root 的 first_element_child 为空时直接 return，
    // 所以必须先把 `<html>` 挂上，否则后续所有读数都是「没布局」的 0。
    doc.resolve(NOW);

    Fixture {
        doc,
        root,
        html,
        body,
        inline_box,
        class_box,
    }
}

fn exp1_exp2_exp3(probe: &mut Probe, out: &mut Outcomes) {
    println!("=== EXP1 动态 DOM ===");
    let mut fx = build_fixture();

    // ---------- 基线读数 ----------
    let w0 = layout_width(&fx.doc, fx.inline_box);
    let w0_css = computed_width_css(&fx.doc, fx.inline_box);
    let cls_w_before = layout_width(&fx.doc, fx.class_box);
    let cls_css_before = computed_width_css(&fx.doc, fx.class_box);
    println!("LAYOUT_W0={w0:?}");
    println!(
        "COMPUTED_W0={}",
        w0_css.clone().unwrap_or_else(|| "<none>".into())
    );
    println!(
        "STYLE_CLASS_BEFORE={}",
        cls_css_before.as_deref() == Some("300px")
    );
    println!("CLASS_BOX_W0={cls_w_before:?}");
    println!(
        "CLASS_BOX_COMPUTED_W0={}",
        cls_css_before.clone().unwrap_or_else(|| "<none>".into())
    );

    probe.record(
        Level::Method,
        w0.is_some() && cls_w_before.is_some(),
        "m5",
        "初始布局宽度可读 —— 后续读数变化可归因于变更而非读不出数",
        "初始布局宽度读不出（get_node / final_layout 不可用），EXP1 结论不可信",
    );
    probe.record(
        Level::Method,
        matches!(w0, Some(x) if close(x, W_INLINE_BASE)),
        "m6",
        "inline width:100px 的初始布局读数为 100 —— 读数方法已被一个已知值自证",
        "inline width:100px 的布局读数不是 100，测量方法本身可疑，后续「读数变了」无法归因",
    );

    // ---------- 步骤 3~5：改 inline style 的 width ⇒ layout 级失效 ----------
    {
        let mut m = fx.doc.mutate();
        m.set_style_property(fx.inline_box, "width", "200px");
    } // mutator 在 drop 时 flush
    fx.doc.resolve(NOW);

    let w1 = layout_width(&fx.doc, fx.inline_box);
    let w1_css = computed_width_css(&fx.doc, fx.inline_box);
    println!("LAYOUT_W1={w1:?}");
    println!(
        "COMPUTED_W1={}",
        w1_css.clone().unwrap_or_else(|| "<none>".into())
    );

    out.layout_invalidation_works = matches!(w0, Some(a) if close(a, W_INLINE_BASE))
        && matches!(w1, Some(b) if close(b, W_INLINE_MUTATED));
    out.dynamic_dom_works = out.layout_invalidation_works && w1 != w0;
    println!(
        "LAYOUT_INVALIDATION_WORKS={}",
        out.layout_invalidation_works
    );
    println!("DYNAMIC_DOM_WORKS={}", out.dynamic_dom_works);

    probe.record(
        Level::Capability,
        out.layout_invalidation_works,
        "c7-layout",
        "set_style_property(\"width\",\"200px\") 后布局读数变为 200 —— layout 级失效成立",
        "改 inline style 后布局读数未变为 200px",
    );
    probe.record(
        Level::Capability,
        out.dynamic_dom_works,
        "c6",
        "同一文档上「插入 → 改 style → 再改 style → 读数」连续成立 —— 动态 DOM 成立",
        "连续变更后读数未按序变化，动态 DOM 不成立",
    );

    // ---------- 步骤 6~8：改 class ⇒ style 级失效 ----------
    // 「正负成对」设计：先确认 `.big` 规则**在 class 不存在时不生效**
    //（否则「改了 class 宽度就变」可能与本规则无关），再改 class 看它生效。
    probe.record(
        Level::Method,
        cls_css_before.as_deref() != Some("300px"),
        "m7",
        "class 不存在时 `.big` 规则不生效（负对照）—— 后续宽度变化只能归因于 class 变更",
        "class 不存在时宽度已是 300px，style 失效的断言无法归因（缺少负对照）",
    );

    {
        let mut m = fx.doc.mutate();
        m.set_attribute(fx.class_box, attr_name("class"), "big");
    }
    fx.doc.resolve(NOW);

    let w2 = layout_width(&fx.doc, fx.class_box);
    let w2_css = computed_width_css(&fx.doc, fx.class_box);
    println!("LAYOUT_W2={w2:?}");
    println!("STYLE_CLASS_AFTER={}", w2_css.as_deref() == Some("300px"));
    println!(
        "CLASS_BOX_COMPUTED_W2={}",
        w2_css.clone().unwrap_or_else(|| "<none>".into())
    );

    // style 级失效的判据：计算样式层变了 **且** 布局层跟着变。
    // 只看布局层会与 layout 失效混淆；只看计算样式层则无法证明「投影到布局」也成立。
    let style_layer_changed =
        w2_css.as_deref() == Some("300px") && cls_css_before.as_deref() != Some("300px");
    let layout_layer_followed = matches!(w2, Some(x) if close(x, W_CLASS_RULE));
    out.style_invalidation_works = style_layer_changed && layout_layer_followed;
    println!("STYLE_INVALIDATION_WORKS={}", out.style_invalidation_works);

    probe.record(
        Level::Capability,
        out.style_invalidation_works,
        "c7-style",
        "class 变更后计算样式的 width 变为 300px 且布局层同步跟随 —— style 级失效成立",
        "class 变更后计算样式/布局未变为 300px，restyle 可能未生效",
    );

    // ---------- EXP2：NodeId 稳定性 ----------
    println!("=== EXP2 NodeId 稳定性 ===");
    let tracked: Vec<NodeId> = vec![fx.root, fx.html, fx.body, fx.inline_box, fx.class_box];
    let snapshot: Vec<u64> = tracked.iter().map(|id| id.as_u64()).collect();

    let all_live = tracked.iter().all(|id| fx.doc.get_node(*id).is_some());
    probe.record(
        Level::Method,
        all_live,
        "m8",
        "变更序列结束后全部被跟踪的 NodeId 仍解析到活节点",
        "有 NodeId 在变更序列中失效 —— NodeId 断言不可信",
    );

    // blitz 的 NodeId 是 slot+version 打包（blitz-traits/src/node_id.rs:5-8）。
    // 真正的「漂移」检测是：**回收一个节点后再建一个节点，新节点不能占用旧 id**
    // ——否则宿主侧的 `NodeId -> 节点` 映射表会把新节点误认成旧节点，
    // 这正是技术方案 §8「保持 NodeId 对应表」要防的事故。
    let recycle = (|| -> (NodeId, NodeId, bool) {
        let old_id = {
            let mut m = fx.doc.mutate();
            let tmp = m.create_element(html_name("span"), vec![attr("id", "recycled")]);
            m.append_children(fx.body, &[tmp]);
            tmp
        };
        fx.doc.resolve(NOW);
        {
            let mut m = fx.doc.mutate();
            m.remove_and_drop_node(old_id);
        }
        fx.doc.resolve(NOW);
        let still_resolves = fx.doc.get_node(old_id).is_some();
        let new_id = {
            let mut m = fx.doc.mutate();
            let fresh = m.create_element(html_name("span"), vec![attr("id", "recycled")]);
            m.append_children(fx.body, &[fresh]);
            fresh
        };
        fx.doc.resolve(NOW);
        (old_id, new_id, still_resolves)
    })();

    let (old_id, new_id, old_still_resolves) = recycle;
    println!("RECYCLED_OLD={}", old_id.as_u64());
    println!("RECYCLED_NEW={}", new_id.as_u64());
    println!("RECYCLED_OLD_STILL_RESOLVES={old_still_resolves}");

    let no_alias = !old_still_resolves && old_id != new_id && fx.doc.get_node(old_id).is_none();
    // 受跟踪节点在整轮变更（含一次回收+重建）后 id 逐字不变、仍可解析。
    let ids_stable = tracked
        .iter()
        .zip(snapshot.iter())
        .all(|(id, raw)| id.as_u64() == *raw && fx.doc.get_node(*id).is_some());

    out.node_id_map_consistent = no_alias && ids_stable;
    println!("NODE_ID_STABLE={ids_stable}");
    println!("NODE_ID_MAP_CONSISTENT={}", out.node_id_map_consistent);

    probe.record(
        Level::Capability,
        no_alias,
        "c8-noalias",
        "回收节点的 id 不再解析、且新建节点未复用该 id —— NodeId 不漂移（slot+version 生效）",
        "回收后新节点复用了旧 NodeId（或旧 id 仍解析）—— NodeId 映射表不可信",
    );
    probe.record(
        Level::Capability,
        ids_stable,
        "c8",
        "受跟踪节点在整轮变更序列（含回收+重建）中 NodeId 逐字不变且仍可解析",
        "受跟踪节点的 NodeId 发生了漂移",
    );

    // ---------- EXP3：后端树只是缓存 ----------
    println!("=== EXP3 后端树只是缓存 ===");
    // 技术方案 §8 原话：「后端树只作为缓存」。可验证形式：删掉后端节点后，
    // 能从规范 DOM 重建，且布局读数一致。
    //
    // 本探针没有「规范 DOM / 后端树」两个独立存储（blitz-dom 只有一棵 NodeTree），
    // 所以用可观察的等价形式：把 root 的**全部**子节点删掉（连`<html>` 一起，否则
    // 重建后会出现两个 `<html>`，root_element() 取到旧的那个，读数必然对不上），
    // 再用同样的 DOM 描述重新 create_element + append_children，然后对比布局读数。
    let before_w = layout_width(&fx.doc, fx.inline_box);
    let before_hits = fx
        .doc
        .query_selector_all("#box")
        .map(|v| v.len())
        .unwrap_or(0);
    let before_w_class = layout_width(&fx.doc, fx.class_box);
    println!("BEFORE_WIPE_W={before_w:?}");
    println!("BEFORE_WIPE_CLASS_W={before_w_class:?}");
    println!("BEFORE_WIPE_HITS={before_hits}");

    {
        let mut m = fx.doc.mutate();
        m.remove_and_drop_all_children(fx.root);
    }
    fx.doc.resolve(NOW);

    let wiped_hits = fx
        .doc
        .query_selector_all("#box")
        .map(|v| v.len())
        .unwrap_or(0);
    let gone = fx.doc.get_node(fx.inline_box).is_none();
    println!("AFTER_WIPE_HITS={wiped_hits}");
    println!("OLD_NODE_STILL_PRESENT={}", !gone);
    probe.record(
        Level::Method,
        wiped_hits == 0 && gone,
        "m9",
        "「删掉后端树」这一步真的删掉了（重建对比的前置条件成立）",
        "删除后端节点后查询仍命中或旧节点仍在，EXP3 的重建对比不成立",
    );

    // 从规范 DOM 描述重建（inline width 与 class 都复原）
    let (r_html, r_body, r_inline, r_class) = {
        let mut m = fx.doc.mutate();
        let html = m.create_element(html_name("html"), vec![]);
        let body = m.create_element(html_name("body"), vec![]);
        let ib = m.create_element(
            html_name("div"),
            vec![attr("id", "box"), attr("style", "width:200px;height:50px")],
        );
        let cb = m.create_element(
            html_name("div"),
            vec![
                attr("id", "clsbox"),
                attr("class", "big"),
                attr("style", "height:50px"),
            ],
        );
        m.append_children(fx.root, &[html]);
        m.append_children(html, &[body]);
        m.append_children(body, &[ib, cb]);
        (html, body, ib, cb)
    };
    fx.doc.resolve(NOW);

    let after_w = layout_width(&fx.doc, r_inline);
    let after_w_class = layout_width(&fx.doc, r_class);
    let after_hits = fx
        .doc
        .query_selector_all("#box")
        .map(|v| v.len())
        .unwrap_or(0);
    let after_class_hits = fx
        .doc
        .query_selector_all("#clsbox")
        .map(|v| v.len())
        .unwrap_or(0);
    println!("AFTER_REBUILD_W={after_w:?}");
    println!("AFTER_REBUILD_CLASS_W={after_w_class:?}");
    println!("AFTER_REBUILD_HITS={after_hits}");

    out.backend_is_cache = after_hits == 1
        && after_class_hits == 1
        && matches!((before_w, after_w), (Some(a), Some(b)) if close(a, b))
        && matches!((before_w_class, after_w_class), (Some(a), Some(b)) if close(a, b));
    println!("BACKEND_IS_CACHE={}", out.backend_is_cache);
    probe.record(
        Level::Capability,
        out.backend_is_cache,
        "c10",
        "删掉后端节点后可从规范 DOM 重建，两个被观测节点的布局读数均与删除前一致 —— 后端树确实只是缓存",
        "重建后布局读数与删除前不一致（或节点未重建成功）",
    );

    // EXP4 需要一棵活的树来挂 shadow host，用重建后的这棵。
    fx.html = r_html;
    fx.body = r_body;
    fx.inline_box = r_inline;
    fx.class_box = r_class;

    // ===============================================================
    // EXP4 —— Shadow DOM
    // ===============================================================
    println!();
    println!("=== EXP4 Shadow DOM ===");
    exp4_shadow(probe, out, &mut fx);
}

/// EXP4：Shadow DOM。
///
/// **必须真去尝试**（设计文档 §2.4 关键设计点 1：若因为「源码写了 `todo!()`」就直接写
/// `SHADOW_DOM_WORKS=false`，这条断言就是恒假门禁，无法区分「确实不支持」与「没测出来」）。
///
/// 三条真实探测路径，全部走 blitz-dom 暴露的安全 API：
/// 1. `TNode::as_shadow_root()` —— 问节点「你是不是一个 shadow root」。
/// 2. `TShadowRoot::host()` —— 真去调那个 `todo!()`，用 `catch_unwind` 捕获 panic 消息。
/// 3. 选择器作用域隔离 —— 读 `selectors::Element` 的
///    `parent_node_is_shadow_root()` / `containing_shadow_host()`，并做 light DOM 选择器查询。
fn exp4_shadow(probe: &mut Probe, out: &mut Outcomes, fx: &mut Fixture) {
    // 造一个「宿主」元素
    let host = {
        let mut m = fx.doc.mutate();
        let h = m.create_element(html_name("div"), vec![attr("id", "shadow-host")]);
        m.append_children(fx.body, &[h]);
        h
    };
    fx.doc.resolve(NOW);

    // ---- 路径 1：as_shadow_root() ----
    // `TNode` / `TShadowRoot` / `selectors::Element` 都实现给 `&Node`
    // （blitz-dom 里 `type BlitzNode<'a> = &'a Node`），所以方法调用要传 `&&Node`。
    let as_shadow_is_some = {
        let node: &Node = fx.doc.get_node(host).expect("host 节点应存在");
        TNode::as_shadow_root(&node).is_some()
    };
    println!("AS_SHADOW_ROOT_IS_SOME={as_shadow_is_some}");

    // ---- 路径 2：真去调 TShadowRoot::host()，捕获上游 todo!() 的 panic 消息 ----
    //
    // `impl<'a> TShadowRoot for BlitzNode<'a>` 且 `type BlitzNode<'a> = &'a Node`
    // （blitz-dom src/stylo.rs:191/223），所以 `&Node` **本身就是** `TShadowRoot` 的实现类型，
    // 可以直接调 `host()` —— 这会命中 stylo.rs:231 的
    // `todo!("Shadow roots not implemented")`。
    //
    // 用 `AssertUnwindSafe`：`&Node` 内部含裸树指针（`tree: *mut NodeTree`），
    // 不满足 `RefUnwindSafe`。这里是安全的：本探针**只读**节点、不修改任何状态，
    // 且 `todo!()` 在函数体第一行就展开，尚未读任何字段，panic 不跨调用破坏不变量。
    let shadow_attach = catch_unwind(AssertUnwindSafe(|| {
        let node: &Node = fx.doc.get_node(host).expect("host 节点应存在");
        let _ = TShadowRoot::host(&node);
    }));
    let attach_panicked = shadow_attach.is_err();
    let shadow_panic_msg = match shadow_attach {
        Ok(_) => "<no panic: host() 正常返回>".to_string(),
        Err(payload) => panic_message(&payload),
    };
    println!("SHADOW_ATTACH_API_CALLED=true");
    println!("SHADOW_ATTACH_PANICKED={attach_panicked}");
    println!("SHADOW_ATTACH_PANIC={shadow_panic_msg:?}");

    // ---- 路径 3：选择器作用域隔离 ----
    // 技术方案 §7 要求 shadow 树中的节点不出现在 light DOM 的选择器结果里。
    // blitz-dom 的 `selectors::Element` 实现把
    // `parent_node_is_shadow_root()` 硬编码为 `false`、`containing_shadow_host()`
    // 硬编码为 `None`（src/stylo.rs:337/341）—— 桩实现，作用域隔离不可能成立。
    let shadow_child = {
        let mut m = fx.doc.mutate();
        let c = m.create_element(html_name("span"), vec![attr("id", "shadow-probe")]);
        m.append_children(host, &[c]);
        c
    };
    fx.doc.resolve(NOW);

    let visible_from_light_dom = fx
        .doc
        .query_selector_all("#shadow-probe")
        .map(|v| v.len())
        .unwrap_or(0);
    let visible_from_host = fx
        .doc
        .query_selector_all_in(host, "#shadow-probe")
        .map(|v| v.len())
        .unwrap_or(0);
    println!("LIGHT_DOM_VISIBLE={visible_from_light_dom}");
    println!("HOST_SCOPE_VISIBLE={visible_from_host}");

    let parent_is_shadow_root = {
        let node: &Node = fx.doc.get_node(shadow_child).expect("影子节点应存在");
        selectors::Element::parent_node_is_shadow_root(&node)
    };
    let containing_host_is_some = {
        let node: &Node = fx.doc.get_node(shadow_child).expect("影子节点应存在");
        selectors::Element::containing_shadow_host(&node).is_some()
    };
    println!("PARENT_NODE_IS_SHADOW_ROOT={parent_is_shadow_root}");
    println!("CONTAINING_SHADOW_HOST_IS_SOME={containing_host_is_some}");

    // ---- slot 分发 ----
    // C9 的第二条判据：slot 分发生效（assigned nodes 可查）。
    // blitz-dom 的 `NodeData` 只有 Document / Element / AnonymousBlock / Text / Comment
    // 五个变体，**没有 ShadowRoot**；也没有 `assigned_nodes` 一类 API，
    // 所以「造 slot 并查 assigned nodes」在 API 层面就不可构造。
    // 记录 `slot` 元素被当作普通元素处理这一可观察事实。
    let slot_treated_as_plain_element = {
        let slot_id = {
            let mut m = fx.doc.mutate();
            let s = m.create_element(html_name("slot"), vec![attr("name", "probe-slot")]);
            m.append_children(host, &[s]);
            s
        };
        fx.doc.resolve(NOW);
        let node: &Node = fx.doc.get_node(slot_id).expect("slot 节点应存在");
        let is_plain = node.is_element() && !selectors::Element::parent_node_is_shadow_root(&node);
        // slot 的 light DOM 子节点数（真shadow DOM 下这里应是 0，assigned nodes 在影子树里）
        let assigned_via_light_children = node.children.len();
        println!("SLOT_LIGHT_CHILD_COUNT={assigned_via_light_children}");
        is_plain
    };
    println!("SLOT_ASSIGNED_NODES_API_PRESENT=false");
    println!("SLOT_TREATED_AS_PLAIN_ELEMENT={slot_treated_as_plain_element}");

    // ---- 汇总裁决 ----
    //
    // 「可工作」必须**同时**满足：能拿到 shadow root + 作用域隔离成立 + 父节点被识别为 shadow root。
    // 关键：这里不是把 false 写死，而是由上面**实测到的**读数算出来的。
    out.shadow_dom_works =
        as_shadow_is_some && visible_from_light_dom == 0 && parent_is_shadow_root;
    println!("SHADOW_DOM_WORKS={}", out.shadow_dom_works);

    // 先自证「C9 的探测方法本身有效」：确实调了一个公开 trait 方法并捕获到上游 panic。
    // 这条是「非恒假门禁」的对冲：若它失败，则 SHADOW_DOM_WORKS=false 这个结论不可信。
    probe.record(
        Level::Capability,
        attach_panicked && shadow_panic_msg.contains("Shadow roots not implemented"),
        "c9-probe",
        "确实调用了 TShadowRoot::host() 并捕获到上游 todo!(\"Shadow roots not implemented\") —— C9 的探测方法有效（非恒假门禁）",
        "未能触发/捕获 TShadowRoot::host() 的上游 panic，C9 的探测方法可疑，SHADOW_DOM_WORKS 结论不可信",
    );
    probe.record(
        Level::Capability,
        out.shadow_dom_works,
        "c9",
        "Shadow DOM 可用：as_shadow_root 有值 + 作用域隔离成立 + 父节点被识别为 shadow root",
        &format!(
            "blitz-dom 0.3.0-beta.2 未实现 shadow root：\
             as_shadow_root().is_some()={}（impl 恒返回 None，stylo.rs:311-313）、\
             作用域隔离不成立（该节点在 light DOM 选择器结果里可见 {} 个）、\
             parent_node_is_shadow_root()={} 且 containing_shadow_host().is_some()={}（均为桩实现，stylo.rs:337/341）、\
             TShadowRoot::host() panic={:?}（stylo.rs:231 todo!()）、\
             NodeData 无 ShadowRoot 变体故 slot assigned nodes API 不存在",
            as_shadow_is_some,
            visible_from_light_dom,
            parent_is_shadow_root,
            containing_host_is_some,
            shadow_panic_msg
        ),
    );
}

// ===========================================================================
// main
// ===========================================================================

fn main() {
    let mut probe = Probe::new();
    let mut out = Outcomes::default();

    exp0(&mut probe, &mut out);

    println!();
    println!("HTML5EVER_VERSION_FORK_IMPACT=BREAKING");
    println!(
        "HTML5EVER_FORK_SOURCE=docs/design-remaining-tasks-2026-10-12.md §0 Q-3 \
         (架构师已定：markup5ever 0.12 vs 0.39 源码级不兼容；探针不重跑，属独立任务 T2.3-bis)"
    );

    println!();
    exp1_exp2_exp3(&mut probe, &mut out);

    // ---- 二值结论汇总 ----
    println!();
    println!("=== 二值结论汇总 ===");
    println!("TARGET_LINUX_OK={}", out.target_linux_ok);
    println!("TARGET_MAC_OK={}", std::env::consts::OS == "macos");
    println!("DYNAMIC_DOM_WORKS={}", out.dynamic_dom_works);
    println!("STYLE_INVALIDATION_WORKS={}", out.style_invalidation_works);
    println!(
        "LAYOUT_INVALIDATION_WORKS={}",
        out.layout_invalidation_works
    );
    println!("NODE_ID_MAP_CONSISTENT={}", out.node_id_map_consistent);
    println!("BACKEND_IS_CACHE={}", out.backend_is_cache);
    println!("SHADOW_DOM_WORKS={}", out.shadow_dom_works);

    // ---- 裁决 ----
    println!();
    println!("=== 裁决 ===");
    let verdict = if out.keep_blitz() {
        "KEEP_BLITZ"
    } else {
        "SWITCH_TO_STYLO"
    };
    println!("T2_4_VERDICT={verdict}");
    if !out.keep_blitz() {
        // 平台说明与 C1 的**实际值**都动态化：裁决串不得含手写常量
        // （与 C1 恒假门禁同源的缺陷，见 ADR-0008 决策 3 / C9a 纪律）。
        // 注意：`keep_blitz()` / C11 的判据本身**未改动**，两平台裁决仍为
        // SWITCH_TO_STYLO，驱动因素是 C9（SHADOW_DOM_WORKS=false）。
        let c1_platform_note = if out.target_linux_ok {
            "x86_64 Linux 本机实跑"
        } else {
            "待远程 x86_64 Linux 复跑"
        };
        println!(
            "VERDICT_DRIVER=C9 SHADOW_DOM_WORKS=false（架构师已在源码层确证 stylo.rs:231/238/311-313）\
             + C1 TARGET_LINUX_OK={}（{}；\
             即便 C1 为 true，C9 仍为 false，裁决不变）",
            out.target_linux_ok, c1_platform_note
        );
        println!(
            "VERDICT_ROUTE=按技术方案 §8 切换为直接 Stylo + Taffy + Parley 适配；\
             LayoutBackend 对外接口保持不变（C14）"
        );
    }

    // ---- 探针有效性 ----
    println!("ASSERTIONS_RUN={}", probe.checks);
    println!("PROBE_METHOD_VALID={}", probe.method_ok);
    // PROBE_OK 只反映「探针自身全部断言跑完且方法有效」，不反映被测能力真假。
    // 与 ADR-0007 的 BASELINE_VALID 范式一致；PROBE_OK + SWITCH_TO_STYLO 是合法组合。
    println!("T2_4_PROBE_{}", if probe.method_ok { "OK" } else { "FAIL" });
}

// ===========================================================================
// C9 的可执行证据（`cargo test` 路径）
// ===========================================================================
//
// `main()` 里已经用 `catch_unwind` 把上游 panic 变成可打印的证据（ADR-0011 原文来源）。
// 但 `catch_unwind` **本身不会让测试失败** —— 它对「panic」和「不 panic」都返回 `Ok(())`
// 之外的路径，若上游哪天把 `todo!()` 换成真实现，`main()` 里的 `c9-probe` 断言会失败（能发现），
// 可这条线索埋在 `cargo run` 的输出里，不在 `cargo test` 的信号里。
//
// 所以下面把「必须 panic」钉成**独立、可进 CI 的回归契约**，同时给 `catch_unwind` 配一个
// **正面对照**，证明它能区分 panic 与不 panic（避免误以为 `catch_unwind` 恒返回 `Err`）。
#[cfg(test)]
mod tests {
    use super::*;

    /// C9 正面证据：`TShadowRoot::host()` 必须 panic。
    ///
    /// `#[should_panic(expected = ...)]` 做的是**子串匹配**，所以期望值写
    /// `"Shadow roots not implemented"`（实测原文是
    /// `not yet implemented: Shadow roots not implemented`，前缀由 `todo!()` 宏生成）。
    ///
    /// ⚠️ 上游一旦实现 shadow root，这个测试会**失败** —— 那是**正确**的失败：
    /// 它在提醒维护者回来更新 C9 结论与 ADR-0011，而不是让一条过期结论悄悄留在文档里。
    #[test]
    #[should_panic(expected = "Shadow roots not implemented")]
    fn shadow_root_host_is_not_implemented() {
        let fx = build_fixture();
        let node: &Node = fx.doc.get_node(fx.inline_box).expect("节点应存在");
        // `TShadowRoot` 实现给 `&Node`（`type BlitzNode<'a> = &'a Node`，stylo.rs:191），
        // 所以这里要传 `&&Node`。
        let _ = TShadowRoot::host(&node);
    }

    /// 正面对照：`catch_unwind` 对**不 panic** 的闭包必须返回 `Ok`。
    ///
    /// 没有这条，`c9-probe` 的「捕获到 panic」就无法区分
    /// 「上游真的炸了」与「`catch_unwind` 本身就总是 `Err`」—— 那就是另一种恒假门禁。
    #[test]
    fn catch_unwind_returns_ok_when_no_panic() {
        let r = catch_unwind(AssertUnwindSafe(|| 1 + 1));
        assert_eq!(r.ok(), Some(2), "catch_unwind 对不 panic 的闭包必须返回 Ok");
    }

    /// C2 的机器复核：黑名单里的包确实不在 `Cargo.lock` 里。
    ///
    /// 与 `main()` 的 EXP0 同源同数据源（同一个 `parse_lock_versions`），
    /// 目的是让「不整包依赖 GUI」这条**在 `cargo test` 里也可回归**。
    #[test]
    fn no_gui_crates_in_lockfile() {
        let lock_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.lock");
        let lock = std::fs::read_to_string(&lock_path).expect("Cargo.lock 应可读");
        let versions = parse_lock_versions(&lock);
        for pkg in FORBIDDEN_PACKAGES {
            assert!(
                !versions.contains_key(*pkg),
                "GUI 栈包 {pkg} 不应出现在 Cargo.lock 里（技术方案 §3）"
            );
        }
    }
}
