//! redb-backed session mapping for the IM bridge.
//!
//! 每个 `(platform, chat_id)` 对应一条 NeoMind chat session，首次入站时建，
//! 后续复用；`/reset` 或管理命令可清掉重建。

use redb::{Database, ReadableTable, TableDefinition};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Arc;

const IM_SESSIONS_TABLE: TableDefinition<&str, &str> = TableDefinition::new("im_sessions");
const IM_INVITES_TABLE: TableDefinition<&str, &str> = TableDefinition::new("im_invites");
const IM_ALLOWLIST_TABLE: TableDefinition<&str, &str> = TableDefinition::new("im_allowlist");

/// 唯一定位一条 IM↔NeoMind session 映射。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionKey {
    pub platform: String,
    pub chat_id: String,
}

impl SessionKey {
    /// redb 单表 key：`<platform>:<chat_id>`。
    pub fn composite(&self) -> String {
        format!("{}:{}", self.platform, self.chat_id)
    }
}

/// 一条映射记录。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImSessionRecord {
    pub neo_session_id: String,
    pub bound_agent_id: String,
    pub alias: Option<String>,
    pub created_at: i64,
    pub last_active: i64,
}

/// 一次性邀请 token 的记录。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InviteRecord {
    pub created_at: i64,
    pub used: bool,
    pub bound_chat_id: Option<String>,
    pub bound_at: Option<i64>,
}

/// IM session 存储（redb），并发安全（`Arc<Database>` 内部可跨线程共享）。
pub struct ImSessionStore {
    db: Arc<Database>,
}

impl ImSessionStore {
    /// 在 `data_dir` 下创建/打开 `im_sessions.redb`，并确保表存在。
    pub fn open<P: AsRef<Path>>(data_dir: P) -> Result<Self, anyhow::Error> {
        let path = data_dir.as_ref().join("im_sessions.redb");
        let db = Database::create(&path)?;
        // 确保表存在（沿用 MessageStore::open / ChannelRegistry::with_storage 的模式）。
        let tx = db.begin_write()?;
        {
            tx.open_table(IM_SESSIONS_TABLE)?;
            tx.open_table(IM_INVITES_TABLE)?;
            tx.open_table(IM_ALLOWLIST_TABLE)?;
        }
        tx.commit()?;
        Ok(Self { db: Arc::new(db) })
    }

    /// 读取一条记录，不存在返回 `None`。
    pub fn get(&self, key: &SessionKey) -> Result<Option<ImSessionRecord>, anyhow::Error> {
        let tx = self.db.begin_read()?;
        let t = tx.open_table(IM_SESSIONS_TABLE)?;
        Ok(match t.get(key.composite().as_str())? {
            Some(v) => Some(serde_json::from_str(v.value())?),
            None => None,
        })
    }

    /// 不存在则用给定 `(session_id, agent_id)` 建；存在则返回现有（**不覆盖**）。返回当前记录。
    pub fn get_or_create(
        &self,
        key: &SessionKey,
        session_id: &str,
        agent_id: &str,
    ) -> Result<ImSessionRecord, anyhow::Error> {
        if let Some(r) = self.get(key)? {
            return Ok(r);
        }
        let now = now_secs();
        let rec = ImSessionRecord {
            neo_session_id: session_id.into(),
            bound_agent_id: agent_id.into(),
            alias: None,
            created_at: now,
            last_active: now,
        };
        self.put(key, &rec)?;
        Ok(rec)
    }

    /// 更新 `last_active`；不存在则空操作。
    pub fn touch(&self, key: &SessionKey) -> Result<(), anyhow::Error> {
        if let Some(mut r) = self.get(key)? {
            r.last_active = now_secs();
            self.put(key, &r)?;
        }
        Ok(())
    }

    /// 删除记录（`/reset` 等命令）；不存在算成功。
    pub fn reset(&self, key: &SessionKey) -> Result<(), anyhow::Error> {
        let tx = self.db.begin_write()?;
        {
            let mut t = tx.open_table(IM_SESSIONS_TABLE)?;
            t.remove(key.composite().as_str())?;
        }
        tx.commit()?;
        Ok(())
    }

    fn put(&self, key: &SessionKey, rec: &ImSessionRecord) -> Result<(), anyhow::Error> {
        let json = serde_json::to_string(rec)?;
        let tx = self.db.begin_write()?;
        {
            let mut t = tx.open_table(IM_SESSIONS_TABLE)?;
            t.insert(key.composite().as_str(), json.as_str())?;
        }
        tx.commit()?;
        Ok(())
    }

    /// 删除 `last_active` 早于 `cutoff` 的记录，返回删除数。
    ///
    /// 用于后台周期性清理（见 `start_im_router` 的 cleanup task）：
    /// 长期不活跃的 IM↔NeoMind 映射会被回收，下次该 chat 入站时自动重建。
    /// 空表或全新鲜记录返回 0。先收集 key 再删，避免在 iter 借用期 mutate 同表。
    pub fn evict_expired(&self, cutoff: i64) -> Result<usize, anyhow::Error> {
        let tx = self.db.begin_write()?;
        let mut removed = 0;
        {
            let mut t = tx.open_table(IM_SESSIONS_TABLE)?;
            // 先收集待删除 key（String 拷贝出借用域），避免一边 iter 一边 remove。
            let stale_keys: Vec<String> = {
                let mut out = Vec::new();
                for item in t.iter()? {
                    let (k, v) = item?;
                    let rec: ImSessionRecord = serde_json::from_str(v.value())?;
                    if rec.last_active < cutoff {
                        out.push(k.value().to_string());
                    }
                }
                out
            };
            for k in stale_keys {
                t.remove(k.as_str())?;
                removed += 1;
            }
        }
        tx.commit()?;
        Ok(removed)
    }

    /// 生成一条未使用的 invite token（32 随机字节 → base64url，无 padding）。
    pub fn create_invite(&self) -> Result<String, anyhow::Error> {
        let token = random_token();
        let rec = InviteRecord {
            created_at: now_secs(),
            used: false,
            bound_chat_id: None,
            bound_at: None,
        };
        let json = serde_json::to_string(&rec)?;
        let tx = self.db.begin_write()?;
        {
            let mut t = tx.open_table(IM_INVITES_TABLE)?;
            t.insert(token.as_str(), json.as_str())?;
        }
        tx.commit()?;
        Ok(token)
    }

    /// 原子消费：未使用 → 标记 used + 绑定 chat_id，返回 true；已用/缺失 → false。
    ///
    /// get + conditional-set 必须在同一个写事务内，否则两个并发的 `/start`
    /// 绑定可能都看到「未使用」并双绑。
    pub fn consume_invite(&self, token: &str, chat_id: &str) -> Result<bool, anyhow::Error> {
        let tx = self.db.begin_write()?;
        let consumed = {
            let mut t = tx.open_table(IM_INVITES_TABLE)?;
            // 先把 guard 里的数据拷出（释放不可变借用），再决定是否写入。
            // 读+写仍在同一个写事务内，redb 的写锁保证两个并发 bind 不会都成功。
            let existing: Option<InviteRecord> = match t.get(token)? {
                Some(v) => Some(serde_json::from_str(v.value())?),
                None => None,
            };
            match existing {
                Some(mut rec) if !rec.used => {
                    rec.used = true;
                    rec.bound_chat_id = Some(chat_id.to_string());
                    rec.bound_at = Some(now_secs());
                    let json = serde_json::to_string(&rec)?;
                    t.insert(token, json.as_str())?;
                    true
                }
                _ => false,
            }
        };
        tx.commit()?;
        Ok(consumed)
    }

    /// 列出全部 invite（`(token, record)`），无顺序保证。
    pub fn list_invites(&self) -> Result<Vec<(String, InviteRecord)>, anyhow::Error> {
        let tx = self.db.begin_read()?;
        let t = tx.open_table(IM_INVITES_TABLE)?;
        let mut out = Vec::new();
        for item in t.iter()? {
            let (k, v) = item?;
            let rec: InviteRecord = serde_json::from_str(v.value())?;
            out.push((k.value().to_string(), rec));
        }
        Ok(out)
    }

    /// 撤销 invite；不存在算成功（幂等）。
    pub fn revoke_invite(&self, token: &str) -> Result<(), anyhow::Error> {
        let tx = self.db.begin_write()?;
        {
            let mut t = tx.open_table(IM_INVITES_TABLE)?;
            t.remove(token)?;
        }
        tx.commit()?;
        Ok(())
    }

    /// 把 chat_id 加入白名单（允许新会话通过 invite 自动绑定后入站）。
    /// 重复 add 幂等（同 key 覆盖时间戳，不产生重复项）。
    pub fn allow_add(&self, chat_id: &str) -> Result<(), anyhow::Error> {
        let now = now_secs().to_string();
        let tx = self.db.begin_write()?;
        {
            let mut t = tx.open_table(IM_ALLOWLIST_TABLE)?;
            t.insert(chat_id, now.as_str())?;
        }
        tx.commit()?;
        Ok(())
    }

    /// 从白名单移除；不存在算成功（幂等）。
    pub fn allow_remove(&self, chat_id: &str) -> Result<(), anyhow::Error> {
        let tx = self.db.begin_write()?;
        {
            let mut t = tx.open_table(IM_ALLOWLIST_TABLE)?;
            t.remove(chat_id)?;
        }
        tx.commit()?;
        Ok(())
    }

    /// 列出全部白名单 chat_id（无顺序保证）。
    pub fn allow_list(&self) -> Result<Vec<String>, anyhow::Error> {
        let tx = self.db.begin_read()?;
        let t = tx.open_table(IM_ALLOWLIST_TABLE)?;
        let mut out = Vec::new();
        for item in t.iter()? {
            let (k, _v) = item?;
            out.push(k.value().to_string());
        }
        Ok(out)
    }
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

/// 生成 32 随机字节的 URL-safe base64（无 padding），用作 invite token。
/// 用 `OsRng` 拿密码学级熵，避免可预测 token 被枚举绑定他人 chat。
fn random_token() -> String {
    use base64::Engine;
    use rand::RngCore;
    let mut bytes = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn get_or_create_then_get() {
        let tmp = tempfile::tempdir().unwrap();
        let store = ImSessionStore::open(tmp.path()).unwrap();
        let key = SessionKey {
            platform: "telegram".into(),
            chat_id: "123".into(),
        };
        assert!(store.get(&key).unwrap().is_none());
        let r = store.get_or_create(&key, "sess-1", "agent-1").unwrap();
        assert_eq!(r.neo_session_id, "sess-1");
        let r2 = store.get(&key).unwrap().unwrap();
        assert_eq!(r2.neo_session_id, "sess-1"); // 复用，不新建
    }

    #[test]
    fn evict_expired_removes_stale_records() {
        let tmp = tempfile::tempdir().unwrap();
        let store = ImSessionStore::open(tmp.path()).unwrap();
        let key = SessionKey {
            platform: "telegram".into(),
            chat_id: "123".into(),
        };
        // 插入一条 last_active = now - 8 天 的过期记录。
        let now = now_secs();
        let stale = ImSessionRecord {
            neo_session_id: "s1".into(),
            bound_agent_id: "a1".into(),
            alias: None,
            created_at: now - 8 * 86400,
            last_active: now - 8 * 86400,
        };
        store.put(&key, &stale).unwrap();
        // cutoff = now - 7 天：stale (8 天前) 应被删除。
        let removed = store.evict_expired(now - 7 * 86400).unwrap();
        assert_eq!(removed, 1);
        assert!(store.get(&key).unwrap().is_none());
    }

    #[test]
    fn evict_expired_keeps_fresh_records() {
        let tmp = tempfile::tempdir().unwrap();
        let store = ImSessionStore::open(tmp.path()).unwrap();
        let key = SessionKey {
            platform: "telegram".into(),
            chat_id: "456".into(),
        };
        // 一条新鲜记录（刚刚 active）+ 一条 10 天前的记录。
        let now = now_secs();
        let fresh = ImSessionRecord {
            neo_session_id: "fresh".into(),
            bound_agent_id: "a".into(),
            alias: None,
            created_at: now,
            last_active: now,
        };
        let old_key = SessionKey {
            platform: "telegram".into(),
            chat_id: "old".into(),
        };
        let old = ImSessionRecord {
            neo_session_id: "old".into(),
            bound_agent_id: "a".into(),
            alias: None,
            created_at: now - 10 * 86400,
            last_active: now - 10 * 86400,
        };
        store.put(&key, &fresh).unwrap();
        store.put(&old_key, &old).unwrap();
        let removed = store.evict_expired(now - 7 * 86400).unwrap();
        assert_eq!(removed, 1);
        // fresh 保留，old 删除。
        assert!(store.get(&key).unwrap().is_some());
        assert!(store.get(&old_key).unwrap().is_none());
    }

    #[test]
    fn evict_expired_empty_table_returns_zero() {
        let tmp = tempfile::tempdir().unwrap();
        let store = ImSessionStore::open(tmp.path()).unwrap();
        let now = now_secs();
        let removed = store.evict_expired(now - 7 * 86400).unwrap();
        assert_eq!(removed, 0);
    }

    #[test]
    fn invite_create_consume_once_then_revoked() {
        let tmp = tempfile::tempdir().unwrap();
        let store = ImSessionStore::open(tmp.path()).unwrap();

        // create -> consume 成功返回 true，并绑定 chat_id。
        let token = store.create_invite().unwrap();
        let invites = store.list_invites().unwrap();
        assert_eq!(invites.len(), 1);
        let (_, rec) = &invites[0];
        assert!(!rec.used);
        assert!(rec.bound_chat_id.is_none());

        assert!(store.consume_invite(&token, "chat-1").unwrap());
        // 绑定后记录应反映 used=true + bound_chat_id。
        let rec = store
            .list_invites()
            .unwrap()
            .into_iter()
            .find(|(t, _)| t == &token)
            .map(|(_, r)| r)
            .unwrap();
        assert!(rec.used);
        assert_eq!(rec.bound_chat_id.as_deref(), Some("chat-1"));

        // 再次 consume 同 token 必须返回 false（已用，不能双绑）。
        assert!(!store.consume_invite(&token, "chat-2").unwrap());

        // revoke 后再 consume 仍返回 false（不存在）。
        store.revoke_invite(&token).unwrap();
        assert!(!store
            .list_invites()
            .unwrap()
            .into_iter()
            .any(|(t, _)| t == token.as_str()));
        assert!(!store.consume_invite(&token, "chat-3").unwrap());
    }

    #[test]
    fn consume_missing_invite_returns_false() {
        let tmp = tempfile::tempdir().unwrap();
        let store = ImSessionStore::open(tmp.path()).unwrap();
        // 不存在的 token consume 返回 false（不报错）。
        assert!(!store.consume_invite("nonexistent-token", "chat-x").unwrap());
    }

    #[test]
    fn revoke_missing_invite_is_ok() {
        let tmp = tempfile::tempdir().unwrap();
        let store = ImSessionStore::open(tmp.path()).unwrap();
        // 删除不存在的 invite 视为成功（幂等）。
        store.revoke_invite("ghost").unwrap();
    }

    #[test]
    fn allowlist_add_remove_list_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let store = ImSessionStore::open(tmp.path()).unwrap();

        assert!(store.allow_list().unwrap().is_empty());

        store.allow_add("chat-a").unwrap();
        store.allow_add("chat-b").unwrap();

        let mut listed = store.allow_list().unwrap();
        listed.sort();
        assert_eq!(listed, vec!["chat-a".to_string(), "chat-b".to_string()]);

        // 重复 add 幂等（不报错；列表不应出现重复项）。
        store.allow_add("chat-a").unwrap();
        let mut listed2 = store.allow_list().unwrap();
        listed2.sort();
        assert_eq!(listed2, vec!["chat-a".to_string(), "chat-b".to_string()]);

        store.allow_remove("chat-a").unwrap();
        // remove 不存在的项也幂等。
        store.allow_remove("chat-ghost").unwrap();
        let mut listed3 = store.allow_list().unwrap();
        listed3.sort();
        assert_eq!(listed3, vec!["chat-b".to_string()]);
    }
}
