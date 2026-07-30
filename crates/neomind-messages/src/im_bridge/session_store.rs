//! redb-backed session mapping for the IM bridge.
//!
//! 每个 `(platform, chat_id)` 对应一条 NeoMind chat session，首次入站时建，
//! 后续复用；`/reset` 或管理命令可清掉重建。

use redb::{Database, TableDefinition};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Arc;

const IM_SESSIONS_TABLE: TableDefinition<&str, &str> = TableDefinition::new("im_sessions");

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
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn get_or_create_then_get() {
        let tmp = tempfile::tempdir().unwrap();
        let store = ImSessionStore::open(tmp.path()).unwrap();
        let key = SessionKey { platform: "telegram".into(), chat_id: "123".into() };
        assert!(store.get(&key).unwrap().is_none());
        let r = store.get_or_create(&key, "sess-1", "agent-1").unwrap();
        assert_eq!(r.neo_session_id, "sess-1");
        let r2 = store.get(&key).unwrap().unwrap();
        assert_eq!(r2.neo_session_id, "sess-1"); // 复用，不新建
    }
}
