use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;
use tracing::error;

pub type ApiResult<T> = Result<T, ApiError>;

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("{0}")]
    Config(String),
    #[error("{0}")]
    BadRequest(String),
    #[error("需要登录")]
    Unauthorized,
    #[error("CSRF 校验失败")]
    Csrf,
    #[error("资源不存在")]
    NotFound,
    #[error("{0}")]
    Conflict(String),
    #[error("{0}")]
    Validation(String),
    #[error("数据库操作失败")]
    Database(#[from] sqlx::Error),
    #[error("身份提供方通信失败: {0}")]
    Oidc(String),
    #[error("通知发送失败: {0}")]
    Delivery(String),
    #[error("服务内部错误")]
    Internal(String),
}

impl ApiError {
    pub fn config(message: impl Into<String>) -> Self {
        Self::Config(message.into())
    }

    pub fn validation(message: impl Into<String>) -> Self {
        Self::Validation(message.into())
    }
}

#[derive(Serialize)]
struct Problem {
    r#type: &'static str,
    title: &'static str,
    status: u16,
    detail: String,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, kind, title, expose) = match &self {
            Self::Config(_) | Self::Internal(_) | Self::Database(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "urn:lumeno:error:internal",
                "服务内部错误",
                false,
            ),
            Self::BadRequest(_) => (
                StatusCode::BAD_REQUEST,
                "urn:lumeno:error:bad-request",
                "请求无效",
                true,
            ),
            Self::Unauthorized => (
                StatusCode::UNAUTHORIZED,
                "urn:lumeno:error:unauthorized",
                "需要登录",
                true,
            ),
            Self::Csrf => (
                StatusCode::FORBIDDEN,
                "urn:lumeno:error:csrf",
                "请求校验失败",
                true,
            ),
            Self::NotFound => (
                StatusCode::NOT_FOUND,
                "urn:lumeno:error:not-found",
                "资源不存在",
                true,
            ),
            Self::Conflict(_) => (
                StatusCode::CONFLICT,
                "urn:lumeno:error:conflict",
                "状态冲突",
                true,
            ),
            Self::Validation(_) => (
                StatusCode::UNPROCESSABLE_ENTITY,
                "urn:lumeno:error:validation",
                "校验失败",
                true,
            ),
            Self::Oidc(_) => (
                StatusCode::BAD_GATEWAY,
                "urn:lumeno:error:oidc",
                "身份提供方错误",
                true,
            ),
            Self::Delivery(_) => (
                StatusCode::BAD_GATEWAY,
                "urn:lumeno:error:notification-delivery",
                "通知发送失败",
                true,
            ),
        };

        if !expose {
            error!(error = ?self, "request failed");
        }
        let detail = if expose {
            self.to_string()
        } else {
            "请求未能完成，请稍后重试。".to_owned()
        };
        (
            status,
            [("content-type", "application/problem+json")],
            Json(Problem {
                r#type: kind,
                title,
                status: status.as_u16(),
                detail,
            }),
        )
            .into_response()
    }
}
