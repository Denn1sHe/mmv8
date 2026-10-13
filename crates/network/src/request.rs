//! 请求模型（`network::request`）。

use policy::NavigationEpoch;

/// 请求标识。
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct RequestId(u64);

impl RequestId {
    /// 由原始 `u64` 构造。
    #[must_use]
    pub const fn new(raw: u64) -> Self {
        Self(raw)
    }

    /// 取原始 `u64`。
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// 请求凭据模式（ADR-0017·D7）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Credentials {
    /// 不发送凭据。
    Omit,
    /// 仅同源发送。
    SameOrigin,
    /// 总是发送。
    Include,
}

/// 一次请求的描述。
#[derive(Clone, Debug)]
pub struct RequestSpec {
    /// 请求 id。
    pub id: RequestId,
    /// 绝对 URL。
    pub url: String,
    /// HTTP 方法。
    pub method: String,
    /// 请求头（**保序** —— 多个 `Set-Cookie` 场景）。
    pub headers: Vec<(String, String)>,
    /// 请求体。
    pub body: Option<Vec<u8>>,
    /// 关联**导航代次**（类型归 `policy` —— ADR-0017·D1）。
    pub epoch: NavigationEpoch,
    /// 凭据模式。
    pub credentials: Credentials,
}

impl RequestSpec {
    /// 建一个简单 `GET` 请求（无头、无体、`SameOrigin`）。
    #[must_use]
    pub fn simple(id: RequestId, url: &str, epoch: NavigationEpoch) -> Self {
        Self {
            id,
            url: url.to_owned(),
            method: "GET".to_owned(),
            headers: Vec::new(),
            body: None,
            epoch,
            credentials: Credentials::SameOrigin,
        }
    }
}
