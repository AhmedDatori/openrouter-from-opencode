use axum::{
    Json,
    http::{HeaderValue, StatusCode},
    response::{IntoResponse, Response},
};
use serde_json::{Value, json};

#[derive(Debug)]
pub struct ApiError {
    pub status: StatusCode,
    pub message: String,
    pub metadata: Value,
    pub retry_after: Option<String>,
}
impl ApiError {
    pub fn new(code: u16, message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::from_u16(code).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
            message: message.into(),
            metadata: json!({}),
            retry_after: None,
        }
    }
    pub fn bad(message: impl Into<String>) -> Self {
        Self::new(400, message)
    }
    pub fn internal(error: impl std::fmt::Display) -> Self {
        eprintln!("internal error: {error}");
        Self::new(500, "Internal gateway error")
    }
    pub fn body(&self) -> Value {
        json!({"error":{"code":self.status.as_u16(),"message":self.message,"metadata":self.metadata}})
    }
}
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let mut res = (self.status, Json(self.body())).into_response();
        if let Some(v) = self
            .retry_after
            .and_then(|s| HeaderValue::from_str(&s).ok())
        {
            res.headers_mut().insert("retry-after", v);
        }
        res
    }
}
pub type ApiResult<T> = Result<T, ApiError>;
