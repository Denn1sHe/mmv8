# PoC 探针目录

技术方案 §18 的 **T2 五项阻断性 PoC** 的可执行证据。它们回答的是
「这条路到底走不走得通」，而不是「功能实现了没有」。

## 为什么不放进主 workspace

见 `docs/adr/0008-T2探针落盘与poc-unsafe白名单.md`：每个探针是**独立 Cargo
workspace**（`Cargo.toml` 顶部有空 `[workspace]`），不占主 workspace 的 19 个成员名额，
也不会把 `v8` 这类重依赖拖进每次门禁（远程验证机只有 3.6 GB 内存）。

## 运行

```bash
cd poc/t2_0_v8_embed        && cargo run   # V8 嵌入前置（T2.1/2/5 的共同依赖）
cd poc/t2_1_cppgc_cycle     && cargo run   # T2.1 跨语言循环回收
cd poc/t2_3_html5ever_pause && cargo run   # T2.3 脚本暂停/恢复 + document.write
```

首次运行会下载 `v8` 的预编译静态库（约 40 MB）。本地与远程均已验证可跑。

## 当前状态

| 探针 | 对应 | 状态 | 结论落档 |
|---|---|---|---|
| `t2_0_v8_embed` | T2.1/2/5 前置 | ✅ 通过 | （尚未单独成 ADR，见 ADR-0007 附表） |
| `t2_1_cppgc_cycle` | T2.1 | ✅ 通过 | `docs/adr/0007-T2.1-cppgc跨语言环无法回收.md` |
| `t2_3_html5ever_pause` | T2.3 | ✅ 通过（5/5 断言） | `docs/adr/0006-T2.3-html5ever-脚本暂停与document-write插入点.md` |

尚未开始：T2.2（逻辑 Date 与 Profile 时钟隔离）、T2.4（Blitz / Stylo 集成）、
T2.5（Inspector 暂停消息泵与 Python 回调死锁边界）。

## unsafe 与铁律 4

`poc/` 在 `scripts/check-unsafe-whitelist.sh` 的白名单里，与 `crates/engine-v8/`
**同等约束**：每个 `unsafe` 必须在**前 2 行内**有 `SAFETY:` 标记，
禁止 crate 级与 mod/impl 级开口。白名单外的目录一律禁止 unsafe。