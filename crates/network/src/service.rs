//! 网络服务（`network::service`，ADR-0018·D3）。
//!
//! **本 crate 零 `engine-v8` 依赖**：`res`/`rej` 的 `HandleLease` 表在 `navigation` 侧，不在本处。
//! `complete` 只标记状态并记录 netLog；**入队 resolve 任务**由 `navigation` 完成。

use std::collections::BTreeMap;

use policy::NavigationEpoch;

use crate::error::{NetworkError, NetworkErrorKind};
use crate::log::{NetLogEntry, NetLogPhase};
use crate::request::{RequestId, RequestSpec};
use crate::response::Response;
use crate::transport::Transport;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum PendingState {
    Pending,
    Completed,
    Aborted,
}

#[derive(Debug)]
struct PendingRequest {
    state: PendingState,
    epoch: NavigationEpoch,
    url: String,
}

/// `fetch` 的即时结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FetchOutcome {
    /// 命中（就地可解）。
    Ready(RequestId),
    /// 未命中，待宿主 `complete`（走 `TaskSource::Network`）。
    Pending(RequestId),
}

/// 网络服务：请求登记 + `Transport` + netLog。
#[derive(Debug)]
pub struct NetworkService {
    pending: BTreeMap<RequestId, PendingRequest>,
    log: Vec<NetLogEntry>,
    transport: Box<dyn Transport>,
    next_id: u64,
}

impl NetworkService {
    /// 用给定传输建服务。
    #[must_use]
    pub fn new(transport: Box<dyn Transport>) -> Self {
        Self {
            pending: BTreeMap::new(),
            log: Vec::new(),
            transport,
            next_id: 0,
        }
    }

    /// 分配一个请求 id。
    pub fn next_request_id(&mut self) -> RequestId {
        let id = RequestId::new(self.next_id);
        self.next_id += 1;
        id
    }

    /// 发起请求：命中 ⇒ `Ready`；transport `NotFound` ⇒ `Pending`；其他错误 ⇒ 上抛。
    ///
    /// # Errors
    /// transport 返回非 `NotFound` 的错误时透传。
    #[allow(clippy::needless_pass_by_value)] // 设计契约签名（§1.2）按值收 `RequestSpec`。
    pub fn fetch(&mut self, spec: RequestSpec) -> Result<FetchOutcome, NetworkError> {
        let id = spec.id;
        let epoch = spec.epoch;
        let url = spec.url.clone();
        self.log.push(NetLogEntry {
            url: url.clone(),
            status: None,
            phase: NetLogPhase::RequestSent,
            epoch,
        });
        match self.transport.send(&spec) {
            Ok(response) => {
                self.log.push(NetLogEntry {
                    url: url.clone(),
                    status: Some(response.status),
                    phase: NetLogPhase::ResponseReceived,
                    epoch,
                });
                self.pending.insert(
                    id,
                    PendingRequest {
                        state: PendingState::Completed,
                        epoch,
                        url,
                    },
                );
                Ok(FetchOutcome::Ready(id))
            }
            Err(err) if err.kind() == NetworkErrorKind::NotFound => {
                self.pending.insert(
                    id,
                    PendingRequest {
                        state: PendingState::Pending,
                        epoch,
                        url,
                    },
                );
                Ok(FetchOutcome::Pending(id))
            }
            Err(err) => {
                self.log.push(NetLogEntry {
                    url,
                    status: None,
                    phase: NetLogPhase::Failed,
                    epoch,
                });
                Err(err)
            }
        }
    }

    /// 待完成请求 id（按 id 序）。
    #[must_use]
    pub fn pending(&self) -> Vec<RequestId> {
        self.pending
            .iter()
            .filter(|(_, request)| request.state == PendingState::Pending)
            .map(|(id, _)| *id)
            .collect()
    }

    /// 回填响应（ADR-0018·D3）。
    ///
    /// 语义：pending 且未中止 ⇒ `Ok`（标记完成并记 netLog）；**同一 id 已完成再次 `complete`
    /// ⇒ 幂等 `Ok`（no-op）**；**已 abort ⇒ `Err(Stale)`**（不写入）；未知 id ⇒ `Err(Unknown)`。
    ///
    /// # Errors
    /// 已中止 ⇒ [`NetworkErrorKind::Stale`](crate::error::NetworkErrorKind::Stale)；
    /// 未知 id ⇒ [`NetworkErrorKind::Unknown`](crate::error::NetworkErrorKind::Unknown)。
    #[allow(clippy::needless_pass_by_value)] // 设计契约签名（§1.2）按值收 `Response`。
    pub fn complete(&mut self, id: RequestId, response: Response) -> Result<(), NetworkError> {
        let (epoch, url) = match self.pending.get(&id) {
            None => return Err(NetworkError::unknown(id)),
            Some(request) => match request.state {
                PendingState::Completed => return Ok(()),
                PendingState::Aborted => return Err(NetworkError::stale(id)),
                PendingState::Pending => (request.epoch, request.url.clone()),
            },
        };
        if let Some(request) = self.pending.get_mut(&id) {
            request.state = PendingState::Completed;
        }
        self.log.push(NetLogEntry {
            url,
            status: Some(response.status),
            phase: NetLogPhase::ResponseReceived,
            epoch,
        });
        Ok(())
    }

    /// 作废某代次的 pending 请求（不回写新文档；`res`/`rej` 的释放由 `navigation` 完成）。
    pub fn abort_epoch(&mut self, epoch: NavigationEpoch) {
        let mut aborted: Vec<(String, NavigationEpoch)> = Vec::new();
        for request in self.pending.values_mut() {
            if request.epoch == epoch && request.state == PendingState::Pending {
                request.state = PendingState::Aborted;
                aborted.push((request.url.clone(), request.epoch));
            }
        }
        for (url, epoch) in aborted {
            self.log.push(NetLogEntry {
                url,
                status: None,
                phase: NetLogPhase::Failed,
                epoch,
            });
        }
    }

    /// 导出 netLog（副本）。
    #[must_use]
    pub fn export_log(&self) -> Vec<NetLogEntry> {
        self.log.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::{FetchOutcome, NetworkService};
    use crate::error::NetworkErrorKind;
    use crate::log::NetLogPhase;
    use crate::offline::OfflineBundle;
    use crate::request::{RequestId, RequestSpec};
    use crate::response::Response;
    use policy::NavigationEpoch;

    fn service() -> NetworkService {
        let mut bundle = OfflineBundle::new();
        bundle.insert("https://a.test/hit.js", 200, vec![], b"ok".to_vec());
        NetworkService::new(Box::new(bundle))
    }

    #[test]
    fn offline_hit_miss() {
        // §3.5：命中 ⇒ Ready；未命中 ⇒ Pending。
        let mut net = service();
        let epoch = NavigationEpoch::initial();
        let hit = net
            .fetch(RequestSpec::simple(
                RequestId::new(1),
                "https://a.test/hit.js",
                epoch,
            ))
            .expect("fetch hit");
        assert_eq!(hit, FetchOutcome::Ready(RequestId::new(1)));
        assert!(net.pending().is_empty(), "命中不进 pending");

        let miss = net
            .fetch(RequestSpec::simple(
                RequestId::new(2),
                "https://a.test/miss.js",
                epoch,
            ))
            .expect("fetch miss");
        assert_eq!(miss, FetchOutcome::Pending(RequestId::new(2)));
        assert_eq!(net.pending(), vec![RequestId::new(2)]);
    }

    #[test]
    fn complete_resolves_and_clears_pending() {
        let mut net = service();
        let epoch = NavigationEpoch::initial();
        net.fetch(RequestSpec::simple(
            RequestId::new(1),
            "https://a.test/x.js",
            epoch,
        ))
        .expect("fetch");
        net.complete(
            RequestId::new(1),
            Response::ok(200, vec![], b"body".to_vec()),
        )
        .expect("complete");
        assert!(net.pending().is_empty(), "完成后应清出 pending");
    }

    #[test]
    fn complete_is_idempotent_and_stale_rejected() {
        // §3.5：同 id 重复 complete ⇒ Ok（no-op）；已 abort ⇒ Err(Stale)。
        let mut net = service();
        let epoch = NavigationEpoch::initial();
        net.fetch(RequestSpec::simple(
            RequestId::new(1),
            "https://a.test/x.js",
            epoch,
        ))
        .expect("fetch");
        net.complete(RequestId::new(1), Response::ok(200, vec![], Vec::new()))
            .expect("首次 complete");
        net.complete(RequestId::new(1), Response::ok(200, vec![], Vec::new()))
            .expect("重复 complete 应幂等 Ok");

        net.fetch(RequestSpec::simple(
            RequestId::new(2),
            "https://a.test/y.js",
            epoch,
        ))
        .expect("fetch");
        net.abort_epoch(epoch);
        let err = net
            .complete(RequestId::new(2), Response::ok(200, vec![], Vec::new()))
            .expect_err("已中止 ⇒ Stale");
        assert_eq!(err.kind(), NetworkErrorKind::Stale);

        // 未知 id。
        let err = net
            .complete(RequestId::new(99), Response::ok(200, vec![], Vec::new()))
            .expect_err("未知 id");
        assert_eq!(err.kind(), NetworkErrorKind::Unknown);
    }

    #[test]
    fn abort_epoch_only_aborts_matching_epoch() {
        let mut net = service();
        let e0 = NavigationEpoch::initial();
        let e1 = e0.next();
        net.fetch(RequestSpec::simple(
            RequestId::new(1),
            "https://a.test/old.js",
            e0,
        ))
        .expect("fetch old");
        net.fetch(RequestSpec::simple(
            RequestId::new(2),
            "https://a.test/new.js",
            e1,
        ))
        .expect("fetch new");
        net.abort_epoch(e0);
        assert_eq!(net.pending(), vec![RequestId::new(2)], "仅旧代次被作废");
        assert_eq!(
            net.complete(RequestId::new(1), Response::ok(200, vec![], Vec::new()))
                .expect_err("旧代次 Stale")
                .kind(),
            NetworkErrorKind::Stale
        );
        net.complete(RequestId::new(2), Response::ok(200, vec![], Vec::new()))
            .expect("新代次可完成");
    }

    #[test]
    fn netlog_records_request_and_response() {
        // §3.5：export_log 含发起 + 完成两条（带 url/status）。
        let mut net = service();
        let epoch = NavigationEpoch::initial();
        net.fetch(RequestSpec::simple(
            RequestId::new(1),
            "https://a.test/hit.js",
            epoch,
        ))
        .expect("hit");
        let log = net.export_log();
        assert_eq!(log.len(), 2);
        assert_eq!(log[0].phase, NetLogPhase::RequestSent);
        assert_eq!(log[1].phase, NetLogPhase::ResponseReceived);
        assert_eq!(log[1].status, Some(200));
        assert_eq!(log[1].url, "https://a.test/hit.js");

        net.fetch(RequestSpec::simple(
            RequestId::new(2),
            "https://a.test/miss.js",
            epoch,
        ))
        .expect("miss");
        net.complete(RequestId::new(2), Response::ok(201, vec![], Vec::new()))
            .expect("complete");
        let log = net.export_log();
        assert_eq!(log.len(), 4);
        assert_eq!(log[3].status, Some(201));
    }

    #[test]
    fn next_request_id_monotonic() {
        let mut net = service();
        assert_eq!(net.next_request_id(), RequestId::new(0));
        assert_eq!(net.next_request_id(), RequestId::new(1));
    }
}
