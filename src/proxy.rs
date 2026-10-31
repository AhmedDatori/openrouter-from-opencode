use crate::{
    App,
    audit::{Context, redact},
    auth,
    db::Key,
    error::{ApiError, ApiResult},
    models::Model,
    sse,
};
use axum::{
    Json,
    body::{Body, to_bytes},
    extract::{Extension, Request, State},
    http::{HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
};
use futures_util::StreamExt;
use serde_json::{Value, json};
use std::{
    collections::HashSet,
    time::{Duration, Instant},
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Chat,
    Completions,
    Responses,
    Messages,
}
impl Kind {
    fn path(self) -> &'static str {
        match self {
            Self::Chat | Self::Completions => "openai/v1/chat/completions",
            Self::Responses => "openai/v1/responses",
            Self::Messages => "anthropic/v1/messages",
        }
    }
}
pub async fn chat(State(a): State<App>, Extension(c): Extension<Context>, r: Request) -> Response {
    let result = handle(a, c.clone(), r, Kind::Chat).await;
    finish(&c, Kind::Chat, result)
}
pub async fn completions(
    State(a): State<App>,
    Extension(c): Extension<Context>,
    r: Request,
) -> Response {
    let result = handle(a, c.clone(), r, Kind::Completions).await;
    finish(&c, Kind::Completions, result)
}
pub async fn responses(
    State(a): State<App>,
    Extension(c): Extension<Context>,
    r: Request,
) -> Response {
    let result = handle(a, c.clone(), r, Kind::Responses).await;
    finish(&c, Kind::Responses, result)
}
pub async fn messages(
    State(a): State<App>,
    Extension(c): Extension<Context>,
    r: Request,
) -> Response {
    let result = handle(a, c.clone(), r, Kind::Messages).await;
    finish(&c, Kind::Messages, result)
}
fn finish(ctx: &Context, kind: Kind, result: ApiResult<Response>) -> Response {
    match result {
        Ok(response) => response,
        Err(error) => {
            ctx.edit(|r| r.error = Some(error.message.clone()));
            if matches!(kind, Kind::Chat | Kind::Completions) {
                return error.into_response();
            }
            let category = match error.status.as_u16() {
                401 => "authentication_error",
                403 => "permission_error",
                404 => "not_found_error",
                429 => "rate_limit_error",
                503 => "overloaded_error",
                400 | 413 | 422 | 501 => "invalid_request_error",
                _ => "api_error",
            };
            let body = if kind == Kind::Messages {
                json!({"type":"error","error":{"type":category,"message":error.message}})
            } else {
                json!({"error":{"type":category,"code":category,"message":error.message,"param":null}})
            };
            let mut response = (error.status, Json(body)).into_response();
            if let Some(retry) = error
                .retry_after
                .and_then(|v| HeaderValue::from_str(&v).ok())
            {
                response.headers_mut().insert("retry-after", retry);
            }
            response
        }
    }
}

pub async fn read_json(req: Request, max: usize) -> ApiResult<Value> {
    if !req
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|s| {
            s.split(';')
                .next()
                .is_some_and(|v| v.trim().eq_ignore_ascii_case("application/json"))
        })
    {
        return Err(ApiError::bad("Content-Type must be application/json"));
    }
    let bytes = to_bytes(req.into_body(), max).await.map_err(|_| {
        ApiError::new(
            413,
            "Request body exceeds MAX_BODY_BYTES or could not be read",
        )
    })?;
    let value: Value =
        serde_json::from_slice(&bytes).map_err(|e| ApiError::bad(format!("Invalid JSON: {e}")))?;
    if !value.is_object() {
        return Err(ApiError::bad("Request body must be a JSON object"));
    }
    Ok(value)
}
async fn handle(app: App, ctx: Context, req: Request, kind: Kind) -> ApiResult<Response> {
    ctx.edit(|r| r.kind = "inference".into());
    let headers = req.headers().clone();
    let key = auth::api_key(&app, &headers, Some(&ctx), true).await?;
    let upstream_key = app.upstream.read().unwrap().key.clone();
    if upstream_key.is_empty() {
        return Err(ApiError::new(
            503,
            "OpenCode Go is not configured. The administrator must save a Go API key in Dashboard > Setup & settings.",
        ));
    }
    let permit = app.slots.clone().try_acquire_owned().map_err(|_| {
        let mut e = ApiError::new(429, "Gateway concurrency limit reached");
        e.retry_after = Some("1".into());
        e
    })?;
    let mut body = read_json(req, app.cfg.max_body_bytes).await?;
    ctx.edit(|r| {
        r.bytes_in = body.to_string().len();
        if app.cfg.log_bodies {
            r.request = Some(redact(&body, app.cfg.max_log_bytes));
        }
    });
    let stream = body
        .get("stream")
        .map(|s| {
            s.as_bool()
                .ok_or_else(|| ApiError::bad("stream must be boolean"))
        })
        .transpose()?
        .unwrap_or(false);
    let candidates = select_models(&app, &body, &key)?;
    let exclude = prepare(&mut body, kind, stream)?;
    let session = session_id(&headers, &body, &key);
    ctx.edit(|r| r.session_id = Some(session.clone()));
    if kind == Kind::Responses
        && body
            .get("previous_response_id")
            .is_some_and(|v| !v.is_null())
    {
        return Err(ApiError::new(
            501,
            "Stateful previous_response_id is not supported by this Go gateway. Send full conversation input instead.",
        ));
    }
    let mut last_error = None;
    for model in candidates {
        let mut upstream_body = body.clone();
        upstream_body["model"] = json!(model.upstream_id);
        if let Err(e) = validate_media(&upstream_body, &model, kind) {
            last_error = Some(e);
            continue;
        }
        if kind == Kind::Messages && upstream_body.get("max_tokens").is_none() {
            upstream_body["max_tokens"] = json!(4096.min(model.max_completion_tokens))
        }
        ctx.edit(|r| r.model = Some(model.id.clone()));
        let start = Instant::now();
        let mut request = app
            .http
            .post(format!("{}/{}", app.cfg.upstream_base, kind.path()))
            .bearer_auth(&upstream_key)
            .header("x-opencode-session", &session)
            .header(
                "accept",
                if stream {
                    "text/event-stream"
                } else {
                    "application/json"
                },
            )
            .json(&upstream_body);
        if kind == Kind::Messages {
            request = request.header(
                "anthropic-version",
                headers
                    .get("anthropic-version")
                    .cloned()
                    .unwrap_or(HeaderValue::from_static("2023-06-01")),
            )
        }
        {
            let h = "anthropic-beta";
            if let Some(v) = headers.get(h) {
                request = request.header(h, v)
            }
        }
        let remaining =
            Duration::from_secs(app.cfg.timeout_secs).saturating_sub(ctx.start.elapsed());
        if remaining.is_zero() {
            return Err(ApiError::new(504, "Request deadline exceeded"));
        }
        let response = tokio::time::timeout(remaining, request.send()).await;
        let upstream = match response {
            Ok(Ok(r)) => r,
            failure => {
                let error = if failure.is_err() {
                    ApiError::new(504, "Upstream response timed out")
                } else {
                    ApiError::new(502, "Could not connect to OpenCode Go")
                };
                ctx.edit(|r|r.attempts.push(json!({"model":model.id,"status":error.status.as_u16(),"duration_ms":start.elapsed().as_millis()})));
                last_error = Some(error);
                continue;
            }
        };
        let status = upstream.status();
        ctx.edit(|r|r.attempts.push(json!({"model":model.id,"status":status.as_u16(),"duration_ms":start.elapsed().as_millis()})));
        if !status.is_success() {
            let retry = upstream
                .headers()
                .get("retry-after")
                .and_then(|s| s.to_str().ok())
                .map(String::from);
            let raw = bounded_json(
                upstream,
                app.cfg.max_response_bytes,
                Duration::from_secs(app.cfg.timeout_secs).saturating_sub(ctx.start.elapsed()),
            )
            .await
            .unwrap_or(json!({}));
            let mut error = upstream_error(status, &raw, &upstream_key);
            error.retry_after = retry;
            if matches!(status.as_u16(), 401 | 402) {
                return Err(error);
            }
            last_error = Some(error);
            continue;
        }
        if stream {
            if !upstream
                .headers()
                .get("content-type")
                .is_some_and(|s| s.as_bytes().starts_with(b"text/event-stream"))
            {
                last_error = Some(ApiError::new(502, "Upstream did not return an SSE stream"));
                continue;
            }
            let mut bytes = upstream.bytes_stream();
            let mut parser = sse::Parser::default();
            let mut state = sse::State::new(
                kind,
                ctx.id(),
                model.id.clone(),
                exclude,
                if app.cfg.log_bodies {
                    app.cfg.max_log_bytes
                } else {
                    0
                },
            );
            let idle = app.cfg.idle_timeout_secs;
            let total = app.cfg.timeout_secs;
            let limit = app.cfg.max_response_bytes;
            let c = ctx.clone();
            let output = async_stream::stream! {
                                        let _permit=permit;let mut heartbeat=tokio::time::interval(Duration::from_secs(10));let mut last_data=Instant::now();
                                        loop {
                                            if c.start.elapsed().as_secs()>=total||last_data.elapsed().as_secs()>=idle {
                                                let message="Upstream stream timed out";c.edit(|r|{r.outcome="upstream_error".into();r.error=Some(message.into())});
                                                yield Ok::<_,std::io::Error>(bytes::Bytes::from(state.error(message)));break
                                            }
                                            tokio::select! {
                                                _=tokio::time::sleep(Duration::from_secs(total).saturating_sub(c.start.elapsed()))=>{
                                                    let message="Request deadline exceeded during stream";c.edit(|r|{r.outcome="upstream_error".into();r.error=Some(message.into())});yield Ok(bytes::Bytes::from(state.error(message)));break;
                                                },
                                                _=tokio::time::sleep(Duration::from_secs(idle).saturating_sub(last_data.elapsed()))=>{
                                                    let message="Upstream stream idle timeout";c.edit(|r|{r.outcome="upstream_error".into();r.error=Some(message.into())});yield Ok(bytes::Bytes::from(state.error(message)));break;
                                                },
                                                _=heartbeat.tick()=>{yield Ok(bytes::Bytes::from_static(b": OPENROUTER PROCESSING\n\n"));},
                                                next=bytes.next()=>{
                                                    match next {
                                                        Some(Ok(chunk))=>{
                                                            last_data=Instant::now();
                                                            match parser.feed(&chunk,limit) {
                                                                Ok(frames)=>{
                                                                    let mut invalid=false;
                                                                    for frame in frames {
                                                                        match state.transform(frame){Ok(out)=>{c.edit(|r|{r.usage=state.usage.clone();r.upstream_id=state.upstream_id.clone();if state.received_token && r.first_token_ms.is_none(){r.first_token_ms=Some(c.elapsed())}
            if let Some(event)=state.last_event.take(){if r.response.is_none(){r.response=Some(json!({"stream_events":[]}))}r.response.as_mut().unwrap()["stream_events"].as_array_mut().unwrap().push(event)}});yield Ok(bytes::Bytes::from(out));},Err(message)=>{c.edit(|r|{r.outcome="upstream_error".into();r.error=Some(message.clone())});yield Ok(bytes::Bytes::from(state.error(&message)));invalid=true;break}}
                                                                        c.edit(|r|{r.usage=state.usage.clone();r.upstream_id=state.upstream_id.clone()});
                                                                    }
                                                                    if invalid||state.done {break}
                                                                },
                                                                Err(message)=>{c.edit(|r|{r.outcome="upstream_error".into();r.error=Some(message.clone())});yield Ok(bytes::Bytes::from(state.error(&message)));break}
                                                            }
                                                        },
                                                        Some(Err(_))=>{let message="Upstream connection ended unexpectedly";c.edit(|r|{r.outcome="upstream_error".into();r.error=Some(message.into())});yield Ok(bytes::Bytes::from(state.error(message)));break},
                                                        None=>{
                                                            if !state.done||parser.has_pending(){let message="Upstream stream ended without a terminal event";c.edit(|r|{r.outcome="upstream_error".into();r.error=Some(message.into())});yield Ok(bytes::Bytes::from(state.error(message)));}
                                                            break
                                                        },
                                                    }
                                                }
                                            }
                                        }
                                        c.edit(|r|{r.usage=state.usage;r.upstream_id=state.upstream_id;if state.failed{r.outcome="upstream_error".into()}
                        if app.cfg.log_bodies{r.response=Some(json!({"stream_events":state.events,"capture_limit_bytes":app.cfg.max_log_bytes}))}});
                                    };
            let mut res = Response::new(Body::from_stream(output));
            res.headers_mut().insert(
                "content-type",
                HeaderValue::from_static("text/event-stream; charset=utf-8"),
            );
            res.headers_mut().insert(
                "cache-control",
                HeaderValue::from_static("no-cache, no-transform"),
            );
            res.headers_mut()
                .insert("x-accel-buffering", HeaderValue::from_static("no"));
            return Ok(res);
        }
        let mut value = match bounded_json(
            upstream,
            app.cfg.max_response_bytes,
            Duration::from_secs(app.cfg.timeout_secs).saturating_sub(ctx.start.elapsed()),
        )
        .await
        {
            Ok(value) => value,
            Err(error) => {
                last_error = Some(error);
                continue;
            }
        };
        if value.get("error").is_some_and(|e| !e.is_null()) {
            return Err(upstream_error(
                StatusCode::BAD_GATEWAY,
                &value,
                &upstream_key,
            ));
        }
        if !value.is_object()
            || (matches!(kind, Kind::Chat | Kind::Completions)
                && !value.get("choices").is_some_and(Value::is_array))
        {
            return Err(ApiError::new(
                502,
                "Upstream returned an invalid response shape",
            ));
        }
        let upstream_id = value.get("id").and_then(Value::as_str).map(String::from);
        match kind {
            Kind::Chat | Kind::Completions => {
                normalize_chat(&mut value, &ctx.id(), &model.id, exclude, false);
                if kind == Kind::Completions {
                    sse::legacy(&mut value)
                }
            }
            Kind::Responses | Kind::Messages => {
                value["model"] = json!(model.id);
                if let Some(u) = value.get_mut("usage") {
                    normalize_usage(u)
                }
            }
        }
        ctx.edit(|r| {
            r.usage = value.get("usage").cloned();
            r.upstream_id = upstream_id;
        });
        drop(permit);
        return Ok(Json(value).into_response());
    }
    Err(last_error.unwrap_or_else(|| ApiError::new(503, "No Go endpoint satisfies this request")))
}
async fn bounded_json(
    response: reqwest::Response,
    max: usize,
    timeout: Duration,
) -> ApiResult<Value> {
    let mut s = response.bytes_stream();
    let mut bytes = Vec::new();
    tokio::time::timeout(timeout, async {
        while let Some(c) = s.next().await {
            let c = c.map_err(|_| ApiError::new(502, "Upstream body read failed"))?;
            if bytes.len() + c.len() > max {
                return Err(ApiError::new(
                    502,
                    "Upstream JSON response exceeds MAX_RESPONSE_BYTES",
                ));
            }
            bytes.extend_from_slice(&c);
        }
        serde_json::from_slice(&bytes)
            .map_err(|_| ApiError::new(502, "Upstream returned an invalid JSON response"))
    })
    .await
    .map_err(|_| ApiError::new(504, "Upstream body timed out"))?
}
fn upstream_error(status: StatusCode, raw: &Value, secret: &str) -> ApiError {
    let message = raw
        .pointer("/error/message")
        .or_else(|| raw.get("message"))
        .and_then(Value::as_str)
        .unwrap_or("OpenCode Go rejected the request")
        .replace(secret, "[redacted]");
    let mut e = ApiError::new(status.as_u16(), message);
    e.metadata = json!({"provider_name":"OpenCode Go","upstream_status":status.as_u16()});
    e
}
fn select_models(app: &App, body: &Value, key: &Key) -> ApiResult<Vec<Model>> {
    let mut names = vec![];
    if let Some(v) = body.get("model") {
        names.push(
            v.as_str()
                .ok_or_else(|| ApiError::bad("model must be a string"))?
                .to_string(),
        )
    }
    if let Some(v) = body.get("models") {
        let a = v
            .as_array()
            .ok_or_else(|| ApiError::bad("models must be a nonempty array of model IDs"))?;
        if a.is_empty() {
            return Err(ApiError::bad("models cannot be empty"));
        }
        for id in a {
            names.push(
                id.as_str()
                    .ok_or_else(|| ApiError::bad("models must contain strings"))?
                    .into(),
            )
        }
    }
    if names.is_empty() {
        names.push(app.cfg.default_model.clone())
    }
    if names.len() > 8 {
        return Err(ApiError::bad("At most eight fallback models are allowed"));
    }
    let catalog = app.catalog.read().unwrap();
    let mut models = vec![];
    let mut seen = HashSet::new();
    let mut last_error = None;
    for id in names {
        let Some(model) = catalog.resolve(&id) else {
            last_error = Some(ApiError::new(
                404,
                format!("Model {id} is not available in the configured OpenCode Go catalog"),
            ));
            continue;
        };
        if !key.allowed_models.is_empty() && !key.allowed_models.contains(&model.id) {
            last_error = Some(ApiError::new(
                403,
                format!("This key cannot use {}", model.id),
            ));
            continue;
        }
        if let Some(provider) = body.get("provider")
            && let Err(e) = model.check_provider(provider)
        {
            last_error = Some(e);
            continue;
        }
        if seen.insert(model.id.clone()) {
            models.push(model.clone())
        }
    }
    if models.is_empty() {
        return Err(last_error.unwrap_or_else(|| ApiError::new(503, "No eligible Go model")));
    }
    Ok(models)
}
fn prepare(body: &mut Value, kind: Kind, stream: bool) -> ApiResult<bool> {
    for field in ["plugins", "transforms"] {
        if body
            .get(field)
            .is_some_and(|v| v != &json!([]) && !v.is_null())
        {
            return Err(ApiError::new(
                501,
                format!("OpenRouter {field} services are not provided by OpenCode Go"),
            ));
        }
    }
    for field in ["web_search_options", "preset", "image_config", "audio"] {
        if body.get(field).is_some_and(|v| !v.is_null()) {
            return Err(ApiError::new(
                501,
                format!("{field} is not supported by this Go gateway"),
            ));
        }
    }
    if body
        .get("modalities")
        .is_some_and(|v| v != &json!(["text"]) && v != &json!([]))
    {
        return Err(ApiError::new(
            501,
            "Go catalog models expose text output only",
        ));
    }
    if body.pointer("/debug/echo_upstream_body") == Some(&Value::Bool(true)) {
        return Err(ApiError::new(
            501,
            "debug.echo_upstream_body is not implemented",
        ));
    }
    if body.get("route").is_some_and(|v| v != "fallback") {
        return Err(ApiError::bad("Only route=fallback is supported"));
    }
    let mut exclude = body.get("include_reasoning") == Some(&Value::Bool(false));
    if kind == Kind::Chat || kind == Kind::Completions {
        if let Some(reasoning) = body.get("reasoning") {
            let o = reasoning
                .as_object()
                .ok_or_else(|| ApiError::bad("reasoning must be an object"))?;
            if o.get("max_tokens").is_some() {
                return Err(ApiError::new(
                    501,
                    "Exact reasoning.max_tokens budgets cannot be translated to Go's effort control; use reasoning.effort",
                ));
            }
            if o.get("exclude") == Some(&Value::Bool(true)) {
                exclude = true
            }
            let effort = if o.get("enabled") == Some(&Value::Bool(false)) {
                Some(json!("none"))
            } else {
                o.get("effort").cloned()
            };
            if let Some(e) = effort {
                body["reasoning_effort"] = e
            }
        }
        if body.get("max_tokens").is_some() {
            if let Some(other) = body.get("max_completion_tokens")
                && other != &body["max_tokens"]
            {
                return Err(ApiError::bad(
                    "Conflicting max_tokens and max_completion_tokens",
                ));
            }
            body["max_completion_tokens"] = body["max_tokens"].clone();
            body.as_object_mut().unwrap().remove("max_tokens");
        }
        if kind == Kind::Completions
            || (body.get("messages").is_none() && body.get("prompt").is_some())
        {
            if body.get("echo") == Some(&Value::Bool(true)) || body.get("suffix").is_some() {
                return Err(ApiError::new(
                    501,
                    "Legacy completion echo and suffix are not supported",
                ));
            }
            let prompt = body.get("prompt").and_then(Value::as_str).ok_or_else(|| {
                ApiError::bad("prompt must be a string; token arrays and batches are unsupported")
            })?;
            body["messages"] = json!([{"role":"user","content":prompt}]);
        }
        let messages = body
            .get_mut("messages")
            .and_then(Value::as_array_mut)
            .filter(|a| !a.is_empty())
            .ok_or_else(|| ApiError::bad("messages must be a nonempty array"))?;
        for m in messages {
            if m.get("role").and_then(Value::as_str) == Some("assistant")
                && m.get("reasoning_content").is_none()
                && let Some(r) = m.get("reasoning").and_then(Value::as_str)
            {
                m["reasoning_content"] = json!(r)
            }
        }
        if stream {
            let mut options = body
                .get("stream_options")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            options.insert("include_usage".into(), json!(true));
            body["stream_options"] = Value::Object(options);
        }
    } else if kind == Kind::Messages {
        if body
            .get("messages")
            .and_then(Value::as_array)
            .is_none_or(|a| a.is_empty())
        {
            return Err(ApiError::bad("messages must be a nonempty array"));
        }
    } else if body.get("input").is_none() {
        return Err(ApiError::bad("Responses requests require input"));
    }
    for field in [
        "provider",
        "models",
        "route",
        "plugins",
        "transforms",
        "prompt",
        "include_reasoning",
        "debug",
        "modalities",
    ] {
        body.as_object_mut().unwrap().remove(field);
    }
    if matches!(kind, Kind::Chat | Kind::Completions) {
        body.as_object_mut().unwrap().remove("reasoning");
    }
    Ok(exclude)
}
fn validate_media(body: &Value, m: &Model, kind: Kind) -> ApiResult<()> {
    fn walk(v: &Value, found: &mut HashSet<String>) {
        match v {
            Value::Array(a) => {
                for v in a {
                    walk(v, found)
                }
            }
            Value::Object(o) => {
                if let Some(t) = o.get("type").and_then(Value::as_str) {
                    let modality = match t {
                        "image_url" | "input_image" | "image" => Some("image"),
                        "input_audio" | "audio" => Some("audio"),
                        "video_url" | "video" => Some("video"),
                        "file" | "input_file" | "document" => Some("file"),
                        _ => None,
                    };
                    if let Some(m) = modality {
                        found.insert(m.into());
                    }
                }
                for (k, v) in o {
                    if !matches!(
                        k.as_str(),
                        "tools" | "parameters" | "response_format" | "json_schema"
                    ) {
                        walk(v, found)
                    }
                }
            }
            _ => {}
        }
    }
    let mut found = HashSet::new();
    walk(
        if kind == Kind::Responses {
            &body["input"]
        } else {
            &body["messages"]
        },
        &mut found,
    );
    for input in found {
        if !m.input_modalities.contains(&input) {
            return Err(ApiError::bad(format!(
                "{} does not advertise {input} input in the Go catalog",
                m.id
            )));
        }
    }
    Ok(())
}
fn session_id(headers: &HeaderMap, body: &Value, key: &Key) -> String {
    for h in ["x-opencode-session", "x-session-id", "x-conversation-id"] {
        if let Some(s) = headers
            .get(h)
            .and_then(|s| s.to_str().ok())
            .filter(|s| !s.is_empty() && s.len() <= 200)
        {
            return format!("gorouter-{}", auth::hash(&format!("{}:{s}", key.id)));
        }
    }
    let first = body
        .get("messages")
        .and_then(Value::as_array)
        .and_then(|a| a.iter().find(|m| m["role"] == "user"))
        .or_else(|| body.get("input"))
        .cloned()
        .unwrap_or(Value::Null);
    format!(
        "gorouter-{}",
        auth::hash(&format!(
            "{}:{}:{}",
            key.id,
            body.get("user").unwrap_or(&Value::Null),
            first
        ))
    )
}
pub fn normalize_usage(u: &mut Value) {
    if !u.is_object() {
        return;
    }
    if u.get("prompt_tokens").is_some()
        && u.get("completion_tokens").is_some()
        && u.get("total_tokens").is_none()
        && let (Some(p), Some(c)) = (u["prompt_tokens"].as_u64(), u["completion_tokens"].as_u64())
    {
        u["total_tokens"] = json!(p.saturating_add(c))
    }
    if let Some(v) = u.pointer("/opencode/tokens/reasoning").cloned()
        && u.get("completion_tokens").is_some()
        && u.pointer("/completion_tokens_details/reasoning_tokens")
            .is_none()
    {
        if !u["completion_tokens_details"].is_object() {
            u["completion_tokens_details"] = json!({})
        }
        u["completion_tokens_details"]["reasoning_tokens"] = v;
    }
}
pub fn normalize_chat(v: &mut Value, id: &str, model: &str, exclude: bool, stream: bool) {
    if !v.is_object() {
        return;
    }
    if let Some(e) = v.get_mut("error").filter(|e| !e.is_null()) {
        if e.get("code").is_none() {
            e["code"] = json!(502)
        }
        return;
    }
    v["id"] = json!(id);
    v["model"] = json!(model);
    v["provider"] = json!("OpenCode Go");
    if v.get("system_fingerprint").is_none() {
        v["system_fingerprint"] = Value::Null
    }
    v["object"] = json!(if stream {
        "chat.completion.chunk"
    } else {
        "chat.completion"
    });
    if v.get("created").is_none() {
        v["created"] = json!(chrono::Utc::now().timestamp())
    }
    if let Some(u) = v.get_mut("usage") {
        normalize_usage(u)
    }
    if let Some(choices) = v.get_mut("choices").and_then(Value::as_array_mut) {
        for c in choices {
            if let Some(reason) = c.get("finish_reason").cloned().filter(|v| !v.is_null()) {
                c["native_finish_reason"] = reason.clone();
                c["finish_reason"] = json!(match reason.as_str().unwrap_or("stop") {
                    "end_turn" | "eos" | "stop_sequence" => "stop",
                    "max_tokens" => "length",
                    "tool_use" => "tool_calls",
                    s => s,
                });
            }
            if let Some(m) = c
                .get_mut(if stream { "delta" } else { "message" })
                .and_then(Value::as_object_mut)
            {
                for field in ["tool_calls", "images", "audio", "reasoning_details"] {
                    if m.get(field).is_some_and(Value::is_null) {
                        m.remove(field);
                    }
                }
                if exclude {
                    for k in ["reasoning_content", "reasoning", "reasoning_details"] {
                        m.remove(k);
                    }
                } else if !m.contains_key("reasoning")
                    && let Some(r) = m.get("reasoning_content").cloned()
                {
                    m.insert("reasoning".into(), r);
                }
            }
        }
    }
}
