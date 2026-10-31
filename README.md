# OpenRouter from OpenCode

A small Rust gateway that exposes **OpenRouter-compatible inference endpoints** using
an **OpenCode Go** subscription. One binary, one SQLite database, one admin dashboard.
No GPU, local model, Redis, PostgreSQL, Node.js runtime, or OpenCode CLI required.

Connect an existing client by changing its **base URL**, **API key**, and, if needed,
its **model ID** to one available in your Go plan:

```text
Base URL: https://your-domain.example/api/v1
API key:  create one in /admin
Models:   GET /api/v1/models
```

The `/v1` prefix is also supported. Your upstream Go key stays on the server.
Client keys are separate and can be revoked individually.

> Compatibility covers inference APIs, not every hosted OpenRouter product.
> OpenRouter's hosted search/plugins, credits/payments, embeddings, batches,
> image/video generation, files storage, and stateful Responses are explicit
> unsupported operations. See the [compatibility matrix](docs/compatibility.md).
> Model availability and the Go workspace's permissions still apply.

## What is included

- Chat Completions: JSON and SSE, tools, tool-result history, structured output
  schemas, sampling parameters, reasoning effort, images, and model fallbacks.
- Native Responses and Anthropic Messages request/stream forwarding, subject to
  upstream support; full conversation input is required for Responses.
- Audio/video/PDF input passthrough when the selected Go model and its API adapter
  support it. No local transcription, OCR, or media model is installed.
- OpenRouter-shaped model catalog, model aliases, provider/model metadata,
  key introspection, generation lookup, numeric chat errors, native error skins,
  `Retry-After`, heartbeat comments, usage chunks, and `[DONE]` framing.
- A single admin dashboard: create/edit/revoke/expire keys, model scopes, per-key
  request limits, lifetime usage/cost totals, request logs, filters, export, model
  catalog, and admin audit events.
- Actual cash cost, list-price value, and Go allowance consumption shown separately.
- Bounded payload logging, media redaction, detailed log retention, and durable
  accounting totals that survive detailed-log cleanup.
- Docker/Compose with optional HTTPS, systemd, CI tests, release packaging, and
  setup instructions for x86_64 and ARM64 servers.

## Quick start with Docker

On **Windows Server**, use the native ZIP package and the
[public Windows deployment guide](docs/windows-public.md). No Docker or compiler is required;
PowerShell helpers configure private credentials and an automatic startup task.

Requirements: Docker Engine, Compose plugin, an active Go subscription and API key.
Python 3 is used only by the optional setup helper. You can edit `.env` manually.

```bash
git clone https://github.com/AhmedDatori/openrouter-from-opencode.git go-router
cd go-router
python3 scripts/setup.py
docker compose up -d --build gateway
```

The public repository and release downloads require no GitHub sign-in. The helper
reads your Go key with hidden terminal input, generates a strong admin password, and creates
`.env` with mode `600`. It does not print credentials or overwrite an existing file.

Open **http://localhost:8080/admin**. Read your generated login information locally
from `data/admin-login.txt`, sign in, and create your first application API key.
The host-side login file is private setup output; container database storage uses
its own named volume. Do not commit or share either.

For a remote server, initially use an SSH tunnel:

```bash
ssh -L 8080:127.0.0.1:8080 your-user@your-server
```

Then visit `http://localhost:8080/admin`. For a public HTTPS URL, follow
[server deployment](docs/deployment.md#https-with-caddy).

```bash
docker compose logs -f gateway
docker compose exec gateway /app/go-router --healthcheck
```

## Run a binary or build from source

A release package contains the binary, catalog, and documentation. It needs no Rust
compiler. For Linux x86_64 with glibc 2.34+ (for example Debian 12 or Ubuntu 22.04+):

```bash
curl -fLO https://github.com/AhmedDatori/openrouter-from-opencode/releases/download/v0.1.0/go-router-linux-x86_64.tar.gz
curl -fLO https://github.com/AhmedDatori/openrouter-from-opencode/releases/download/v0.1.0/go-router-linux-x86_64.tar.gz.sha256
sha256sum -c go-router-linux-x86_64.tar.gz.sha256
tar xzf go-router-linux-x86_64.tar.gz
cd go-router
python3 scripts/setup.py
./go-router --check
./go-router
```

Release downloads are public. You can also download the archive in your browser
and copy it to the server. ARM64 can build
the Dockerfile/source natively; the release workflow includes an ARM64 build job.

For source builds, install Rust **1.88+** and a C compiler for bundled SQLite:

```bash
# Debian/Ubuntu, for building only:
sudo apt-get update
sudo apt-get install -y build-essential pkg-config
# Install Rust from https://rustup.rs if it is not already installed.
cargo build --release --locked
python3 scripts/setup.py
./target/release/go-router --check
./target/release/go-router
```

Start the binary **from the project/package directory** so `.env` and
`config/models.json` resolve correctly. Use a system service for unattended operation.
See [systemd setup](docs/deployment.md#systemd-with-a-native-binary).

A small runtime server does not have to compile Rust: build on another machine,
use Docker, or download a release produced by the included GitHub workflow.

## Use an existing backend

### Python / OpenAI-compatible client

```python
import os
from openai import OpenAI

client = OpenAI(
    base_url=os.environ["GATEWAY_BASE_URL"],  # https://your-domain/api/v1
    api_key=os.environ["GATEWAY_API_KEY"],   # a key created in this dashboard
)
response = client.chat.completions.create(
    model="deepseek/deepseek-v4.1-flash",
    messages=[{"role": "user", "content": "Hello"}],
    stream=True,
)
for event in response:
    if event.choices:
        print(event.choices[0].delta.content or "", end="")
```

### JavaScript

```javascript
import OpenAI from "openai";

const client = new OpenAI({
  baseURL: process.env.GATEWAY_BASE_URL,
  apiKey: process.env.GATEWAY_API_KEY,
});
const response = await client.chat.completions.create({
  model: "deepseek/deepseek-v4.1-flash",
  messages: [{ role: "user", content: "Hello" }],
});
console.log(response.choices[0].message.content);
```

### HTTP

```bash
curl "$GATEWAY_BASE_URL/chat/completions" \
  -H "Authorization: Bearer $GATEWAY_API_KEY" \
  -H "Content-Type: application/json" \
  -H "x-opencode-session: your-stable-conversation-id" \
  -d '{"model":"deepseek/deepseek-v4.1-flash","messages":[{"role":"user","content":"Hello"}],"stream":true}'
```

A stable `x-opencode-session` per conversation improves upstream routing and caching.
If omitted, the gateway derives a key-scoped routing ID from the first user message.
This header does not store model history: send your messages/tool results each turn.

[More examples](examples/) · [API reference](docs/api.md) ·
[Compatibility](docs/compatibility.md) · [Operations](docs/deployment.md) ·
[Security](SECURITY.md) · [Tests and measurements](docs/testing.md)

## Models

The reviewed catalog starts with 30 Go models, including DeepSeek, GLM, Qwen, Kimi,
MiMo, MiniMax, GPT Luna, Haiku, Gemini, and the Go free routes. The catalog includes
input modalities, context limits, list prices, and privacy flags. Go may restrict
some entries by region, quota, or workspace privacy settings.

Canonical OpenRouter-style IDs and raw Go IDs are accepted. For example:

```text
deepseek/deepseek-v4.1-flash  -> deepseek-v4.1-flash
opencode-go/deepseek-v4.1-flash -> deepseek-v4.1-flash
deepseek-v4.1-flash          -> deepseek-v4.1-flash
```

Catalog maintenance is deliberate: edit `config/models.json`, run
`python3 scripts/check_catalog.py`, then click **Reload model catalog** in the admin
settings. New Go models are not silently enabled from an unreviewed global catalog.
Prices are snapshots; the actual usage receipt is authoritative.

## Costs and limits

```json
{
  "usage": {
    "prompt_tokens": 1200,
    "completion_tokens": 300,
    "cost": 0,
    "opencode": {
      "billing_source": "go",
      "list_cost": 0.0012,
      "allowance": 0.0009
    }
  }
}
```

- `cost`: actual additional cash charged according to the upstream receipt.
- `list_cost`: nominal model-price value; it is not necessarily a charge.
- `allowance`: amount consumed from Go's weighted allowance.
- `MONTHLY_SUBSCRIPTION_USD`: your fixed monthly price, displayed separately.

A `$0` request can consume allowance. Missing usage remains unknown and is counted
as missing; disconnected streams are never presented as confirmed free requests.
Only final reported usage is authoritative. No usage or price figures are invented.

All downstream keys share the same upstream subscription. Creating more client keys
does not multiply Go limits. The gateway never falls back to the paid Console API.
Go is intended for coding-agent traffic; consult its current allowed-use guidance
before exposing the service to other workloads or users.

## Configuration and development

All configuration is documented in [.env.example](.env.example). Defaults: loopback
bind, 8 concurrent inference calls, 20 MiB requests, 16 MiB JSON/SSE event limit,
600-second request deadline, 90-second stream idle timeout, 64 KiB captured payloads,
and 30-day detailed logs. Admin sessions last 12 hours and expire on restart.

```bash
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --locked
python3 tests/integration.py
python3 scripts/check_catalog.py
```

Integration tests use a local mock upstream and fake credentials. They verify
protocol behavior without consuming any model quota. Real upstream availability
is tested separately and documented with its limitations.

## Upstream references

- [OpenRouter API](https://openrouter.ai/docs/api_reference/overview)
- [OpenRouter streaming](https://openrouter.ai/docs/api_reference/streaming)
- [OpenCode Go usage and allowed clients](https://opencode.ai/v2/docs/console/go/)
- [OpenCode inference cost fields](https://opencode.ai/v2/docs/console/api/inference/)

MIT licensed. Go Router is an independent project.
