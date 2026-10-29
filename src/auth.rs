use crate::{
    App,
    audit::Context,
    db::Key,
    error::{ApiError, ApiResult},
};
use axum::http::HeaderMap;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::RngCore;
use sha2::{Digest, Sha256};
use std::time::Instant;

pub fn secret(prefix: &str) -> String {
    let mut bytes = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    format!("{prefix}{}", URL_SAFE_NO_PAD.encode(bytes))
}
pub fn hash(s: &str) -> String {
    format!("{:x}", Sha256::digest(s.as_bytes()))
}
pub fn bearer(headers: &HeaderMap) -> ApiResult<&str> {
    if let Some(raw) = headers.get("authorization") {
        let s = raw
            .to_str()
            .map_err(|_| ApiError::new(401, "Invalid bearer authentication"))?;
        let (scheme, key) = s
            .split_once(' ')
            .ok_or_else(|| ApiError::new(401, "Invalid bearer authentication"))?;
        if scheme.eq_ignore_ascii_case("bearer")
            && !key.is_empty()
            && !key.contains(char::is_whitespace)
        {
            return Ok(key);
        }
        return Err(ApiError::new(401, "Invalid bearer authentication"));
    }
    if let Some(key) = headers
        .get("x-api-key")
        .and_then(|v| v.to_str().ok())
        .filter(|s| !s.is_empty() && !s.contains(char::is_whitespace))
    {
        return Ok(key);
    }
    Err(ApiError::new(
        401,
        "Missing Authorization: Bearer <gateway-api-key>",
    ))
}
pub async fn api_key(
    app: &App,
    headers: &HeaderMap,
    ctx: Option<&Context>,
    rate_limit: bool,
) -> ApiResult<Key> {
    let key = bearer(headers)?;
    let key = app
        .db
        .find_key(hash(key))
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::new(401, "Invalid API key"))?;
    if key.disabled
        || key
            .expires_at
            .is_some_and(|t| t <= chrono::Utc::now().timestamp())
    {
        return Err(ApiError::new(401, "API key is disabled or expired"));
    }
    if let Some(ctx) = ctx {
        ctx.edit(|r| r.key_id = Some(key.id.clone()));
    }
    if rate_limit && key.rpm > 0 {
        let mut rates = app.rate.lock().unwrap();
        let now = Instant::now();
        rates.retain(|_, (t, _)| now.duration_since(*t).as_secs() < 60);
        let (t, n) = rates.entry(key.id.clone()).or_insert((now, 0));
        if *n >= key.rpm {
            let mut error = ApiError::new(429, "API key request rate exceeded");
            error.retry_after = Some((60 - now.duration_since(*t).as_secs()).max(1).to_string());
            return Err(error);
        }
        *n += 1;
    }
    Ok(key)
}
pub fn cookie(headers: &HeaderMap) -> Option<String> {
    headers
        .get("cookie")?
        .to_str()
        .ok()?
        .split(';')
        .find_map(|p| p.trim().strip_prefix("go_admin=").map(String::from))
}
pub fn origin(app: &App, headers: &HeaderMap) -> ApiResult<()> {
    if let Some(origin) = headers.get("origin").and_then(|v| v.to_str().ok()) {
        let expected = app.cfg.public_url.clone().unwrap_or_else(|| {
            format!(
                "http://{}",
                headers
                    .get("host")
                    .and_then(|s| s.to_str().ok())
                    .unwrap_or("localhost")
            )
        });
        if origin != expected {
            return Err(ApiError::new(
                403,
                "Admin origin check failed; configure PUBLIC_URL to match the dashboard URL",
            ));
        }
    }
    Ok(())
}
pub async fn admin(app: &App, headers: &HeaderMap, write: bool) -> ApiResult<(String, String)> {
    origin(app, headers)?;
    let token = cookie(headers).ok_or_else(|| ApiError::new(401, "Admin login required"))?;
    let hash = hash(&token);
    let (csrf, expiry) = app
        .db
        .session(hash.clone())
        .await
        .map_err(ApiError::internal)?
        .filter(|(_, t)| *t > chrono::Utc::now().timestamp())
        .ok_or_else(|| ApiError::new(401, "Admin session expired"))?;
    let _ = expiry;
    if write && headers.get("x-csrf-token").and_then(|s| s.to_str().ok()) != Some(csrf.as_str()) {
        return Err(ApiError::new(403, "Invalid CSRF token"));
    }
    Ok((hash, csrf))
}
