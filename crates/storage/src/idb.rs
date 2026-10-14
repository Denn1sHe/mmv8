//! **`IndexedDB` 面级契约**（`G4-S1-d` / `§4.1c` + `§4.1c′`）。
//!
//! 口径（docx `§11` + 设计 v3 `§4.1c`）：
//!
//! - **面级序列**：`open → onupgradeneeded → createObjectStore → add → onsuccess`
//!   （首次打开/版本提升触发 `upgradeneeded`；同版本重开**不**触发）；
//! - **未达成员逐员具名**：`Transaction` / `Index` / `Cursor` / `IDBKeyRange` 等
//!   **不进本面**（manifest 中逐员 `missing`，面级**不吞**成员）⇒ 一律抛
//!   **`NotSupportedError`**（真原语属铁律 6 上报，本批只做面级/行为级契约）；
//! - **事件为 pull 模型**：宿主显式 `pump` 才推进（与 `S1-c` 的 storage 事件同构，
//!   不引入隐藏的并发面）。

use std::collections::BTreeMap;

/// 未达成员的**具名错误名**（`§4.1c′`：面级不得冒充成员级）。
pub const UNSUPPORTED_ERROR: &str = "NotSupportedError";

/// 一次 `pump` 产生的事件（宿主按序派发）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IdbEvent {
    /// 需要建/改 object store（`onupgradeneeded`）。
    UpgradeNeeded {
        /// 请求 id。
        request: u64,
        /// 新版本。
        version: u32,
    },
    /// 打开成功（`onsuccess`；数据库已就绪）。
    OpenSuccess {
        /// 请求 id。
        request: u64,
        /// 数据库名。
        database: String,
    },
    /// `add` 成功（`onsuccess`；结果 = 键）。
    AddSuccess {
        /// 请求 id。
        request: u64,
        /// 实际存入的键。
        key: String,
    },
    /// 具名错误（未达成员 / 非法调用）。
    Error {
        /// 请求 id（`0` = 无关联请求）。
        request: u64,
        /// 错误名（`NotSupportedError` / `NotFoundError` / …）。
        name: &'static str,
        /// 人读消息。
        message: String,
    },
}

/// 打开请求的阶段（面级状态机）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OpenPhase {
    /// 待推进（尚未触发 `upgradeneeded`/`success`）。
    Pending,
    /// 已触发 `upgradeneeded`，等待 `createObjectStore`。
    Upgraded,
    /// 已就绪。
    Done,
}

/// 一个请求（打开 / `add`）。
#[derive(Clone, Debug)]
struct Request {
    kind: RequestKind,
}

#[derive(Clone, Debug)]
enum RequestKind {
    Open {
        name: String,
        version: u32,
        phase: OpenPhase,
    },
    Add {
        /// 所属 store 句柄（面级上仅用于排查；pump 不读）。
        #[allow(dead_code)]
        store: u64,
        key: String,
        /// 是否已产出结果。
        settled: bool,
    },
}

/// 一个 object store（面级：仅 `add`）。
#[derive(Clone, Debug, Default)]
struct Store {
    #[allow(dead_code)]
    name: String,
    entries: Vec<(String, String)>,
}

/// `IndexedDB` **面级**表面（内存后端；跨实例传导走快照，同 `S1` 存储口径）。
#[derive(Clone, Debug, Default)]
pub struct IdbSurface {
    requests: BTreeMap<u64, Request>,
    /// 库名 ⇒ 版本（已建库）。
    databases: BTreeMap<String, u32>,
    stores: BTreeMap<u64, Store>,
    next_request: u64,
    next_store: u64,
}

impl IdbSurface {
    /// 新建空表面。
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// `indexedDB.open(name, version)` ⇒ 请求 id（**尚不触发事件**，等 `pump`）。
    pub fn open(&mut self, name: &str, version: u32) -> u64 {
        self.next_request += 1;
        let id = self.next_request;
        self.requests.insert(
            id,
            Request {
                kind: RequestKind::Open {
                    name: name.to_owned(),
                    version,
                    phase: OpenPhase::Pending,
                },
            },
        );
        id
    }

    /// `db.createObjectStore(name)`（仅在 `upgradeneeded` 期间合法）⇒ store 句柄 id。
    ///
    /// # Errors
    /// 库未就绪/未在升级期 ⇒ `IdbEvent::Error`（`InvalidStateError`）。
    pub fn create_object_store(&mut self, request: u64, store: &str) -> Result<u64, IdbEvent> {
        let Some(req) = self.requests.get(&request) else {
            return Err(IdbEvent::Error {
                request,
                name: "NotFoundError",
                message: format!("请求 {request} 不存在"),
            });
        };
        let RequestKind::Open { phase, .. } = &req.kind else {
            return Err(IdbEvent::Error {
                request,
                name: "InvalidStateError",
                message: "createObjectStore 只对 open 请求合法".to_owned(),
            });
        };
        if *phase != OpenPhase::Upgraded {
            return Err(IdbEvent::Error {
                request,
                name: "InvalidStateError",
                message: "createObjectStore 只能在 onupgradeneeded 期间调用".to_owned(),
            });
        }
        self.next_store += 1;
        let id = self.next_store;
        self.stores.insert(
            id,
            Store {
                name: store.to_owned(),
                entries: Vec::new(),
            },
        );
        Ok(id)
    }

    /// `store.add(value, key?)` ⇒ 请求 id（**立即落库**；`pump` 只负责事件时序）。
    ///
    /// `key` 缺省 ⇒ 自增数字键（面级简化：不做真实 key 生成器）。
    ///
    /// # Errors
    /// store 句柄不存在 ⇒ `IdbEvent::Error { name: "NotFoundError" }`。
    pub fn add(&mut self, store: u64, value: &str, key: Option<&str>) -> Result<u64, IdbEvent> {
        if !self.stores.contains_key(&store) {
            return Err(IdbEvent::Error {
                request: 0,
                name: "NotFoundError",
                message: format!("object store {store} 不存在"),
            });
        }
        self.next_request += 1;
        let id = self.next_request;
        let key = key.map_or_else(|| format!("k{id}"), ToOwned::to_owned);
        if let Some(s) = self.stores.get_mut(&store) {
            s.entries.push((key.clone(), value.to_owned()));
        }
        self.requests.insert(
            id,
            Request {
                kind: RequestKind::Add {
                    store,
                    key,
                    settled: false,
                },
            },
        );
        Ok(id)
    }

    /// **未达成员**的具名错误（`§4.1c′`：逐员 `missing`，不吞成员）。
    #[must_use]
    pub fn unsupported(member: &str) -> IdbEvent {
        IdbEvent::Error {
            request: 0,
            name: UNSUPPORTED_ERROR,
            message: format!(
                "IndexedDB 成员 `{member}` 属未达面级（manifest 中逐员 missing；真实现须铁律 6 上报）"
            ),
        }
    }

    /// 推进**一个**请求一步（pull 模型）⇒ 产生的事件（可能多个：升级 + 就绪）。
    pub fn pump(&mut self, request: u64) -> Vec<IdbEvent> {
        let Some(req) = self.requests.get(&request).cloned() else {
            return vec![IdbEvent::Error {
                request,
                name: "NotFoundError",
                message: format!("请求 {request} 不存在"),
            }];
        };
        match req.kind {
            RequestKind::Open {
                name,
                version,
                phase,
            } => match phase {
                OpenPhase::Pending => {
                    let known = self.databases.get(&name).copied();
                    if let Some(slot) = self.requests.get_mut(&request) {
                        if let RequestKind::Open { phase, .. } = &mut slot.kind {
                            *phase = OpenPhase::Upgraded;
                        }
                    }
                    if known.is_none() || known < Some(version) {
                        // 首建 / 版本提升 ⇒ 先升级（需建 store），下一步才就绪。
                        vec![IdbEvent::UpgradeNeeded { request, version }]
                    } else {
                        // 同版本重开 ⇒ 直接就绪（**不**触发 upgradeneeded）。
                        if let Some(slot) = self.requests.get_mut(&request) {
                            if let RequestKind::Open { phase, .. } = &mut slot.kind {
                                *phase = OpenPhase::Done;
                            }
                        }
                        vec![IdbEvent::OpenSuccess {
                            request,
                            database: name,
                        }]
                    }
                }
                OpenPhase::Upgraded => {
                    self.databases.insert(name.clone(), version);
                    if let Some(slot) = self.requests.get_mut(&request) {
                        if let RequestKind::Open { phase, .. } = &mut slot.kind {
                            *phase = OpenPhase::Done;
                        }
                    }
                    vec![IdbEvent::OpenSuccess {
                        request,
                        database: name,
                    }]
                }
                OpenPhase::Done => vec![IdbEvent::Error {
                    request,
                    name: "InvalidStateError",
                    message: "请求已就绪（重复 pump）".to_owned(),
                }],
            },
            RequestKind::Add { key, settled, .. } => {
                if settled {
                    return vec![IdbEvent::Error {
                        request,
                        name: "InvalidStateError",
                        message: "add 请求已结算".to_owned(),
                    }];
                }
                if let Some(slot) = self.requests.get_mut(&request) {
                    if let RequestKind::Add { settled, .. } = &mut slot.kind {
                        *settled = true;
                    }
                }
                vec![IdbEvent::AddSuccess { request, key }]
            }
        }
    }

    /// 读某 store 的条目数（判别性观测用）。
    #[must_use]
    pub fn store_len(&self, store: u64) -> usize {
        self.stores.get(&store).map_or(0, |s| s.entries.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_sequence_is_upgrade_then_success() {
        // `§4.1c` 面级序列：首建 ⇒ pump 一次给 `upgradeneeded`（**不是**直接 success），
        // 再 pump 才 `onsuccess`（升级期结束后就绪）。
        let mut idb = IdbSurface::new();
        let req = idb.open("db", 1);
        let first = idb.pump(req);
        assert_eq!(
            first,
            vec![IdbEvent::UpgradeNeeded {
                request: req,
                version: 1
            }],
            "首建必须先 upgradeneeded（给机会建 store）"
        );
        let second = idb.pump(req);
        assert_eq!(
            second,
            vec![IdbEvent::OpenSuccess {
                request: req,
                database: "db".to_owned()
            }]
        );
    }

    #[test]
    fn reopen_same_version_skips_upgrade() {
        // 同版本重开 **不**触发 upgradeneeded（否则每次开库都白跑一次升级）。
        let mut idb = IdbSurface::new();
        let first = idb.open("db", 1);
        idb.pump(first);
        idb.pump(first);
        let again = idb.open("db", 1);
        assert_eq!(
            idb.pump(again),
            vec![IdbEvent::OpenSuccess {
                request: again,
                database: "db".to_owned()
            }],
            "同版本重开应直接就绪"
        );
    }

    #[test]
    fn create_store_only_during_upgrade() {
        // `createObjectStore` 只在 `onupgradeneeded` 期间合法（规范 InvalidStateError）。
        let mut idb = IdbSurface::new();
        let req = idb.open("db", 1);
        let too_early = idb.create_object_store(req, "s");
        assert!(
            matches!(&too_early, Err(IdbEvent::Error { name, .. }) if *name == "InvalidStateError"),
            "pump 前建 store ⇒ InvalidStateError（实得 {too_early:?}）"
        );
        idb.pump(req); // ⇒ Upgraded
        let store = idb
            .create_object_store(req, "s")
            .expect("升级期建 store 应成功");
        assert!(store > 0);
    }

    #[test]
    fn add_yields_success_with_key() {
        // `add → onsuccess(result=key)` + 落库可观测（store_len）。
        let mut idb = IdbSurface::new();
        let req = idb.open("db", 1);
        idb.pump(req);
        let store = idb.create_object_store(req, "s").expect("store");
        idb.pump(req);
        let add = idb.add(store, "v", Some("k1")).expect("add");
        assert_eq!(
            idb.pump(add),
            vec![IdbEvent::AddSuccess {
                request: add,
                key: "k1".to_owned()
            }]
        );
        assert_eq!(idb.store_len(store), 1, "add 应立即落库");
    }

    #[test]
    fn unimplemented_members_are_named_errors() {
        // `§4.1c′`：未达成员**逐员具名** `NotSupportedError`（面级不吞成员）。
        for member in [
            "IDBTransaction",
            "IDBIndex",
            "IDBCursor",
            "IDBKeyRange",
            "IDBDatabase.transaction",
            "IDBObjectStore.getAll",
            "IDBObjectStore.delete",
            "IDBObjectStore.openCursor",
        ] {
            let event = IdbSurface::unsupported(member);
            match event {
                IdbEvent::Error { name, message, .. } => {
                    assert_eq!(
                        name, "NotSupportedError",
                        "{member} 应具名 NotSupportedError"
                    );
                    assert!(message.contains(member), "消息应指名成员");
                }
                other => panic!("{member} 应产 Error（实得 {other:?}）"),
            }
        }
    }

    #[test]
    fn pumping_unknown_request_is_not_found() {
        // 负向：pump 不存在的请求 ⇒ NotFoundError（不静默）。
        let mut idb = IdbSurface::new();
        let events = idb.pump(999);
        assert!(
            matches!(&events[0], IdbEvent::Error { name, .. } if *name == "NotFoundError"),
            "实得 {events:?}"
        );
    }
}
