//! S13 纵向集成（engine-v8 侧）：**原生 fn → 微任务 → checkpoint** 的端到端值链。
//!
//! 本文件是**新增**的集成测试（仅用 `engine-v8` 的公开安全 API），不修改任何 `engine-v8`
//! 既有源码。Python 回调那一环的纵向值链在 `crates/sdk-python/tests/test_iv8.py`
//! 的 `test_vertical_native_callback_microtask_checkpoint`（需真解释器，故放那边）。

use engine_v8::HostValue;
use engine_v8::isolate::AgentRuntime;
use engine_v8::microtask::MicrotaskPolicy;
use engine_v8::native_fn::NativeFn;
use engine_v8::realm::Realm;

/// 建一个独立 Realm（G1 单 Isolate 单 Realm）。
fn new_realm() -> Realm {
    AgentRuntime::new()
        .expect("建 AgentRuntime")
        .create_realm()
        .expect("建 Realm")
}

/// 取全局对象句柄。
fn global_handle(realm: &mut Realm) -> engine_v8::JsHandle {
    match realm.eval("globalThis").expect("取 globalThis") {
        HostValue::Object { handle, .. } => handle,
        other => panic!("globalThis 非对象：{other:?}"),
    }
}

#[test]
fn vertical_native_fn_to_microtask_to_checkpoint() {
    let mut realm = new_realm();
    // §6：宿主显式管理微任务。
    realm.set_microtasks_policy(MicrotaskPolicy::Explicit);

    // 一环：宿主原生 fn（入参 +1），经 FunctionTemplate 路径注册到全局。
    let bump: NativeFn = Box::new(
        |_realm: &mut Realm, args: &[HostValue]| match args.first() {
            Some(HostValue::Number(n)) => Ok(HostValue::Number(n + 1.0)),
            _ => Ok(HostValue::Number(f64::NAN)),
        },
    );
    let bump_handle = realm.new_function("bump", 1, bump).expect("注册原生 fn");

    let global = global_handle(&mut realm);
    // 显式 root，保证 global 在整条链存活（ADR-0013：句柄非根）。
    let _global_lease = realm.root(global).expect("root global");
    realm
        .set_prop(
            global,
            "bump",
            HostValue::Function {
                realm: realm.id(),
                handle: bump_handle,
            },
        )
        .expect("挂载 bump 到全局");

    // 二环：微任务里调用原生 fn，结果写回全局。
    realm
        .eval(
            "globalThis.__out = 'pending'; \
             Promise.resolve().then(() => { globalThis.__out = bump(41); });",
        )
        .expect("排入微任务");

    // Explicit 策略：checkpoint 前微任务**未**跑。
    assert_eq!(
        realm.get_prop(global, "__out").expect("读 __out"),
        HostValue::Str("pending".to_owned()),
        "checkpoint 前微任务不应执行"
    );

    // 三环：显式 checkpoint 跑微任务 → 原生 fn 被调用 → 值链闭合。
    realm.perform_checkpoint().expect("跑 checkpoint");
    assert_eq!(
        realm.get_prop(global, "__out").expect("读 __out"),
        HostValue::Number(42.0),
        "微任务经原生 fn 应把 41+1 写回全局"
    );
}
