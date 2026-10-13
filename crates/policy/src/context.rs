//! 安全上下文与页面策略最小面（`policy::context`）。

use crate::origin::Origin;

/// 一个文档的安全上下文（origin + 是否安全上下文）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SecurityContext {
    origin: Origin,
    secure: bool,
}

impl SecurityContext {
    /// 由 origin 与「是否安全上下文」构造。
    #[must_use]
    pub fn new(origin: Origin, secure: bool) -> Self {
        Self { origin, secure }
    }

    /// origin。
    #[must_use]
    pub fn origin(&self) -> &Origin {
        &self.origin
    }

    /// 是否安全上下文（如 `https`）。
    #[must_use]
    pub fn is_secure(&self) -> bool {
        self.secure
    }

    /// 与另一安全上下文是否同源。
    #[must_use]
    pub fn is_same_origin(&self, other: &Self) -> bool {
        self.origin.is_same_origin(&other.origin)
    }
}

/// 页面策略最小面（G2：两个开关，默认宽松）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PagePolicy {
    allow_scripts: bool,
    allow_same_origin_fetch: bool,
}

impl Default for PagePolicy {
    fn default() -> Self {
        Self {
            allow_scripts: true,
            allow_same_origin_fetch: true,
        }
    }
}

impl PagePolicy {
    /// 默认策略（脚本与同源 fetch 均允许）。
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// 是否允许脚本执行。
    #[must_use]
    pub fn allow_scripts(&self) -> bool {
        self.allow_scripts
    }

    /// 设置是否允许脚本执行。
    pub fn set_allow_scripts(&mut self, allow: bool) {
        self.allow_scripts = allow;
    }

    /// 是否允许同源 fetch。
    #[must_use]
    pub fn allow_same_origin_fetch(&self) -> bool {
        self.allow_same_origin_fetch
    }

    /// 设置是否允许同源 fetch。
    pub fn set_allow_same_origin_fetch(&mut self, allow: bool) {
        self.allow_same_origin_fetch = allow;
    }
}

#[cfg(test)]
mod tests {
    use super::{PagePolicy, SecurityContext};
    use crate::origin::Origin;

    #[test]
    fn security_context_and_policy() {
        let origin = Origin::parse("https://a.example/").expect("origin");
        let ctx = SecurityContext::new(origin.clone(), true);
        assert!(ctx.is_secure());
        assert_eq!(ctx.origin(), &origin);
        let other = SecurityContext::new(Origin::parse("https://a.example:443/").expect("o"), true);
        assert!(ctx.is_same_origin(&other));

        let mut policy = PagePolicy::new();
        assert!(policy.allow_scripts() && policy.allow_same_origin_fetch());
        policy.set_allow_scripts(false);
        assert!(!policy.allow_scripts());
        assert_eq!(PagePolicy::default(), {
            let mut p = PagePolicy::new();
            p.set_allow_same_origin_fetch(true);
            p
        });
    }
}
