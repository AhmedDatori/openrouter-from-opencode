use crate::{
    App,
    audit::Context,
    auth,
    error::{ApiError, ApiResult},
};
use axum::{
    Json,
    extract::{Extension, Path, Query, Request, State},
    http::HeaderMap,
};
use serde_json::{Value, json};
use std::collections::HashMap;

pub async fn models(
    State(app): State<App>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult<Json<Value>> {
    let allowed = if headers.contains_key("authorization") || headers.contains_key("x-api-key") {
        auth::api_key(&app, &headers, None, false)
            .await?
            .allowed_models
    } else {
        vec![]
    };
    let mut data = app.catalog.read().unwrap().public(&allowed);
    if let Some(modalities) = q.get("output_modalities") {
        let wanted = modalities.split(',').collect::<Vec<_>>();
        data["data"].as_array_mut().unwrap().retain(|m| {
            wanted.iter().all(|w| {
                m["architecture"]["output_modalities"]
                    .as_array()
                    .unwrap()
                    .contains(&json!(w))
            })
        });
    }
    paginate(&mut data, &q, "/api/v1/models")?;
    Ok(Json(data))
}
fn paginate(data: &mut Value, q: &HashMap<String, String>, path: &str) -> ApiResult<()> {
    let parse = |name: &str, default: u64| {
        q.get(name)
            .map(|v| {
                v.parse::<u64>()
                    .map_err(|_| ApiError::bad(format!("{name} must be a non-negative integer")))
            })
            .unwrap_or(Ok(default))
    };
    let offset = parse("offset", 0)?;
    let limit = parse("limit", 500)?;
    if !(1..=500).contains(&limit) {
        return Err(ApiError::bad("limit must be from 1 to 500"));
    }
    let all = data["data"].as_array().unwrap();
    let count = all.len() as u64;
    let page = all
        .iter()
        .skip(offset.min(count) as usize)
        .take(limit as usize)
        .cloned()
        .collect::<Vec<_>>();
    data["data"] = json!(page);
    data["total_count"] = json!(count);
    if offset.saturating_add(limit) < count {
        let mut url = reqwest::Url::parse("http://gateway.local").unwrap();
        url.set_path(path);
        {
            let mut pairs = url.query_pairs_mut();
            for (k, v) in q {
                if k != "offset" && k != "limit" {
                    pairs.append_pair(k, v);
                }
            }
            pairs.append_pair("offset", &(offset + limit).to_string());
            pairs.append_pair("limit", &limit.to_string());
        }
        data["links"]["next"] = json!(format!("{}?{}", url.path(), url.query().unwrap()));
    }
    Ok(())
}
pub async fn user_models(State(app): State<App>, headers: HeaderMap) -> ApiResult<Json<Value>> {
    let key = auth::api_key(&app, &headers, None, false).await?;
    let v = app.catalog.read().unwrap().public(&key.allowed_models);
    Ok(Json(v))
}
pub async fn model_count(State(app): State<App>) -> Json<Value> {
    Json(json!({"data":{"count":app.catalog.read().unwrap().models.len()}}))
}
pub async fn model_detail(
    State(app): State<App>,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let cat = app.catalog.read().unwrap();
    let model = cat
        .resolve(&id)
        .ok_or_else(|| ApiError::new(404, "Unknown Go model"))?;
    Ok(Json(json!({"data":model.public()})))
}
pub async fn model_endpoints(
    State(app): State<App>,
    Path(path): Path<String>,
) -> ApiResult<Json<Value>> {
    let id = path
        .strip_suffix("/endpoints")
        .ok_or_else(|| ApiError::new(404, "Unknown models endpoint"))?;
    let cat = app.catalog.read().unwrap();
    let m = cat
        .resolve(id)
        .ok_or_else(|| ApiError::new(404, "Unknown Go model"))?;
    let mut data = m.public();
    let o = data.as_object_mut().unwrap();
    o.insert("endpoints".into(),json!([{"name":"OpenCode Go","model_id":m.id,"model_name":m.name,"provider_name":"OpenCode Go","tag":"opencode-go","service_tier":null,"context_length":m.context_length,"max_completion_tokens":m.max_completion_tokens,"max_prompt_tokens":null,"pricing":m.pricing,"supported_parameters":m.supported_parameters,"supports_tool_choice":{"none":true,"auto":true,"required":true,"function":true},"quantization":null,"status":0,"uptime_last_30m":null,"uptime_last_5m":null,"uptime_last_1d":null,"supports_implicit_caching":true,"native_tools":{},"supports_diarization":false,"latency_last_30m":null,"throughput_last_30m":null}]));
    Ok(Json(json!({"data":data})))
}
pub async fn providers() -> Json<Value> {
    Json(
        json!({"data":[{"name":"OpenCode Go","slug":"opencode-go","privacy_policy_url":"https://opencode.ai/v2/docs/console/go/#privacy","terms_of_service_url":"https://opencode.ai/legal/terms-of-service","status_page_url":null}]}),
    )
}
pub async fn key_info(
    State(app): State<App>,
    Extension(ctx): Extension<Context>,
    headers: HeaderMap,
) -> ApiResult<Json<Value>> {
    let k = auth::api_key(&app, &headers, Some(&ctx), false).await?;
    let id = k.id.clone();
    let now = chrono::Utc::now().date_naive();
    let today = now.to_string();
    let week = (now - chrono::Duration::days(6)).to_string();
    let month = now.format("%Y-%m-01").to_string();
    let usage=app.db.call(move|c|{Ok(c.query_row("SELECT coalesce(sum(cash),0),coalesce(sum(list_cost),0),coalesce(sum(allowance),0),coalesce(sum(CASE WHEN day>=?2 THEN cash ELSE 0 END),0),coalesce(sum(CASE WHEN day>=?3 THEN cash ELSE 0 END),0),coalesce(sum(CASE WHEN day>=?4 THEN cash ELSE 0 END),0) FROM usage_daily WHERE key_id=?1",rusqlite::params![id,today,week,month],|r|Ok((r.get::<_,f64>(0)?,r.get::<_,f64>(1)?,r.get::<_,f64>(2)?,r.get::<_,f64>(3)?,r.get::<_,f64>(4)?,r.get::<_,f64>(5)?)))?)}).await.map_err(ApiError::internal)?;
    Ok(Json(
        json!({"data":{"label":k.name,"is_free_tier":false,"limit":null,"limit_remaining":null,"limit_reset":null,"usage":usage.0,"usage_daily":usage.3,"usage_weekly":usage.4,"usage_monthly":usage.5,"byok_usage":0,"byok_usage_daily":0,"byok_usage_weekly":0,"byok_usage_monthly":0,"is_management_key":false,"is_provisioning_key":false,"include_byok_in_limit":false,"budgets":[],"effective_budget":null,"creator_user_id":null,"organization_id":null,"workspace_id":null,"allowed_data_regions":["global","us","europe"],"free_model_daily_requests":{"used":0,"limit":0,"remaining":0},"rate_limit":{"requests":k.rpm,"interval":"1m","note":"Gateway-local RPM. 0 means unlimited locally; upstream Go limits still apply."},"expires_at":k.expires_at.and_then(|t|chrono::DateTime::from_timestamp(t,0)).map(|t|t.to_rfc3339()),"go_router":{"key_id":k.id,"list_cost_usd":usage.1,"allowance_usd":usage.2,"allowed_models":k.allowed_models,"upstream_quota_remaining":null,"scope_note":"These are local key policies. OpenRouter :free/BYOK/organization counters do not apply and are zero/null. Upstream Go quota and region settings remain independent."}}}),
    ))
}
pub async fn generation(
    State(app): State<App>,
    Extension(ctx): Extension<Context>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult<Json<Value>> {
    let key = auth::api_key(&app, &headers, Some(&ctx), false).await?;
    let id = q
        .get("id")
        .cloned()
        .ok_or_else(|| ApiError::bad("id query parameter is required"))?;
    let log = app
        .db
        .log(id, Some(key.id))
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::new(404, "Generation not found for this API key"))?;
    let usage = &log["usage"];
    let completion = usage["completion_tokens"]
        .as_u64()
        .or_else(|| usage["output_tokens"].as_u64());
    let prompt = usage["prompt_tokens"]
        .as_u64()
        .or_else(|| usage["input_tokens"].as_u64());
    let cash=usage.get("cost").and_then(Value::as_f64).ok_or_else(||ApiError::new(409,"Generation has no finalized usage receipt. Inspect the admin log for interruption/error details."))?;
    let mut data = json!({"id":log["id"],"upstream_id":log["upstream_id"],"model":log["model"],"created_at":log["created_at"],"total_cost":cash,"usage":cash,"provider_name":"OpenCode Go","generation_time":log["duration_ms"],"latency":log["first_token_ms"],"tokens_prompt":prompt,"tokens_completion":completion,"native_tokens_prompt":prompt,"native_tokens_completion":completion,"native_tokens_reasoning":usage.pointer("/completion_tokens_details/reasoning_tokens"),"native_tokens_cached":usage.pointer("/opencode/tokens/cache_read"),"is_byok":false,"cancelled":log["outcome"]=="client_disconnected","streamed":log.pointer("/request/stream").cloned().unwrap_or(Value::Null),"finish_reason":null,"origin":"api","data_region":"global","api_type":"completions","go_router":{"usage_receipt":usage,"outcome":log["outcome"],"attempts":log["attempts"],"upstream_data_region":null,"region_note":"global describes the gateway's unconstrained routing policy, not the upstream datacenter location"}});
    for field in [
        "cache_discount",
        "upstream_inference_cost",
        "app_id",
        "moderation_latency",
        "service_tier",
        "native_tokens_completion_images",
        "num_media_prompt",
        "num_input_audio_prompt",
        "num_media_completion",
        "num_search_results",
        "num_fetches",
        "web_search_engine",
        "native_finish_reason",
        "external_user",
        "preset_id",
        "router",
        "provider_responses",
        "user_agent",
        "http_referer",
        "workspace_id",
    ] {
        data[field] = Value::Null;
    }
    Ok(Json(json!({"data":data})))
}
pub async fn generation_content(
    State(app): State<App>,
    Extension(ctx): Extension<Context>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult<Json<Value>> {
    let key = auth::api_key(&app, &headers, Some(&ctx), false).await?;
    let id = q
        .get("id")
        .cloned()
        .ok_or_else(|| ApiError::bad("id query parameter is required"))?;
    let log = app
        .db
        .log(id, Some(key.id))
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::new(404, "Generation not found for this API key"))?;
    Ok(Json(
        json!({"data":{"id":log["id"],"request":log["request"],"response":log["response"],"capture_note":"Bodies follow server logging and redaction settings; they may be absent or truncated."}}),
    ))
}
pub async fn unsupported(req: Request) -> ApiError {
    let path = req.uri().path();
    ApiError::new(
        501,
        format!(
            "{path} is an OpenRouter platform service that OpenCode Go does not provide. See docs/compatibility.md."
        ),
    )
}
