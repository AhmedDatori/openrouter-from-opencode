# Verification and performance

Checked on **29 October 2026**. Test levels are separated below: protocol contracts,
real upstream functionality, browser behavior, and local gateway overhead.

## Automated tests

- **7 Rust unit tests:** SSE fragmentation, UTF-8/CRLF/multiline framing, usage chunk
  normalization, tool/reasoning fragments, stream errors, event size bounds, durable
  accounting after payload cleanup, and duplicate-log accounting protection.
- **31 black-box integration tests:** a real HTTP gateway process against a local
  mock upstream. Covers authentication, model aliases/pagination, JSON/SSE, usage,
  tool history, structured-output parameter forwarding, fallback, provider constraints,
  unsupported features, key scopes/expiry/revocation/RPM, CSRF/origin checks, generation
  ownership, media inputs, Responses, Messages, legacy completions, native errors,
  redaction, concurrency, cancellation, idle timeouts, and missing accounting receipts.
- **8 published OpenAPI response contracts:** models, user models, providers, single
  model, endpoints, key, generation and chat responses validated against OpenRouter's
  public specification. Its explicit `x-speakeasy-unknown-values: allow` extension is
  respected for our custom Go provider name. Closed-enum clients need to allow that
  provider value, as documented in the compatibility contract.
- **Browser regression checks:** sign in, create/edit/revoke a scoped key, model
  search, request details, admin audit events, mobile layout, and sign out.

Run the offline/core checks:

```bash
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --locked
python3 tests/integration.py
python3 scripts/check_catalog.py
node --check static/app.js   # optional development syntax check
```

OpenAPI validation uses a Python development environment, not server dependencies:

```bash
python3 -m venv .venv
.venv/bin/pip install -r requirements-dev.txt
.venv/bin/python tests/contract.py
# Or validate against a previously downloaded official spec:
.venv/bin/python tests/contract.py --spec /path/to/openrouter-openapi.json
```

It downloads only the public specification and sends inference traffic only to the
local mock. No Go key or allowance is used. Future schema additions can surface as
contract failures, prompting a reviewed compatibility update.

For browser checks, run a configured local gateway first. Install Playwright only
in an ignored development folder:

```bash
npm install --prefix data/browser-test --no-audit --no-fund playwright
node data/browser-test/node_modules/playwright/cli.js install chromium
PLAYWRIGHT_MODULE="$PWD/data/browser-test/node_modules/playwright" \
  GATEWAY_URL=http://127.0.0.1:8787 node tests/browser.cjs
```

The browser check reads the local generated `.env` admin password, creates and revokes
a uniquely named test key, and saves screenshots under ignored `data/screenshots/`.
No browser/Node dependency is needed to run the gateway in production.

## Live OpenCode Go checks

These were real requests through the local gateway, using the existing Go connection.
They are small smoke checks, not broad model-accuracy or throughput benchmarks.

| Case | Model | Result | Observed full request time |
|---|---|---|---:|
| Chat JSON | Beam Free | Correct short reply; cost and allowance both zero | 1.460 s |
| Chat SSE | Beam Free | Incremental reply, final usage, `[DONE]` | 1.483 s |
| Native Responses JSON | GPT 6 Luna | Completed native response and usage | 1.343 s |
| Native Responses SSE | GPT 6 Luna | `response.completed` and usage | 1.276 s |
| Anthropic Messages JSON | Claude Haiku 5.5 | Completed native message and usage | 1.035 s |
| Invoice image | DeepSeek V4.1 Flash | Requested invoice ID and grand total matched the source | 3.160 s |
| Audio input | MiMo V2.6 Flash | Accepted a short synthetic WAV; returned a response with audio tokens | 3.039 s |

Successful included-plan requests reported **$0 additional cash charged**. Paid-model
requests consumed their reported Go allowances; these are different quantities.
The invoice check requested only two fields and is not a complete OCR accuracy score.
The synthetic audio check is not an Arabic transcription-accuracy benchmark.

**Observed upstream limitation:** Gemini 4 Argon's Chat audio route returned `503`
“Endpoint is unavailable” for the tested audio payload, while its text/native Google
route worked. MiMo accepted audio through Chat. The gateway preserves the upstream
error and does not silently replace Gemini with another model. Use an explicitly
configured model fallback or select a working audio model.

Upstream models, availability, permissions, quotas and request adapters can change.
The maintained catalog does not assert that every listed model or modality was
successfully exercised live. Core protocol tests cannot replace upstream tests.

## Local overhead and small-server measurement

Workload: 100 sequential HTTP requests to a mock upstream, after 10 gateway warm-ups.
Fresh client HTTP connections, short fixed JSON replies, full payload logging enabled.
This measures the gateway around a local mock, not model speed or maximum throughput.

| Measurement | Native release | Docker, 0.5 CPU / 128 MiB cap |
|---|---:|---:|
| Direct mock median | 0.617 ms | 0.582 ms |
| Gateway + mock median | 1.428 ms | 1.054 ms |
| Gateway + mock p95 | 1.817 ms | 1.352 ms |
| Gateway resident memory after workload | 30,004 KiB | 29,400 KiB |
| Gateway process threads | 5 | 5 |
| Native release binary | 8,276,936 bytes | — |

The difference between medians was roughly **0.5–0.8 ms** in these local samples.
Native and container samples used different builds/compiler environments and host
load; they should not be read as a ranking. The container completed the test within
its explicit 128 MiB / half-CPU limits. This does not prove that arbitrary 20 MiB
multimodal payloads or many concurrent streams fit those limits.

A practical modest-traffic deployment starts at **1 CPU / 512 MiB RAM**. Smaller
hosts can use stricter concurrency and payload limits. Rust compilation is a separate,
more demanding workload; build elsewhere for very small production machines.

Reproduce without spending model quota:

```bash
cargo build --release --locked
python3 tests/benchmark.py
# Optional restricted-container run, on Linux:
docker build -t go-router:test .
python3 tests/benchmark.py --docker-image go-router:test
```

Actual network/model latency dominates typical inference calls. The gateway does no
local model inference, transcription, OCR, tokenization, or GPU work.

## Server monitor and concurrent chat load test (v0.3.0)

The dashboard **Server monitor** tab samples the gateway every 5 seconds and keeps
one hour in memory. It shows process CPU (100% = one vCPU), resident memory (the
working set on Windows), open requests, model slots in use, throughput, errors,
requests rejected at `MAX_CONCURRENT_REQUESTS`, response time and time to first token
(p50/p95), and the SQLite write queue. **Reset measurement** restarts the peak and
percentile summary, so one test run can be read on its own. **CSV** downloads the
samples. The same summary is written to stdout (`data/server.log` on Windows) once a
minute while traffic flows and every 15 minutes when idle. Sampling costs about 1 ms
of CPU every 5 seconds.

`tests/loadtest.py` simulates users holding multi-turn streamed conversations. Each
one sends a message, reads the reply for 3–10 seconds, then continues with the last six
turns of history. It reports client-side latency, errors and tokens. With an admin
password it also resets and reads the server monitor.

```bash
# Offline: local release binary + simulated model (0.6 s to first token, 60 tokens/s)
cargo build --release --locked
python3 tests/loadtest.py --mock --users 50 --duration 300

# Real gateway. Uses Go allowance. Use a key with rate limit 0 and set
# MAX_CONCURRENT_REQUESTS above the expected open requests (64 for 50 users).
GATEWAY_KEY=sk-gr-... GATEWAY_ADMIN_PASSWORD=... \
  python3 tests/loadtest.py --url https://gateway.example.com --users 50 --duration 300
```

Offline result on 31 October 2026 (Linux, 20-core development machine, release build,
full payload logging, simulated model): 50 users for 5 minutes.

| Measurement | Result |
|---|---:|
| Chats completed / failed | 1,356 / 0 |
| Peak open requests | 33 |
| Gateway CPU average / peak | 5.8% / 9.8% of one core |
| Gateway memory start / peak | 36.0 / 57.2 MiB |
| Response time p50 / p95 (simulated model) | 4.37 / 5.75 s |
| Gateway-added latency (client p50 − gateway p50) | about 1 ms |
| Peak SQLite write queue | 0 |

The simulated model sets the response times. A real model run on the deployment
server measures that server's CPU, network and TLS costs.

## GitHub hosted runners

The initial private-repository CI run could not start either job. GitHub reported:
“Recent account payments have failed or your spending limit needs to be increased.”
This is an account runner restriction, not a test result. Local unit/integration/
contract/browser checks and the Docker build were completed separately.
The supplied Linux x86_64 binary was built locally from the Dockerfile and passed
the HTTP integration and published-schema checks. Deployment does not depend on
Actions. After resolving GitHub account billing/limits, rerun the CI workflow.
# Dashboard credential checks (v0.2.0)

`python3 tests/credentials.py` uses fake keys and a local mock to check keyless
startup, admin/CSRF/origin protections, immediate save/rotation, persistence across
restarts, disabling without reviving an environment fallback, secret redaction,
noninteractive setup, and safe errors for malformed dotenv files.
`tests/browser-credentials.cjs` uses an isolated gateway/database and fake keys to
check first-run guidance, masked input, save/replace/remove, response/markup secrecy,
and mobile layout. It never contacts OpenCode. These are Linux runtime checks;
native Windows startup/service behavior still requires Windows verification.
