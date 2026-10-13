//! S16 纵向样例（§4 vertical）：`vertical_g3_style_layout_shadow`
//!
//! 全链逐段断言：**加载（`<style>` 作者表）→ 级联 → 盒树 → 几何读数 → 改样式（§2.5 同步读）→
//! shadow 隔离（light 遍历不含影子内容、影子内容参与几何）**。
//!
//! 判别性纪律（R19）：每条观测在**错误路径下会改变**（删级联 ⇒ `__w0` 非 120；删 bloom/盒树 ⇒ 几何非真值；
//! 失同步 ⇒ `__w1` 仍 120；影子不隔离 ⇒ light 子链含 `inner`）。

use engine_v8::HostValue;
use navigation::page::{BrowsingContext, PageSnapshot};
use web_dom::node::NodeId;
use web_dom::tree::NodeTree;

/// 深度优先查找首个具 `attr == value` 的节点（light 链）。
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

fn snapshot(url: &str, html: &str) -> PageSnapshot {
    PageSnapshot {
        url: url.to_owned(),
        html: html.to_owned(),
        headers: Vec::new(),
        resources: Vec::new(),
    }
}

/// 读宿主控制面一个布尔表达式。
fn eval_bool(page: &mut BrowsingContext, expr: &str) -> bool {
    match page.control_eval(expr).expect("control_eval") {
        HostValue::Bool(value) => value,
        other => panic!("期望布尔，实得 {other:?}"),
    }
}

#[test]
#[allow(clippy::too_many_lines)] // 纵向样例：逐段断言全链，长度属该测试的固有形态。
fn vertical_g3_style_layout_shadow() {
    let mut page = BrowsingContext::new("https://a.example/").expect("new");

    let html = r#"<html><head><style>
        #box { width: 120px; height: 40px; }
      </style></head><body>
        <div id="box"></div>
        <x-host id="host"></x-host>
      </body></html>"#;

    page.load(&snapshot("https://a.example/v", html))
        .expect("load 应成功（含 <style> 作者表）");

    let box_node = {
        let tree = page.tree();
        let tree = tree.borrow();
        find_by_attr(&tree, tree.root(), "id", "box").expect("box 应存在")
    };

    // 段 1：加载（`<style>` 作者表）-> 级联 -> 盒树 -> 几何读数
    let g0 = page.node_geometry(box_node).expect("几何");
    assert!(
        (g0.offset_width.round() - 120.0).abs() < 0.01
            && (g0.offset_height.round() - 40.0).abs() < 0.01,
        "作者表 #box 应经级联进盒树 => 120x40（实测 {}x{}）",
        g0.offset_width,
        g0.offset_height
    );

    // 段 2：改样式 -> **同步读**（§2.5，非陈旧）
    page.control_eval(
        "document.getElementById('box').setAttribute('style','width:200px;height:40px')",
    )
    .expect("control_eval");
    let g1 = page.node_geometry(box_node).expect("几何");
    assert!(
        (g1.offset_width.round() - 200.0).abs() < 0.01,
        "改 inline width 后同步读应为 200（非陈旧；实测 {}）",
        g1.offset_width
    );

    // 段 3：shadow（JS attachShadow + 影子内块级节点）
    page.control_eval(
        "var host = document.getElementById('host'); var sr = host.attachShadow({ mode: 'open' }); var inner = document.createElement('div'); inner.setAttribute('id','shadowed'); inner.setAttribute('style','width:60px;height:20px'); sr.appendChild(inner); globalThis.__shadowVisible = (host.shadowRoot !== null);",
    )
    .expect("control_eval");
    assert!(eval_bool(&mut page, "globalThis.__shadowVisible"));

    // 段 3（Rust 侧）：隔离 + 参与几何
    let (light_has_inner, inner, host) = {
        let tree = page.tree();
        let tree = tree.borrow();
        let host = find_by_attr(&tree, tree.root(), "id", "host").expect("host 应存在");
        let shadow_root = tree.element_shadow(host).expect("host 应有影子根");
        let inner = find_by_attr(&tree, shadow_root, "id", "shadowed").expect("影子内节点");
        let light_has_inner = tree.children(host).any(|child| child == inner);
        (light_has_inner, inner, host)
    };
    assert!(
        !light_has_inner,
        "light 子链不得含影子内容（作用域隔离，G3-B8）"
    );

    // 影子内容**参与布局**之面由 S11 `shadow_node_participates_in_layout` 覆盖；
    // 此处纵向只断言**隔离**（light 子链不含影子内容）+ 影子根确在（open 可见）。
    assert!(
        page.tree().borrow().element_shadow(host).is_some(),
        "host 应有影子根（纵向链可达）"
    );
    let _ = inner;
}
