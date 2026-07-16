use std::net::SocketAddr;
use std::path::PathBuf;

/// 全部来自环境变量；SHARE_HMAC_SECRET 必填，其余带默认。
pub struct Config {
    pub listen: SocketAddr,
    pub db_path: PathBuf,
    pub base_url: String,
    pub secret: String,
    #[allow(dead_code)]
    // Task 5 静态托管接入后删除本行 allow
    pub web_dir: PathBuf,
    pub device_daily_limit: i64,
    pub ip_minute_limit: u32,
}

impl Config {
    pub fn from_env() -> anyhow::Result<Config> {
        let secret = std::env::var("SHARE_HMAC_SECRET")
            .map_err(|_| anyhow::anyhow!("SHARE_HMAC_SECRET 未设置"))?;
        if secret.trim().len() < 16 {
            anyhow::bail!("SHARE_HMAC_SECRET 至少 16 字符");
        }
        Ok(Config {
            listen: std::env::var("SHARE_LISTEN")
                .unwrap_or_else(|_| "0.0.0.0:8787".into())
                .parse()?,
            db_path: std::env::var("SHARE_DB_PATH")
                .unwrap_or_else(|_| "./data/shares.db".into())
                .into(),
            base_url: std::env::var("SHARE_BASE_URL")
                .unwrap_or_else(|_| "https://share.loongcode.cc".into())
                .trim_end_matches('/')
                .to_string(),
            // 存 trim 后的值:HMAC 密钥两端一致性要求(客户端同样 trim)
            secret: secret.trim().to_string(),
            web_dir: std::env::var("SHARE_WEB_DIR")
                .unwrap_or_else(|_| "./web/dist".into())
                .into(),
            device_daily_limit: std::env::var("SHARE_DEVICE_DAILY_LIMIT")
                .ok().and_then(|v| v.parse().ok()).unwrap_or(50),
            ip_minute_limit: std::env::var("SHARE_IP_MINUTE_LIMIT")
                .ok().and_then(|v| v.parse().ok()).unwrap_or(20),
        })
    }
}
