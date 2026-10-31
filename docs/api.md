# API usage

Base URL: `https://your-domain/api/v1` (or `/v1`).

Authentication: `Authorization: Bearer <gateway-key>` for inference, key metadata,
and generation lookup. `/models` and model/provider metadata are public, like an
OpenRouter catalog. Supplying a key to `/models` filters it to that key's model scope.

`x-api-key` authentication is also accepted for Anthropic-compatible clients.

A gateway API key is created in the admin dashboard. It is not your Go API key.

## Chat

```json
{
  "model": "deepseek/deepseek-v4.1-flash",
  "messages": [{"role":"user","content":"Hello"}],
  "stream": false,
  "max_tokens": 1000,
  "reasoning": {"effort":"low","exclude":true}
}
```

All model history belongs in `messages`. The gateway does not execute tools or
remember a conversation's missing turns for you. For model fallbacks:

```json
{
  "models": ["deepseek/deepseek-v4.1-flash", "glm-5.3-flash"],
  "route": "fallback",
  "messages": [{"role":"user","content":"Hello"}]
}
```

Fallback occurs only before a downstream response starts. An upstream authentication
or payment error stops routing. Once streaming starts, failures are in-band events.
At most eight model candidates are allowed. API-key model scopes are enforced on
all candidates. No request is rerouted to a non-Go provider.

## Multimodal input

Use standard OpenRouter content parts. The gateway preserves the part's payload and
lets Go perform the model/API conversion. Examples are in `examples/multimodal.json`.
A non-vision model rejects image input rather than silently dropping the image.
Native Responses and Messages formats use their own content-part schemas.

There is no local audio transcription or OCR stage. Input availability depends on
both the model and the upstream adapter. The gateway returns the upstream error if
that service is unavailable.

## Errors and streaming

```json
{"error":{"code":401,"message":"Invalid API key","metadata":{}}}
```

HTTP status and `error.code` agree for ordinary chat errors. Native Responses and
Messages have their corresponding string-coded errors. Local throttling returns
`429` with a `Retry-After` header. Upstream retry hints are preserved.

With `stream=true`, consume SSE `data:` events, ignore comments, handle an `error`
field, and finish on `[DONE]`. OpenRouter-style usage chunks contain a content-free
choice; don't treat every terminal `finish_reason` as a separate completion.

Native Responses and Messages use their native SSE event names and terminal events.
A broken stream emits an explicit error rather than masquerading as a success.

## Response headers

| Header | Meaning |
|---|---|
| `X-Request-Id` | Local request/generation ID |
| `X-Generation-Id` | Same local generation ID, including native API calls |
| `X-Go-Router-Latency-Ms` | Gateway time until response headers were prepared; not full model time |
| `Retry-After` | Retry delay when known |
| `X-Accel-Buffering: no` | SSE buffering hint |

Full elapsed milliseconds, first transport byte, and first token time are in the
completed admin log. A heartbeat can precede the first model token. Non-streamed
calls do not provide a separately observed first-token timestamp.

## Admin endpoints

All paths below are under `/admin/api`. Authentication uses the dashboard cookie,
not a client API key. Mutations require `X-CSRF-Token` from the session/login result.

| Method/path | Purpose |
|---|---|
| `POST /login` | `{password}` → HttpOnly session cookie and CSRF token |
| `GET /session` | Current session and CSRF token |
| `DELETE /session` | Logout |
| `GET /stats` | Lifetime totals, daily history, per-key totals |
| `GET /keys` | Client key metadata; never raw secrets/hashes |
| `POST /keys` | `{name,rpm?,allowed_models?,expires_at?}` → one-time key |
| `PATCH /keys/{id}` | Update name, RPM, scopes, expiry, disabled state |
| `DELETE /keys/{id}` | Revoke key, preserving audit history |
| `GET /logs` | Paginated request summaries; `page`, `limit`, `key_id`, `model`, `errors` |
| `GET /logs/{id}` | Details and bounded/redacted captured payloads |
| `GET /events` | Most recent 100 admin audit events |
| `GET /models` | Configured models with privacy/capability metadata |
| `POST /models/reload` | Validate and reload the on-disk catalog |
| `GET /settings` | Non-secret configuration only |

`expires_at` is a future Unix timestamp in seconds or null. `rpm=0` means no local
per-key RPM limit. `allowed_models=[]` means every configured Go model. Upstream
limits always remain in force. The admin password cannot be used as an inference key.

The dashboard's log export downloads up to 5000 filtered summaries. Individual
payloads can be inspected in request details; use paginated admin APIs or a private
database backup for larger operational exports.
# Dashboard upstream credentials

The server can start without `OPENCODE_GO_API_KEY`. Sign in to the dashboard and
save a key under **Setup & settings**. `POST /admin/api/upstream-key` accepts
`{"api_key":"YOUR_OPENCODE_GO_KEY"}`. `DELETE` on the same route disables the
credential, including after restart. Both require the admin session, CSRF token,
and valid origin. They never return the saved key. Settings return only configured
status/source. The value is stored privately in SQLite; it is not separately
encrypted at rest. Legacy environment credentials are used only when no dashboard
setting exists. Changes apply to new requests; in-flight requests keep their
original credential snapshot. Saving checks format, not provider validity.
