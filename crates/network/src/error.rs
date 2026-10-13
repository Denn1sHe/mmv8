//! 网络层错误（`network::error`）。

use std::fmt;

use crate::request::RequestId;

/// 网络层错误类别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NetworkErrorKind {
    /// transport 无该资源（`fetch` 据此转 `Pending` 等待宿主注入）。
    NotFound,
    /// 请求所属代次已过期 / 已中止（ADR-0018·D3）。
    Stale,
    /// 未知请求 id。
    Unknown,
    /// 请求已中止。
    Aborted,
    /// 内部不变量被破坏。
    Internal,
}

/// 网络层错误（`kind` + 人类可读消息）。
#[derive(Clone, Debug)]
pub struct NetworkError {
    kind: NetworkErrorKind,
    message: String,
}

impl NetworkError {
    /// 构造错误。
    pub fn new(kind: NetworkErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    /// transport 未提供该 URL。
    #[must_use]
    pub fn not_found(url: &str) -> Self {
        Self::new(NetworkErrorKind::NotFound, format!("无可用资源：{url}"))
    }

    /// 请求已过期/中止。
    #[must_use]
    pub fn stale(id: RequestId) -> Self {
        Self::new(
            NetworkErrorKind::Stale,
            format!("请求 {} 已过期或已中止", id.get()),
        )
    }

    /// 未知请求 id。
    #[must_use]
    pub fn unknown(id: RequestId) -> Self {
        Self::new(
            NetworkErrorKind::Unknown,
            format!("未知请求 id：{}", id.get()),
        )
    }

    /// 错误类别。
    #[must_use]
    pub fn kind(&self) -> NetworkErrorKind {
        self.kind
    }

    /// 错误消息。
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for NetworkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "network error ({:?}): {}", self.kind, self.message)
    }
}

impl std::error::Error for NetworkError {}
