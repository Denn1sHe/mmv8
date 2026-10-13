//! 可插拔传输（`network::transport`）。

use std::fmt;

use crate::error::NetworkError;
use crate::request::RequestSpec;
use crate::response::Response;

/// 传输后端：把 [`RequestSpec`] 变成 [`Response`]。
///
/// G2 主路径是 [`crate::offline::OfflineBundle`]（同步查表）；在线传输留 G6。
pub trait Transport: fmt::Debug {
    /// 发送请求。
    ///
    /// # Errors
    /// - [`NetworkErrorKind::NotFound`](crate::error::NetworkErrorKind::NotFound)：无该资源
    ///   （`NetworkService::fetch` 据此转 `Pending`，等待宿主注入）。
    /// - 其他网络错误。
    fn send(&mut self, spec: &RequestSpec) -> Result<Response, NetworkError>;
}
