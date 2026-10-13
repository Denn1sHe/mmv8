//! 模块宿主与模块缓存（S10）。
//!
//! G1 只做**同步、内存/固定来源**的模块解析与加载（不接网络）。模块的解析/加载钩子由宿主
//! 实现 [`ModuleHost`] 提供；[`ModuleRegistry`] 保证「同一 URL 两次加载 ⇒ 同一 namespace」。

use std::collections::HashMap;

use crate::error::{JsError, JsErrorKind};

/// 模块解析/加载钩子（§7 宿主钩子：模块解析、动态 import、`import.meta`、模块缓存）。
pub trait ModuleHost {
    /// 把 specifier 解析为绝对 URL；返回 `None` 表示无法解析（抛 `TypeError`）。
    fn resolve(&self, referrer: &str, specifier: &str) -> Option<String>;

    /// 加载模块源码（同步；G1 只做内存/固定来源，不接网络）。
    ///
    /// # Errors
    /// URL 不存在时返回 [`JsErrorKind::Internal`]。
    fn load(&self, url: &str) -> Result<String, JsError>;
}

/// 内存模块宿主：`url → 源码`。用于测试与「离线生成」形态。
#[derive(Debug, Default)]
pub struct MemoryModuleHost {
    modules: HashMap<String, String>,
}

impl MemoryModuleHost {
    /// 建空宿主。
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// 登记一个模块的源码。
    pub fn insert(&mut self, url: &str, source: &str) {
        self.modules.insert(url.to_owned(), source.to_owned());
    }
}

impl ModuleHost for MemoryModuleHost {
    fn resolve(&self, referrer: &str, specifier: &str) -> Option<String> {
        // 支持 `./x.js`（相对 referrer 目录）与绝对 URL 两种简化形式。
        let candidate = if let Some(rest) = specifier.strip_prefix("./") {
            let base = referrer.rsplit_once('/').map_or("", |(dir, _)| dir);
            format!("{base}/{rest}")
        } else {
            specifier.to_owned()
        };
        self.modules.contains_key(&candidate).then_some(candidate)
    }

    fn load(&self, url: &str) -> Result<String, JsError> {
        self.modules
            .get(url)
            .cloned()
            .ok_or_else(|| JsError::new(JsErrorKind::Internal, format!("模块未找到：{url}")))
    }
}

/// 模块缓存：`URL → v8::Global<Module>`，保证同一 URL 复用同一 namespace（identity 稳定）。
///
/// 注：`v8::Global<Module>` 是 V8 类型，本结构**只在 engine-v8 内部**使用，不进任何签名。
#[derive(Default)]
pub(crate) struct ModuleRegistry {
    modules: HashMap<String, v8::Global<v8::Module>>,
}

impl ModuleRegistry {
    /// 取已缓存的模块。
    pub(crate) fn get(&self, url: &str) -> Option<&v8::Global<v8::Module>> {
        self.modules.get(url)
    }

    /// 缓存一个模块（`Global` 由调用方构造）。
    pub(crate) fn insert(&mut self, url: String, module: v8::Global<v8::Module>) {
        self.modules.insert(url, module);
    }

    /// 清空缓存（`Realm::close`）。
    pub(crate) fn clear(&mut self) {
        self.modules.clear();
    }
}
