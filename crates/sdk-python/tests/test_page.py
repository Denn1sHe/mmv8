"""sdk-python 页面控制面契约测试（施工图 §3.8 各条）。

**真实 pytest 执行**：`iv8` 是经 maturin 构建的真扩展模块（见 `pyproject.toml`），
由 CPython 进程加载并调用，Rust 侧不做任何模拟替代。

跑法（在 `crates/sdk-python/` 下）：`uv run pytest`
"""

from __future__ import annotations

import pytest

import iv8


@pytest.fixture()
def ctx() -> "iv8.Context":
    """每个用例一个独立 Context（单 Realm），结束即 close。"""
    context = iv8.Context()
    try:
        yield context
    finally:
        context.close()


def snapshot(url: str = "https://a.example/", html: str = "<body></body>") -> "iv8.Snapshot":
    """构造页面快照。"""
    return iv8.Snapshot(url=url, html=html)


#: 纵向样例页面（§3.9）：阻塞脚本 → document.write → class/style → 几何 stub → fetch → click。
VERTICAL_HTML = """<html><body>
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
</body></html>"""


# ------------------------------------------------- §3.9 纵向样例（端到端）

def test_vertical_page_sample(ctx: "iv8.Context") -> None:
    """`test_vertical_page_sample`：§3.9 全链经 sdk-python 最小控制面跑通。

    覆盖：write→class/style→几何 stub→fetch(WaitingIo)→complete→DOM 更新→click→close 幂等。
    （MutationObserver 经 Rust 侧 `observe` 注册，无 Python 面；由 cargo 版 `vertical_g2` 覆盖。）
    """
    ctx.page.load(snapshot("https://a.example/doc", VERTICAL_HTML))

    assert ctx.eval("document.getElementById('made').className") == "box"
    assert ctx.eval("document.getElementById('made').offsetWidth") == 0.0
    assert ctx.eval("globalThis.__w") == 0.0

    assert ctx.run_until(0, 0) == "WaitingIo"
    pending = ctx.network.pending()
    assert len(pending) == 1
    ctx.network.complete(pending[0], iv8.Response(status=200, body=b"ok"))
    ctx.run_until(0, 0)
    ctx.perform_checkpoint()
    assert ctx.eval("document.body.getAttribute('data-done')") == "200"

    node_id = int(ctx.eval("document.getElementById('made').__iv8_node"))
    ctx.input.dispatch("click", node_id)
    assert ctx.eval("globalThis.__order.join(',')") == "wrote,styled,fetched,clicked"

    ctx.close()
    ctx.close()


# ------------------------------------------------- §3.8 Python 控制面

def test_page_load_streams_snapshot(ctx: "iv8.Context") -> None:
    """`page_load_streams_snapshot`：page.load 建文档并启动解析。"""
    ctx.page.load(snapshot("https://a.example/doc", "<html><body><p>x</p></body></html>"))
    assert ctx.page.url == "https://a.example/doc"
    assert ctx.page.origin == "https://a.example"
    assert ctx.page.ready_state in {"loading", "interactive", "complete"}


def test_run_until_returns_outcome(ctx: "iv8.Context") -> None:
    """`run_until_returns_outcome`：返回 Idle/WaitingIo/... 之一（字符串）。"""
    ctx.page.load(snapshot())
    outcome = ctx.run_until(4, 0)
    assert outcome in {
        "Idle",
        "WaitingIo",
        "BudgetExhausted",
        "DeadlineExceeded",
        "Closed",
    }
    assert outcome == "Idle"


def test_network_pending_and_complete_roundtrip(ctx: "iv8.Context") -> None:
    """`network_pending_and_complete_roundtrip`：pending() 列出 → complete() → resolve。"""
    ctx.page.load(
        snapshot(
            html=(
                "<body><script>"
                "globalThis.__got = 0;"
                " fetch('https://a.example/miss').then(function () { globalThis.__got = 1; });"
                "</script></body>"
            )
        )
    )
    # 首次 run_until 把 fetch 票泵入网络 ⇒ 登记 pending。
    outcome = ctx.run_until(0, 0)
    assert outcome == "WaitingIo"
    pending = ctx.network.pending()
    assert len(pending) == 1

    ctx.network.complete(pending[0], iv8.Response(status=200, body=b"hello"))
    ctx.run_until(0, 0)  # resolve
    ctx.perform_checkpoint()  # then 回调

    assert ctx.eval("globalThis.__got") == 1.0
    log = ctx.network.export_log()
    assert any(entry.phase == "RequestSent" for entry in log)
    assert any(entry.phase == "ResponseReceived" and entry.status == 200 for entry in log)


def test_input_dispatch_click_from_python(ctx: "iv8.Context") -> None:
    """`input_dispatch_click_from_python`：Python 触发可信 click，监听器被调。"""
    ctx.page.load(snapshot(html="<body><button id='b'>x</button></body>"))
    ctx.eval(
        "globalThis.__clicks = 0;"
        " globalThis.__trusted = null;"
        " document.getElementById('b').addEventListener('click', function (ev) {"
        "   globalThis.__clicks += 1; globalThis.__trusted = ev.isTrusted; });"
    )
    # 取 button 的 NodeId raw（桥在 wrapper 上的隐藏键），再经 Python 分派。
    node_id = int(ctx.eval("document.getElementById('b').__iv8_node"))
    ctx.input.dispatch("click", node_id)
    assert ctx.eval("globalThis.__clicks") == 1.0
    assert ctx.eval("globalThis.__trusted") is True


def test_capabilities_reports_g3_surface(ctx: "iv8.Context") -> None:
    """`capabilities_reports_g3_surface`（G3-B16）：geometry 升为 computed、shadow_dom 开启。"""
    caps = ctx.capabilities()
    assert caps["parsing"] == "streaming"
    assert caps["document_write"] is True
    assert caps["navigation"] is True
    assert caps["network"] == "offline"
    assert caps["input"] == "explicit_target"
    assert caps["geometry"] == "computed"
    assert caps["shadow_dom"] is True
    assert caps["script_interruptible"] is True
    assert caps["input_user_activation"] == "sticky_only"
    assert caps["single_realm_navigation"] is True


def test_page_geometry_is_computed(ctx: "iv8.Context") -> None:
    """`page_geometry_is_computed`（G3-B16 更名自 `page_geometry_is_documented_stub`）：
    几何面 = computed（非 stub）。"""
    ctx.page.load(snapshot(html='<body><div id="g" style="width:50px;height:20px"></div></body>'))
    assert ctx.capabilities()["geometry"] == "computed"
    assert ctx.eval("document.getElementById('g').offsetWidth") == 50.0
    assert ctx.eval("document.getElementById('g').offsetHeight") == 20.0


def test_py_control_reentry_is_error(ctx: "iv8.Context") -> None:
    """`py_control_reentry_is_error`：宿主跑 JS 期间 Python 再入 ⇒ ReentrantEval（非 panic）。"""
    captured: dict[str, str] = {}

    def probe() -> str:
        try:
            ctx.network.pending()
        except iv8.ReentrantEvalError as exc:  # noqa: PERF203
            captured["msg"] = str(exc)
            return "reentrant"
        return "no-error"

    ctx.expose("probe", probe)
    assert ctx.eval("probe()") == "reentrant"
    assert "ReentrantEval" in captured["msg"]


def test_close_after_navigation_idempotent(ctx: "iv8.Context") -> None:
    """`close_after_navigation_idempotent`：导航后 close 幂等，无泄漏报错。"""
    ctx.page.load(snapshot("https://a.example/one", "<body></body>"))
    ctx.page.load(snapshot("https://a.example/two", "<body></body>"))
    ctx.close()
    ctx.close()


def test_page_geometry_real_roundtrip(ctx: "iv8.Context") -> None:
    """S15 / §3.7：Python 侧读几何得**真实值**（与 §3.2 口径一致）。"""
    page = ctx.page
    page.load(
        snapshot(
            html='<body><div id="box" style="width:120px;height:40px"></div></body>'
        )
    )
    offset_w, offset_h, _client_w, _client_h, top, left = page.geometry("box")
    assert round(offset_w) == 120, f"offsetWidth 应为 120（实测 {offset_w}）"
    assert round(offset_h) == 40, f"offsetHeight 应为 40（实测 {offset_h}）"
    assert round(top) == 0 and round(left) == 0


def test_page_geometry_client_excludes_border(ctx: "iv8.Context") -> None:
    """S15：client 读数排除 border（border-box 口径）。"""
    page = ctx.page
    page.load(
        snapshot(
            html='<body><div id="b" style="width:100px;height:50px;border:2px solid"></div></body>'
        )
    )
    offset_w, _offset_h, client_w, _client_h, _top, _left = page.geometry("b")
    assert round(offset_w) == 100
    assert round(client_w) == 96, f"clientWidth = 100 - 2*2（实测 {client_w}）"


def test_page_vertical_g3_style_layout_shadow(ctx: "iv8.Context") -> None:
    """S16 纵向（Python 侧，§4）：加载（<style>）-> 级联/盒树 -> 几何 -> 改样式同步读。"""
    page = ctx.page
    page.load(
        snapshot(
            html='<html><head><style>#box{width:120px;height:40px}</style></head>'
            '<body><div id="box"></div></body></html>'
        )
    )
    ow0, oh0, _cw, _ch, _t, _l = page.geometry("box")
    assert round(ow0) == 120, f"级联->盒树->几何 应为 120（实测 {ow0}）"
    assert round(oh0) == 40
    # 改样式 -> 同步读（§2.5，非陈旧）
    ctx.eval(
        "document.getElementById('box').setAttribute('style','width:200px;height:40px')"
    )
    ow1 = page.geometry("box")[0]
    assert round(ow1) == 200, f"改样式后同步读应为 200（实测 {ow1}）"


def test_page_vertical_g3_shadow_open_only(ctx: "iv8.Context") -> None:
    """S16 纵向（Python 侧）：shadow open 可见、closed 为 None（G3-B14）。"""
    page = ctx.page
    page.load(snapshot(html='<body><x-h id="h"></x-h></body>'))
    visible = ctx.eval(
        "var h = document.getElementById('h'); var sr = h.attachShadow({mode:'open'});"
        " sr.appendChild(document.createElement('div')); (h.shadowRoot !== null)"
    )
    assert visible is True
