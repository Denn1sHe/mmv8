//! origin 规范化（`policy::origin`，ADR-0018·D6）。
//!
//! `Origin` 由 `url` crate 解析并**规范化**：scheme/host 小写、**default-port 折叠**
//! （`https://a:443` ≡ `https://a`）、query/fragment 剥离（origin 本就不含 path/query/fragment）。
//! 非特殊 scheme（无 host，如 `data:`/`about:`）在 G2 归 **不透明 origin** ⇒ `parse` 返回 `None`。

use std::fmt;

use url::Url;

/// 一个**规范化** origin：`(scheme, host, port)`。
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Origin {
    scheme: String,
    host: String,
    port: Option<u16>,
}

impl Origin {
    /// 解析并规范化一个绝对 URL；无 host（不透明 origin）或非法 ⇒ `None`。
    #[must_use]
    pub fn parse(input: &str) -> Option<Self> {
        let url = Url::parse(input).ok()?;
        let host = url.host_str()?;
        Some(Self {
            scheme: url.scheme().to_owned(),
            host: host.to_owned(),
            port: url.port(),
        })
    }

    /// scheme（已小写）。
    #[must_use]
    pub fn scheme(&self) -> &str {
        &self.scheme
    }

    /// host（已小写）。
    #[must_use]
    pub fn host(&self) -> &str {
        &self.host
    }

    /// 显式端口（default port 已折叠为 `None`）。
    #[must_use]
    pub fn port(&self) -> Option<u16> {
        self.port
    }

    /// 是否同源（scheme + host + 端口全等）。
    #[must_use]
    pub fn is_same_origin(&self, other: &Self) -> bool {
        self == other
    }
}

impl fmt::Display for Origin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.port {
            Some(port) => write!(f, "{}://{}:{}", self.scheme, self.host, port),
            None => write!(f, "{}://{}", self.scheme, self.host),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Origin;

    #[test]
    fn origin_from_url_cases() {
        // §3.4 / S4：规范化含 default-port / 大小写 / 端口断言。
        let o = Origin::parse("https://a.example:8443/x?q=1#frag").expect("应解析");
        assert_eq!(o.scheme(), "https");
        assert_eq!(o.host(), "a.example", "host 小写、剥离 path/query/fragment");
        assert_eq!(o.port(), Some(8443), "端口断言");

        // default-port 折叠 + host 小写。
        let folded = Origin::parse("https://A.Example:443/").expect("应解析");
        assert_eq!(folded.host(), "a.example");
        assert_eq!(folded.port(), None, "443 应折叠为 None");
        assert_eq!(folded.scheme(), "https");

        let http = Origin::parse("http://a.example:80/").expect("应解析");
        assert_eq!(http.port(), None, "80 应折叠为 None");
        assert_eq!(http.scheme(), "http");

        let http_custom = Origin::parse("http://a.example:8080/").expect("应解析");
        assert_eq!(http_custom.port(), Some(8080));

        // 同源判定。
        assert!(
            Origin::parse("https://a.example/x")
                .expect("lhs")
                .is_same_origin(&Origin::parse("https://a.example:443/y").expect("rhs"))
        );
        assert!(
            !Origin::parse("https://a.example")
                .expect("lhs")
                .is_same_origin(&Origin::parse("http://a.example").expect("rhs"))
        );
        assert!(
            !Origin::parse("https://a.example:8443")
                .expect("lhs")
                .is_same_origin(&Origin::parse("https://a.example").expect("rhs"))
        );

        // 序列化。
        assert_eq!(
            Origin::parse("https://b.example:9000/")
                .expect("应解析")
                .to_string(),
            "https://b.example:9000"
        );

        // 非法 / 不透明 origin。
        assert_eq!(Origin::parse("not a url"), None);
        assert_eq!(
            Origin::parse("data:text/plain,hi"),
            None,
            "无 host ⇒ 不透明"
        );
    }
}
