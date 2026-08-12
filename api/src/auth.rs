use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use axum::{
    Json,
    extract::{Form, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Redirect, Response},
};
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{DateTime, Utc};
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header, jwk::JwkSet};
use openidconnect::{
    Audience, AuthorizationCode, ClientId, ClientSecret, CsrfToken, EndpointMaybeSet,
    EndpointNotSet, EndpointSet, IssuerUrl, Nonce, PkceCodeChallenge, PkceCodeVerifier,
    ProviderMetadataWithLogout, RedirectUrl, Scope, TokenResponse,
    core::{CoreAuthenticationFlow, CoreClient},
};
use rand::RngCore;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use uuid::Uuid;

use crate::{
    AppState,
    config::Config,
    error::{ApiError, ApiResult},
    models::{MeResponse, SessionUser, UpdateSettingsRequest},
};

pub const SESSION_COOKIE: &str = "lumeno_session";
pub const CSRF_COOKIE: &str = "lumeno_csrf";

type OidcClient = CoreClient<
    EndpointSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointMaybeSet,
    EndpointMaybeSet,
>;

#[derive(Clone)]
pub struct OidcRuntime {
    client: Arc<OidcClient>,
    http: reqwest::Client,
    issuer: String,
    client_id: String,
    trusted_audiences: Arc<HashSet<String>>,
    end_session_url: Option<String>,
    logout_jwks: Arc<JwkSet>,
}

impl OidcRuntime {
    pub async fn discover(config: &Config) -> ApiResult<Self> {
        let issuer = IssuerUrl::new(config.oidc_issuer.clone())
            .map_err(|error| ApiError::config(format!("OIDC_ISSUER 无效: {error}")))?;
        if issuer.url().scheme() != "https" && issuer.url().host_str() != Some("localhost") {
            return Err(ApiError::config(
                "OIDC issuer 在非 localhost 环境必须使用 HTTPS",
            ));
        }
        let http = reqwest::ClientBuilder::new()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .map_err(|error| ApiError::config(format!("OIDC HTTP 客户端创建失败: {error}")))?;
        let metadata = ProviderMetadataWithLogout::discover_async(issuer, &http)
            .await
            .map_err(|error| ApiError::Oidc(error.to_string()))?;
        let end_session_url = metadata
            .additional_metadata()
            .end_session_endpoint
            .as_ref()
            .map(|url| url.url().to_string());
        if let Some(endpoint) = end_session_url.as_deref() {
            let endpoint = url::Url::parse(endpoint)
                .map_err(|error| ApiError::config(format!("OIDC end-session URL 无效: {error}")))?;
            if endpoint.scheme() != "https" && endpoint.host_str() != Some("localhost") {
                return Err(ApiError::config(
                    "OIDC end-session endpoint 在非 localhost 环境必须使用 HTTPS",
                ));
            }
        }
        let logout_jwks = serde_json::from_value(
            serde_json::to_value(metadata.jwks())
                .map_err(|error| ApiError::config(format!("OIDC JWKS 序列化失败: {error}")))?,
        )
        .map_err(|error| ApiError::config(format!("OIDC JWKS 解析失败: {error}")))?;
        let redirect = config
            .public_url
            .join("api/v1/auth/callback")
            .map_err(|error| ApiError::config(format!("OIDC callback URL 无效: {error}")))?;
        let client = CoreClient::from_provider_metadata(
            metadata,
            ClientId::new(config.oidc_client_id.clone()),
            Some(ClientSecret::new(config.oidc_client_secret.clone())),
        )
        .set_redirect_uri(
            RedirectUrl::new(redirect.to_string())
                .map_err(|error| ApiError::config(format!("OIDC callback URL 无效: {error}")))?,
        );
        Ok(Self {
            client: Arc::new(client),
            http,
            issuer: config.oidc_issuer.clone(),
            client_id: config.oidc_client_id.clone(),
            trusted_audiences: Arc::new(config.oidc_trusted_audiences.clone()),
            end_session_url,
            logout_jwks: Arc::new(logout_jwks),
        })
    }
}

#[derive(Deserialize)]
pub struct LoginQuery {
    #[serde(default = "default_return_path")]
    return_path: String,
}

#[derive(Deserialize)]
pub struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}

#[derive(Deserialize)]
pub struct BackchannelLogoutForm {
    logout_token: String,
}

#[derive(Debug, Deserialize)]
struct LogoutClaims {
    iss: String,
    #[serde(rename = "aud")]
    _aud: serde_json::Value,
    exp: i64,
    iat: i64,
    jti: Option<String>,
    sub: Option<String>,
    sid: Option<String>,
    nonce: Option<serde_json::Value>,
    events: HashMap<String, serde_json::Value>,
}

#[derive(Deserialize)]
struct OidcSessionClaim {
    sid: Option<String>,
}

const BACKCHANNEL_LOGOUT_EVENT: &str = "http://schemas.openid.net/event/backchannel-logout";

fn default_return_path() -> String {
    "/".to_owned()
}

pub async fn login(
    State(state): State<AppState>,
    Query(query): Query<LoginQuery>,
) -> ApiResult<Redirect> {
    let return_path = safe_return_path(&query.return_path)?;
    let (pkce_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();
    let (auth_url, csrf, nonce) = state
        .oidc
        .client
        .authorize_url(
            CoreAuthenticationFlow::AuthorizationCode,
            CsrfToken::new_random,
            Nonce::new_random,
        )
        .add_scope(Scope::new("profile".to_owned()))
        .add_scope(Scope::new("email".to_owned()))
        .set_pkce_challenge(pkce_challenge)
        .url();

    sqlx::query(
        "INSERT INTO auth_flows (state_hash, pkce_verifier, nonce, return_path, expires_at) \
         VALUES ($1, $2, $3, $4, now() + interval '10 minutes')",
    )
    .bind(hash(csrf.secret().as_bytes()).to_vec())
    .bind(pkce_verifier.secret())
    .bind(nonce.secret())
    .bind(return_path)
    .execute(&state.pool)
    .await?;
    sqlx::query(
        "DELETE FROM auth_flows WHERE state_hash IN (SELECT state_hash FROM auth_flows ORDER BY created_at DESC OFFSET 10000)",
    )
    .execute(&state.pool)
    .await?;

    Ok(Redirect::to(auth_url.as_str()))
}

pub async fn callback(
    State(state): State<AppState>,
    jar: CookieJar,
    Query(query): Query<CallbackQuery>,
) -> ApiResult<Response> {
    if let Some(error) = query.error {
        return Err(ApiError::BadRequest(format!(
            "OIDC 登录被拒绝: {}",
            query.error_description.unwrap_or(error)
        )));
    }
    let code = query
        .code
        .ok_or_else(|| ApiError::BadRequest("缺少 code".to_owned()))?;
    let returned_state = query
        .state
        .ok_or_else(|| ApiError::BadRequest("缺少 state".to_owned()))?;

    let flow = sqlx::query_as::<_, (String, String, String)>(
        "DELETE FROM auth_flows WHERE state_hash = $1 AND expires_at > now() \
         RETURNING pkce_verifier, nonce, return_path",
    )
    .bind(hash(returned_state.as_bytes()).to_vec())
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| ApiError::BadRequest("登录状态已失效或已使用".to_owned()))?;

    let token_response = state
        .oidc
        .client
        .exchange_code(AuthorizationCode::new(code))
        .map_err(|error| ApiError::Oidc(error.to_string()))?
        .set_pkce_verifier(PkceCodeVerifier::new(flow.0))
        .request_async(&state.oidc.http)
        .await
        .map_err(|error| ApiError::Oidc(error.to_string()))?;
    let id_token = token_response
        .id_token()
        .ok_or_else(|| ApiError::Oidc("身份提供方未返回 ID token".to_owned()))?;
    let verifier = state
        .oidc
        .client
        .id_token_verifier()
        .require_audience_match(false);
    let claims = id_token
        .claims(&verifier, &Nonce::new(flow.1))
        .map_err(|error| ApiError::Oidc(format!("ID token 校验失败: {error}")))?;
    validate_token_audience(
        &state.oidc.client_id,
        &state.oidc.trusted_audiences,
        claims.audiences(),
        claims.authorized_party(),
    )?;
    // The ID token has passed signature, issuer, expiry and nonce validation above,
    // followed by the project-specific audience and azp policy.
    // Decode only the optional OIDC session identifier used for logout correlation.
    let oidc_session_id =
        jsonwebtoken::dangerous::insecure_decode::<OidcSessionClaim>(id_token.to_string())
            .map_err(|error| ApiError::Oidc(format!("ID token 会话标识解析失败: {error}")))?
            .claims
            .sid;

    let subject = claims.subject().as_str().to_owned();
    if subject.is_empty() || subject.len() > 512 {
        return Err(ApiError::Oidc("ID token 的 subject 长度无效".to_owned()));
    }
    let email = claims
        .email()
        .map(|value| value.as_str().to_owned())
        .filter(|value| value.chars().count() <= 320);
    let display_name = claims
        .name()
        .and_then(|localized| localized.get(None))
        .map(|value| value.as_str().to_owned())
        .or_else(|| {
            claims
                .preferred_username()
                .map(|value| value.as_str().to_owned())
        })
        .or_else(|| email.clone())
        .unwrap_or_else(|| "Lumeno 用户".to_owned());
    let display_name: String = display_name.chars().take(160).collect();
    let display_name = if display_name.trim().is_empty() {
        "Lumeno 用户".to_owned()
    } else {
        display_name
    };
    let mut tx = state.pool.begin().await?;
    let user_id = Uuid::now_v7();
    let user_id: Uuid = sqlx::query_scalar(
        "INSERT INTO app_users (id, oidc_issuer, oidc_subject, display_name, email) \
         VALUES ($1, $2, $3, $4, $5) \
         ON CONFLICT (oidc_issuer, oidc_subject) DO UPDATE \
         SET display_name = EXCLUDED.display_name, email = EXCLUDED.email, updated_at = now() \
         RETURNING id",
    )
    .bind(user_id)
    .bind(&state.oidc.issuer)
    .bind(&subject)
    .bind(display_name)
    .bind(email)
    .fetch_one(&mut *tx)
    .await?;

    let session_token = random_token(32);
    let csrf_token = random_token(32);
    let session_token_hash = hash(session_token.as_bytes()).to_vec();
    let idle_seconds = i64::try_from(state.config.session_idle.as_secs())
        .map_err(|_| ApiError::Internal("会话配置超出范围".to_owned()))?;
    let absolute_seconds = i64::try_from(state.config.session_absolute.as_secs())
        .map_err(|_| ApiError::Internal("会话配置超出范围".to_owned()))?;
    sqlx::query(
        "INSERT INTO auth_sessions \
         (token_hash, user_id, oidc_subject, oidc_session_id, csrf_hash, idle_timeout_seconds, idle_expires_at, absolute_expires_at) \
         VALUES ($1, $2, $3, $4, $5, $6, now() + make_interval(secs => $6), now() + make_interval(secs => $7))",
    )
    .bind(&session_token_hash)
    .bind(user_id)
    .bind(subject)
    .bind(oidc_session_id)
    .bind(hash(csrf_token.as_bytes()).to_vec())
    .bind(idle_seconds)
    .bind(absolute_seconds)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "DELETE FROM auth_sessions WHERE user_id=$1 AND token_hash IN (\
           SELECT token_hash FROM auth_sessions WHERE user_id=$1 AND token_hash<>$2 \
           ORDER BY created_at DESC,token_hash OFFSET 19\
         )",
    )
    .bind(user_id)
    .bind(&session_token_hash)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    let jar = jar
        .add(session_cookie(&state.config, session_token))
        .add(csrf_cookie(&state.config, csrf_token));
    Ok((jar, Redirect::to(&flow.2)).into_response())
}

fn validate_token_audience(
    client_id: &str,
    trusted_audiences: &HashSet<String>,
    audiences: &[Audience],
    authorized_party: Option<&ClientId>,
) -> ApiResult<()> {
    if audiences.is_empty() {
        return Err(ApiError::Oidc("ID token 缺少 audience".to_owned()));
    }
    let client_is_audience = audiences
        .iter()
        .any(|audience| audience.as_str() == client_id);
    if !trusted_audiences.is_empty()
        && let Some(untrusted) = audiences.iter().find(|audience| {
            audience.as_str() != client_id && !trusted_audiences.contains(audience.as_str())
        })
    {
        return Err(ApiError::Oidc(format!(
            "ID token 包含未受信任的 audience `{}`",
            untrusted.as_str()
        )));
    }
    match authorized_party {
        Some(value) if value.as_str() != client_id => Err(ApiError::Oidc(format!(
            "ID token 的 azp 与 Client ID 不匹配: expected `{client_id}`"
        ))),
        None if audiences.len() > 1 || !client_is_audience => Err(ApiError::Oidc(
            "ID token 的 audience 未唯一绑定当前 Client ID，且缺少匹配的 azp".to_owned(),
        )),
        _ => Ok(()),
    }
}

pub async fn backchannel_logout(
    State(state): State<AppState>,
    Form(form): Form<BackchannelLogoutForm>,
) -> ApiResult<StatusCode> {
    let claims = verify_logout_token(&state.oidc, &form.logout_token)?;
    let mut tx = state.pool.begin().await?;

    if let Some(jti) = claims.jti.as_deref() {
        let expires_at = DateTime::<Utc>::from_timestamp(claims.exp, 0)
            .ok_or_else(|| ApiError::BadRequest("logout_token 的 exp 无效".to_owned()))?;
        let inserted = sqlx::query(
            "INSERT INTO used_logout_tokens (jti, expires_at) VALUES ($1, $2) \
             ON CONFLICT (jti) DO NOTHING",
        )
        .bind(jti)
        .bind(expires_at)
        .execute(&mut *tx)
        .await?;
        if inserted.rows_affected() == 0 {
            tx.rollback().await?;
            return Ok(StatusCode::OK);
        }
    }

    if let Some(sid) = claims.sid.as_deref() {
        sqlx::query("DELETE FROM auth_sessions WHERE oidc_session_id = $1")
            .bind(sid)
            .execute(&mut *tx)
            .await?;
    } else if let Some(subject) = claims.sub.as_deref() {
        sqlx::query("DELETE FROM auth_sessions WHERE oidc_subject = $1")
            .bind(subject)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(StatusCode::OK)
}

fn verify_logout_token(runtime: &OidcRuntime, token: &str) -> ApiResult<LogoutClaims> {
    let header = decode_header(token)
        .map_err(|error| ApiError::BadRequest(format!("logout_token 头部无效: {error}")))?;
    if header.alg.family() == jsonwebtoken::AlgorithmFamily::Hmac {
        return Err(ApiError::BadRequest(
            "logout_token 不允许使用对称签名".to_owned(),
        ));
    }
    let kid = header
        .kid
        .as_deref()
        .ok_or_else(|| ApiError::BadRequest("logout_token 缺少 kid".to_owned()))?;
    let jwk = runtime
        .logout_jwks
        .find(kid)
        .ok_or_else(|| ApiError::BadRequest("logout_token 的 kid 未知".to_owned()))?;
    if let Some(key_algorithm) = jwk.common.key_algorithm {
        let expected = Algorithm::try_from(key_algorithm)
            .map_err(|_| ApiError::BadRequest("logout_token 使用了不支持的 JWK 算法".to_owned()))?;
        if expected != header.alg {
            return Err(ApiError::BadRequest(
                "logout_token 的签名算法与 JWK 不一致".to_owned(),
            ));
        }
    }
    let key = DecodingKey::from_jwk(jwk)
        .map_err(|error| ApiError::BadRequest(format!("logout_token JWK 无效: {error}")))?;
    let mut validation = Validation::new(header.alg);
    validation.set_audience(&[runtime.client_id.as_str()]);
    validation.set_issuer(&[runtime.issuer.as_str()]);
    validation.set_required_spec_claims(&["exp", "aud", "iss"]);
    let claims = decode::<LogoutClaims>(token, &key, &validation)
        .map_err(|error| ApiError::BadRequest(format!("logout_token 校验失败: {error}")))?
        .claims;
    validate_logout_claims(&claims)?;
    Ok(claims)
}

fn validate_logout_claims(claims: &LogoutClaims) -> ApiResult<()> {
    let valid_subject = claims
        .sub
        .as_deref()
        .is_some_and(|value| !value.is_empty() && value.len() <= 2048);
    let valid_session = claims
        .sid
        .as_deref()
        .is_some_and(|value| !value.is_empty() && value.len() <= 2048);
    if claims.iss.is_empty() || (!valid_subject && !valid_session) {
        return Err(ApiError::BadRequest(
            "logout_token 必须包含 sub 或 sid".to_owned(),
        ));
    }
    if claims
        .jti
        .as_deref()
        .is_some_and(|value| value.is_empty() || value.len() > 512)
    {
        return Err(ApiError::BadRequest("logout_token 的 jti 无效".to_owned()));
    }
    if claims.nonce.is_some() {
        return Err(ApiError::BadRequest(
            "logout_token 不允许包含 nonce".to_owned(),
        ));
    }
    if !claims
        .events
        .get(BACKCHANNEL_LOGOUT_EVENT)
        .is_some_and(serde_json::Value::is_object)
    {
        return Err(ApiError::BadRequest(
            "logout_token 缺少后通道登出事件".to_owned(),
        ));
    }
    let now = Utc::now().timestamp();
    if claims.iat > now + 60 || claims.iat < now - 600 {
        return Err(ApiError::BadRequest(
            "logout_token 的 iat 超出允许时间窗口".to_owned(),
        ));
    }
    if claims.exp <= claims.iat || claims.exp > claims.iat + 900 {
        return Err(ApiError::BadRequest(
            "logout_token 的有效期必须在 15 分钟以内".to_owned(),
        ));
    }
    Ok(())
}

pub async fn me(State(state): State<AppState>, jar: CookieJar) -> ApiResult<Json<MeResponse>> {
    let user = require_user(&state.pool, &jar, None, false).await?;
    let csrf_token = jar
        .get(CSRF_COOKIE)
        .map(|cookie| cookie.value().to_owned())
        .ok_or(ApiError::Csrf)?;
    Ok(Json(MeResponse {
        id: user.id,
        display_name: user.display_name,
        email: user.email,
        timezone: user.timezone,
        csrf_token,
    }))
}

pub async fn update_settings(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    Json(request): Json<UpdateSettingsRequest>,
) -> ApiResult<Json<MeResponse>> {
    let user = require_user(&state.pool, &jar, Some(&headers), true).await?;
    validate_timezone(&request.timezone)?;
    sqlx::query("UPDATE app_users SET timezone = $1, updated_at = now() WHERE id = $2")
        .bind(&request.timezone)
        .bind(user.id)
        .execute(&state.pool)
        .await?;
    let csrf_token = jar
        .get(CSRF_COOKIE)
        .map(|cookie| cookie.value().to_owned())
        .ok_or(ApiError::Csrf)?;
    Ok(Json(MeResponse {
        id: user.id,
        display_name: user.display_name,
        email: user.email,
        timezone: request.timezone,
        csrf_token,
    }))
}

pub async fn logout(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
) -> ApiResult<Response> {
    let _ = require_user(&state.pool, &jar, Some(&headers), true).await?;
    if let Some(cookie) = jar.get(SESSION_COOKIE) {
        sqlx::query("DELETE FROM auth_sessions WHERE token_hash = $1")
            .bind(hash(cookie.value().as_bytes()).to_vec())
            .execute(&state.pool)
            .await?;
    }
    let jar = jar
        .remove(removal_cookie(SESSION_COOKIE, state.config.secure_cookies))
        .remove(removal_cookie(CSRF_COOKIE, state.config.secure_cookies));

    let redirect = if let Some(endpoint) = &state.oidc.end_session_url {
        let mut url = url::Url::parse(endpoint)
            .map_err(|error| ApiError::Internal(format!("end_session URL 无效: {error}")))?;
        url.query_pairs_mut()
            .append_pair("client_id", &state.oidc.client_id)
            .append_pair("post_logout_redirect_uri", state.config.public_url.as_str());
        url.to_string()
    } else {
        state.config.public_url.to_string()
    };
    Ok((jar, Json(serde_json::json!({ "redirectTo": redirect }))).into_response())
}

pub async fn require_user(
    pool: &PgPool,
    jar: &CookieJar,
    headers: Option<&HeaderMap>,
    require_csrf: bool,
) -> ApiResult<SessionUser> {
    let session = jar.get(SESSION_COOKIE).ok_or(ApiError::Unauthorized)?;
    let user = sqlx::query_as::<_, SessionUser>(
        "SELECT u.id, u.display_name, u.email, u.timezone, s.csrf_hash \
         FROM auth_sessions s JOIN app_users u ON u.id = s.user_id \
         WHERE s.token_hash = $1 AND s.idle_expires_at > now() AND s.absolute_expires_at > now()",
    )
    .bind(hash(session.value().as_bytes()).to_vec())
    .fetch_optional(pool)
    .await?
    .ok_or(ApiError::Unauthorized)?;

    if require_csrf {
        let cookie_value = jar
            .get(CSRF_COOKIE)
            .map(|cookie| cookie.value())
            .ok_or(ApiError::Csrf)?;
        let header_value = headers
            .and_then(|values| values.get("x-csrf-token"))
            .and_then(|value| value.to_str().ok())
            .ok_or(ApiError::Csrf)?;
        if cookie_value != header_value
            || hash(header_value.as_bytes()).as_slice() != user.csrf_hash
        {
            return Err(ApiError::Csrf);
        }
    }

    sqlx::query(
        "UPDATE auth_sessions SET last_seen_at = now(), \
         idle_expires_at = LEAST(absolute_expires_at, now() + make_interval(secs => idle_timeout_seconds)) \
         WHERE token_hash = $1 AND last_seen_at < now() - interval '15 minutes'",
    )
    .bind(hash(session.value().as_bytes()).to_vec())
    .execute(pool)
    .await?;
    Ok(user)
}

fn safe_return_path(value: &str) -> ApiResult<String> {
    if value.starts_with('/') && !value.starts_with("//") && value.len() <= 512 {
        Ok(value.to_owned())
    } else {
        Err(ApiError::BadRequest("return_path 无效".to_owned()))
    }
}

fn validate_timezone(value: &str) -> ApiResult<()> {
    if value.len() > 64
        || value.is_empty()
        || !value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '/' | '_' | '-' | '+'))
    {
        return Err(ApiError::validation("时区必须是有效的 IANA 时区名称"));
    }
    Ok(())
}

fn random_token(bytes: usize) -> String {
    let mut buffer = vec![0_u8; bytes];
    rand::rng().fill_bytes(&mut buffer);
    URL_SAFE_NO_PAD.encode(buffer)
}

pub fn hash(value: &[u8]) -> [u8; 32] {
    Sha256::digest(value).into()
}

fn session_cookie(config: &Config, value: String) -> Cookie<'static> {
    let mut cookie = Cookie::new(SESSION_COOKIE, value);
    cookie.set_http_only(true);
    cookie.set_same_site(SameSite::Lax);
    cookie.set_secure(config.secure_cookies);
    cookie.set_path("/");
    cookie.set_max_age(time::Duration::days(30));
    cookie
}

fn csrf_cookie(config: &Config, value: String) -> Cookie<'static> {
    let mut cookie = Cookie::new(CSRF_COOKIE, value);
    cookie.set_http_only(false);
    cookie.set_same_site(SameSite::Strict);
    cookie.set_secure(config.secure_cookies);
    cookie.set_path("/");
    cookie.set_max_age(time::Duration::days(30));
    cookie
}

fn removal_cookie(name: &'static str, secure: bool) -> Cookie<'static> {
    let mut cookie = Cookie::new(name, "");
    cookie.set_path("/");
    cookie.set_secure(secure);
    cookie.make_removal();
    cookie
}

pub async fn cleanup_auth(pool: &PgPool) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM auth_flows WHERE expires_at <= now()")
        .execute(pool)
        .await?;
    sqlx::query(
        "DELETE FROM auth_sessions WHERE absolute_expires_at <= now() OR idle_expires_at <= now()",
    )
    .execute(pool)
    .await?;
    sqlx::query("DELETE FROM used_logout_tokens WHERE expires_at <= now()")
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn health() -> StatusCode {
    StatusCode::NO_CONTENT
}

pub async fn ready(State(state): State<AppState>) -> ApiResult<StatusCode> {
    sqlx::query_scalar::<_, i32>("SELECT 1")
        .fetch_one(&state.pool)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn valid_logout_claims() -> LogoutClaims {
        let now = Utc::now().timestamp();
        LogoutClaims {
            iss: "https://issuer.example".to_owned(),
            _aud: json!("lumeno"),
            exp: now + 300,
            iat: now,
            jti: Some("logout-1".to_owned()),
            sub: Some("user-1".to_owned()),
            sid: None,
            nonce: None,
            events: HashMap::from([(BACKCHANNEL_LOGOUT_EVENT.to_owned(), json!({}))]),
        }
    }

    #[test]
    fn accepts_valid_logout_claims() {
        assert!(validate_logout_claims(&valid_logout_claims()).is_ok());
    }

    #[test]
    fn rejects_nonce_and_missing_logout_target() {
        let mut claims = valid_logout_claims();
        claims.nonce = Some(json!("forbidden"));
        assert!(validate_logout_claims(&claims).is_err());

        let mut claims = valid_logout_claims();
        claims.sub = None;
        assert!(validate_logout_claims(&claims).is_err());
    }

    #[test]
    fn rejects_stale_or_long_lived_logout_tokens() {
        let mut claims = valid_logout_claims();
        claims.iat = Utc::now().timestamp() - 601;
        assert!(validate_logout_claims(&claims).is_err());

        let mut claims = valid_logout_claims();
        claims.exp = claims.iat + 901;
        assert!(validate_logout_claims(&claims).is_err());
    }

    #[test]
    fn empty_audience_allowlist_accepts_provider_project_id_with_matching_azp() {
        let audiences = [Audience::new("zitadel-project-id".to_owned())];
        let matching = ClientId::new("lumeno-client".to_owned());

        assert!(
            validate_token_audience(
                "lumeno-client",
                &HashSet::new(),
                &audiences,
                Some(&matching)
            )
            .is_ok()
        );
        assert!(
            validate_token_audience("lumeno-client", &HashSet::new(), &audiences, None).is_err()
        );
    }

    #[test]
    fn configured_audience_allowlist_rejects_untrusted_provider_ids() {
        let trusted = HashSet::from(["385667293859801872".to_owned()]);
        let audiences = [
            Audience::new("lumeno-client".to_owned()),
            Audience::new("385667293859801872".to_owned()),
        ];
        let matching = ClientId::new("lumeno-client".to_owned());

        assert!(
            validate_token_audience("lumeno-client", &trusted, &audiences, Some(&matching)).is_ok()
        );
        let untrusted = [Audience::new("another-project".to_owned())];
        assert!(
            validate_token_audience("lumeno-client", &trusted, &untrusted, Some(&matching))
                .is_err()
        );
    }

    #[test]
    fn audience_validation_rejects_missing_or_mismatched_client_binding() {
        let audiences = [Audience::new("lumeno-client".to_owned())];
        let mismatched = ClientId::new("another-client".to_owned());

        assert!(
            validate_token_audience("lumeno-client", &HashSet::new(), &audiences, None).is_ok()
        );
        assert!(
            validate_token_audience(
                "lumeno-client",
                &HashSet::new(),
                &audiences,
                Some(&mismatched)
            )
            .is_err()
        );
        assert!(validate_token_audience("lumeno-client", &HashSet::new(), &[], None).is_err());
    }
}
