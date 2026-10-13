//! S16 纵向样例（§3.9）：全链路「阻塞脚本 → `document.write` → class/style → 几何 stub →
//! fetch 等待 → complete → DOM 更新 → `MutationObserver` → 可信 click → close 幂等」逐段断言。
//!
//! **落点说明（对施工图 §5 S16 的偏差）**：施工图拟放 `engine-v8/tests/vertical_g2.rs`，
//! 但 `engine-v8` 是 `navigation` 的**上游**依赖，令 `engine-v8` dev-dep `navigation` 会构成
//! **dev 依赖环**。故本纵向样例落在 `navigation/tests/vertical_g2.rs`（`navigation` 已依赖
//! engine-v8/web-dom/network/scheduler/policy，可自洽贯通全链），语义等价、无环。

use std::cell::Cell;
use std::rc::Rc;

use engine_v8::HostValue;
use engine_v8::native_fn::NativeFn;
use navigation::input::InputKind;
use navigation::page::{BrowsingContext, PageSnapshot};
use network::Response;
use scheduler::{RunBudget, RunOutcome};
use web_dom::mutation::ObserveOptions;
use web_dom::node::NodeId;
use web_dom::tree::NodeTree;

/// 深度优先查找首个具 `attr == value` 的节点。
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

#[test]
#[allow(clippy::too_many_lines)] // 纵向样例：逐段断言全链，长度属该测试的固有形态。
fn vertical_page_blocking_write_fetch_observer_click() {
    let mut page = BrowsingContext::new("https://a.example/").expect("new");

    let html = r#"<html><body>
        <script>
          globalThis.__order = [];
          document.write('<div id="made"></div>');
          globalThis.__order.push('wrote');
        </script>
        <script>
          var d = document.getElementById('made');
          d.className = 'box';
          d.style.color = 'red';
          globalThis.__w = d.offsetWidth;
          globalThis.__order.push('styled');
          fetch('https://a.example/miss').then(function (r) {
            document.body.setAttribute('data-done', String(r.status));
            globalThis.__order.push('fetched');
          });
          document.getElementById('made').addEventListener('click', function () {
            globalThis.__order.push('clicked');
          });
        </script>
      </body></html>"#;
    page.load(&PageSnapshot {
        url: "https://a.example/doc".to_owned(),
        html: html.to_owned(),
        headers: Vec::new(),
        resources: Vec::new(),
    })
    .expect("load");

    // MutationObserver（**load 后**注册：导航重绑会重置 observers/tree）。
    let mo_fired = Rc::new(Cell::new(false));
    let flag = Rc::clone(&mo_fired);
    let callback: NativeFn = Box::new(move |_realm, _args| {
        flag.set(true);
        Ok(HostValue::Undefined)
    });
    let mo_handle = page
        .realm_mut()
        .new_function("mo", 1, callback)
        .expect("mo");
    let _mo_lease = page.realm_mut().root(mo_handle).expect("root mo");
    let mo_weak = page.realm_mut().weak_handle(mo_handle).expect("weak mo");
    let document = page.document();
    page.observe(
        document,
        ObserveOptions {
            child_list: true,
            attributes: true,
            subtree: true,
            attribute_old_value: false,
        },
        mo_weak,
    );

    // ① 阻塞脚本就地执行 + document.write 建节点。
    let made = {
        let tree = page.tree();
        let tree = tree.borrow();
        find_by_attr(&tree, tree.root(), "id", "made").expect("document.write 应建出 #made")
    };
    // ② class/style 反映到属性 / 内联声明。
    {
        let tree = page.tree();
        let tree = tree.borrow();
        assert_eq!(tree.get_attribute(made, "class"), Some("box"));
        let element = tree
            .get(made)
            .and_then(|data| data.element.as_ref())
            .unwrap();
        assert_eq!(element.style.get_property("color"), Some("red"));
    }
    // ③ 几何口径 = stub（确定值 0）。
    assert_eq!(
        page.realm_mut().eval("globalThis.__w").unwrap(),
        HostValue::Number(0.0),
        "offsetWidth 应为 stub 0"
    );

    // ④ fetch 未命中 ⇒ WaitingIo（不凭空完成）。
    assert_eq!(
        page.run_until(RunBudget::unlimited()).expect("run"),
        RunOutcome::WaitingIo
    );
    let id = page
        .pending_fetches()
        .first()
        .copied()
        .expect("应有 pending 请求");
    // ⑤ host complete ⇒ 入队 resolve。
    page.complete_fetch(id, Response::ok(200, Vec::new(), b"ok".to_vec()))
        .expect("complete");
    page.run_until(RunBudget::unlimited()).expect("run2");
    // ⑥ checkpoint：MO 交付（前段）+ Promise then 更新 DOM。
    page.perform_checkpoint().expect("checkpoint");
    assert_eq!(
        page.realm_mut()
            .eval("document.body.getAttribute('data-done')")
            .unwrap(),
        HostValue::Str("200".to_owned()),
        "Promise 应更新 DOM"
    );
    assert!(mo_fired.get(), "MutationObserver 应交付");

    // ⑦ 可信 click（显式 target）命中监听器。
    page.input_dispatch(InputKind::Click, Some(made))
        .expect("click");
    let order = match page
        .realm_mut()
        .eval("globalThis.__order.join(',')")
        .unwrap()
    {
        HostValue::Str(text) => text,
        other => panic!("期望 Str，得到 {other:?}"),
    };
    assert_eq!(order, "wrote,styled,fetched,clicked", "全链顺序应逐段可见");
    assert!(page.user_activation().sticky, "可信 click 应置位 sticky");

    // ⑧ close 幂等。
    page.close().expect("close1");
    page.close().expect("close2");
}
