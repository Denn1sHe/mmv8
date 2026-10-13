//! 响应模型（`network::response`）。

/// 响应结局。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ResponseKind {
    /// 正常响应。
    Ok,
    /// 网络错误。
    NetworkError,
    /// 被中止（导航作废）。
    Aborted,
}

/// 一次响应。
#[derive(Clone, Debug)]
pub struct Response {
    /// HTTP 状态码。
    pub status: u16,
    /// 响应头（保序）。
    pub headers: Vec<(String, String)>,
    /// 响应体。
    pub body: Vec<u8>,
    /// 结局。
    pub kind: ResponseKind,
}

impl Response {
    /// 建一个正常响应。
    #[must_use]
    pub fn ok(status: u16, headers: Vec<(String, String)>, body: Vec<u8>) -> Self {
        Self {
            status,
            headers,
            body,
            kind: ResponseKind::Ok,
        }
    }

    /// 建一个网络错误响应。
    #[must_use]
    pub fn network_error() -> Self {
        Self {
            status: 0,
            headers: Vec::new(),
            body: Vec::new(),
            kind: ResponseKind::NetworkError,
        }
    }

    /// 建一个中止响应。
    #[must_use]
    pub fn aborted() -> Self {
        Self {
            status: 0,
            headers: Vec::new(),
            body: Vec::new(),
            kind: ResponseKind::Aborted,
        }
    }
}
