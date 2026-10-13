//! T2.1 探针：`cppgc` ↔ V8 **跨语言循环**能否被回收。
//!
//! 对应技术方案 T2.1：「`rusty_v8`/`cppgc` 能否回收跨语言循环（DOM→监听器→闭包→DOM）」。
//!
//! ## 三段实验
//! - **EXP1 基线**：DOM 的 JS wrapper 失去外部可达 → 应被回收。
//! - **EXP2 单向**：DOM 持有 `v8::Global`（持久**强**引用）指向一个不捕获 DOM 的闭包。
//! - **EXP3 真循环**：DOM 持有 `Global(闭包)`，且闭包**捕获 DOM 自身**，
//!   再从外部切断所有可达路径 —— 即 `DOM → 监听器 → 闭包 → DOM`。
//!
//! ## 判定方法（关键：不能用 `Drop` 判定）
//! 第一版探针用 `impl Drop for DomNode` 打印作为「已回收」的证据，实测**无效**：
//! 三个实验的 `DROP_DOM` 全部出现在程序结束之后——那是进程退出时堆销毁的
//! 统一析构，不是 GC 回收，据此得出的「跨语言环也能回收」是假结论。
//!
//! 本版改用 V8 内建的 **`WeakRef`** 做**确定性**判定：GC 之后
//! `weakRef.deref()` 返回 `undefined` 即为真被回收（返回值直接是布尔量，
//! 无 finalizer 的「尽力而为」时序问题；`v8` 的 `Weak::with_finalizer` 官方
//! 注释也明说回调时机无保证）。
//!
//! `Drop` 打印保留为**辅助**证据，判定结论一律以 `WeakRef` 为准。
//!
//! ## 关键 API（出处：v8 crate 自带 `examples/cppgc.rs` 与 `examples/cppgc-object.rs`）
//! - `v8::V8::set_flags_from_string("--expose-gc")`：`request_garbage_collection_for_testing`
//!   是 for-testing API，缺此 flag 会直接 FATAL。
//! - `v8::cppgc::initialize_process` / `Heap::create` / `CreateParams::cpp_heap`：
//!   受管堆**与 Isolate 绑定**。
//! - `v8::cppgc::make_garbage_collected` + `v8::Object::wrap::<TAG, T>`：
//!   在堆上分配受管对象并**包成 JS 对象**。
//! - `scope.request_garbage_collection_for_testing(GarbageCollectionType::Full)`：确定性 GC。

use std::cell::RefCell;
use std::ffi::CStr;
use std::sync::atomic::{AtomicU16, Ordering};

const TAG: u16 = 1;

/// 代表一个 DOM 节点：由 cppgc 受管堆分配，被 `Object::wrap` 包成 JS 对象。
struct DomNode {
    id: String,
    trace_count: AtomicU16,
    /// 监听器槽：指向 JS 侧闭包的持久**强**引用（DOM → JS 方向）。
    /// 用 `RefCell` 是因为 `GarbageCollected::trace` 只拿到 `&self`。
    listener: RefCell<Option<v8::Global<v8::Value>>>,
}

// `GarbageCollected` 是 unsafe trait：V8 在 GC 期间调用 `trace` 且不借用 `&self`。
// 本 impl 的 `trace` 只做一次原子计数自增，不触碰 V8 堆、不访问需要 `&mut` 的
// `listener` 字段，因此不存在别名或生命周期问题；调用次数可用 trace_count 事后核对。
// SAFETY: 上述论证成立 —— trace(&self) 内无可变访问、无跨堆指针，故实现安全。
unsafe impl v8::cppgc::GarbageCollected for DomNode {
    fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {
        self.trace_count.fetch_add(1, Ordering::Relaxed);
    }

    fn get_name(&self) -> &'static CStr {
        c"Iv8DomNode"
    }
}

impl Drop for DomNode {
    fn drop(&mut self) {
        println!("  [辅助证据] DROP_DOM {}", self.id);
    }
}

/// 在 cppgc 堆上分配一个 DOM 节点并包成 JS 对象，返回 `(JS 对象, 受管指针)`。
macro_rules! make_dom {
    ($scope:expr, $id:expr) => {{
        // `heap` 的借用必须限制在块内：宏不做作用域隔离，留在外层会长期占用
        // 对 `$scope` 的不可变借用，与后续可变借用冲突（E0502）。
        let ptr = {
            let heap = $scope.get_cpp_heap().unwrap();
            // SAFETY: DomNode 满足 GarbageCollected + 'static；堆随 Isolate 绑定并存活。
            // 返回的 UnsafePtr<T> 不借用 heap，可安全离开本块。
            unsafe {
                v8::cppgc::make_garbage_collected(
                    heap,
                    DomNode {
                        id: $id.to_string(),
                        trace_count: AtomicU16::new(0),
                        listener: RefCell::new(None),
                    },
                )
            }
        };
        let obj = v8::Object::new($scope);
        // SAFETY: ptr 来自本堆的 make_garbage_collected；TAG 未被复用（单 TAG 实验）。
        unsafe { v8::Object::wrap::<TAG, DomNode>($scope, obj, &ptr) };
        (obj, ptr)
    }};
}

/// 把一个 JS 值（闭包）挂到 DOM 节点的监听器槽，形成 DOM → JS 强引用。
macro_rules! set_listener {
    ($scope:expr, $ptr:expr, $closure:expr) => {{
        // SAFETY: ptr 由 make_dom 在本堆分配且仍被 wrap 持有；
        // 此处只读写 Rust 侧字段，不触碰 V8 堆。
        let node = unsafe { $ptr.as_ref() };
        *node.listener.borrow_mut() = Some(v8::Global::new($scope, $closure));
    }};
}

/// 确定性触发一次 Full GC。
macro_rules! full_gc {
    ($scope:expr) => {
        $scope.request_garbage_collection_for_testing(v8::GarbageCollectionType::Full)
    };
}

/// 在 JS 侧求值并取回布尔值（避免在 Rust 侧做 `Local<Value> → Function` 的 cast）。
macro_rules! eval_bool {
    ($scope:expr, $expr:expr) => {{
        let src = v8::String::new($scope, $expr).unwrap();
        let v = v8::Script::compile($scope, src, None).unwrap().run($scope).unwrap();
        v.boolean_value($scope)
    }};
}

fn main() {
    // `request_garbage_collection_for_testing` 需要 --expose-gc，否则直接 FATAL。
    v8::V8::set_flags_from_string("--no_freeze_flags_after_init --expose-gc");

    let platform = v8::new_default_platform(0, false).make_shared();
    v8::V8::initialize_platform(platform.clone());
    v8::cppgc::initialize_process(platform.clone());
    v8::V8::initialize();

    let heap = v8::cppgc::Heap::create(platform, v8::cppgc::HeapCreateParams::default());
    let isolate = &mut v8::Isolate::new(v8::CreateParams::default().cpp_heap(heap));

    v8::scope!(handle_scope, isolate);
    let context = v8::Context::new(handle_scope, Default::default());
    let scope = &mut v8::ContextScope::new(handle_scope, context);
    let global = context.global(scope);

    // JS 侧辅助设施：
    //   __mkClosure(dom) —— 返回捕获 dom 的闭包（制造 JS → DOM 强引用）
    //   __alive(weakName) —— WeakRef.deref() !== undefined，即对象**仍存活**
    let helper = v8::String::new(
        scope,
        "globalThis.__mkClosure = function(dom){ return function(){ return dom; }; };\n\
         globalThis.__alive = function(n){ return globalThis[n].deref() !== undefined; };",
    )
    .unwrap();
    v8::Script::compile(scope, helper, None)
        .unwrap()
        .run(scope)
        .unwrap();

    let mut alive_after = Vec::new();

    // ---------------- EXP1 基线 ----------------
    // 每个实验必须跑在**独立 HandleScope** 里：V8 把 HandleScope 内的 Local handle
    // 也当作 GC 根，只删全局属性不足以让对象不可达。cppgc-object.rs 的
    // "Inaccessible after scope" 注释讲的正是这件事。
    println!("=== EXP1 基线：DOM 失去外部可达 ===");
    {
        v8::scope!(hs1, scope);
        let cs1 = &mut v8::ContextScope::new(hs1, context);
        let g1 = context.global(cs1);
        let (obj, _ptr) = make_dom!(cs1, "exp1");
        let k = v8::String::new(cs1, "__n1").unwrap();
        g1.set(cs1, k.into(), obj.into()).unwrap();
        let src =
            v8::String::new(cs1, "globalThis.__w1 = new WeakRef(globalThis.__n1);").unwrap();
        v8::Script::compile(cs1, src, None).unwrap().run(cs1).unwrap();
        let k = v8::String::new(cs1, "__n1").unwrap();
        g1.delete(cs1, k.into()).unwrap();
    } // hs1 在此销毁：DOM 的 Local handle 全部失效
    full_gc!(scope);
    let a1 = eval_bool!(scope, "__alive('__w1')");
    alive_after.push(("exp1", a1));
    println!("  WeakRef 判定：exp1 存活 = {a1}（false = 已被 GC 回收）");

    // ---------------- EXP2 单向 ----------------
    println!("=== EXP2 单向：DOM 强引用 JS 闭包（闭包不捕获 DOM）===");
    {
        v8::scope!(hs2, scope);
        let cs2 = &mut v8::ContextScope::new(hs2, context);
        let g2 = context.global(cs2);
        let (obj, ptr) = make_dom!(cs2, "exp2");
        // WeakRef 只观察、不保活（V8 语义如此）
        g2.set(cs2, v8::String::new(cs2, "__tmp2").unwrap().into(), obj.into())
            .unwrap();
        let src =
            v8::String::new(cs2, "globalThis.__w2 = new WeakRef(globalThis.__tmp2);").unwrap();
        v8::Script::compile(cs2, src, None).unwrap().run(cs2).unwrap();

        // 不捕获任何东西的闭包：DOM → 闭包是单向的，不构成环
        let closure = v8::Script::compile(
            cs2,
            v8::String::new(cs2, "(function(){ return 1; })").unwrap(),
            None,
        )
        .unwrap()
        .run(cs2)
        .unwrap();
        set_listener!(cs2, ptr, closure);

        // 切断外部可达
        g2.delete(cs2, v8::String::new(cs2, "__tmp2").unwrap().into())
            .unwrap();
    }
    full_gc!(scope);
    let a2 = eval_bool!(scope, "__alive('__w2')");
    alive_after.push(("exp2", a2));
    println!("  WeakRef 判定：exp2 存活 = {a2}（false = 已被 GC 回收）");

    // ---------------- EXP3 跨语言真循环 ----------------
    println!("=== EXP3 真循环：DOM → Global(闭包) → 闭包捕获 DOM ===");
    {
        v8::scope!(hs3, scope);
        let cs3 = &mut v8::ContextScope::new(hs3, context);
        let g3 = context.global(cs3);
        let (obj, ptr) = make_dom!(cs3, "exp3");
        let k_node = v8::String::new(cs3, "__node3").unwrap();
        g3.set(cs3, k_node.into(), obj.into()).unwrap();
        // WeakRef 只观察、不保活（V8 语义如此）
        let src = v8::String::new(
            cs3,
            "globalThis.__w3 = new WeakRef(globalThis.__node3);\n\
             globalThis.__closure3 = __mkClosure(globalThis.__node3);",
        )
        .unwrap();
        v8::Script::compile(cs3, src, None).unwrap().run(cs3).unwrap();
        let closure = g3
            .get(cs3, v8::String::new(cs3, "__closure3").unwrap().into())
            .unwrap();
        // DOM → JS 强引用：环闭合
        set_listener!(cs3, ptr, closure);

        // 切断全部外部可达
        g3.delete(cs3, v8::String::new(cs3, "__node3").unwrap().into())
            .unwrap();
        g3.delete(cs3, v8::String::new(cs3, "__closure3").unwrap().into())
            .unwrap();
    } // hs3 销毁
    full_gc!(scope);
    let a3 = eval_bool!(scope, "__alive('__w3')");
    alive_after.push(("exp3", a3));
    println!("  WeakRef 判定：exp3 存活 = {a3}（**true = 被跨语言环保活，即泄漏**）");

    // ---------------- 结论 ----------------
    println!("\n=== 汇总（以 WeakRef 为准）===");
    for (name, alive) in &alive_after {
        println!("  {name}: alive={alive}");
    }
    let baseline_ok = alive_after.iter().all(|(n, a)| (*n != "exp1") || !*a);
    println!("BASELINE_VALID={baseline_ok}");
    println!("CYCLE_LEAK={}", alive_after.iter().any(|(n, a)| *n == "exp3" && *a));
    println!("T2_1_PROBE_DONE");
}