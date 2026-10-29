# Compatibility contract

Go Router implements OpenRouter-shaped inference over one provider: OpenCode Go.
Changing the URL/key works for clients using the supported contracts and Go models.
It cannot make arbitrary OpenRouter platform features or non-Go models available.

## Endpoint matrix

Both `/api/v1` and `/v1` prefixes are accepted.

| Endpoint | Behavior |
|---|---|
| `POST /chat/completions` | Chat JSON and SSE; translated model IDs and OpenRouter usage frame |
| `POST /completions` | Single string prompt mapped to chat; text-completion response shape |
| `POST /responses` | Native stateless Responses JSON/SSE passthrough to Go |
| `POST /messages` | Native Anthropic Messages JSON/SSE passthrough to Go |
| `GET /models` | Reviewed Go-only catalog in OpenRouter shape; optional key scope |
| `GET /models/user` | Key-scoped model catalog, authentication required |
| `GET /models/count` | Configured model count |
| `GET /model/{author}/{slug}` | One configured model |
| `GET /models/{author}/{slug}/endpoints` | Sole configured Go endpoint; unknown health metrics are null |
| `GET /providers` | OpenCode Go provider metadata |
| `GET /key`, `GET /auth/key` | Local key metadata and lifetime reported cash usage |
| `GET /generation?id=…` | Completed retained generation metadata; restricted to the calling key |
| `GET /generation/content?id=…` | Captured payloads subject to redaction/retention; calling key only |
| Other OpenRouter platform endpoints | Explicit `501`, never faked successful responses |

Generation metadata becomes available after response completion or disconnection.
In-flight generations do not have a completed accounting record. Retention can
expire generation details without deleting lifetime usage totals.

## Request features

| Feature | Contract |
|---|---|
| Text, system/developer messages, multi-turn history | Forwarded without local inference |
| Tool definitions, `tool_choice`, parallel tools, tool-result messages | Passed through to Go; actual support depends on the selected model |
| `response_format` including JSON schema | Passed through; constraints depend on the model/Go adapter |
| Images: URL or base64 data URL | Passed through; catalog capability checked |
| Audio, video, native PDF content parts | Passed through to a model advertising that input; no local media conversion |
| Sampling, seed, logprobs, prediction, provider-specific parameters | Forwarded; upstream may reject unsupported parameters |
| `max_tokens` | Chat alias of `max_completion_tokens`; conflicting values are rejected |
| `reasoning.effort`, `reasoning.enabled` | Mapped to Go Chat `reasoning_effort`; native Responses reasoning preserved |
| `reasoning.exclude`, `include_reasoning=false` | Removes reasoning fields from Chat output; does not avoid reasoning token cost |
| Exact `reasoning.max_tokens` budget on Chat | `501`; effort controls cannot preserve an exact token budget |
| `models`, `route=fallback` | Up to 8 eligible models; fallback only before response output is committed |
| Provider order/only/ignore | Single Go provider; impossible selection returns `503` |
| `provider.data_collection`, `provider.zdr` | Checked against reviewed catalog privacy flags |
| `provider.max_price` | Catalog-price filter only; snapshot pricing is not upstream spend enforcement |
| Provider quantization/latency/throughput guarantees | Explicit error when requested; Go exposes no such selection |
| OpenRouter plugins/search, transforms, presets, output audio/images | `501`; these are separate hosted services |
| Legacy prompt token arrays, batches, `echo`, `suffix` | Explicitly unsupported; single string completion prompt is supported |
| Responses `previous_response_id` and stored response operations | `501`; send the full conversation input |

The gateway does not implement or execute client tools. Your application executes
tool calls and sends their results in the next request, just as with an inference API.
It does not add Telegram footers or alter assistant text.

Input formats are constrained by Go's adapters as well as the model itself. A model
advertising audio input does not guarantee that every OpenRouter audio wire variant
is accepted by Go. Unavailable upstream services return genuine errors.

## Streaming

- Incoming SSE is parsed incrementally across TCP/UTF-8/CRLF boundaries, including
  multiple `data:` lines. Content is forwarded as events arrive.
- Chat response ID/model/object/provider are normalized. Tool-call argument fragments
  and choice indices are preserved. Reasoning can be excluded on output.
- Go is asked for final usage. A choice-less usage event is normalized to OpenRouter's
  content-free terminal choice. The `[DONE]` marker is preserved.
- Responses event names and message IDs are preserved. Usage remains in
  `response.completed`. Messages usage remains in `message_delta`.
- Heartbeat SSE comments keep idle proxies alive. They are not model output.
- Errors before streaming return JSON and the appropriate HTTP status. Once headers
  are committed, transport/parse/timeout failures become in-band error events.
- Premature EOF without a terminal event is an error, not a successful completion.
- Disconnecting the client drops the upstream stream. Whether generation/billing
  immediately stops depends on Go and the model provider. Final usage may be missing.
- No fallback or automatic request replay occurs after output begins.

## Errors, IDs, and accounting

Chat errors use `{ "error": { "code": 400, "message": "…", "metadata": {} } }`.
Responses and Messages use their corresponding native error skins. `Retry-After`
is preserved when the upstream supplies it. Request validation and local concurrency
limits produce explicit errors too.

`X-Generation-Id` and `X-Request-Id` identify the local request. Chat IDs use that same
`gen-…` ID. Native Responses/Messages preserve upstream resource IDs; generation
lookup accepts a retained upstream ID too, with key ownership checked.

OpenCode `usage.cost` and `usage.opencode` are retained. A Go list-price value is not
converted into a fake charge. API-key `usage` is lifetime reported cash cost, while
Go allowance/list value appear under the `go_router` extension. The service does
not invent account credit balances, original-provider timing, missing token counts,
or upstream quota remaining.

## Sessions, privacy, and availability

Clients should provide a stable `x-opencode-session` per conversation. It is hashed
with the gateway key ID before forwarding. Without it, first-message-based session
routing is derived automatically. Model conversation state is still supplied by the
client. No automatic context summarization or tokenization model runs locally.

The catalog is a maintained snapshot, not a claim that every listed model is enabled
in a particular workspace. OpenCode's regions, training permissions, and quotas are
still enforced upstream. There is one actual upstream subscription across all keys.

See [testing.md](testing.md) for measured coverage and live upstream failures. A
passing mock contract test is not proof that an unavailable upstream feature works.

## Metadata extensions

Provider name `OpenCode Go` is a custom provider value. The published OpenRouter
schema marks provider enums as allowing unknown values; clients that hard-code a
closed list of OpenRouter provider names must accept this custom value. Response
contracts are validated with that published forward-compatibility rule.

Key metadata describes local gateway policies. OpenRouter organization/BYOK/:free
variant counters do not apply and are represented as null or zero, with an explicit
`go_router.scope_note`. They are never presented as the remaining Go allowance.
Similarly, generation `data_region=global` describes unrestricted local routing;
the actual upstream region is unknown and remains null in the extension.

A generation without a final cost receipt returns `409` instead of inventing a zero
charge. Its raw admin log remains available, including the interruption/error.
