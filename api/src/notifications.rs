use std::{collections::HashSet, sync::Arc, time::Duration};

use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{Aead, Generate, KeyInit, Payload},
};
use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode},
    routing::{get, post},
};
use axum_extra::extract::cookie::CookieJar;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use chrono::Utc;
use rand::RngCore;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use tracing::warn;
use url::Url;
use uuid::Uuid;

use crate::{
    AppState,
    auth::require_user,
    config::Config,
    error::{ApiError, ApiResult},
};

const JOB_BATCH_SIZE: i64 = 50;
const MAX_ATTEMPTS: i32 = 8;
const MAX_ENDPOINT_LENGTH: usize = 2048;
const MAX_BEARER_TOKEN_LENGTH: usize = 2048;

#[derive(Clone)]
pub struct NotificationRuntime {
    cipher: Arc<Aes256Gcm>,
    public_url: Url,
    endpoint_allowlist: Arc<HashSet<String>>,
}

impl NotificationRuntime {
    pub fn from_config(config: &Config) -> ApiResult<Self> {
        let cipher = Aes256Gcm::new_from_slice(&config.notification_encryption_key)
            .map_err(|_| ApiError::config("通知加密密钥长度无效"))?;
        Ok(Self {
            cipher: Arc::new(cipher),
            public_url: config.public_url.clone(),
            endpoint_allowlist: Arc::new(config.notification_endpoint_allowlist.clone()),
        })
    }

    fn encrypt_config(
        &self,
        user_id: Uuid,
        kind: ChannelKind,
        config: &StoredChannelConfig,
    ) -> ApiResult<(Vec<u8>, Vec<u8>)> {
        let plaintext = serde_json::to_vec(config)
            .map_err(|error| ApiError::Internal(format!("通知配置序列化失败: {error}")))?;
        let nonce = Nonce::generate();
        let ciphertext = self
            .cipher
            .encrypt(
                &nonce,
                Payload {
                    msg: &plaintext,
                    aad: &channel_aad(user_id, kind),
                },
            )
            .map_err(|_| ApiError::Internal("通知配置加密失败".to_owned()))?;
        Ok((ciphertext, nonce.to_vec()))
    }

    fn decrypt_config(
        &self,
        user_id: Uuid,
        kind: ChannelKind,
        ciphertext: &[u8],
        nonce: &[u8],
    ) -> Result<StoredChannelConfig, String> {
        let nonce = Nonce::try_from(nonce).map_err(|_| "通知配置 nonce 无效".to_owned())?;
        let plaintext = self
            .cipher
            .decrypt(
                &nonce,
                Payload {
                    msg: ciphertext,
                    aad: &channel_aad(user_id, kind),
                },
            )
            .map_err(|_| "通知配置无法解密".to_owned())?;
        serde_json::from_slice(&plaintext).map_err(|_| "通知配置内容无效".to_owned())
    }

    fn target_url(&self, target_path: &str) -> Url {
        let path = if target_path.starts_with('/') && !target_path.starts_with("//") {
            target_path
        } else {
            "/"
        };
        self.public_url
            .join(path.trim_start_matches('/'))
            .unwrap_or_else(|_| self.public_url.clone())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
enum ChannelKind {
    Bark,
    Webhook,
}

impl ChannelKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Bark => "bark",
            Self::Webhook => "webhook",
        }
    }

    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "bark" => Ok(Self::Bark),
            "webhook" => Ok(Self::Webhook),
            _ => Err("通知渠道类型无效".to_owned()),
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct StoredChannelConfig {
    endpoint: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    bearer_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    bark_encryption_key: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpsertChannelRequest {
    kind: ChannelKind,
    endpoint: String,
    bearer_token: Option<String>,
    bark_encryption_key: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ChannelResponse {
    configured: bool,
    kind: Option<ChannelKind>,
    endpoint_hint: Option<String>,
    has_bearer_token: bool,
    bark_encryption_enabled: bool,
}

#[derive(FromRow)]
struct ChannelRecord {
    kind: String,
    config_ciphertext: Vec<u8>,
    config_nonce: Vec<u8>,
}

#[derive(FromRow)]
struct NotificationJob {
    id: Uuid,
    user_id: Uuid,
    title: String,
    subtitle: Option<String>,
    body: String,
    target_path: String,
    attempts: i32,
    channel_kind: Option<String>,
    config_ciphertext: Option<Vec<u8>>,
    config_nonce: Option<Vec<u8>>,
}

struct NotificationMessage<'a> {
    title: &'a str,
    subtitle: Option<&'a str>,
    body: &'a str,
    target_url: Url,
}

#[derive(Serialize)]
struct BarkPayload<'a> {
    title: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    subtitle: Option<&'a str>,
    body: &'a str,
    group: &'static str,
    level: &'static str,
    icon: &'a str,
    url: &'a str,
}

#[derive(Serialize)]
struct EncryptedBarkPayload<'a> {
    ciphertext: &'a str,
    iv: &'a str,
    level: &'static str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WebhookPayload<'a> {
    version: u8,
    source: &'static str,
    title: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    subtitle: Option<&'a str>,
    body: &'a str,
    url: &'a str,
    sent_at: String,
}

enum DeliveryOutcome {
    Delivered,
    TransientFailure(String),
    PermanentFailure(String),
}

struct PreparedEndpoint {
    client: Client,
    url: Url,
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/notifications/channel",
            get(channel).put(upsert_channel).delete(delete_channel),
        )
        .route("/api/v1/notifications/test", post(test_channel))
}

async fn channel(
    State(state): State<AppState>,
    jar: CookieJar,
) -> ApiResult<Json<ChannelResponse>> {
    let user = require_user(&state.pool, &jar, None, false).await?;
    let record = sqlx::query_as::<_, ChannelRecord>(
        "SELECT kind,config_ciphertext,config_nonce FROM notification_channels WHERE user_id=$1",
    )
    .bind(user.id)
    .fetch_optional(&state.pool)
    .await?;
    let Some(record) = record else {
        return Ok(Json(ChannelResponse {
            configured: false,
            kind: None,
            endpoint_hint: None,
            has_bearer_token: false,
            bark_encryption_enabled: false,
        }));
    };
    let kind = ChannelKind::parse(&record.kind).map_err(ApiError::Internal)?;
    let config = state
        .notifications
        .decrypt_config(
            user.id,
            kind,
            &record.config_ciphertext,
            &record.config_nonce,
        )
        .map_err(ApiError::Internal)?;
    Ok(Json(ChannelResponse {
        configured: true,
        kind: Some(kind),
        endpoint_hint: endpoint_hint(&config.endpoint),
        has_bearer_token: config.bearer_token.is_some(),
        bark_encryption_enabled: config.bark_encryption_key.is_some(),
    }))
}

async fn upsert_channel(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    Json(request): Json<UpsertChannelRequest>,
) -> ApiResult<Json<ChannelResponse>> {
    let user = require_user(&state.pool, &jar, Some(&headers), true).await?;
    let kind = request.kind;
    let config = validate_channel_request(&state.notifications, request).await?;
    let hint = endpoint_hint(&config.endpoint);
    let has_bearer_token = config.bearer_token.is_some();
    let bark_encryption_enabled = config.bark_encryption_key.is_some();
    let (ciphertext, nonce) = state.notifications.encrypt_config(user.id, kind, &config)?;
    sqlx::query(
        "INSERT INTO notification_channels(user_id,kind,config_ciphertext,config_nonce) \
         VALUES($1,$2,$3,$4) ON CONFLICT(user_id) DO UPDATE SET \
         kind=EXCLUDED.kind,config_ciphertext=EXCLUDED.config_ciphertext,\
         config_nonce=EXCLUDED.config_nonce,updated_at=now()",
    )
    .bind(user.id)
    .bind(kind.as_str())
    .bind(ciphertext)
    .bind(nonce)
    .execute(&state.pool)
    .await?;
    Ok(Json(ChannelResponse {
        configured: true,
        kind: Some(kind),
        endpoint_hint: hint,
        has_bearer_token,
        bark_encryption_enabled,
    }))
}

async fn delete_channel(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
) -> ApiResult<StatusCode> {
    let user = require_user(&state.pool, &jar, Some(&headers), true).await?;
    sqlx::query("DELETE FROM notification_channels WHERE user_id=$1")
        .bind(user.id)
        .execute(&state.pool)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn test_channel(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
) -> ApiResult<StatusCode> {
    let user = require_user(&state.pool, &jar, Some(&headers), true).await?;
    let record = sqlx::query_as::<_, ChannelRecord>(
        "SELECT kind,config_ciphertext,config_nonce FROM notification_channels WHERE user_id=$1",
    )
    .bind(user.id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| ApiError::Conflict("请先保存通知渠道".to_owned()))?;
    let kind = ChannelKind::parse(&record.kind).map_err(ApiError::Internal)?;
    let config = state
        .notifications
        .decrypt_config(
            user.id,
            kind,
            &record.config_ciphertext,
            &record.config_nonce,
        )
        .map_err(ApiError::Internal)?;
    let message = NotificationMessage {
        title: "Lumeno 通知测试",
        subtitle: Some("连接、图标与分组已建立"),
        body: "本条测试应显示 Lumeno PNG 图标，并归入 Lumeno 分组。\n之后的预约、专注与任务群提醒会由服务器直接送达。",
        target_url: state.notifications.public_url.clone(),
    };
    match deliver(&state.notifications, kind, &config, &message).await {
        DeliveryOutcome::Delivered => Ok(StatusCode::NO_CONTENT),
        DeliveryOutcome::TransientFailure(message) | DeliveryOutcome::PermanentFailure(message) => {
            Err(ApiError::Delivery(message))
        }
    }
}

async fn validate_channel_request(
    runtime: &NotificationRuntime,
    mut request: UpsertChannelRequest,
) -> ApiResult<StoredChannelConfig> {
    request.endpoint = request.endpoint.trim().to_owned();
    if request.endpoint.is_empty() || request.endpoint.len() > MAX_ENDPOINT_LENGTH {
        return Err(ApiError::validation("通知地址长度无效"));
    }
    let bearer_token = request
        .bearer_token
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty());
    if request.kind.as_str() == "bark" && bearer_token.is_some() {
        return Err(ApiError::validation("Bark 渠道不使用 Bearer Token"));
    }
    let bark_encryption_key = request
        .bark_encryption_key
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty());
    if request.kind.as_str() == "webhook" && bark_encryption_key.is_some() {
        return Err(ApiError::validation("Webhook 渠道不使用 Bark 加密密钥"));
    }
    if bark_encryption_key.as_ref().is_some_and(|value| {
        value.len() != 32 || !value.is_ascii() || value.chars().any(char::is_control)
    }) {
        return Err(ApiError::validation(
            "Bark AES-256-GCM 密钥必须是 32 位 ASCII 字符",
        ));
    }
    if bearer_token.as_ref().is_some_and(|value| {
        value.len() > MAX_BEARER_TOKEN_LENGTH || value.chars().any(char::is_control)
    }) {
        return Err(ApiError::validation("Bearer Token 长度或字符无效"));
    }
    prepare_endpoint(&request.endpoint, &runtime.endpoint_allowlist).map_err(|failure| {
        ApiError::validation(match failure {
            DeliveryOutcome::TransientFailure(message)
            | DeliveryOutcome::PermanentFailure(message) => message,
            DeliveryOutcome::Delivered => "通知地址无效".to_owned(),
        })
    })?;
    Ok(StoredChannelConfig {
        endpoint: request.endpoint,
        bearer_token,
        bark_encryption_key,
    })
}

pub async fn process_jobs(state: &AppState) -> ApiResult<()> {
    let jobs = sqlx::query_as::<_, NotificationJob>(
        "WITH candidates AS (\
           SELECT j.id,c.kind AS channel_kind,c.config_ciphertext,c.config_nonce \
           FROM notification_jobs j LEFT JOIN notification_channels c ON c.user_id=j.user_id \
           WHERE j.sent_at IS NULL AND j.failed_at IS NULL AND j.cancelled_at IS NULL AND j.next_attempt_at<=now() \
           ORDER BY j.next_attempt_at,j.id LIMIT $1 FOR UPDATE OF j SKIP LOCKED\
         ) UPDATE notification_jobs j \
         SET attempts=j.attempts+1,next_attempt_at=now()+interval '60 seconds' \
         FROM candidates c WHERE j.id=c.id \
         RETURNING j.id,j.user_id,j.title,j.subtitle,j.body,j.target_path,j.attempts,\
                   c.channel_kind,c.config_ciphertext,c.config_nonce",
    )
    .bind(JOB_BATCH_SIZE)
    .fetch_all(&state.pool)
    .await?;
    let mut tasks = tokio::task::JoinSet::new();
    for job in jobs {
        let state = state.clone();
        tasks.spawn(async move { process_job(&state, &job).await });
    }
    let mut first_error = None;
    while let Some(result) = tasks.join_next().await {
        let result =
            result.map_err(|error| ApiError::Internal(format!("通知作业任务意外终止: {error}")))?;
        if let Err(error) = result
            && first_error.is_none()
        {
            first_error = Some(error);
        }
    }
    if let Some(error) = first_error {
        return Err(error);
    }
    Ok(())
}

pub async fn cleanup_jobs(pool: &sqlx::PgPool) -> Result<(), sqlx::Error> {
    sqlx::query(
        "DELETE FROM notification_jobs WHERE COALESCE(sent_at,failed_at,cancelled_at) < now() - interval '30 days'",
    )
    .execute(pool)
    .await?;
    Ok(())
}

async fn process_job(state: &AppState, job: &NotificationJob) -> ApiResult<()> {
    let outcome = delivery_for_job(&state.notifications, job).await;
    match outcome {
        DeliveryOutcome::Delivered => {
            sqlx::query("UPDATE notification_jobs SET attempts=$2,sent_at=now() WHERE id=$1")
                .bind(job.id)
                .bind(job.attempts)
                .execute(&state.pool)
                .await?;
        }
        DeliveryOutcome::TransientFailure(message) if job.attempts < MAX_ATTEMPTS => {
            warn!(job_id = %job.id, error = %message, "notification delivery will be retried");
            let backoff_seconds = 30_i64 * 2_i64.pow((job.attempts - 1).min(8) as u32);
            sqlx::query(
                "UPDATE notification_jobs SET attempts=$2,next_attempt_at=now()+make_interval(secs=>$3) WHERE id=$1",
            )
            .bind(job.id)
            .bind(job.attempts)
            .bind(backoff_seconds as f64)
            .execute(&state.pool)
            .await?;
        }
        DeliveryOutcome::TransientFailure(message) | DeliveryOutcome::PermanentFailure(message) => {
            warn!(job_id = %job.id, error = %message, "notification delivery failed permanently");
            sqlx::query("UPDATE notification_jobs SET attempts=$2,failed_at=now() WHERE id=$1")
                .bind(job.id)
                .bind(job.attempts)
                .execute(&state.pool)
                .await?;
        }
    }
    Ok(())
}

async fn delivery_for_job(runtime: &NotificationRuntime, job: &NotificationJob) -> DeliveryOutcome {
    let (Some(kind), Some(ciphertext), Some(nonce)) = (
        job.channel_kind.as_deref(),
        job.config_ciphertext.as_deref(),
        job.config_nonce.as_deref(),
    ) else {
        return if job.channel_kind.is_none()
            && job.config_ciphertext.is_none()
            && job.config_nonce.is_none()
        {
            DeliveryOutcome::Delivered
        } else {
            DeliveryOutcome::PermanentFailure("通知渠道记录不完整".to_owned())
        };
    };
    let kind = match ChannelKind::parse(kind) {
        Ok(kind) => kind,
        Err(message) => return DeliveryOutcome::PermanentFailure(message),
    };
    let config = match runtime.decrypt_config(job.user_id, kind, ciphertext, nonce) {
        Ok(config) => config,
        Err(message) => return DeliveryOutcome::PermanentFailure(message),
    };
    let message = NotificationMessage {
        title: &job.title,
        subtitle: job.subtitle.as_deref(),
        body: &job.body,
        target_url: runtime.target_url(&job.target_path),
    };
    deliver(runtime, kind, &config, &message).await
}

async fn deliver(
    runtime: &NotificationRuntime,
    kind: ChannelKind,
    config: &StoredChannelConfig,
    message: &NotificationMessage<'_>,
) -> DeliveryOutcome {
    let prepared = match prepare_endpoint(&config.endpoint, &runtime.endpoint_allowlist) {
        Ok(prepared) => prepared,
        Err(outcome) => return outcome,
    };
    let target_url = message.target_url.as_str();
    let mut request = match kind {
        ChannelKind::Bark => {
            let icon_url = match message.target_url.join("/icon.png") {
                Ok(icon_url) => icon_url,
                Err(_) => {
                    return DeliveryOutcome::PermanentFailure(
                        "无法生成 Bark 通知图标地址".to_owned(),
                    );
                }
            };
            let payload = BarkPayload {
                title: message.title,
                subtitle: message.subtitle,
                body: message.body,
                group: "Lumeno",
                level: "timeSensitive",
                icon: icon_url.as_str(),
                url: target_url,
            };
            if let Some(key) = config.bark_encryption_key.as_deref() {
                let iv = random_bark_iv();
                let json = match serde_json::to_vec(&payload) {
                    Ok(json) => json,
                    Err(_) => {
                        return DeliveryOutcome::PermanentFailure(
                            "Bark 通知内容无法序列化".to_owned(),
                        );
                    }
                };
                let ciphertext = match encrypt_bark_payload(key, &iv, &json) {
                    Ok(ciphertext) => ciphertext,
                    Err(message) => return DeliveryOutcome::PermanentFailure(message),
                };
                prepared
                    .client
                    .post(prepared.url)
                    .json(&EncryptedBarkPayload {
                        ciphertext: &ciphertext,
                        iv: &iv,
                        level: "timeSensitive",
                    })
            } else {
                prepared.client.post(prepared.url).json(&payload)
            }
        }
        ChannelKind::Webhook => {
            let request = prepared.client.post(prepared.url).json(&WebhookPayload {
                version: 1,
                source: "lumeno",
                title: message.title,
                subtitle: message.subtitle,
                body: message.body,
                url: target_url,
                sent_at: Utc::now().to_rfc3339(),
            });
            if let Some(token) = config.bearer_token.as_deref() {
                request.bearer_auth(token)
            } else {
                request
            }
        }
    };
    request = request.header("user-agent", "Lumeno-Notifications/1");
    match request.send().await {
        Ok(response) if response.status().is_success() => DeliveryOutcome::Delivered,
        Ok(response)
            if response.status() == StatusCode::REQUEST_TIMEOUT
                || response.status() == StatusCode::TOO_MANY_REQUESTS
                || response.status().is_server_error() =>
        {
            DeliveryOutcome::TransientFailure(format!(
                "通知端点暂时返回 HTTP {}",
                response.status().as_u16()
            ))
        }
        Ok(response) => DeliveryOutcome::PermanentFailure(format!(
            "通知端点拒绝请求（HTTP {}）",
            response.status().as_u16()
        )),
        Err(_) => DeliveryOutcome::TransientFailure("无法连接通知端点".to_owned()),
    }
}

fn encrypt_bark_payload(key: &str, iv: &str, plaintext: &[u8]) -> Result<String, String> {
    let cipher = Aes256Gcm::new_from_slice(key.as_bytes())
        .map_err(|_| "Bark 加密密钥长度无效".to_owned())?;
    let nonce = Nonce::try_from(iv.as_bytes()).map_err(|_| "Bark 加密 IV 长度无效".to_owned())?;
    let ciphertext = cipher
        .encrypt(&nonce, plaintext)
        .map_err(|_| "Bark 通知内容加密失败".to_owned())?;
    Ok(STANDARD.encode(ciphertext))
}

fn random_bark_iv() -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut random = [0_u8; 12];
    rand::rng().fill_bytes(&mut random);
    random
        .into_iter()
        .map(|value| ALPHABET[usize::from(value & 63)] as char)
        .collect()
}

fn prepare_endpoint(
    endpoint: &str,
    endpoint_allowlist: &HashSet<String>,
) -> Result<PreparedEndpoint, DeliveryOutcome> {
    let url = parse_endpoint(endpoint).map_err(DeliveryOutcome::PermanentFailure)?;
    if !endpoint_is_allowlisted(endpoint_allowlist, &url) {
        return Err(DeliveryOutcome::PermanentFailure(
            "通知地址的 HTTPS origin 未加入 NOTIFICATION_ENDPOINT_ALLOWLIST".to_owned(),
        ));
    }
    let client = Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|_| DeliveryOutcome::TransientFailure("无法创建通知 HTTP 客户端".to_owned()))?;
    Ok(PreparedEndpoint { client, url })
}

fn endpoint_is_allowlisted(endpoint_allowlist: &HashSet<String>, url: &Url) -> bool {
    endpoint_allowlist.contains(&url.origin().ascii_serialization())
}

fn parse_endpoint(endpoint: &str) -> Result<Url, String> {
    let url = Url::parse(endpoint).map_err(|_| "通知地址不是有效 URL".to_owned())?;
    if url.scheme() != "https" {
        return Err("通知地址必须使用 HTTPS".to_owned());
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("通知地址不能包含用户名或密码".to_owned());
    }
    if url.fragment().is_some() {
        return Err("通知地址不能包含 URL 片段".to_owned());
    }
    match url.host() {
        Some(_) => Ok(url),
        None => Err("通知地址缺少主机名".to_owned()),
    }
}

fn endpoint_hint(endpoint: &str) -> Option<String> {
    let url = Url::parse(endpoint).ok()?;
    let host = url.host_str()?;
    let port = url
        .port()
        .map(|value| format!(":{value}"))
        .unwrap_or_default();
    Some(format!("{}://{host}{port}", url.scheme()))
}

fn channel_aad(user_id: Uuid, kind: ChannelKind) -> Vec<u8> {
    format!("lumeno:notification-channel:v1:{user_id}:{}", kind.as_str()).into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encrypted_channel_config_is_bound_to_user_and_kind() {
        let runtime = NotificationRuntime {
            cipher: Arc::new(Aes256Gcm::new_from_slice(&[3_u8; 32]).unwrap()),
            public_url: Url::parse("https://lumeno.example/").unwrap(),
            endpoint_allowlist: Arc::new(HashSet::new()),
        };
        let user = Uuid::now_v7();
        let config = StoredChannelConfig {
            endpoint: "https://api.day.app/device-key".to_owned(),
            bearer_token: None,
            bark_encryption_key: Some("12345678901234567890123456789012".to_owned()),
        };
        let (ciphertext, nonce) = runtime
            .encrypt_config(user, ChannelKind::Bark, &config)
            .unwrap();

        let decrypted = runtime
            .decrypt_config(user, ChannelKind::Bark, &ciphertext, &nonce)
            .unwrap();
        assert_eq!(decrypted.endpoint, config.endpoint);
        assert!(
            runtime
                .decrypt_config(Uuid::now_v7(), ChannelKind::Bark, &ciphertext, &nonce)
                .is_err()
        );
        assert!(
            runtime
                .decrypt_config(user, ChannelKind::Webhook, &ciphertext, &nonce)
                .is_err()
        );
    }

    #[test]
    fn endpoint_syntax_rejects_insecure_urls_and_credentials() {
        for endpoint in [
            "http://example.com/hook",
            "https://user:password@example.com/hook",
            "https://example.com/hook#fragment",
        ] {
            assert!(parse_endpoint(endpoint).is_err(), "accepted {endpoint}");
        }
        assert!(parse_endpoint("https://8.8.8.8/hook").is_ok());
        assert!(parse_endpoint("https://192.168.1.20/device-key").is_ok());
        assert!(parse_endpoint("https://api.day.app/device-key").is_ok());
    }

    #[test]
    fn public_and_official_bark_endpoints_require_the_allowlist() {
        for endpoint in [
            "https://api.day.app/device-key",
            "https://hooks.example.com/lumeno",
        ] {
            assert!(prepare_endpoint(endpoint, &HashSet::new()).is_err());
        }
    }

    #[test]
    fn official_bark_endpoint_is_allowed_only_by_its_exact_origin() {
        let allowlist = HashSet::from(["https://api.day.app".to_owned()]);

        assert!(prepare_endpoint("https://api.day.app/device-key", &allowlist).is_ok());
        assert!(prepare_endpoint("https://api.day.app:8443/device-key", &allowlist).is_err());
        assert!(prepare_endpoint("https://api.day.app.example/device-key", &allowlist).is_err());
    }

    #[test]
    fn notification_endpoint_requires_an_exact_origin_allowlist_match() {
        let endpoint = "https://192.168.1.20:8443/private-device-key";
        assert!(prepare_endpoint(endpoint, &HashSet::new()).is_err());

        let allowlist = HashSet::from(["https://192.168.1.20:8443".to_owned()]);
        assert!(prepare_endpoint(endpoint, &allowlist).is_ok());
        assert!(prepare_endpoint("https://192.168.1.20/private-device-key", &allowlist).is_err());
    }

    #[test]
    fn allowlisted_internal_hostname_does_not_require_local_dns() {
        let allowlist = HashSet::from(["https://bark.internal.example".to_owned()]);

        assert!(
            prepare_endpoint(
                "https://bark.internal.example/private-device-key",
                &allowlist,
            )
            .is_ok()
        );
    }

    #[test]
    fn endpoint_hint_never_contains_path_or_device_key() {
        assert_eq!(
            endpoint_hint("https://api.day.app/private-device-key"),
            Some("https://api.day.app".to_owned())
        );
    }

    #[test]
    fn bark_gcm_ciphertext_round_trips_with_combined_authentication_tag() {
        let key = "12345678901234567890123456789012";
        let iv = "abcdefghijkl";
        let plaintext = br#"{"title":"Test","body":"private"}"#;
        let encoded = encrypt_bark_payload(key, iv, plaintext).unwrap();
        assert!(!encoded.contains("private"));

        let cipher = Aes256Gcm::new_from_slice(key.as_bytes()).unwrap();
        let nonce = Nonce::try_from(iv.as_bytes()).unwrap();
        let decoded = STANDARD.decode(encoded).unwrap();
        assert_eq!(
            cipher.decrypt(&nonce, decoded.as_slice()).unwrap(),
            plaintext
        );
    }

    #[test]
    fn bark_iv_is_twelve_url_safe_ascii_bytes() {
        let first = random_bark_iv();
        assert_eq!(first.len(), 12);
        assert!(first.is_ascii());
        assert!(
            first
                .bytes()
                .all(|value| value.is_ascii_alphanumeric() || value == b'-' || value == b'_')
        );
    }

    #[test]
    fn bark_payload_uses_the_public_png_icon() {
        let target_url = Url::parse("https://lumeno.example/chains").unwrap();
        let icon_url = target_url.join("/icon.png").unwrap();
        let payload = BarkPayload {
            title: "提醒",
            subtitle: None,
            body: "开始行动",
            group: "Lumeno",
            level: "timeSensitive",
            icon: icon_url.as_str(),
            url: target_url.as_str(),
        };
        let serialized = serde_json::to_value(payload).unwrap();

        assert_eq!(serialized["icon"], "https://lumeno.example/icon.png");
        assert_eq!(serialized["group"], "Lumeno");
        assert_eq!(serialized["level"], "timeSensitive");
    }

    #[test]
    fn encrypted_bark_payload_keeps_icon_and_group_inside_the_ciphertext() {
        let key = "12345678901234567890123456789012";
        let iv = "abcdefghijkl";
        let target_url = Url::parse("https://lumeno.example/").unwrap();
        let icon_url = target_url.join("/icon.png").unwrap();
        let payload = BarkPayload {
            title: "Lumeno 通知测试",
            subtitle: Some("连接已建立"),
            body: "测试图标与分组",
            group: "Lumeno",
            level: "timeSensitive",
            icon: icon_url.as_str(),
            url: target_url.as_str(),
        };
        let plaintext = serde_json::to_vec(&payload).unwrap();
        let encoded = encrypt_bark_payload(key, iv, &plaintext).unwrap();
        let cipher = Aes256Gcm::new_from_slice(key.as_bytes()).unwrap();
        let nonce = Nonce::try_from(iv.as_bytes()).unwrap();
        let decrypted = cipher
            .decrypt(&nonce, STANDARD.decode(encoded).unwrap().as_slice())
            .unwrap();
        let value: serde_json::Value = serde_json::from_slice(&decrypted).unwrap();

        assert_eq!(value["icon"], "https://lumeno.example/icon.png");
        assert_eq!(value["group"], "Lumeno");
    }
}
