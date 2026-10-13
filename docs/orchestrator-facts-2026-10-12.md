# 编排者预检：关键技术事实（原文证据）

> 生成时间：2026-10-12 · 生成者：交付总监（team-lead）
> 用途：这是**从权威源码核实**的事实，不是推测。转交架构师作为方案设计的硬约束输入。
> 证据来源：本地 cargo registry 中已下载的 crate 源码（rsproxy.cn 镜像）。

---

## 证据 1 —— `v8` crate 的时区 API 在 **Isolate** 上，不在 handle 上

`~/.cargo/registry/src/rsproxy.cn-e3de039b2554c837/v8-152.2.0/src/isolate.rs` 第 2016-2031 行**原文**：

```rust
  /// Notification that the embedder has changed the time zone, daylight savings
  /// time or other date / time configuration parameters. V8 keeps a cache of
  /// various values used for date / time computation. This notification will
  /// reset those cached values for the current context so that date / time
  /// configuration changes would be reflected.
  ///
  /// This API should not be called more than needed as it will negatively impact
  /// the performance of date operations.
  #[inline(always)]
  pub fn date_time_configuration_change_notification(
    &mut self,
    time_zone_detection: TimeZoneDetection,
  ) {
    unsafe {
      v8__Isolate__DateTimeConfigurationChangeNotification(
        self.as_real_ptr(),
        time_zone_detection,
      );
    }
  }
```

**对 T2.2 的直接影响（关键）**：
- 该 API 接收的是 `TimeZoneDetection`（枚举：自动探测 / 不探测），**不接受一个「指定时区」的参数**。
- 它是 **Isolate 级**，不是 Context 级、也不是 per-call。
- V8 源码注释明说「V8 keeps a **cache** of various values used for date/time computation」——缓存是 V8 内部的。

⇒ 因此 T2.2 要证伪/证实的命题非常明确：**同一 Isolate 内、不同 Context 之间，时区/时钟是否可隔离**。
`grep -rn "set_date_time\|DateTimeConfiguration\|timezone" $V8/src/` 在 `handle.rs` 上**零命中**，
说明**没有任何 per-Context 的时区设置入口**。

**推论（需架构师确认或推翻）**：
「多个 Profile 共用一个 Isolate、只切 Context 来换时区」这条路**很可能走不通**；
可行方向是「一个 Profile 一个 Isolate」。这是待验证假设，**不是结论**。

## 证据 2 —— 微任务与后台任务 API（T2.5 相关）

同文件原文（`isolate.rs`）：
- 第 1970 行 `pub fn perform_microtask_checkpoint(&mut self)`
- 第 1976 行 `pub fn enqueue_microtask(&mut self, microtask: Local<Function>)`
- 第 2038 行 `pub fn has_pending_background_tasks(&self) -> bool`
- 第 1066 行 `pub fn terminate_execution(&self) -> bool` / 第 1073 行 `cancel_terminate_execution`

## 证据 3 —— Inspector API **完整可用**（T2.5 可行性的强支撑）

`~/.cargo/registry/src/rsproxy.cn-e3de039b2554c837/v8-152.2.0/src/inspector.rs` 的 `pub fn` 实测存在：

| 行号 | API | 用途（对 T2.5） |
|---|---|---|
| 406 | `V8Inspector::new(imp: Box<dyn ChannelImpl>)` | 建Inspector，需实现 Channel 回调 |
| 494 | `V8InspectorSession::new()` | 建会话 |
| 528 | `run_message_loop_on_pause(context_group_id)` | **暂停后驱动消息泵** ← T2.5 核心 |
| 537 | `quit_message_loop_on_pause()` | 退出暂停循环 |
| 543 | `run_if_waiting_for_debugger(context_group_id)` | 等待调试器 |
| 806 | `dispatch_protocol_message(&self, message: StringView)` | 收 CDP 指令 |
| 426 | `send_response(&self, call_id: i32, message: UniquePtr<StringBuffer>)` | 回 CDP 应答 |
| 435 | `send_notification(&self, message: UniquePtr<StringBuffer>)` | 主动推事件 |
| 908 | `schedule_pause_on_next_statement(...)` | 制造暂停点 |
| 924 | `cancel_pause_on_next_statement()` | 取消暂停 |

⇒ T2.5 所需的三件套（**制造暂停 → 跑消息泵 → 退出**）在 `v8` 152.2.0 上**均有绑定**。
注意 `ChannelImpl` / `V8InspectorClientImpl` 是 trait，需在探针里实现。

## 证据 4 —— T2.4 依赖本地已有源码（无需联网，可直接编译验证）

已下载到本地 registry：
```
blitz-dom-0.3.0-beta.2        ← 含 blitz-traits-0.3.0-beta.2
stylo_dom-0.20.0              ← 含 stylo-0.20.0 / stylo_atoms / stylo_taffy-0.3.0-beta.2 /
                                  stylo_traits / stylo_derive / stylo_static_prefs /
                                  stylo_malloc_size_of
```
⇒ `blitz-dom 0.3.0-beta.2` 及其传递依赖**全部已在本地缓存**，T2.4 探针可以离线编译。
**但**：`blitz-dom` 是 **beta 版**，其 `linux x86_64` 可用性**未验证**——这是 T2.4 必须回答的问题之一。

---

## 环境事实（远程 Linux x86_64 验证机）

```
$ ssh ubuntu@124.221.1.78
Mem: 3659 MB total, 2993 MB available      ← 内存是紧约束
/dev/vda2: 69G total, 54G available
rustc 1.99.0 (b940084d7 2026-09-28)
cargo 1.99.0 (5f94df478 2026-08-27)
~/mmv8/poc/ = README.md t2_0_v8_embed t2_1_cppgc_cycle t2_3_html5ever_pause
```

⚠️ **风险**：T2.2 与 T2.5 都要链接 V8（静态库链接期吃内存），远程仅 3.6 GB。
按项目既有对策顺序：`cargo build -j2` → 若仍 OOM 则走决策上报（AI **不许自行改远程系统配置**）。

⚠️ **远程长命令坑**：前台 ssh 长命令会被 SIGTERM（exit 137）。须 `nohup ... &` + 轮询日志。
