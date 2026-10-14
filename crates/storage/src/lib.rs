//! storage —— 浏览器存储：local/`sessionStorage`、`IndexedDB`、`CacheStorage` 与分区
//!
//! # `G4-S1-a`（M1 存储 · 基线）
//!
//! 本笔落 **`StorageService`（origin 分区）+ `Storage` 基线接口**（`getItem`/`setItem`/
//! `removeItem`/`clear`/`key`/`length`）与**具名配额**。`S1-b`~`S1-e` 另笔见
//! `docs/design-g4-真身-2026-10-15.md` `§3`。
//!
//! ## 口径（技术方案 `§11`）
//!
//! - **统一 origin 分区**：`StorageService` 按 origin 分桶；跨 origin **不可见**（硬隔离）。
//! - **离线默认内存存储**：本 crate 默认 [`StorageService::new`] 为内存后端（可显式导入/导出
//!   快照 —— 归 `S1-b`）。
//! - **key 顺序 = 插入序**（更新**保持原位**）；`key(i)` 按该序取。
//! - **配额**：见 [`STORAGE_QUOTA_BYTES`]（**具名策略值**）；`§4.0` 口径：**外部基线 = 申报值，
//!   待 A 路社区实物对账** —— **禁止**从实现里"读回来"当期望（那构成自证）。
//! - **字符串转换**：值经 `String(...)` 归一（JS `ToString`）——**归 shim 侧**（本 crate 收 `&str`）。

#![deny(missing_docs)]

use std::collections::BTreeMap;

/// **存储配额（字节）** —— `G4-S1` 的**具名策略值**（自定基线，`测试友好`规模）。
///
/// **口径**（`§4.0`）：外部 golden 基线 = **本申报值**（**待 A 路社区实物对账**）。
pub const STORAGE_QUOTA_BYTES: usize = 5 * 1024 * 1024;

/// 存储种类（`localStorage` / `sessionStorage`）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum StorageKind {
    /// `localStorage`（跨会话存活）。
    Local,
    /// `sessionStorage`（随会话）。
    Session,
}

/// 存储操作错误（**具名**；shim 侧映射为对应 DOM 异常）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StorageError {
    /// 超出配额（shim 侧应抛 **`QuotaExceededError`**）。
    QuotaExceeded {
        /// 本配额（字节）。
        quota: usize,
        /// 本次写入后**预计占用**（字节）。
        attempted: usize,
    },
}

/// 有序键值表（**插入序**；更新保持原位；`used_bytes` = `Σ(key + value)` 字节）。
#[derive(Clone, Debug, Default)]
struct OrderedMap {
    /// `(key, value)` 依**插入序**；`key` 唯一。
    items: Vec<(String, String)>,
    /// 当前占用（`key.len() + value.len()` 之和）。
    used_bytes: usize,
}

impl OrderedMap {
    /// 按下标查键。
    fn index_of(&self, key: &str) -> Option<usize> {
        self.items.iter().position(|(k, _)| k == key)
    }

    /// 读值。
    fn get(&self, key: &str) -> Option<&str> {
        self.index_of(key).map(|i| self.items[i].1.as_str())
    }

    /// 读键（按下标）。
    fn key_at(&self, index: usize) -> Option<&str> {
        self.items.get(index).map(|(k, _)| k.as_str())
    }

    /// 写值：**新键追加、既有键原位替换**。
    fn set(&mut self, key: &str, value: &str) {
        if let Some(i) = self.index_of(key) {
            let old = self.items[i].1.len();
            // 原位复用分配（避免 `to_owned` 新分配；clippy `assigning_clones` 口径）。
            self.items[i].1.clear();
            self.items[i].1.push_str(value);
            self.used_bytes = self.used_bytes - old + value.len();
        } else {
            self.items.push((key.to_owned(), value.to_owned()));
            self.used_bytes += key.len() + value.len();
        }
    }

    /// 删键（**不改变其余键的相对顺序**）。
    fn remove(&mut self, key: &str) {
        if let Some(i) = self.index_of(key) {
            let (k, v) = self.items.remove(i);
            self.used_bytes -= k.len() + v.len();
        }
    }

    /// 清空本表。
    fn clear(&mut self) {
        self.items.clear();
        self.used_bytes = 0;
    }
}

/// **origin 分区**（一个 origin 下的 `local` / `session` 两桶）。
#[derive(Clone, Debug, Default)]
struct Partition {
    /// `localStorage` 桶。
    local: OrderedMap,
    /// `sessionStorage` 桶。
    session: OrderedMap,
}

/// **存储服务**（`§11`：`StorageService` 管 origin、session 与持久化策略）。
#[derive(Clone, Debug, Default)]
pub struct StorageService {
    /// origin ⇒ 分区（**跨 origin 硬隔离**）。
    partitions: BTreeMap<String, Partition>,
}

impl StorageService {
    /// 新建（**离线默认：内存后端**）。
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// 取某 origin 某种类的**存储句柄**。
    #[must_use]
    pub fn storage(&mut self, origin: &str, kind: StorageKind) -> Storage<'_> {
        Storage {
            origin: origin.to_owned(),
            kind,
            service: self,
        }
    }

    /// 某 origin 分区的**已用字节**（两桶合计；只读）。
    #[must_use]
    pub fn used_bytes(&self, origin: &str) -> usize {
        self.partitions
            .get(origin)
            .map_or(0, |p| p.local.used_bytes + p.session.used_bytes)
    }

    /// 桶可变引用（不存在则建）。
    fn bucket_mut(&mut self, origin: &str, kind: StorageKind) -> &mut OrderedMap {
        let partition = self.partitions.entry(origin.to_owned()).or_default();
        match kind {
            StorageKind::Local => &mut partition.local,
            StorageKind::Session => &mut partition.session,
        }
    }

    /// 桶只读引用。
    fn bucket(&self, origin: &str, kind: StorageKind) -> Option<&OrderedMap> {
        self.partitions.get(origin).map(|p| match kind {
            StorageKind::Local => &p.local,
            StorageKind::Session => &p.session,
        })
    }
}

/// **`Storage` 句柄**（绑定 `(origin, kind)`；`§11`：`localStorage`/`sessionStorage` 语义）。
///
/// 写操作**先过配额**（[`STORAGE_QUOTA_BYTES`]）；超出 ⇒ [`StorageError::QuotaExceeded`]（**不写入**）。
#[derive(Debug)]
pub struct Storage<'a> {
    /// 所属 origin（分区键）。
    origin: String,
    /// 种类。
    kind: StorageKind,
    /// 所属服务。
    service: &'a mut StorageService,
}

impl Storage<'_> {
    /// 所属 origin。
    #[must_use]
    pub fn origin(&self) -> &str {
        &self.origin
    }

    /// 种类。
    #[must_use]
    pub fn kind(&self) -> StorageKind {
        self.kind
    }

    /// `length`。
    #[must_use]
    pub fn length(&self) -> usize {
        self.service
            .bucket(&self.origin, self.kind)
            .map_or(0, |b| b.items.len())
    }

    /// `getItem(key)`（缺省 ⇒ `None`，对应 JS `null`）。
    #[must_use]
    pub fn get_item(&mut self, key: &str) -> Option<String> {
        self.service
            .bucket(&self.origin, self.kind)
            .and_then(|b| b.get(key))
            .map(str::to_owned)
    }

    /// `key(index)`（**按插入序**；越界 ⇒ `None`，对应 JS `null`）。
    #[must_use]
    pub fn key(&self, index: usize) -> Option<String> {
        self.service
            .bucket(&self.origin, self.kind)
            .and_then(|b| b.key_at(index))
            .map(str::to_owned)
    }

    /// `setItem(key, value)` —— **先算后写**（超配额 ⇒ 具名错误，**无半态**）。
    ///
    /// # Errors
    /// 超出配额 ⇒ [`StorageError::QuotaExceeded`]。
    pub fn set_item(&mut self, key: &str, value: &str) -> Result<(), StorageError> {
        let (current, old_value) = {
            let bucket = self.service.bucket(&self.origin, self.kind);
            (
                bucket.map_or(0, |b| b.used_bytes),
                bucket.and_then(|b| b.get(key)).map(str::to_owned),
            )
        };
        let old_cost = old_value.map_or(0, |v| key.len() + v.len());
        let attempted = current - old_cost + key.len() + value.len();
        if attempted > STORAGE_QUOTA_BYTES {
            return Err(StorageError::QuotaExceeded {
                quota: STORAGE_QUOTA_BYTES,
                attempted,
            });
        }
        self.service
            .bucket_mut(&self.origin, self.kind)
            .set(key, value);
        Ok(())
    }

    /// `removeItem(key)`（缺省为**空操作**，与 JS 一致）。
    pub fn remove_item(&mut self, key: &str) {
        self.service.bucket_mut(&self.origin, self.kind).remove(key);
    }

    /// `clear()`（**仅清本 `(origin, kind)` 桶**）。
    pub fn clear(&mut self) {
        self.service.bucket_mut(&self.origin, self.kind).clear();
    }
}

#[cfg(test)]
mod tests {
    use super::{STORAGE_QUOTA_BYTES, StorageError, StorageKind, StorageService};

    #[test]
    fn storage_roundtrip_and_key_order() {
        // `4.1`（前半）：**读写回环** + **key 顺序 = 插入序**（更新**保持原位**）。
        let mut service = StorageService::new();
        let mut local = service.storage("https://a.example", StorageKind::Local);
        local.set_item("b", "1").expect("set");
        local.set_item("a", "2").expect("set");
        local.set_item("c", "3").expect("set");
        local.set_item("b", "9").expect("update");

        assert_eq!(local.length(), 3);
        assert_eq!(local.get_item("b").as_deref(), Some("9"));
        assert_eq!(local.key(0).as_deref(), Some("b"), "更新应保持原位");
        assert_eq!(local.key(1).as_deref(), Some("a"));
        assert_eq!(local.key(2).as_deref(), Some("c"));
        assert_eq!(local.key(3), None, "越界 ⇒ None（JS null）");
        assert_eq!(local.get_item("missing"), None);

        local.remove_item("a");
        assert_eq!(local.length(), 2);
        assert_eq!(local.key(0).as_deref(), Some("b"));
        assert_eq!(local.key(1).as_deref(), Some("c"));
    }

    #[test]
    fn origin_partition_is_hard_isolated() {
        // `4.1` **负对照**：**不同 origin 不可见**（若共用默认分区 ⇒ 本测红）。
        let mut service = StorageService::new();
        service
            .storage("https://a.example", StorageKind::Local)
            .set_item("k", "v-a")
            .expect("set");
        let mut other = service.storage("https://b.example", StorageKind::Local);
        assert_eq!(other.get_item("k"), None, "跨 origin 必须不可见");
        assert_eq!(other.length(), 0);
        let mut back = service.storage("https://a.example", StorageKind::Local);
        assert_eq!(back.get_item("k").as_deref(), Some("v-a"));
    }

    #[test]
    fn local_and_session_buckets_are_separate() {
        // 同 origin 的 `local` 与 `session` **互不相扰**；`clear` 亦然。
        let mut service = StorageService::new();
        service
            .storage("https://a.example", StorageKind::Local)
            .set_item("k", "local-v")
            .expect("set");
        service
            .storage("https://a.example", StorageKind::Session)
            .set_item("k", "session-v")
            .expect("set");
        service
            .storage("https://a.example", StorageKind::Session)
            .clear();
        let mut local = service.storage("https://a.example", StorageKind::Local);
        assert_eq!(
            local.get_item("k").as_deref(),
            Some("local-v"),
            "clear 只清本桶"
        );
        assert_eq!(local.length(), 1);
        let session = service.storage("https://a.example", StorageKind::Session);
        assert_eq!(session.length(), 0);
    }

    #[test]
    fn quota_exceeded_on_n_plus_one_write() {
        // `4.1`（后半）：**第 N+1 次写抛具名错误**。
        // **负对照**：撤配额检查（去掉 `attempted > QUOTA` 分支）⇒ 本测红。
        let mut service = StorageService::new();
        let mut local = service.storage("https://a.example", StorageKind::Local);

        // **每项恰占 1024 B**（键 8 = `"kkkk"+4 位序号` + 值 1016）⇒ `fits = QUOTA / 1024` 精确填满。
        let key = "kkkk";
        let chunk = "x".repeat(1016);
        let fits = STORAGE_QUOTA_BYTES / 1024;
        for i in 0..fits {
            let single = format!("{key}{i:0>4}");
            local
                .set_item(&single, &chunk)
                .unwrap_or_else(|e| panic!("第 {i} 次写不应超配额（{e:?}）"));
        }
        let err = local
            .set_item("overflow", "y")
            .expect_err("第 N+1 次写必须抛具名错误");
        match err {
            StorageError::QuotaExceeded { quota, attempted } => {
                assert_eq!(quota, STORAGE_QUOTA_BYTES);
                assert!(attempted > quota, "attempted 应超配额");
            }
        }
        assert_eq!(local.get_item("overflow"), None, "超配额不得留下半态");
    }

    #[test]
    fn quota_allows_shrinking_update() {
        // 边界正例：**缩短值的原位更新** 不因重算而误判超配额。
        let mut service = StorageService::new();
        let mut local = service.storage("https://a.example", StorageKind::Local);
        let big = "y".repeat(4096);
        local.set_item("k", &big).expect("首次写");
        local.set_item("k", "small").expect("缩短值应允许");
        assert_eq!(local.get_item("k").as_deref(), Some("small"));
    }
}
