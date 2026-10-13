//! 最小可行探针：宿主上下文内创建 V8 Isolate / Context，执行 JS、注入宿主回调。
//! 目的：验证 `v8` crate 152.2.0（Deno rusty_v8 的发布名）+ prebuilt 静态库
//! 在本机 macOS arm64 上可获取、可编译、可链接、可运行。
//!
//! API 基线取自 crate 自带的 `examples/hello_world.rs`（v8 152 的 scope API
//! 相较旧版已改为 pin-scope 模型，须用 `v8::scope!` 宏）。另有一条实测约束：
//! `Function::new` 要求 `&mut PinScope`，而 `String::new` / `Script::compile`
//! 只需 `&PinScope`，故 scope 须绑为 `mut` 后分别 reborrow。

fn main() {
    let platform = v8::new_default_platform(0, false).make_shared();
    v8::V8::initialize_platform(platform);
    v8::V8::initialize();

    let isolate = &mut v8::Isolate::new(v8::CreateParams::default());
    v8::scope!(let handle_scope, isolate);
    let context = v8::Context::new(handle_scope, Default::default());
    let mut scope = v8::ContextScope::new(handle_scope, context);

    // 1) 基本求值
    let code = v8::String::new(&scope, "1 + 2").unwrap();
    let script = v8::Script::compile(&scope, code, None).unwrap();
    let result = script.run(&scope).unwrap();
    let result = result.to_string(&scope).unwrap();
    println!("PROBE_EVAL 1+2 = {}", result.to_rust_string_lossy(&scope));

    // 2) 跨语言回调：宿主函数注入 JS 全局，JS 调用它
    //    写法基线来自 v8 crate 自带 examples/cppgc-object.rs：回调不返回 Result，
    //    而是直接设置返回值参数 rv。
    let cb = v8::Function::new(
        &mut scope,
        |scope: &mut v8::PinScope<'_, '_>,
         args: v8::FunctionCallbackArguments,
         _rv: v8::ReturnValue| {
            println!("PROBE_CALLBACK host fn invoked, argc = {}", args.length());
        },
    )
    .unwrap();
    let global = context.global(&scope);
    let key = v8::String::new(&scope, "__host_probe").unwrap();
    global.set(&scope, key.into(), cb.into()).unwrap();

    let code2 = v8::String::new(&scope, "__host_probe(1, 2, 3); 'callback-ok'").unwrap();
    let script2 = v8::Script::compile(&scope, code2, None).unwrap();
    let r2 = script2.run(&scope).unwrap().to_string(&scope).unwrap();
    println!("PROBE_CALLBACK_RESULT = {}", r2.to_rust_string_lossy(&scope));

    // 3) 异常路径（T2.5 死锁边界的前置能力：宿主需能观测 JS 抛出）
    let code3 = v8::String::new(&scope, "(() => { throw new Error('boom'); })()").unwrap();
    let script3 = v8::Script::compile(&scope, code3, None).unwrap();
    match script3.run(&scope) {
        Some(_) => println!("PROBE_THROW unexpectedly returned a value"),
        None => println!("PROBE_THROW propagated as None (需 TryCatch 取异常)"),
    }

    println!("PROBE_OK");
}
