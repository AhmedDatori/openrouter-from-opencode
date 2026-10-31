use crate::{
    App, auth,
    db::Key,
    error::{ApiError, ApiResult},
    models::Catalog,
    proxy::read_json,
};
use argon2::{Argon2, PasswordHash, PasswordVerifier};
use axum::{
    Json,
    extract::{Path, Query, Request, State},
    http::{HeaderMap, HeaderValue},
    middleware::Next,
    response::{Html, IntoResponse, Response},
};
use serde_json::{Value, json};
use std::{collections::HashMap, time::Instant};

pub async fn index() -> Html<&'static str> {
    Html(include_str!("../static/index.html"))
}
pub async fn js() -> impl IntoResponse {
    (
        [("content-type", "application/javascript; charset=utf-8")],
        include_str!("../static/app.js"),
    )
}
pub async fn css() -> impl IntoResponse {
    (
        [("content-type", "text/css; charset=utf-8")],
        include_str!("../static/style.css"),
    )
}
pub async fn ready(State(app): State<App>) -> ApiResult<Json<Value>> {
    app.db.flush().await.map_err(ApiError::internal)?;
    Ok(Json(json!({"status":"ready"})))
}
pub async fn security_headers(req: Request, next: Next) -> Response {
    let is_admin = req.uri().path() == "/" || req.uri().path().starts_with("/admin");
    let mut res = next.run(req).await;
    res.headers_mut().insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    if is_admin {
        res.headers_mut().insert("content-security-policy",HeaderValue::from_static("default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' data:; connect-src 'self'; object-src 'none'; base-uri 'self'; frame-ancestors 'none'; form-action 'self'"));
        res.headers_mut()
            .insert("referrer-policy", HeaderValue::from_static("no-referrer"));
        res.headers_mut()
            .insert("cache-control", HeaderValue::from_static("no-store"));
    }
    res
}
pub async fn login(State(app): State<App>, req: Request) -> ApiResult<Response> {
    auth::origin(&app, req.headers())?;
    {
        let mut rate = app.login_rate.lock().unwrap();
        if rate.0.elapsed().as_secs() >= 60 {
            *rate = (Instant::now(), 0)
        }
        if rate.1 >= 10 {
            let mut e = ApiError::new(429, "Too many admin login attempts; retry in one minute");
            e.retry_after = Some("60".into());
            return Err(e);
        }
        rate.1 += 1;
    }
    let _slot = app
        .login_slots
        .try_acquire()
        .map_err(|_| ApiError::new(429, "Admin login is busy"))?;
    let body = read_json(req, 4096).await?;
    let password = body
        .get("password")
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::bad("password is required"))?
        .to_string();
    let saved = app.password_hash.clone();
    let valid = tokio::task::spawn_blocking(move || {
        PasswordHash::new(&saved).is_ok_and(|hash| {
            Argon2::default()
                .verify_password(password.as_bytes(), &hash)
                .is_ok()
        })
    })
    .await
    .map_err(ApiError::internal)?;
    if !valid {
        app.db
            .event("login_failed", json!({}))
            .await
            .map_err(ApiError::internal)?;
        return Err(ApiError::new(401, "Invalid admin password"));
    }
    let token = auth::secret("");
    let csrf = auth::secret("");
    app.db
        .new_session(auth::hash(&token), csrf.clone())
        .await
        .map_err(ApiError::internal)?;
    app.db
        .event("login", json!({}))
        .await
        .map_err(ApiError::internal)?;
    let mut response = Json(json!({"authenticated":true,"csrf_token":csrf})).into_response();
    response
        .headers_mut()
        .insert("set-cookie", cookie(&app, &token, 43200).parse().unwrap());
    Ok(response)
}
fn cookie(app: &App, token: &str, age: u32) -> String {
    format!(
        "go_admin={token}; Path=/admin; HttpOnly; SameSite=Strict; Max-Age={age}{}",
        if app.cfg.cookie_secure {
            "; Secure"
        } else {
            ""
        }
    )
}
pub async fn session(State(app): State<App>, headers: HeaderMap) -> ApiResult<Json<Value>> {
    let (_, csrf) = auth::admin(&app, &headers, false).await?;
    Ok(Json(json!({"authenticated":true,"csrf_token":csrf})))
}
pub async fn logout(State(app): State<App>, headers: HeaderMap) -> ApiResult<Response> {
    let (hash, _) = auth::admin(&app, &headers, true).await?;
    app.db
        .drop_session(hash)
        .await
        .map_err(ApiError::internal)?;
    let mut r = Json(json!({"ok":true})).into_response();
    r.headers_mut()
        .insert("set-cookie", cookie(&app, "", 0).parse().unwrap());
    Ok(r)
}
pub async fn stats(State(app): State<App>, headers: HeaderMap) -> ApiResult<Json<Value>> {
    auth::admin(&app, &headers, false).await?;
    Ok(Json(app.db.stats().await.map_err(ApiError::internal)?))
}
pub async fn keys(State(app): State<App>, headers: HeaderMap) -> ApiResult<Json<Value>> {
    auth::admin(&app, &headers, false).await?;
    Ok(Json(
        json!({"data":app.db.keys().await.map_err(ApiError::internal)?}),
    ))
}
fn validate_key(app: &App, body: &mut Value, create: bool) -> ApiResult<()> {
    if create || body.get("name").is_some() {
        let name = body
            .get("name")
            .and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty() && s.len() <= 120)
            .ok_or_else(|| ApiError::bad("name must contain 1–120 bytes"))?;
        body["name"] = json!(name.trim())
    }
    if let Some(rpm) = body.get("rpm")
        && rpm.as_u64().is_none_or(|n| n > 100000)
    {
        return Err(ApiError::bad(
            "rpm must be an integer from 0 to 100000 (0 means unlimited)",
        ));
    }
    if let Some(v) = body.get("disabled")
        && !v.is_boolean()
    {
        return Err(ApiError::bad("disabled must be boolean"));
    }
    if let Some(v) = body.get("expires_at")
        && !v.is_null()
        && v.as_i64()
            .is_none_or(|t| t <= chrono::Utc::now().timestamp())
    {
        return Err(ApiError::bad(
            "expires_at must be null or a future Unix timestamp",
        ));
    }
    if let Some(v) = body.get("allowed_models") {
        let list = v
            .as_array()
            .ok_or_else(|| ApiError::bad("allowed_models must be an array"))?;
        let catalog = app.catalog.read().unwrap();
        let mut normalized = vec![];
        for id in list {
            let id = id
                .as_str()
                .ok_or_else(|| ApiError::bad("Model IDs must be strings"))?;
            let m = catalog
                .resolve(id)
                .ok_or_else(|| ApiError::bad(format!("Unknown Go model {id}")))?;
            if !normalized.contains(&m.id) {
                normalized.push(m.id.clone())
            }
        }
        body["allowed_models"] = json!(normalized);
    }
    Ok(())
}
pub async fn create_key(State(app): State<App>, req: Request) -> ApiResult<Json<Value>> {
    auth::admin(&app, req.headers(), true).await?;
    let mut body = read_json(req, 32768).await?;
    validate_key(&app, &mut body, true)?;
    let secret = auth::secret("sk-gr-");
    let key = Key {
        id: uuid::Uuid::new_v4().to_string(),
        name: body["name"].as_str().unwrap().into(),
        prefix: secret.chars().take(14).collect(),
        created_at: chrono::Utc::now().to_rfc3339(),
        disabled: false,
        rpm: body["rpm"].as_u64().unwrap_or(60) as u32,
        allowed_models: serde_json::from_value(body["allowed_models"].clone()).unwrap_or_default(),
        expires_at: body["expires_at"].as_i64(),
    };
    let key = app
        .db
        .create_key(key, auth::hash(&secret))
        .await
        .map_err(ApiError::internal)?;
    app.db
        .event("key_created", json!({"key_id":key.id,"name":key.name}))
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(
        json!({"data":key,"key":secret,"notice":"Copy this key now. Only its SHA-256 hash is stored; it cannot be retrieved again."}),
    ))
}
pub async fn update_key(
    State(app): State<App>,
    Path(id): Path<String>,
    req: Request,
) -> ApiResult<Json<Value>> {
    auth::admin(&app, req.headers(), true).await?;
    let mut body = read_json(req, 32768).await?;
    validate_key(&app, &mut body, false)?;
    let key = app
        .db
        .update_key(id.clone(), body)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::new(404, "API key not found"))?;
    app.db
        .event("key_updated", json!({"key_id":id}))
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(json!({"data":key})))
}
pub async fn delete_key(
    State(app): State<App>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> ApiResult<Json<Value>> {
    auth::admin(&app, &headers, true).await?;
    if !app
        .db
        .delete_key(id.clone())
        .await
        .map_err(ApiError::internal)?
    {
        return Err(ApiError::new(404, "API key not found"));
    }
    app.db
        .event("key_revoked", json!({"key_id":id}))
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(json!({"ok":true})))
}
pub async fn logs(
    State(app): State<App>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult<Json<Value>> {
    auth::admin(&app, &headers, false).await?;
    let page = q
        .get("page")
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(1)
        .clamp(1, 1000000);
    let limit = q
        .get("limit")
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(30)
        .clamp(1, 200);
    Ok(Json(
        app.db
            .logs(
                page,
                limit,
                q.get("key_id").filter(|s| !s.is_empty()).cloned(),
                q.get("model").filter(|s| !s.is_empty()).cloned(),
                q.get("errors").is_some_and(|s| s == "true"),
            )
            .await
            .map_err(ApiError::internal)?,
    ))
}
pub async fn log(
    State(app): State<App>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> ApiResult<Json<Value>> {
    auth::admin(&app, &headers, false).await?;
    Ok(Json(
        app.db
            .log(id, None)
            .await
            .map_err(ApiError::internal)?
            .ok_or_else(|| ApiError::new(404, "Request log not found"))?,
    ))
}
pub async fn models(State(app): State<App>, headers: HeaderMap) -> ApiResult<Json<Value>> {
    auth::admin(&app, &headers, false).await?;
    Ok(Json(
        json!({"data":app.catalog.read().unwrap().models,"default_model":app.cfg.default_model}),
    ))
}
pub async fn reload_models(State(app): State<App>, headers: HeaderMap) -> ApiResult<Json<Value>> {
    auth::admin(&app, &headers, true).await?;
    let path = app.cfg.catalog_path.clone();
    let text = tokio::task::spawn_blocking(move || std::fs::read_to_string(path))
        .await
        .map_err(ApiError::internal)?
        .map_err(ApiError::internal)?;
    let catalog =
        Catalog::parse(&text).map_err(|e| ApiError::bad(format!("Invalid catalog: {e}")))?;
    if catalog.resolve(&app.cfg.default_model).is_none() {
        return Err(ApiError::bad("Catalog must include DEFAULT_MODEL"));
    }
    let count = catalog.models.len();
    *app.catalog.write().unwrap() = catalog;
    app.db
        .event("catalog_reloaded", json!({"models":count}))
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(json!({"ok":true,"models":count})))
}
pub async fn settings(State(app): State<App>, headers: HeaderMap) -> ApiResult<Json<Value>> {
    auth::admin(&app, &headers, false).await?;
    let upstream = app.upstream.read().unwrap();
    Ok(Json(
        json!({"version":env!("CARGO_PKG_VERSION"),"default_model":app.cfg.default_model,"max_concurrent_requests":app.cfg.max_concurrent,"max_body_bytes":app.cfg.max_body_bytes,"max_response_bytes":app.cfg.max_response_bytes,"log_bodies":app.cfg.log_bodies,"max_log_bytes":app.cfg.max_log_bytes,"retention_days":app.cfg.retention_days,"request_timeout_seconds":app.cfg.timeout_secs,"stream_idle_timeout_seconds":app.cfg.idle_timeout_secs,"monthly_subscription_usd":app.cfg.subscription_usd,"public_url":app.cfg.public_url,"cookie_secure":app.cfg.cookie_secure,"catalog_models":app.catalog.read().unwrap().models.len(),"upstream_connected":!upstream.key.is_empty(),"upstream_configured":!upstream.key.is_empty(),"upstream_source":upstream.source,"notice":"Cash cost is the amount actually reported as charged. List value and Go allowance are separate. Subscription price is fixed and is not a per-request charge."}),
    ))
}

pub async fn events(State(app): State<App>, headers: HeaderMap) -> ApiResult<Json<Value>> {
    auth::admin(&app, &headers, false).await?;
    Ok(Json(app.db.events().await.map_err(ApiError::internal)?))
}

pub async fn save_upstream_key(State(app): State<App>, req: Request) -> ApiResult<Json<Value>> {
    auth::admin(&app, req.headers(), true).await?;
    let body = read_json(req, 8192).await?;
    let key = body
        .get("api_key")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| {
            (16..=4096).contains(&s.len())
                && s.bytes().all(|b| (33..=126).contains(&b))
                && !s.starts_with("replace-with-")
        })
        .ok_or_else(|| {
            ApiError::bad(
                "Enter a Go API key containing 16-4096 printable ASCII characters without spaces.",
            )
        })?
        .to_string();
    let _guard = app.credential_updates.lock().await;
    app.db
        .set_upstream_credential(key.clone())
        .await
        .map_err(ApiError::internal)?;
    *app.upstream.write().unwrap() = crate::UpstreamCredential {
        key,
        source: "dashboard",
    };
    Ok(Json(
        json!({"ok":true,"upstream_configured":true,"notice":"Saved on the server. New requests use this key immediately. The key is never returned by the dashboard API."}),
    ))
}

pub async fn remove_upstream_key(
    State(app): State<App>,
    headers: HeaderMap,
) -> ApiResult<Json<Value>> {
    auth::admin(&app, &headers, true).await?;
    let _guard = app.credential_updates.lock().await;
    // Persist an empty value so a legacy environment key does not reappear on restart.
    app.db
        .set_upstream_credential(String::new())
        .await
        .map_err(ApiError::internal)?;
    *app.upstream.write().unwrap() = crate::UpstreamCredential {
        key: String::new(),
        source: "dashboard",
    };
    Ok(Json(json!({"ok":true,"upstream_configured":false})))
}
