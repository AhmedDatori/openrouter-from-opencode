# Deploying on a server

For a native Windows Server deployment and a first connection from Linux, see
[the Windows setup guide](windows.md).

## Sizing

Use a small Linux VPS with one CPU core and **512 MiB RAM** as a practical starting
point for modest text/image traffic. The gateway runs no model locally. Large
multimodal payloads and many simultaneous requests increase memory consumption;
reduce `MAX_CONCURRENT_REQUESTS` and `MAX_BODY_BYTES` for very small servers.
Build-time Rust/Docker memory is separate from runtime memory: compile on a larger
machine or use a prebuilt release for a constrained host.

`docs/testing.md` records measurements from this implementation. Those are not a
capacity guarantee for your traffic or your upstream model latency.

Runtime files: one binary, `config/models.json`, `.env`, and writable `data/`.
The dashboard is embedded in the binary. SQLite is bundled; no database service is
needed. x86_64 and ARM64 can build the same source or the Dockerfile natively.

## HTTPS with Caddy

1. Point a domain's A/AAAA records to the server. Permit inbound TCP 80/443 and
   optional UDP 443. Docker must be able to reach the internet for Go and TLS.
2. Clone the private repository using your GitHub credentials.
3. Generate configuration for that exact origin:

   ```bash
   python3 scripts/setup.py --origin https://api.example.com
   ```

4. Edit `.env`: set `DOMAIN=api.example.com`. The helper has already set
   `PUBLIC_URL=https://api.example.com` and `COOKIE_SECURE=true`.
5. Start the gateway and HTTPS proxy:

   ```bash
   docker compose --profile https up -d --build
   ```

6. Visit `https://api.example.com/admin`, sign in with the password in the private
   `data/admin-login.txt` setup output, and create an application key.
7. Point your backend to `https://api.example.com/api/v1` and use that new key.

The gateway's direct Docker port is bound to host loopback; Caddy reaches it over
the private Compose network. HTTPS certificate storage and SQLite use persistent
named volumes. Do not run two gateway replicas against the same SQLite file.

`PUBLIC_URL` must match the browser origin exactly. A mismatched hostname/port
causes an intentional admin origin failure. On local HTTP, keep `COOKIE_SECURE=false`.

## systemd with a native binary

On Debian/Ubuntu, create a dedicated service account and install a compatible binary.
Use a release built on a compatible distribution, or build from source on that host.
With the prebuilt archive, replace `target/release/go-router` in the copy command
below with `go-router`.

```bash
sudo useradd --system --home /opt/go-router --shell /usr/sbin/nologin go-router
sudo mkdir -p /opt/go-router/data /opt/go-router/config
sudo cp target/release/go-router /opt/go-router/go-router
sudo cp config/models.json /opt/go-router/config/
sudo cp .env /opt/go-router/.env
sudo chmod 600 /opt/go-router/.env
sudo chown -R go-router:go-router /opt/go-router
sudo cp deploy/go-router.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now go-router
sudo systemctl status go-router
```

Use `BIND=127.0.0.1:8080` and put your existing HTTPS reverse proxy in front. An Nginx
location example is supplied in `deploy/nginx.conf`; it disables response buffering.
For standalone host Caddy, use:

```caddyfile
api.example.com {
    reverse_proxy 127.0.0.1:8080 {
        flush_interval -1
    }
}
```

Do not use the container-oriented `deploy/Caddyfile` unchanged on the host: its
upstream hostname is `gateway`, which resolves inside the Compose network.

```bash
sudo journalctl -u go-router -f
curl http://127.0.0.1:8080/healthz
curl http://127.0.0.1:8080/readyz
```

`healthz` is process health. `readyz` also checks the database worker. Neither spends
Go quota. Neither claims every upstream model is available.

## Upgrades, backups, and recovery

Before upgrading, preserve `.env`, your catalog, and the complete database directory.
SQLite uses WAL: copying only a live `.db` file is not a reliable backup.

For Docker, stop traffic and copy the complete data directory while stopped:

```bash
docker compose stop gateway
docker compose cp gateway:/app/data ./gateway-backup
docker compose start gateway
```

For a native process, use SQLite's online backup API (Python 3 is an optional
operations tool, not a runtime dependency):

```bash
python3 -c 'import sqlite3; src=sqlite3.connect("data/go-router.db"); dst=sqlite3.connect("backup.db"); src.backup(dst); dst.close(); src.close()'
```

Restore with the gateway stopped, preserve file ownership, then restart. Backups
contain sensitive logs and active session hashes; protect them like credentials.

Upgrade Docker with `git pull` and `docker compose up -d --build gateway`. For native
installs, replace the binary and restart the service. A restart expires admin sessions;
application keys, lifetime accounting and logs persist. An actively streaming request
may be interrupted when the process is forcibly stopped after its service grace period.

## Resource controls

- `MAX_CONCURRENT_REQUESTS`: caps upstream calls; saturated requests receive `429`.
- `MAX_BODY_BYTES`: limits incoming JSON, including base64 media. Default 20 MiB.
- `MAX_RESPONSE_BYTES`: limits non-streamed responses and individual SSE events.
- `MAX_LOG_BYTES`: bounds a captured request and captured response independently.
- `LOG_BODIES=false`: retain metadata and usage without storing payload content.
- `LOG_RETENTION_DAYS`: removes old detailed request logs hourly. Lifetime daily
  accounting aggregates remain. Old generation lookups eventually return `404`.
- `REQUEST_TIMEOUT_SECONDS`: overall upstream request budget across model fallbacks.
- `STREAM_IDLE_TIMEOUT_SECONDS`: maximum interval without upstream stream bytes.
- Per-key RPM limits: configured in the dashboard; `0` disables the local key limit.

The dashboard polls only while visible, and only on the overview/log pages.
Keep reverse-proxy read timeouts longer than the gateway request timeout and buffering
disabled for SSE. Keep proxy body limits aligned with your configured gateway limit.

## Model and credential maintenance

Update the upstream key in `.env` and restart. Client API keys remain unchanged.
Changing the upstream key does not change Go subscription limits or privacy settings.

Edit the reviewed model catalog and reload it through the dashboard. Existing in-flight
requests retain their selected model snapshot; new requests use the reloaded catalog.

## Troubleshooting

| Symptom | Check |
|---|---|
| Admin login returns `403` | Exact `PUBLIC_URL`, hostname, port, and Origin; use one origin |
| Login succeeds but session disappears | Secure cookies on HTTP; use HTTPS or local `COOKIE_SECURE=false` |
| Model privacy/region error | OpenCode Console → Settings → Privacy; the gateway cannot override it |
| `429` with a gateway message | Per-key RPM or global concurrent request limit |
| `429`/`503` with upstream metadata | Go limits/model availability; respect `Retry-After` when present |
| Streaming arrives all at once | Reverse-proxy buffering/compression/client streaming logic |
| Response uses $0 but allowance increases | Expected for included Go requests; check the separate receipt fields |
| Database permission error | Writable data directory / named volume ownership for UID 10001 |
| Docker build needs too much RAM | Build elsewhere or use a release artifact; runtime is much smaller |
| API not reachable externally | Local bind/loopback port is intentional; configure the HTTPS proxy |

Inspect the request in the dashboard for duration, selected model, attempt statuses,
usage, and the sanitized error. Logs do not contain the upstream authorization header.
