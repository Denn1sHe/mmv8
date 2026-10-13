"""sdk-python 的 pytest 契约测试（施工图 §3.6 十条 + S13 纵向值链）。

**真实 pytest 执行**：`iv8` 是经 maturin 构建的**真扩展模块**（见 `pyproject.toml`），
由 CPython 进程加载并调用，Rust 侧不做任何模拟替代（对齐 ADR-0012 的降级纪律）。

跑法（在 `crates/sdk-python/` 下）：`uv run pytest`
  - uv 自动建隔离构建环境装 maturin → 构建并安装 `iv8` → 装 pytest → 跑本目录。
"""

from __future__ import annotations

import time

import pytest

import iv8


@pytest.fixture()
def ctx() -> "iv8.Context":
    """每个用例一个独立 Context（G1 单 Realm），结束即 close。"""
    context = iv8.Context()
    try:
        yield context
    finally:
        context.close()


# ---------------------------------------------------------------- §2.2 转换

def test_undefined_is_singleton() -> None:
    """`iv8.undefined` 是单例哨兵（多次取同一对象）。"""
    assert iv8.undefined is iv8.undefined


def test_eval_basic_values_roundtrip(ctx: "iv8.Context") -> None:
    """§2.2 基础类型映射：Undefined/Null/Bool/Number/Str。"""
    assert ctx.eval("undefined") is iv8.undefined
    assert ctx.eval("null") is None
    assert ctx.eval("true") is True
    assert ctx.eval("42") == 42.0
    assert ctx.eval("'hello'") == "hello"


def test_eval_object_becomes_js_ref(ctx: "iv8.Context") -> None:
    """对象类值映射为 `iv8.JsRef`（内持 lease）。"""
    ref = ctx.eval("({a: 1})")
    assert isinstance(ref, iv8.JsRef)
    assert isinstance(ref.realm_id, int)
    assert isinstance(ref.handle_id, int)


# ------------------------------------------------- §3.6 Python 回调（pytest）

def test_py_callback_returns_value(ctx: "iv8.Context") -> None:
    """§3.6 `py_callback_returns_value`：Python 函数经 expose 后 JS 调用返回值正确。"""
    ctx.expose("mul", lambda a, b: a * b)
    assert ctx.eval("mul(6, 7)") == 42.0


def test_py_callback_receives_args(ctx: "iv8.Context") -> None:
    """§3.6 `py_callback_receives_args`：多参 / 字符串 / 数字跨边界转换正确。"""
    ctx.expose(
        "describe",
        lambda a, b, c: f"{type(a).__name__}:{a}|{type(b).__name__}:{b}|{type(c).__name__}:{c}",
    )
    assert ctx.eval("describe('s', 4, 2.5)") == "str:s|float:4.0|float:2.5"


def test_py_callback_exception_propagates(ctx: "iv8.Context") -> None:
    """§3.6 `py_callback_exception_propagates`：Python 异常 ⇒ JS 可捕获，含类型名。"""

    def boom() -> None:
        raise ValueError("kaboom")

    ctx.expose("boom", boom)
    message = ctx.eval("try { boom(); 'no-throw' } catch (e) { e.message }")
    assert "ValueError" in message
    assert "kaboom" in message


def test_reentrant_eval_rejected(ctx: "iv8.Context") -> None:
    """§3.6 `reentrant_eval_rejected`：回调内 `ctx.eval` ⇒ `ReentrantEval`。"""
    captured: dict[str, str] = {}

    def reenter() -> str:
        try:
            ctx.eval("1 + 1")
        except iv8.ReentrantEvalError as exc:  # noqa: PERF203
            captured["msg"] = str(exc)
            return "reentrant"
        return "no-error"

    ctx.expose("reenter", reenter)
    assert ctx.eval("reenter()") == "reentrant"
    assert "ReentrantEval" in captured["msg"]


def test_reentrant_eval_rejected_in_microtask(ctx: "iv8.Context") -> None:
    """§3.6 `reentrant_in_microtask_rejected` 的 Python 侧投影：微任务执行期隐式重入 ⇒ 被拒。"""
    captured: dict[str, str] = {}

    def reenter() -> str:
        try:
            ctx.eval("1 + 1")
        except iv8.ReentrantEvalError as exc:  # noqa: PERF203
            captured["msg"] = str(exc)
            return "reentrant"
        return "no-error"

    ctx.expose("reenter", reenter)
    ctx.eval(
        "globalThis.__r = 'pending';"
        " Promise.resolve().then(() => { globalThis.__r = reenter(); });"
    )
    ctx.perform_checkpoint()
    assert ctx.eval("globalThis.__r") == "reentrant"
    assert "ReentrantEval" in captured["msg"]


def test_callback_deadline_enforced(ctx: "iv8.Context") -> None:
    """§3.6 `callback_deadline_enforced`：墙钟超限 ⇒ `CallbackDeadline`（事后可判定，不挂死）。"""
    ctx.set_callback_deadline_ms(20)

    def slow() -> int:
        time.sleep(0.2)
        return 1

    ctx.expose("slow", slow)
    message = ctx.eval("try { slow(); 'no-deadline' } catch (e) { e.message }")
    assert "CallbackDeadline" in message


def test_no_gil_wait_in_callback(ctx: "iv8.Context") -> None:
    """§3.6 `no_gil_wait_in_callback`：派发路径不发起跨线程阻塞式取锁（有界墙钟判据）。"""
    ctx.expose("noop", lambda: 0)
    start = time.monotonic()
    for _ in range(100):
        assert ctx.eval("noop()") == 0.0
    elapsed = time.monotonic() - start
    assert elapsed < 5.0, f"100 次回调往返耗时 {elapsed:.3f}s，疑似阻塞式取锁"


def test_close_is_idempotent() -> None:
    """§3.6 `close_is_idempotent`：close 两次不报错；关闭后 eval 报明确错误。"""
    context = iv8.Context()
    context.close()
    context.close()
    with pytest.raises(iv8.Iv8Error):
        context.eval("1 + 1")


# ------------------------------------------------ §2.4 控制入口（单 Realm 双模式）

def test_control_eval_installs_control_global(ctx: "iv8.Context") -> None:
    """控制范围：`__iv8__` 可见；页面范围：不可见（RAII 退出即撤）。"""
    assert ctx.control_eval("typeof __iv8__") == "object"
    assert ctx.eval("typeof __iv8__") == "undefined"


# ------------------------------------------------- S13 纵向值链（§5 S13 行）

def test_vertical_native_callback_microtask_checkpoint(ctx: "iv8.Context") -> None:
    """S13：原生 fn（Python 回调）→ 微任务 → checkpoint 的端到端值链。"""
    ctx.expose("bump", lambda x: x + 1)
    ctx.eval(
        "globalThis.__out = undefined;"
        " Promise.resolve().then(() => { globalThis.__out = bump(41); });"
    )
    # Explicit 策略（§6 默认）：checkpoint 前微任务未跑，`__out` 仍是 undefined。
    assert ctx.eval("globalThis.__out") is iv8.undefined

    ctx.perform_checkpoint()
    assert ctx.eval("globalThis.__out") == 42.0
