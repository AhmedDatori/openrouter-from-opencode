use crate::proxy::{Kind, normalize_chat, normalize_usage};
use serde_json::{Value, json};
use std::collections::BTreeMap;

#[derive(Default)]
pub struct Parser {
    line: Vec<u8>,
    frame: Vec<u8>,
    size: usize,
}
impl Parser {
    pub fn feed(&mut self, bytes: &[u8], limit: usize) -> Result<Vec<Frame>, String> {
        let mut frames = vec![];
        for &b in bytes {
            self.size += 1;
            if self.size > limit {
                return Err("Upstream SSE event exceeds MAX_RESPONSE_BYTES".into());
            }
            self.line.push(b);
            if b == b'\n' {
                if self.line == b"\n" || self.line == b"\r\n" {
                    let raw = std::mem::take(&mut self.frame);
                    self.line.clear();
                    self.size = 0;
                    frames.push(Frame::parse(&raw)?);
                } else {
                    self.frame.append(&mut self.line)
                }
            }
        }
        Ok(frames)
    }
    pub fn has_pending(&self) -> bool {
        !self.line.is_empty() || !self.frame.is_empty()
    }
}
pub struct Frame {
    pub event: Option<String>,
    pub data: Option<String>,
    pub comments: Vec<String>,
}
impl Frame {
    fn parse(raw: &[u8]) -> Result<Self, String> {
        let s = std::str::from_utf8(raw).map_err(|_| "Invalid UTF-8 in upstream SSE")?;
        let mut f = Self {
            event: None,
            data: None,
            comments: vec![],
        };
        let mut data = vec![];
        for line in s.lines() {
            if line.starts_with(':') {
                f.comments.push(line.into());
                continue;
            }
            let (name, val) = line.split_once(':').unwrap_or((line, ""));
            let val = val.strip_prefix(' ').unwrap_or(val);
            match name {
                "event" => f.event = Some(val.into()),
                "data" => data.push(val),
                _ => {}
            }
        }
        if !data.is_empty() {
            f.data = Some(data.join("\n"))
        }
        Ok(f)
    }
    pub fn encode(&self) -> Vec<u8> {
        let mut s = String::new();
        for c in &self.comments {
            s.push_str(c);
            s.push('\n')
        }
        if let Some(e) = &self.event {
            s.push_str("event: ");
            s.push_str(e);
            s.push('\n')
        }
        if let Some(d) = &self.data {
            for line in d.split('\n') {
                s.push_str("data: ");
                s.push_str(line);
                s.push('\n')
            }
        }
        s.push('\n');
        s.into_bytes()
    }
}
pub struct State {
    pub kind: Kind,
    pub id: String,
    pub model: String,
    pub exclude: bool,
    pub usage: Option<Value>,
    pub upstream_id: Option<String>,
    pub done: bool,
    pub failed: bool,
    pub received_token: bool,
    pub events: Vec<Value>,
    pub last_event: Option<Value>,
    pub finish: BTreeMap<u64, String>,
    log_bytes: usize,
    max_log: usize,
}
impl State {
    pub fn new(kind: Kind, id: String, model: String, exclude: bool, max_log: usize) -> Self {
        Self {
            kind,
            id,
            model,
            exclude,
            usage: None,
            upstream_id: None,
            done: false,
            failed: false,
            received_token: false,
            events: vec![],
            last_event: None,
            finish: BTreeMap::new(),
            log_bytes: 0,
            max_log,
        }
    }
    pub fn transform(&mut self, mut frame: Frame) -> Result<Vec<u8>, String> {
        let Some(data) = &frame.data else {
            return Ok(frame.encode());
        };
        if data == "[DONE]" {
            self.done = true;
            return Ok(frame.encode());
        }
        let mut v: Value = serde_json::from_str(data)
            .map_err(|_| "Upstream sent invalid JSON in an SSE data event")?;
        self.received_token |= v.get("choices").and_then(Value::as_array).is_some_and(|a| {
            a.iter().any(|c| {
                let d = &c["delta"];
                ["content", "reasoning", "reasoning_content"]
                    .iter()
                    .any(|k| {
                        d.get(k)
                            .and_then(Value::as_str)
                            .is_some_and(|s| !s.is_empty())
                    })
                    || d.get("tool_calls")
                        .and_then(Value::as_array)
                        .is_some_and(|a| !a.is_empty())
            })
        }) || v
            .get("type")
            .and_then(Value::as_str)
            .is_some_and(|t| t == "content_block_delta" || t.ends_with(".delta"));
        if self.upstream_id.is_none() {
            self.upstream_id = v
                .get("id")
                .or_else(|| v.pointer("/response/id"))
                .and_then(Value::as_str)
                .map(String::from)
        }
        if v.get("error").is_some_and(|e| !e.is_null())
            || matches!(
                v.get("type").and_then(Value::as_str),
                Some("error" | "response.failed")
            )
        {
            self.failed = true
        }
        match self.kind {
            Kind::Chat | Kind::Completions => {
                if let Some(choices) = v.get("choices").and_then(Value::as_array) {
                    for c in choices {
                        if let Some(reason) = c.get("finish_reason").and_then(Value::as_str) {
                            self.finish
                                .insert(c["index"].as_u64().unwrap_or(0), reason.into());
                        }
                    }
                }
                normalize_chat(&mut v, &self.id, &self.model, self.exclude, true);
                if let Some(u) = v.get("usage").filter(|v| !v.is_null()) {
                    self.usage = Some(u.clone());
                    if v["choices"].as_array().is_none_or(|a| a.is_empty()) {
                        let finish = if self.finish.is_empty() {
                            BTreeMap::from([(0, "stop".into())])
                        } else {
                            self.finish.clone()
                        };
                        v["choices"]=json!(finish.into_iter().map(|(index,reason)|json!({"index":index,"delta":{"content":"","role":"assistant"},"finish_reason":reason,"native_finish_reason":reason})).collect::<Vec<_>>());
                    }
                }
                if self.kind == Kind::Completions {
                    legacy(&mut v)
                }
            }
            Kind::Responses => {
                if let Some(response) = v.get_mut("response") {
                    if let Some(u) = response.get_mut("usage") {
                        normalize_usage(u);
                        self.usage = Some(u.clone())
                    }
                    if response.get("model").is_some() {
                        response["model"] = json!(self.model)
                    }
                }
                if v["type"] == "response.completed" {
                    self.done = true
                }
            }
            Kind::Messages => {
                if let Some(m) = v.get_mut("message") {
                    if m.get("model").is_some() {
                        m["model"] = json!(self.model)
                    };
                    if let Some(u) = m.get_mut("usage") {
                        normalize_usage(u);
                        self.usage = Some(u.clone())
                    }
                }
                if let Some(u) = v.get_mut("usage") {
                    normalize_usage(u);
                    if let Some(old) = self.usage.as_ref().and_then(Value::as_object) {
                        let mut merged = old.clone();
                        if let Some(new) = u.as_object() {
                            merged.extend(new.clone())
                        }
                        self.usage = Some(Value::Object(merged))
                    } else {
                        self.usage = Some(u.clone())
                    }
                }
                if v["type"] == "message_stop" {
                    self.done = true
                }
            }
        }
        if self.max_log > 0 {
            let n = v.to_string().len();
            if self.log_bytes + n <= self.max_log {
                self.log_bytes += n;
                self.events.push(v.clone());
                self.last_event = Some(v.clone())
            }
        }
        frame.data = Some(v.to_string());
        Ok(frame.encode())
    }
    pub fn error(&mut self, message: &str) -> Vec<u8> {
        self.failed = true;
        let data = match self.kind {
            Kind::Messages => {
                json!({"type":"error","error":{"type":"api_error","message":message}})
            }
            Kind::Responses => {
                json!({"type":"error","code":"upstream_error","message":message,"param":null})
            }
            _ => {
                json!({"id":self.id,"object":"chat.completion.chunk","created":chrono::Utc::now().timestamp(),"model":self.model,"error":{"code":502,"message":message},"choices":[{"index":0,"delta":{},"finish_reason":"error","native_finish_reason":"error"}]})
            }
        };
        let event = match self.kind {
            Kind::Messages | Kind::Responses => Some("error".into()),
            _ => None,
        };
        let mut out = Frame {
            event,
            data: Some(data.to_string()),
            comments: vec![],
        }
        .encode();
        if matches!(self.kind, Kind::Chat | Kind::Completions) {
            out.extend(b"data: [DONE]\n\n");
        }
        out
    }
}
pub fn legacy(v: &mut Value) {
    if let Some(o) = v.as_object_mut() {
        o.insert("object".into(), json!("text_completion"));
    }
    if let Some(choices) = v.get_mut("choices").and_then(Value::as_array_mut) {
        for c in choices {
            let text = c
                .pointer("/delta/content")
                .or_else(|| c.pointer("/message/content"))
                .cloned()
                .unwrap_or(json!(""));
            if let Some(o) = c.as_object_mut() {
                o.remove("delta");
                o.remove("message");
                o.insert("text".into(), text);
                o.entry("logprobs").or_insert(Value::Null);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fragmented_utf8_crlf_and_multiline_data() {
        let input = "event: message\r\ndata: {\"text\":\r\ndata: \"مرحبا\"}\r\n\r\n".as_bytes();
        let mut p = Parser::default();
        let mut frames = vec![];
        for b in input {
            frames.extend(p.feed(&[*b], 4096).unwrap())
        }
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].event.as_deref(), Some("message"));
        let value: Value = serde_json::from_str(frames[0].data.as_deref().unwrap()).unwrap();
        assert_eq!(value["text"], "مرحبا");
        assert!(!p.has_pending());
    }
    #[test]
    fn bounded_events_and_incomplete_frames() {
        let mut p = Parser::default();
        assert!(p.feed(b"data: {}", 16).unwrap().is_empty());
        assert!(p.has_pending());
        assert!(p.feed(b"xxxxxxxxxxxxxxxxx", 16).is_err());
    }
    #[test]
    fn comments_done_and_multiple_frames_are_preserved() {
        let mut p = Parser::default();
        let f = p.feed(b": keepalive\n\ndata: [DONE]\n\n", 1024).unwrap();
        assert_eq!(f.len(), 2);
        assert_eq!(f[0].comments, vec![": keepalive"]);
        let mut s = State::new(
            Kind::Chat,
            "gen-test".into(),
            "go/model".into(),
            false,
            1024,
        );
        assert_eq!(
            s.transform(f.into_iter().nth(1).unwrap()).unwrap(),
            b"data: [DONE]\n\n"
        );
        assert!(s.done);
    }
    #[test]
    fn usage_chunk_gets_openrouter_choice_without_losing_cash_or_allowance() {
        let mut p = Parser::default();
        let f=p.feed(b"data: {\"id\":\"upstream\",\"choices\":[],\"usage\":{\"prompt_tokens\":2,\"completion_tokens\":3,\"cost\":0,\"opencode\":{\"allowance\":0.3,\"list_cost\":0.1}}}\n\n",4096).unwrap().pop().unwrap();
        let mut s = State::new(
            Kind::Chat,
            "gen-test".into(),
            "go/model".into(),
            false,
            4096,
        );
        let bytes = s.transform(f).unwrap();
        let value: Value = serde_json::from_slice(&bytes[6..bytes.len() - 2]).unwrap();
        assert_eq!(value["id"], "gen-test");
        assert_eq!(value["choices"][0]["delta"]["content"], "");
        assert_eq!(value["usage"]["cost"], 0);
        assert_eq!(value["usage"]["opencode"]["allowance"], 0.3);
        assert_eq!(value["usage"]["total_tokens"], 5);
    }
    #[test]
    fn tool_deltas_and_reasoning_exclusion_survive_normalization() {
        let mut v = json!({"choices":[{"index":0,"delta":{"reasoning_content":"hidden","tool_calls":[{"index":0,"function":{"arguments":"{\"city\":"}}]},"finish_reason":null}]});
        normalize_chat(&mut v, "gen-x", "m", true, true);
        assert!(v["choices"][0]["delta"].get("reasoning_content").is_none());
        assert_eq!(
            v["choices"][0]["delta"]["tool_calls"][0]["function"]["arguments"],
            "{\"city\":"
        );
    }
    #[test]
    fn committed_stream_failure_is_in_band_and_terminal() {
        let mut s = State::new(Kind::Chat, "gen-test".into(), "m".into(), false, 1024);
        let bytes = s.error("provider disconnected");
        assert!(s.failed);
        assert!(
            String::from_utf8(bytes)
                .unwrap()
                .ends_with("data: [DONE]\n\n")
        );
    }
}
