use axum::body::Bytes;
use axum::extract::{ConnectInfo, DefaultBodyLimit, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::Digest;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

pub const MAX_MESSAGES: usize = 500;
pub const MAX_TEXT_BYTES: usize = 100_000;
pub const MAX_TITLE_CHARS: usize = 200;
pub const BODY_LIMIT_BYTES: usize = 2 * 1024 * 1024;

#[derive(Clone)]
pub struct AppState {
    pub db: Arc<Mutex<rusqlite::Connection>>,
    pub cfg: Arc<crate::config::Config>,
    /// ip → (分钟桶, 命中数)。进程内滑窗，重启清零可接受。
    pub ip_hits: Arc<Mutex<HashMap<String, (u64, u32)>>>,
}

#[derive(Deserialize, Serialize, Clone)]
pub struct ShareMessage {
    pub role: String,
    pub text: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateShareReq {
    pub workspace_name: String,
    pub task_title: String,
    pub expires_in_days: u8,
    pub messages: Vec<ShareMessage>,
}

fn err(status: StatusCode, code: &str, message: &str) -> Response {
    (status, Json(json!({ "error": { "code": code, "message": message } }))).into_response()
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("系统时钟早于 1970")
        .as_secs() as i64
}

fn validate(req: &CreateShareReq) -> Result<(), &'static str> {
    if ![1u8, 3, 7].contains(&req.expires_in_days) {
        return Err("expiresInDays 必须是 1/3/7");
    }
    if req.messages.is_empty() || req.messages.len() > MAX_MESSAGES {
        return Err("messages 数量非法");
    }
    if req.workspace_name.chars().count() > MAX_TITLE_CHARS
        || req.task_title.chars().count() > MAX_TITLE_CHARS
    {
        return Err("标题过长");
    }
    for m in &req.messages {
        if m.role != "user" && m.role != "assistant" {
            return Err("role 非法");
        }
        if m.text.len() > MAX_TEXT_BYTES {
            return Err("单条消息过长");
        }
    }
    Ok(())
}

fn rand_b62(len: usize) -> String {
    use rand::Rng;
    rand::thread_rng()
        .sample_iter(&rand::distributions::Alphanumeric)
        .take(len)
        .map(char::from)
        .collect()
}

/// 反代场景取 X-Forwarded-For 最后一个条目，其余取 peer IP。
/// `proxy_add_x_forwarded_for` 是追加模式，最后一段才是自家反代写入的真实来源；
/// 客户端可任意伪造首段，但无法覆盖反代追加在末尾的真实值，故按最后一段键控可防伪造。
fn client_ip(headers: &HeaderMap, peer: &std::net::SocketAddr) -> String {
    headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.split(',').next_back())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| peer.ip().to_string())
}

/// 分钟桶计数；true = 超限。
fn ip_over_limit(state: &AppState, ip: &str, now: i64) -> bool {
    let bucket = (now / 60) as u64;
    let mut map = state.ip_hits.lock().unwrap();
    let e = map.entry(ip.to_string()).or_insert((bucket, 0));
    if e.0 != bucket {
        *e = (bucket, 0);
    }
    e.1 += 1;
    e.1 > state.cfg.ip_minute_limit
}

fn verify_headers(
    state: &AppState,
    headers: &HeaderMap,
    method: &str,
    path: &str,
    body: &[u8],
    now: i64,
) -> Result<String, Response> {
    let h = |k: &str| headers.get(k).and_then(|v| v.to_str().ok()).unwrap_or("");
    crate::auth::verify(
        &state.cfg.secret,
        h("x-device-id"),
        h("x-timestamp"),
        h("x-signature"),
        method,
        path,
        body,
        now,
    )
    .map_err(|_| err(StatusCode::UNAUTHORIZED, "unauthorized", "签名校验失败"))
}

async fn create_share(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<std::net::SocketAddr>,
    headers: HeaderMap,
    body: Result<Bytes, axum::extract::rejection::BytesRejection>,
) -> Response {
    let now = now_secs();
    let body = match body {
        Ok(b) => b,
        Err(rej) => {
            let status = rej.status();
            let code = if status == StatusCode::PAYLOAD_TOO_LARGE { "payload_too_large" } else { "bad_request" };
            return err(status, code, "请求体超限或不可读");
        }
    };
    let device = match verify_headers(&state, &headers, "POST", "/api/shares", &body, now) {
        Ok(d) => d,
        Err(resp) => return resp,
    };
    let ip = client_ip(&headers, &peer);
    if ip_over_limit(&state, &ip, now) {
        return err(StatusCode::TOO_MANY_REQUESTS, "rate_limited", "请求过于频繁，请稍后再试");
    }
    let req: CreateShareReq = match serde_json::from_slice(&body) {
        Ok(r) => r,
        Err(_) => return err(StatusCode::BAD_REQUEST, "bad_request", "请求体解析失败"),
    };
    if let Err(m) = validate(&req) {
        return err(StatusCode::BAD_REQUEST, "bad_request", m);
    }

    let db = state.db.lock().unwrap();
    match crate::store::count_since(&db, &device, now - 86_400) {
        Ok(n) if n >= state.cfg.device_daily_limit => {
            return err(StatusCode::TOO_MANY_REQUESTS, "rate_limited", "该设备今日分享数已达上限");
        }
        Ok(_) => {}
        Err(_) => return err(StatusCode::INTERNAL_SERVER_ERROR, "internal", "存储错误"),
    }

    let delete_token = rand_b62(32);
    let token_hash = hex::encode(sha2::Sha256::digest(delete_token.as_bytes()));
    let expires_at = now + i64::from(req.expires_in_days) * 86_400;
    let payload_json = serde_json::to_string(&req.messages).expect("已校验的 messages 必可序列化");
    for _ in 0..3 {
        let share_id = rand_b62(12);
        let row = crate::store::ShareRow {
            device_id: device.clone(),
            share_id: share_id.clone(),
            workspace_name: req.workspace_name.clone(),
            task_title: req.task_title.clone(),
            payload_json: payload_json.clone(),
            message_count: req.messages.len() as i64,
            delete_token_hash: token_hash.clone(),
            created_at: now,
            expires_at,
        };
        match crate::store::insert_share(&db, &row) {
            Ok(true) => {
                let url = format!("{}/s/{}/{}", state.cfg.base_url, device, share_id);
                return (
                    StatusCode::OK,
                    Json(json!({
                        "shareId": share_id,
                        "deviceId": device,
                        "url": url,
                        "deleteToken": delete_token,
                        "expiresAt": expires_at,
                    })),
                )
                    .into_response();
            }
            Ok(false) => continue,
            Err(_) => return err(StatusCode::INTERNAL_SERVER_ERROR, "internal", "存储错误"),
        }
    }
    err(StatusCode::INTERNAL_SERVER_ERROR, "internal", "分享 ID 生成冲突")
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/shares", post(create_share))
        .layer(DefaultBodyLimit::max(BODY_LIMIT_BYTES))
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    const SECRET: &str = "test-secret-0123";
    const DEVICE: &str = "abcdef0123456789";

    fn test_state(device_daily_limit: i64, ip_minute_limit: u32) -> AppState {
        let cfg = crate::config::Config {
            listen: "127.0.0.1:0".parse().unwrap(),
            db_path: "unused".into(),
            base_url: "http://sh.test".into(),
            secret: SECRET.into(),
            web_dir: "unused".into(),
            device_daily_limit,
            ip_minute_limit,
        };
        AppState {
            db: std::sync::Arc::new(std::sync::Mutex::new(crate::store::open_in_memory().unwrap())),
            cfg: std::sync::Arc::new(cfg),
            ip_hits: std::sync::Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
        }
    }

    fn now() -> i64 {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64
    }

    fn valid_body() -> String {
        r#"{"workspaceName":"longlong-ade","taskTitle":"标题","expiresInDays":1,"messages":[{"role":"user","text":"你好"},{"role":"assistant","text":"回复"}]}"#.into()
    }

    /// 组一个带合法签名与 ConnectInfo 的 POST（oneshot 场景 ConnectInfo 走 extension 注入）。
    fn signed_post(body: &str, sig_override: Option<&str>) -> Request<Body> {
        let ts = now();
        let sig = crate::auth::compute_signature(SECRET, ts, "POST", "/api/shares", body.as_bytes(), DEVICE);
        let mut req = Request::builder()
            .method("POST")
            .uri("/api/shares")
            .header("content-type", "application/json")
            .header("x-device-id", DEVICE)
            .header("x-timestamp", ts.to_string())
            .header("x-signature", sig_override.unwrap_or(&sig))
            .body(Body::from(body.to_string()))
            .unwrap();
        req.extensions_mut().insert(axum::extract::ConnectInfo(
            std::net::SocketAddr::from(([127, 0, 0, 1], 9999)),
        ));
        req
    }

    async fn body_json(resp: axum::response::Response) -> serde_json::Value {
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[tokio::test]
    async fn create_ok_returns_link_fields() {
        let app = router(test_state(50, 20));
        let resp = app.oneshot(signed_post(&valid_body(), None)).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let v = body_json(resp).await;
        assert_eq!(v["deviceId"], DEVICE);
        let share_id = v["shareId"].as_str().unwrap();
        assert_eq!(share_id.len(), 12);
        assert_eq!(v["url"], format!("http://sh.test/s/{DEVICE}/{share_id}"));
        assert_eq!(v["deleteToken"].as_str().unwrap().len(), 32);
        assert!(v["expiresAt"].as_i64().unwrap() > now() + 86_000, "1 天有效期");
    }

    #[tokio::test]
    async fn bad_signature_401() {
        let app = router(test_state(50, 20));
        let resp = app.oneshot(signed_post(&valid_body(), Some("00".repeat(32).as_str()))).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(body_json(resp).await["error"]["code"], "unauthorized");
    }

    #[tokio::test]
    async fn invalid_expiry_400() {
        let app = router(test_state(50, 20));
        let body = valid_body().replace(r#""expiresInDays":1"#, r#""expiresInDays":2"#);
        let resp = app.oneshot(signed_post(&body, None)).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn bad_role_400() {
        let app = router(test_state(50, 20));
        let body = valid_body().replace(r#""role":"assistant""#, r#""role":"tool""#);
        let resp = app.oneshot(signed_post(&body, None)).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn device_daily_limit_429() {
        let app = router(test_state(1, 20));
        let r1 = app.clone().oneshot(signed_post(&valid_body(), None)).await.unwrap();
        assert_eq!(r1.status(), StatusCode::OK);
        let r2 = app.oneshot(signed_post(&valid_body(), None)).await.unwrap();
        assert_eq!(r2.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(body_json(r2).await["error"]["code"], "rate_limited");
    }

    #[tokio::test]
    async fn ip_minute_limit_429() {
        let app = router(test_state(50, 1));
        let r1 = app.clone().oneshot(signed_post(&valid_body(), None)).await.unwrap();
        assert_eq!(r1.status(), StatusCode::OK);
        let r2 = app.oneshot(signed_post(&valid_body(), None)).await.unwrap();
        assert_eq!(r2.status(), StatusCode::TOO_MANY_REQUESTS);
    }

    /// XFF 首段可被客户端任意伪造；只有按最后一段（自家反代追加写入）键控才防得住伪造绕过限流。
    #[tokio::test]
    async fn xff_spoofed_prefix_cannot_bypass_ip_limit() {
        let app = router(test_state(50, 1));
        let mut req1 = signed_post(&valid_body(), None);
        req1.headers_mut().insert("x-forwarded-for", "spoof-a, 7.7.7.7".parse().unwrap());
        let r1 = app.clone().oneshot(req1).await.unwrap();
        assert_eq!(r1.status(), StatusCode::OK);

        let mut req2 = signed_post(&valid_body(), None);
        req2.headers_mut().insert("x-forwarded-for", "spoof-b, 7.7.7.7".parse().unwrap());
        let r2 = app.oneshot(req2).await.unwrap();
        assert_eq!(r2.status(), StatusCode::TOO_MANY_REQUESTS, "首段不同但尾段相同，应视为同一 IP 计数");
    }

    #[tokio::test]
    async fn oversize_body_413_unified_shape() {
        let app = router(test_state(50, 20));
        let body = "a".repeat(BODY_LIMIT_BYTES + 1);
        let resp = app.oneshot(signed_post(&body, None)).await.unwrap();
        assert_eq!(resp.status(), StatusCode::PAYLOAD_TOO_LARGE);
        assert_eq!(body_json(resp).await["error"]["code"], "payload_too_large");
    }
}
