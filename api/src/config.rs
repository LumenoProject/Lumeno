use std::{collections::HashSet, env, net::SocketAddr, path::PathBuf, time::Duration};

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use url::Url;

use crate::error::{ApiError, ApiResult};

#[derive(Clone, Debug)]
pub struct Config {
    pub bind: SocketAddr,
    pub database_url: String,
    pub static_dir: PathBuf,
    pub public_url: Url,
    pub oidc_issuer: String,
    pub oidc_client_id: String,
    pub oidc_client_secret: String,
    pub oidc_trusted_audiences: HashSet<String>,
    pub secure_cookies: bool,
    pub session_idle: Duration,
    pub session_absolute: Duration,
    pub notification_encryption_key: [u8; 32],
    pub notification_endpoint_allowlist: HashSet<String>,
}

impl Config {
    pub fn from_env() -> ApiResult<Self> {
        let public_url = required("LUMENO_PUBLIC_URL")?
            .parse::<Url>()
            .map_err(|error| ApiError::config(format!("LUMENO_PUBLIC_URL 无效: {error}")))?;
        if public_url.scheme() != "https" && public_url.host_str() != Some("localhost") {
            return Err(ApiError::config(
                "LUMENO_PUBLIC_URL 在非 localhost 环境必须使用 HTTPS",
            ));
        }
        if public_url.path() != "/"
            || public_url.query().is_some()
            || public_url.fragment().is_some()
            || !public_url.username().is_empty()
            || public_url.password().is_some()
        {
            return Err(ApiError::config(
                "LUMENO_PUBLIC_URL 必须是以 / 结尾且不含路径、凭据、查询或片段的站点根 URL",
            ));
        }

        let bind = env::var("LUMENO_BIND")
            .unwrap_or_else(|_| "0.0.0.0:3000".to_owned())
            .parse()
            .map_err(|error| ApiError::config(format!("LUMENO_BIND 无效: {error}")))?;

        let session_idle = Duration::from_secs(read_u64("SESSION_IDLE_SECONDS", 604_800)?);
        let session_absolute =
            Duration::from_secs(read_u64("SESSION_ABSOLUTE_SECONDS", 2_592_000)?);
        if session_idle.as_secs() < 60
            || session_absolute < session_idle
            || session_absolute.as_secs() > 31_536_000
        {
            return Err(ApiError::config(
                "会话时长必须满足 60 秒 <= SESSION_IDLE_SECONDS <= SESSION_ABSOLUTE_SECONDS <= 31536000 秒",
            ));
        }

        let oidc_client_id = required("OIDC_CLIENT_ID")?;

        Ok(Self {
            bind,
            database_url: database_url_from_env()?,
            static_dir: env::var("LUMENO_STATIC_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|_| PathBuf::from("web/dist")),
            oidc_issuer: required("OIDC_ISSUER")?,
            oidc_client_id,
            oidc_client_secret: required("OIDC_CLIENT_SECRET")?,
            oidc_trusted_audiences: parse_trusted_audiences(
                optional("OIDC_TRUSTED_AUDIENCES").as_deref(),
            )?,
            secure_cookies: public_url.scheme() == "https",
            public_url,
            session_idle,
            session_absolute,
            notification_encryption_key: parse_notification_encryption_key(&required(
                "NOTIFICATION_ENCRYPTION_KEY",
            )?)?,
            notification_endpoint_allowlist: parse_notification_endpoint_allowlist(
                optional("NOTIFICATION_ENDPOINT_ALLOWLIST").as_deref(),
            )?,
        })
    }
}

fn database_url_from_env() -> ApiResult<String> {
    resolve_database_url(
        optional("DATABASE_URL"),
        optional("POSTGRES_USER"),
        optional("POSTGRES_PASSWORD"),
        optional("POSTGRES_DB"),
    )
}

fn resolve_database_url(
    database_url: Option<String>,
    postgres_user: Option<String>,
    postgres_password: Option<String>,
    postgres_db: Option<String>,
) -> ApiResult<String> {
    if let Some(database_url) = database_url {
        return Ok(database_url);
    }

    let user = postgres_user.unwrap_or_else(|| "lumeno".to_owned());
    let password = postgres_password
        .ok_or_else(|| ApiError::config("未配置 DATABASE_URL 时必须设置 POSTGRES_PASSWORD"))?;
    let database = postgres_db.unwrap_or_else(|| "lumeno".to_owned());
    let mut url = Url::parse("postgresql://db:5432/")
        .map_err(|_| ApiError::config("无法生成内置 PostgreSQL 连接地址"))?;
    url.set_username(&user)
        .map_err(|_| ApiError::config("POSTGRES_USER 无法写入数据库连接地址"))?;
    url.set_password(Some(&password))
        .map_err(|_| ApiError::config("POSTGRES_PASSWORD 无法写入数据库连接地址"))?;
    url.path_segments_mut()
        .map_err(|_| ApiError::config("POSTGRES_DB 无法写入数据库连接地址"))?
        .clear()
        .push(&database);
    Ok(url.into())
}

const MAX_NOTIFICATION_ENDPOINT_ORIGINS: usize = 32;
const MAX_NOTIFICATION_ENDPOINT_ORIGIN_LENGTH: usize = 2048;

fn parse_notification_endpoint_allowlist(value: Option<&str>) -> ApiResult<HashSet<String>> {
    let Some(value) = value else {
        return Ok(HashSet::new());
    };
    if value.trim().is_empty() {
        return Ok(HashSet::new());
    }

    let mut origins = HashSet::new();
    for (index, raw_origin) in value.split(',').enumerate() {
        if index >= MAX_NOTIFICATION_ENDPOINT_ORIGINS {
            return Err(ApiError::config(format!(
                "NOTIFICATION_ENDPOINT_ALLOWLIST 最多允许 {MAX_NOTIFICATION_ENDPOINT_ORIGINS} 个 origin"
            )));
        }
        let raw_origin = raw_origin.trim();
        if raw_origin.is_empty() || raw_origin.len() > MAX_NOTIFICATION_ENDPOINT_ORIGIN_LENGTH {
            return Err(ApiError::config(
                "NOTIFICATION_ENDPOINT_ALLOWLIST 包含空项或过长的 origin",
            ));
        }
        let url = Url::parse(raw_origin)
            .map_err(|_| ApiError::config("NOTIFICATION_ENDPOINT_ALLOWLIST 包含无效 URL origin"))?;
        if url.scheme() != "https"
            || url.host().is_none()
            || url.path() != "/"
            || url.query().is_some()
            || url.fragment().is_some()
            || !url.username().is_empty()
            || url.password().is_some()
        {
            return Err(ApiError::config(
                "NOTIFICATION_ENDPOINT_ALLOWLIST 只能包含不带路径、凭据、查询或片段的 HTTPS origin",
            ));
        }
        origins.insert(url.origin().ascii_serialization());
    }
    Ok(origins)
}

fn parse_notification_encryption_key(value: &str) -> ApiResult<[u8; 32]> {
    let decoded = URL_SAFE_NO_PAD
        .decode(value.trim())
        .map_err(|_| ApiError::config("NOTIFICATION_ENCRYPTION_KEY 必须是 Base64URL 编码"))?;
    decoded
        .try_into()
        .map_err(|_| ApiError::config("NOTIFICATION_ENCRYPTION_KEY 解码后必须恰好为 32 字节"))
}

const MAX_TRUSTED_AUDIENCES: usize = 32;
const MAX_AUDIENCE_LENGTH: usize = 2048;

fn parse_trusted_audiences(value: Option<&str>) -> ApiResult<HashSet<String>> {
    let Some(value) = value else {
        return Ok(HashSet::new());
    };
    if value.trim().is_empty() {
        return Ok(HashSet::new());
    }

    let mut audiences = HashSet::new();
    for raw_audience in value.split(',') {
        let audience = raw_audience.trim();
        if audience.is_empty() {
            return Err(ApiError::config(
                "OIDC_TRUSTED_AUDIENCES 必须是非空 audience 的逗号分隔列表",
            ));
        }
        if audience.len() > MAX_AUDIENCE_LENGTH {
            return Err(ApiError::config(format!(
                "OIDC_TRUSTED_AUDIENCES 中的 audience 长度不能超过 {MAX_AUDIENCE_LENGTH} 字节"
            )));
        }
        audiences.insert(audience.to_owned());
        if audiences.len() > MAX_TRUSTED_AUDIENCES {
            return Err(ApiError::config(format!(
                "OIDC_TRUSTED_AUDIENCES 最多允许 {MAX_TRUSTED_AUDIENCES} 个不同的 audience"
            )));
        }
    }
    Ok(audiences)
}

fn optional(name: &str) -> Option<String> {
    env::var(name).ok().filter(|value| !value.trim().is_empty())
}

fn required(name: &str) -> ApiResult<String> {
    env::var(name).map_err(|_| ApiError::config(format!("缺少必需环境变量 {name}")))
}

fn read_u64(name: &str, default: u64) -> ApiResult<u64> {
    env::var(name)
        .ok()
        .map(|value| {
            value
                .parse()
                .map_err(|error| ApiError::config(format!("{name} 无效: {error}")))
        })
        .transpose()
        .map(|value| value.unwrap_or(default))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn database_url_prefers_explicit_configuration() {
        let explicit = "postgresql://external.example/lumeno?sslmode=require";
        assert_eq!(
            resolve_database_url(Some(explicit.to_owned()), None, None, None).unwrap(),
            explicit
        );
    }

    #[test]
    fn database_url_defaults_to_the_bundled_postgres_service() {
        assert_eq!(
            resolve_database_url(None, None, Some("secret".to_owned()), None).unwrap(),
            "postgresql://lumeno:secret@db:5432/lumeno"
        );
    }

    #[test]
    fn database_url_escapes_bundled_postgres_credentials() {
        assert_eq!(
            resolve_database_url(
                None,
                Some("user name".to_owned()),
                Some("p@ss:/?#".to_owned()),
                Some("db/name".to_owned()),
            )
            .unwrap(),
            "postgresql://user%20name:p%40ss%3A%2F%3F%23@db:5432/db%2Fname"
        );
    }

    #[test]
    fn database_url_requires_a_password_for_the_bundled_service() {
        assert!(resolve_database_url(None, None, None, None).is_err());
    }

    #[test]
    fn parses_and_deduplicates_trusted_audiences() {
        let audiences = parse_trusted_audiences(Some("app-1, project-2,app-1")).unwrap();

        assert_eq!(audiences.len(), 2);
        assert!(audiences.contains("app-1"));
        assert!(audiences.contains("project-2"));
    }

    #[test]
    fn accepts_missing_trusted_audiences() {
        assert!(parse_trusted_audiences(None).unwrap().is_empty());
        assert!(parse_trusted_audiences(Some("  ")).unwrap().is_empty());
    }

    #[test]
    fn rejects_empty_trusted_audience_items() {
        assert!(parse_trusted_audiences(Some("app-1,,project-2")).is_err());
        assert!(parse_trusted_audiences(Some("app-1,")).is_err());
    }

    #[test]
    fn bounds_trusted_audience_configuration() {
        let long_audience = "a".repeat(MAX_AUDIENCE_LENGTH + 1);
        assert!(parse_trusted_audiences(Some(&long_audience)).is_err());

        let too_many = (0..=MAX_TRUSTED_AUDIENCES)
            .map(|index| format!("audience-{index}"))
            .collect::<Vec<_>>()
            .join(",");
        assert!(parse_trusted_audiences(Some(&too_many)).is_err());
    }

    #[test]
    fn accepts_a_32_byte_notification_key() {
        let encoded = URL_SAFE_NO_PAD.encode([7_u8; 32]);
        assert_eq!(
            parse_notification_encryption_key(&encoded).unwrap(),
            [7_u8; 32]
        );
    }

    #[test]
    fn rejects_notification_keys_with_the_wrong_length() {
        let encoded = URL_SAFE_NO_PAD.encode([7_u8; 31]);
        assert!(parse_notification_encryption_key(&encoded).is_err());
    }

    #[test]
    fn parses_and_normalizes_notification_endpoint_origins() {
        let origins = parse_notification_endpoint_allowlist(Some(
            "https://192.168.1.20:8443, https://BARK.INTERNAL.example:443,https://bark.internal.example",
        ))
        .unwrap();

        assert_eq!(origins.len(), 2);
        assert!(origins.contains("https://192.168.1.20:8443"));
        assert!(origins.contains("https://bark.internal.example"));
    }

    #[test]
    fn rejects_non_origin_notification_allowlist_entries() {
        for value in [
            "http://192.168.1.20",
            "https://192.168.1.20/device-key",
            "https://user:password@192.168.1.20",
            "https://192.168.1.20?key=value",
            "https://192.168.1.20/#fragment",
            "not-a-url",
        ] {
            assert!(
                parse_notification_endpoint_allowlist(Some(value)).is_err(),
                "accepted {value}"
            );
        }
    }

    #[test]
    fn bounds_notification_endpoint_allowlist() {
        let too_many = (0..=MAX_NOTIFICATION_ENDPOINT_ORIGINS)
            .map(|index| format!("https://10.0.0.{index}:8443"))
            .collect::<Vec<_>>()
            .join(",");

        assert!(parse_notification_endpoint_allowlist(Some(&too_many)).is_err());

        let repeated = std::iter::repeat_n("https://10.0.0.1:8443", 33)
            .collect::<Vec<_>>()
            .join(",");
        assert!(parse_notification_endpoint_allowlist(Some(&repeated)).is_err());

        let too_long = format!("https://{}.example", "a".repeat(2048));
        assert!(parse_notification_endpoint_allowlist(Some(&too_long)).is_err());
    }
}
