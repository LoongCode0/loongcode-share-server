# loongcode-share-server

LoongCode 分享服务器：为 LoongCode 客户端提供对话内容安全分享与设备级访问控制，通过 HMAC 签名和 IP/设备限流保护。

## 功能

- 通过共享链接发布对话（带访问令牌）
- 设备日限、IP 分钟限流
- 内置 SPA 前端用于渲染分享
- SQLite 持久化

## 环境变量

| 环境变量 | 默认值 | 说明 |
|---------|-------|------|
| `SHARE_HMAC_SECRET` | *必填* | HMAC 签名密钥，至少 16 字符（两端空白自动 trim） |
| `SHARE_LISTEN` | `0.0.0.0:8787` | 监听地址与端口 |
| `SHARE_DB_PATH` | `./data/shares.db` | SQLite 数据库路径 |
| `SHARE_BASE_URL` | `https://share.loongcode.cc` | 分享链接基础 URL（用于生成可复制的完整链接） |
| `SHARE_WEB_DIR` | `./web/dist` | 前端构建目录 |
| `SHARE_DEVICE_DAILY_LIMIT` | `50` | 单设备每日最大分享数 |
| `SHARE_IP_MINUTE_LIMIT` | `20` | 单 IP 每分钟最大请求数 |

## API

所有写操作（POST/DELETE）必须携带三个签名请求头：

```
X-Device-Id: <16 位小写十六进制设备 ID>
X-Timestamp: <Unix 秒>
X-Signature: <hex(HMAC-SHA256(secret, 规范化串))，64 位小写十六进制>
```

规范化串（客户端/服务端逐字节一致，定义见 src/auth.rs 的 canonical_message）：

```
{timestamp}\n{METHOD大写}\n{path}\n{hex(SHA256(body))}\n{deviceId}
```

时间窗 ±300 秒；签名缺失、不匹配或超窗一律 401。

| 方法 | 路径 | 签名 | 说明 |
|---|---|---|---|
| POST | `/api/shares` | 需要 | 创建分享。body：`{"workspaceName","taskTitle","expiresInDays":1\|3\|7,"messages":[{"role":"user"\|"assistant","text"}],"withPassword":false}`（`withPassword` 可省略，默认 false）。响应：`{"shareId","deviceId","url","deleteToken","expiresAt"}`，`withPassword:true` 时额外含 `"password"`（明文，仅此一次） |
| GET | `/api/shares/{deviceId}/{shareId}` | 不需要（链接即凭证）；若分享设了密码，需带 `X-Share-Password` 请求头 | 分享 JSON：`{"workspaceName","taskTitle","createdAt","expiresAt","messages"}`；密码缺失或错误返回 401 `password_required`，响应体不含任何内容字段 |
| DELETE | `/api/shares/{deviceId}/{shareId}` | 需要 | 撤销。body：`{"deleteToken":"..."}`；签名设备必须等于路径 deviceId |
| GET | `/s/{deviceId}/{shareId}` | 不需要 | 分享页（SPA） |

限制：body ≤ 2MB；messages 1..=500 条；单条 ≤ 100KB；标题/工作区名 ≤ 200 字符；设备日创建上限默认 50；IP 分钟上限默认 20（按 X-Forwarded-For **最后一个**条目键控）。

## 错误响应

统一形状 `{"error":{"code":"...","message":"..."}}`：

| 状态码 | code | 触发 |
|---|---|---|
| 400 | bad_request | body 解析失败 / 字段校验失败 |
| 401 | unauthorized | 签名缺失、不匹配或时间窗外 |
| 404 | not_found | 分享不存在、已过期、ID 格式非法、删除凭证不符、签名设备≠路径设备——统一同码同文案，防枚举探测 |
| 401 | password_required | 分享设了访问密码，但未带 `X-Share-Password` 或密码错误 |
| 413 | payload_too_large | 请求体超过 2MB |
| 429 | rate_limited | 设备日限或 IP 分钟限触发 |
| 500 | internal | 存储等内部错误 |

## 部署

### Docker 方式

构建镜像：
```bash
docker build -t share-server:latest .
```

运行容器：
```bash
docker run -d \
  -e SHARE_HMAC_SECRET="your-secret-at-least-16-chars" \
  -e SHARE_BASE_URL="https://share.example.com" \
  -v shared_data:/data \
  -p 8787:8787 \
  share-server:latest
```

### 反代配置

**HTTPS 由反代终结**（Nginx/Caddy 等），本服务仅听内网 HTTP。反代必须追加/覆写 `X-Forwarded-For` 头透传客户端真实 IP：

```nginx
location / {
  proxy_pass http://127.0.0.1:8787;
  proxy_set_header X-Forwarded-For $remote_addr;
}
```

**安全提示**：后端端口必须以防火墙/安全组限定仅反代可达。否则直连时伪造单段 `X-Forwarded-For` 仍可绕过 IP 层限流（设备日限不受影响）。

## 开发

终端 1：启动后端服务
```bash
bin/dev.sh
```

终端 2：启动前端热更开发服务
```bash
pnpm --dir web dev
```

前端开发服务已代理 `/api` 至后端 8787 端口。

## 构建

```bash
bin/build.sh
```

生成 `target/release/share-server` 可执行文件与 `web/dist/` 前端构建产物。

## 安全边界

1. **内置密钥逆向提取**：HMAC 密钥可通过逆向工程从二进制文件或容器中提取，仅用于防止滥用配合限流，不能作为强身份验证。

2. **设备关联**：`device_id` 作为 URL 前缀（`/api/shares/{device_id}/{share_id}`），同设备的分享可被关联。不应在 `device_id` 中包含敏感信息（真名、机器码等）；建议使用客户端生成的随机 UUID。
