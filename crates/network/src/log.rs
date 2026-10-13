//! netLog 条目（`network::log`，ADR-0017·D7）。

use policy::NavigationEpoch;

/// netLog 阶段。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NetLogPhase {
    /// 请求已发出。
    RequestSent,
    /// 响应已收到。
    ResponseReceived,
    /// 失败/中止。
    Failed,
}

/// 一条 netLog 记录。
#[derive(Clone, Debug)]
pub struct NetLogEntry {
    /// 目标 URL。
    pub url: String,
    /// HTTP 状态码（尚未收到时为 `None`）。
    pub status: Option<u16>,
    /// 阶段。
    pub phase: NetLogPhase,
    /// 关联导航代次。
    pub epoch: NavigationEpoch,
}
