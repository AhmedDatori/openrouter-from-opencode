use crate::{App, db::Log};
use axum::{
    body::Body,
    extract::{Request, State},
    middleware::Next,
    response::Response,
};
use futures_util::StreamExt;
use serde_json::{Value, json};
use std::{
    sync::{Arc, Mutex},
    time::Instant,
};

#[derive(Clone)]
pub struct Context {
    pub record: Arc<Mutex<Log>>,
    pub start: Instant,
}
impl Context {
    pub fn edit(&self, f: impl FnOnce(&mut Log)) {
        f(&mut self.record.lock().unwrap())
    }
    pub fn id(&self) -> String {
        self.record.lock().unwrap().id.clone()
    }
    pub fn elapsed(&self) -> u64 {
        self.start.elapsed().as_millis() as u64
    }
}
struct Guard {
    app: App,
    ctx: Context,
    complete: bool,
    capture: Vec<u8>,
    capture_body: bool,
}
impl Drop for Guard {
    fn drop(&mut self) {
        let mut r = self.ctx.record.lock().unwrap().clone();
        r.duration_ms = self.ctx.elapsed();
        if !self.complete {
            r.outcome = "client_disconnected".into();
            if r.status < 400 {
                r.error = Some(
                    "Client disconnected before response completed; usage may be incomplete".into(),
                )
            }
        }
        if self.capture_body && self.app.cfg.log_bodies && !self.capture.is_empty() {
            r.response = Some(match serde_json::from_slice(&self.capture) {
                Ok(v) => redact(&v, self.app.cfg.max_log_bytes),
                Err(_) => {
                    json!({"preview":String::from_utf8_lossy(&self.capture),"truncated_or_non_json":true})
                }
            })
        }
        self.app.db.save_log(r);
    }
}
pub async fn middleware(State(app): State<App>, mut req: Request, next: Next) -> Response {
    let path = req.uri().path().to_string();
    let method = req.method().to_string();
    let mut record = Log::new(
        format!("gen-{}", uuid::Uuid::new_v4().simple()),
        path,
        method,
    );
    record.app = req
        .headers()
        .get("x-openrouter-title")
        .or_else(|| req.headers().get("x-title"))
        .and_then(|s| s.to_str().ok())
        .map(|s| s.chars().take(200).collect());
    let ctx = Context {
        record: Arc::new(Mutex::new(record)),
        start: Instant::now(),
    };
    req.extensions_mut().insert(ctx.clone());
    let mut res = next.run(req).await;
    ctx.edit(|r| {
        r.status = res.status().as_u16();
        if res.status().is_client_error() || res.status().is_server_error() {
            r.outcome = "error".into()
        }
    });
    let id = ctx.id();
    res.headers_mut()
        .insert("x-request-id", id.parse().unwrap());
    res.headers_mut()
        .insert("x-generation-id", id.parse().unwrap());
    res.headers_mut().insert(
        "x-go-router-latency-ms",
        ctx.elapsed().to_string().parse().unwrap(),
    );
    let capture = !res
        .headers()
        .get("content-type")
        .is_some_and(|s| s.as_bytes().starts_with(b"text/event-stream"));
    let (parts, body) = res.into_parts();
    let guard = Guard {
        app: app.clone(),
        ctx: ctx.clone(),
        complete: false,
        capture: vec![],
        capture_body: capture,
    };
    let stream = async_stream::stream! {
        let mut guard=guard;let mut stream=body.into_data_stream();
        while let Some(chunk)=stream.next().await {
            if let Ok(bytes)=&chunk {
                guard.ctx.edit(|r|{if r.first_byte_ms.is_none(){r.first_byte_ms=Some(guard.ctx.elapsed())}r.bytes_out+=bytes.len()});
                if guard.capture_body && guard.app.cfg.log_bodies {let max=guard.app.cfg.max_log_bytes.saturating_sub(guard.capture.len());guard.capture.extend_from_slice(&bytes[..bytes.len().min(max)]);}
            }
            yield chunk;
        }
        guard.complete=true;
    };
    Response::from_parts(parts, Body::from_stream(stream))
}
pub fn redact(v: &Value, max: usize) -> Value {
    fn clean(v: &Value) -> Value {
        match v {
            Value::Object(m) => Value::Object(
                m.iter()
                    .map(|(k, v)| {
                        let lower = k.to_lowercase();
                        let value = if matches!(
                            lower.as_str(),
                            "authorization"
                                | "api_key"
                                | "password"
                                | "access_token"
                                | "refresh_token"
                        ) {
                            json!("[redacted]")
                        } else if lower == "data" && v.as_str().is_some_and(|s| s.len() > 256) {
                            json!(format!(
                                "[media data omitted: {} bytes]",
                                v.as_str().unwrap().len()
                            ))
                        } else {
                            clean(v)
                        };
                        (k.clone(), value)
                    })
                    .collect(),
            ),
            Value::Array(a) => Value::Array(a.iter().map(clean).collect()),
            Value::String(s) if s.starts_with("data:") => {
                json!(format!("[data URL omitted: {} bytes]", s.len()))
            }
            _ => v.clone(),
        }
    }
    let clean = clean(v);
    let s = clean.to_string();
    if s.len() > max {
        json!({"truncated":true,"preview":s.chars().take(max/2).collect::<String>(),"original_bytes":s.len()})
    } else {
        clean
    }
}
