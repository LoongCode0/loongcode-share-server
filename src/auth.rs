use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};

type HmacSha256 = Hmac<Sha256>;

pub const TS_WINDOW_SECS: i64 = 300;

#[derive(Debug)]
pub enum AuthError {
    BadDeviceId,
    BadTimestamp,
    Expired,
    BadSignature,
}

/// 规范化签名串：ts \n METHOD \n path \n hex(sha256(body)) \n deviceId。
/// ⚠ 与客户端 share/sign.rs 逐字节一致，改动必须双仓同步。
fn canonical_message(timestamp: i64, method: &str, path: &str, body: &[u8], device_id: &str) -> String {
    let body_hash = hex::encode(Sha256::digest(body));
    format!("{timestamp}\n{}\n{path}\n{body_hash}\n{device_id}", method.to_uppercase())
}

/// 仅测试使用：服务端运行时只做校验（verify 内联重算 HMAC）；
/// 签名构造的生产实现在客户端仓库 share/sign.rs（双仓契约见 canonical_message 注释）。
#[cfg(test)]
pub fn compute_signature(secret: &str, timestamp: i64, method: &str, path: &str, body: &[u8], device_id: &str) -> String {
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).expect("HMAC 接受任意长度密钥");
    mac.update(canonical_message(timestamp, method, path, body, device_id).as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

/// 16 位小写十六进制。
pub fn is_valid_device_id(s: &str) -> bool {
    s.len() == 16 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// 校验通过返回 device_id。常量时间比较由 `Mac::verify_slice` 保证。
#[allow(clippy::too_many_arguments)]
pub fn verify(
    secret: &str,
    device_id: &str,
    timestamp_raw: &str,
    signature_hex: &str,
    method: &str,
    path: &str,
    body: &[u8],
    now: i64,
) -> Result<String, AuthError> {
    if !is_valid_device_id(device_id) {
        return Err(AuthError::BadDeviceId);
    }
    let ts: i64 = timestamp_raw.parse().map_err(|_| AuthError::BadTimestamp)?;
    if (now - ts).abs() > TS_WINDOW_SECS {
        return Err(AuthError::Expired);
    }
    let sig = hex::decode(signature_hex).map_err(|_| AuthError::BadSignature)?;
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).expect("HMAC 接受任意长度密钥");
    mac.update(canonical_message(ts, method, path, body, device_id).as_bytes());
    mac.verify_slice(&sig).map_err(|_| AuthError::BadSignature)?;
    Ok(device_id.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &str = "test-secret-0123";
    const DEVICE: &str = "abcdef0123456789";

    #[test]
    fn roundtrip_ok() {
        let sig = compute_signature(SECRET, 1_700_000_000, "POST", "/api/shares", br#"{"k":"v"}"#, DEVICE);
        assert_eq!(sig.len(), 64, "hex(HMAC-SHA256) 应 64 字符");
        let got = verify(SECRET, DEVICE, "1700000000", &sig, "POST", "/api/shares", br#"{"k":"v"}"#, 1_700_000_100);
        assert_eq!(got.unwrap(), DEVICE);
    }

    #[test]
    fn tampered_body_rejected() {
        let sig = compute_signature(SECRET, 1_700_000_000, "POST", "/api/shares", br#"{"k":"v"}"#, DEVICE);
        assert!(verify(SECRET, DEVICE, "1700000000", &sig, "POST", "/api/shares", br#"{"k":"X"}"#, 1_700_000_100).is_err());
    }

    #[test]
    fn expired_timestamp_rejected() {
        let sig = compute_signature(SECRET, 1_700_000_000, "POST", "/api/shares", b"{}", DEVICE);
        assert!(verify(SECRET, DEVICE, "1700000000", &sig, "POST", "/api/shares", b"{}", 1_700_000_301).is_err(), "301 秒 > 窗口");
        assert!(verify(SECRET, DEVICE, "1700000000", &sig, "POST", "/api/shares", b"{}", 1_700_000_300).is_ok(), "300 秒 = 窗口边界应通过");
    }

    #[test]
    fn bad_device_id_rejected() {
        for bad in ["ABCDEF0123456789", "abcdef012345678", "abcdef0123456789a", "ghijkl0123456789"] {
            assert!(!is_valid_device_id(bad), "{bad} 应非法");
        }
        assert!(is_valid_device_id(DEVICE));
    }

    #[test]
    fn bad_hex_signature_rejected() {
        assert!(verify(SECRET, DEVICE, "1700000000", "zz-not-hex", "POST", "/api/shares", b"{}", 1_700_000_000).is_err());
    }

    #[test]
    fn verify_rejects_bad_device_id() {
        let sig = compute_signature(SECRET, 1_700_000_000, "POST", "/api/shares", b"{}", DEVICE);
        let result = verify(SECRET, "BADDEVICE", "1700000000", &sig, "POST", "/api/shares", b"{}", 1_700_000_100);
        assert!(matches!(result, Err(AuthError::BadDeviceId)), "不合法 device_id 应返回 BadDeviceId");
    }

    #[test]
    fn verify_rejects_non_numeric_timestamp() {
        let sig = compute_signature(SECRET, 1_700_000_000, "POST", "/api/shares", b"{}", DEVICE);
        let result = verify(SECRET, DEVICE, "not-a-number", &sig, "POST", "/api/shares", b"{}", 1_700_000_100);
        assert!(matches!(result, Err(AuthError::BadTimestamp)), "非数字 timestamp 应返回 BadTimestamp");
    }

    #[test]
    fn verify_rejects_future_timestamp_beyond_window() {
        let sig = compute_signature(SECRET, 1_700_000_000, "POST", "/api/shares", b"{}", DEVICE);
        // now 比 ts 超前 301 秒（未来方向超过窗口）
        let result = verify(SECRET, DEVICE, "1700000000", &sig, "POST", "/api/shares", b"{}", 1_700_000_301);
        assert!(matches!(result, Err(AuthError::Expired)), "未来时戳超过 300 秒窗口应返回 Expired");
    }

    /// 跨仓库锚定向量：`cargo test print_vector -- --ignored --nocapture` 打印后，
    /// 把输出的 hex 粘贴到客户端 src-tauri/src/share/sign.rs 测试的 SERVER_VECTOR 常量。
    #[test]
    #[ignore]
    fn print_vector() {
        println!("VECTOR={}", compute_signature(SECRET, 1_700_000_000, "POST", "/api/shares", br#"{"k":"v"}"#, DEVICE));
    }
}
