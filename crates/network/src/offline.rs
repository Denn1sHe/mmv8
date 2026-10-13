//! 离线资源束（`network::offline`，§9 社区版主路径 + 可重复测试）。
//!
//! 命中即**同步**解决（`FetchOutcome::Ready`）；未命中 ⇒ transport 返回
//! [`NetworkErrorKind::NotFound`](crate::error::NetworkErrorKind::NotFound) ⇒ `fetch` 转 `Pending`。

use std::collections::HashMap;

use crate::error::NetworkError;
use crate::request::RequestSpec;
use crate::response::{Response, ResponseKind};
use crate::transport::Transport;

/// 一条离线资源：`(status, headers, body)`。
type ResourceEntry = (u16, Vec<(String, String)>, Vec<u8>);

/// 预置的离线资源表：`url → (status, headers, body)`。
#[derive(Debug, Default)]
pub struct OfflineBundle {
    resources: HashMap<String, ResourceEntry>,
}

impl OfflineBundle {
    /// 建空资源束。
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// 登记一个资源。
    pub fn insert(
        &mut self,
        url: &str,
        status: u16,
        headers: Vec<(String, String)>,
        body: Vec<u8>,
    ) {
        self.resources
            .insert(url.to_owned(), (status, headers, body));
    }

    /// 查表：命中 ⇒ 一个正常响应。
    #[must_use]
    pub fn resolve(&self, url: &str) -> Option<Response> {
        self.resources
            .get(url)
            .map(|(status, headers, body)| Response {
                status: *status,
                headers: headers.clone(),
                body: body.clone(),
                kind: ResponseKind::Ok,
            })
    }
}

impl Transport for OfflineBundle {
    fn send(&mut self, spec: &RequestSpec) -> Result<Response, NetworkError> {
        self.resolve(&spec.url)
            .ok_or_else(|| NetworkError::not_found(&spec.url))
    }
}
