# 契约探针用例

每个 `*.yaml` 文件可放一条用例（对象）或一组用例（数组）。字段：

| 字段 | 必填 | 说明 |
|---|---|---|
| `id` | ✅ | 成员级唯一标识，与 `compat-manifest.json` 条目 id 对齐，如 `Window.setTimeout` |
| `category` | ✅ | 分组，如 `timer` / `dom` / `crypto` |
| `script` | ✅ | 待执行的 JS 源码片段 |
| `spec_expect` | ✅ | **规范模式**期望值（对照 WHATWG / W3C） |
| `iv8_expect` | ✅ | **iv8 兼容模式**期望值（对照社区版 v0.1.4 实际行为） |
| `severity` | | `blocker` / `critical` / `major` / `minor` / `info`，默认 `major` |
| `divergence` | | 双期望不一致时的说明，缺省自动填充 |

`spec_expect` 与 `iv8_expect` 不同**不是错误**，而是信号：说明社区版此处偏离规范，
而我们选择兼容而非修正。
