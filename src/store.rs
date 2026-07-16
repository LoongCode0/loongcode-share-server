use anyhow::Result;
use rusqlite::{Connection, OptionalExtension};

pub struct ShareRow {
    pub device_id: String,
    pub share_id: String,
    pub workspace_name: String,
    pub task_title: String,
    pub payload_json: String,
    pub message_count: i64,
    pub delete_token_hash: String,
    pub created_at: i64,
    pub expires_at: i64,
}

pub fn open(path: &std::path::Path) -> Result<Connection> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let conn = Connection::open(path)?;
    migrate(&conn)?;
    Ok(conn)
}

/// 仅测试使用：生产路径走 open(路径)。
#[cfg(test)]
pub fn open_in_memory() -> Result<Connection> {
    let conn = Connection::open_in_memory()?;
    migrate(&conn)?;
    Ok(conn)
}

fn migrate(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS shares (
            device_id TEXT NOT NULL,
            share_id TEXT NOT NULL,
            workspace_name TEXT NOT NULL,
            task_title TEXT NOT NULL,
            payload_json TEXT NOT NULL,
            message_count INTEGER NOT NULL,
            delete_token_hash TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            expires_at INTEGER NOT NULL,
            PRIMARY KEY (device_id, share_id)
        );
        CREATE INDEX IF NOT EXISTS idx_shares_expires ON shares(expires_at);",
    )?;
    Ok(())
}

/// INSERT OR IGNORE；false = (device_id, share_id) 冲突，调用方换随机段重试。
pub fn insert_share(conn: &Connection, row: &ShareRow) -> Result<bool> {
    let n = conn.execute(
        "INSERT OR IGNORE INTO shares (device_id, share_id, workspace_name, task_title,
             payload_json, message_count, delete_token_hash, created_at, expires_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        rusqlite::params![
            row.device_id, row.share_id, row.workspace_name, row.task_title,
            row.payload_json, row.message_count, row.delete_token_hash,
            row.created_at, row.expires_at
        ],
    )?;
    Ok(n == 1)
}

/// 只返回未过期行（expires_at > now）；过期与不存在同为 None——统一 404 的数据层基础。
pub fn get_share(conn: &Connection, device_id: &str, share_id: &str, now: i64) -> Result<Option<ShareRow>> {
    conn.query_row(
        "SELECT device_id, share_id, workspace_name, task_title, payload_json,
                message_count, delete_token_hash, created_at, expires_at
         FROM shares WHERE device_id = ?1 AND share_id = ?2 AND expires_at > ?3",
        rusqlite::params![device_id, share_id, now],
        |r| {
            Ok(ShareRow {
                device_id: r.get(0)?,
                share_id: r.get(1)?,
                workspace_name: r.get(2)?,
                task_title: r.get(3)?,
                payload_json: r.get(4)?,
                message_count: r.get(5)?,
                delete_token_hash: r.get(6)?,
                created_at: r.get(7)?,
                expires_at: r.get(8)?,
            })
        },
    )
    .optional()
    .map_err(Into::into)
}

/// 凭 token 散列删除；false = token 不符 / 不存在 / 已删。
pub fn delete_share(conn: &Connection, device_id: &str, share_id: &str, token_hash: &str) -> Result<bool> {
    let n = conn.execute(
        "DELETE FROM shares WHERE device_id = ?1 AND share_id = ?2 AND delete_token_hash = ?3",
        rusqlite::params![device_id, share_id, token_hash],
    )?;
    Ok(n == 1)
}

/// 该设备 since 之后创建的分享数（设备日限流数据源）。
pub fn count_since(conn: &Connection, device_id: &str, since: i64) -> Result<i64> {
    conn.query_row(
        "SELECT COUNT(*) FROM shares WHERE device_id = ?1 AND created_at > ?2",
        rusqlite::params![device_id, since],
        |r| r.get(0),
    )
    .map_err(Into::into)
}

/// 删过期行，返回删除数（cleanup 周期任务用）。
pub fn delete_expired(conn: &Connection, now: i64) -> Result<usize> {
    let n = conn.execute("DELETE FROM shares WHERE expires_at <= ?1", rusqlite::params![now])?;
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(device: &str, share: &str, expires_at: i64) -> ShareRow {
        ShareRow {
            device_id: device.into(),
            share_id: share.into(),
            workspace_name: "ws".into(),
            task_title: "标题".into(),
            payload_json: r#"[{"role":"user","text":"你好"}]"#.into(),
            message_count: 1,
            delete_token_hash: "hash0".into(),
            created_at: 1_000,
            expires_at,
        }
    }

    #[test]
    fn insert_then_get_roundtrip() {
        let c = open_in_memory().unwrap();
        assert!(insert_share(&c, &row("abcdef0123456789", "AbCdEfGhIjKl", 2_000)).unwrap());
        let got = get_share(&c, "abcdef0123456789", "AbCdEfGhIjKl", 1_500).unwrap().unwrap();
        assert_eq!(got.task_title, "标题");
        assert_eq!(got.message_count, 1);
    }

    #[test]
    fn expired_or_missing_both_none() {
        let c = open_in_memory().unwrap();
        insert_share(&c, &row("abcdef0123456789", "AbCdEfGhIjKl", 2_000)).unwrap();
        assert!(get_share(&c, "abcdef0123456789", "AbCdEfGhIjKl", 2_000).unwrap().is_none(), "过期(=now)应 None");
        assert!(get_share(&c, "abcdef0123456789", "ZzZzZzZzZzZz", 1_500).unwrap().is_none(), "不存在应 None");
    }

    #[test]
    fn insert_conflict_returns_false() {
        let c = open_in_memory().unwrap();
        assert!(insert_share(&c, &row("abcdef0123456789", "AbCdEfGhIjKl", 2_000)).unwrap());
        assert!(!insert_share(&c, &row("abcdef0123456789", "AbCdEfGhIjKl", 3_000)).unwrap());
    }

    #[test]
    fn delete_requires_matching_token_hash() {
        let c = open_in_memory().unwrap();
        insert_share(&c, &row("abcdef0123456789", "AbCdEfGhIjKl", 2_000)).unwrap();
        assert!(!delete_share(&c, "abcdef0123456789", "AbCdEfGhIjKl", "wrong").unwrap());
        assert!(delete_share(&c, "abcdef0123456789", "AbCdEfGhIjKl", "hash0").unwrap());
        assert!(!delete_share(&c, "abcdef0123456789", "AbCdEfGhIjKl", "hash0").unwrap(), "重复删除 false");
    }

    #[test]
    fn count_since_and_delete_expired() {
        let c = open_in_memory().unwrap();
        insert_share(&c, &row("abcdef0123456789", "AbCdEfGhIjK1", 2_000)).unwrap();
        insert_share(&c, &row("abcdef0123456789", "AbCdEfGhIjK2", 9_000)).unwrap();
        assert_eq!(count_since(&c, "abcdef0123456789", 500).unwrap(), 2);
        assert_eq!(count_since(&c, "abcdef0123456789", 1_500).unwrap(), 0, "created_at=1000 早于 since");
        assert_eq!(delete_expired(&c, 5_000).unwrap(), 1);
        assert!(get_share(&c, "abcdef0123456789", "AbCdEfGhIjK2", 5_000).unwrap().is_some());
    }
}
