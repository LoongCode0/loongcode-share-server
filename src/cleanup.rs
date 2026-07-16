use std::sync::{Arc, Mutex};

/// 每小时删一次过期行；GET 查询本身已按 expires_at 过滤，这里只是回收存储。
pub fn spawn(db: Arc<Mutex<rusqlite::Connection>>) {
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("系统时钟早于 1970")
                .as_secs() as i64;
            if let Ok(conn) = db.lock() {
                if let Ok(n) = crate::store::delete_expired(&conn, now) {
                    if n > 0 {
                        println!("cleanup: removed {n} expired shares");
                    }
                }
            }
        }
    });
}
