use crate::error::{ApiError, ApiResult};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashSet;

#[derive(Clone, Deserialize, Serialize)]
pub struct Model {
    pub id: String,
    pub upstream_id: String,
    pub name: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    pub context_length: u64,
    pub max_completion_tokens: u64,
    pub input_modalities: Vec<String>,
    pub output_modalities: Vec<String>,
    pub pricing: Value,
    pub supported_parameters: Vec<String>,
    #[serde(default)]
    pub trains_on_data: bool,
    #[serde(default)]
    pub zero_data_retention: bool,
    #[serde(default)]
    pub variants: Vec<String>,
}
#[derive(Clone)]
pub struct Catalog {
    pub models: Vec<Model>,
}
impl Catalog {
    pub fn parse(s: &str) -> anyhow::Result<Self> {
        let models: Vec<Model> = serde_json::from_str(s)?;
        anyhow::ensure!(!models.is_empty(), "Model catalog is empty");
        let mut names = HashSet::new();
        for m in &models {
            anyhow::ensure!(
                m.context_length > 0
                    && m.max_completion_tokens > 0
                    && !m.upstream_id.contains('/')
                    && !m.id.is_empty(),
                "Invalid model {}",
                m.id
            );
            for name in std::iter::once(&m.id).chain(m.aliases.iter()) {
                anyhow::ensure!(names.insert(name.clone()), "Duplicate model/alias {name}");
            }
            for k in ["prompt", "completion"] {
                let price = m.pricing[k].as_str().and_then(|s| s.parse::<f64>().ok());
                anyhow::ensure!(
                    price.is_some_and(|p| p.is_finite() && p >= 0.0),
                    "Invalid price on {}",
                    m.id
                );
            }
        }
        Ok(Self { models })
    }
    pub fn resolve(&self, id: &str) -> Option<&Model> {
        self.models
            .iter()
            .find(|m| m.id == id || m.upstream_id == id || m.aliases.iter().any(|a| a == id))
    }
    pub fn public(&self, allowed: &[String]) -> Value {
        let data = self
            .models
            .iter()
            .filter(|m| allowed.is_empty() || allowed.contains(&m.id))
            .map(Model::public)
            .collect::<Vec<_>>();
        json!({"total_count":data.len(),"links":{"next":null},"data":data})
    }
}
impl Model {
    pub fn public(&self) -> Value {
        json!({"id":self.id,"canonical_slug":self.id,"name":self.name,"created":1793232000,"description":format!("{} through OpenCode Go. Availability and privacy settings are controlled by your Go workspace.",self.name),"context_length":self.context_length,"hugging_face_id":null,"architecture":{"modality":format!("{}->{}",self.input_modalities.join("+"),self.output_modalities.join("+")),"input_modalities":self.input_modalities,"output_modalities":self.output_modalities,"tokenizer":"Other","instruct_type":null},"pricing":self.pricing,"top_provider":{"context_length":self.context_length,"max_completion_tokens":self.max_completion_tokens,"is_moderated":false},"per_request_limits":null,"supported_parameters":self.supported_parameters,"default_parameters":{},"supported_voices":null,"links":{"details":format!("/api/v1/models/{}/endpoints",self.id)},"expiration_date":null,"go_router":{"upstream_id":self.upstream_id,"aliases":self.aliases,"trains_on_data":self.trains_on_data,"zero_data_retention":self.zero_data_retention,"price_type":"Go list value; actual cash and allowance are in response usage"}})
    }
    pub fn check_provider(&self, p: &Value) -> ApiResult<()> {
        let Some(o) = p.as_object() else {
            return Err(ApiError::bad("provider must be an object"));
        };
        fn go(s: &str) -> bool {
            matches!(
                s.to_lowercase().as_str(),
                "opencode-go" | "opencode go" | "opencode" | "go-router"
            )
        }
        for name in ["only", "order"] {
            if let Some(v) = o.get(name) {
                let a = v
                    .as_array()
                    .ok_or_else(|| ApiError::bad(format!("provider.{name} must be an array")))?;
                if !a.is_empty()
                    && !a.iter().any(|v| v.as_str().is_some_and(go))
                    && (name == "only" || o.get("allow_fallbacks") == Some(&Value::Bool(false)))
                {
                    return Err(ApiError::new(
                        503,
                        "No endpoint matches provider preferences; this gateway has only OpenCode Go",
                    ));
                }
            }
        }
        if o.get("ignore")
            .and_then(Value::as_array)
            .is_some_and(|a| a.iter().any(|v| v.as_str().is_some_and(go)))
        {
            return Err(ApiError::new(
                503,
                "OpenCode Go was excluded by provider.ignore",
            ));
        }
        if o.get("data_collection").and_then(Value::as_str) == Some("deny") && self.trains_on_data {
            return Err(ApiError::new(
                503,
                "This model trains on request data and cannot satisfy data_collection=deny",
            ));
        }
        if o.get("zdr").and_then(Value::as_bool) == Some(true) && !self.zero_data_retention {
            return Err(ApiError::new(
                503,
                "This model cannot satisfy the requested zero-retention policy",
            ));
        }
        for field in [
            "quantizations",
            "preferred_min_throughput",
            "preferred_max_latency",
            "enforce_distillable_text",
        ] {
            if o.get(field)
                .is_some_and(|v| !v.is_null() && v != &json!([]) && v != &Value::Bool(false))
            {
                return Err(ApiError::bad(format!(
                    "provider.{field} cannot be enforced by this single-upstream gateway"
                )));
            }
        }
        if let Some(prices) = o.get("max_price").and_then(Value::as_object) {
            for (key, price) in [("prompt", "prompt"), ("completion", "completion")] {
                if let Some(max) = prices.get(key).and_then(Value::as_f64) {
                    let actual = self.pricing[price]
                        .as_str()
                        .and_then(|s| s.parse::<f64>().ok())
                        .unwrap_or(f64::INFINITY)
                        * 1e6;
                    if actual > max {
                        return Err(ApiError::new(
                            503,
                            "Go model list price exceeds provider.max_price",
                        ));
                    }
                }
            }
        }
        Ok(())
    }
}
