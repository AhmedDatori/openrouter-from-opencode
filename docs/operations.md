# Operations

Day-to-day running of an installed gateway. Setup is in
[Windows](setup-windows.md) and [Linux](setup-linux.md).

## Status and logs

| | Windows (Administrator PowerShell) | Linux |
|---|---|---|
| Gateway running? | `Get-ScheduledTask GoRouter` | `systemctl status go-router` |
| HTTPS running? | `Get-Service GoRouterCaddy` | `systemctl status caddy` |
| Gateway log | `Get-Content C:\go-router\data\server.log -Tail 50` | `journalctl -u go-router -n 50` |
| HTTPS / certificate log | `Get-Content C:\go-router\data\caddy\process.log -Tail 50` | `journalctl -u caddy -n 50` |
| Restart gateway | `Stop-ScheduledTask GoRouter; Start-ScheduledTask GoRouter` | `sudo systemctl restart go-router` |
| Restart HTTPS | `Restart-Service GoRouterCaddy` | `sudo systemctl restart caddy` |

Health checks (no model calls, no cost):

- `GET /healthz`: the process is up. Returns the version.
- `GET /readyz`: the process and its database are working.

Restarting the gateway interrupts requests in progress and signs out the dashboard.
Keys, logs and usage totals are kept.

## Monitoring

The dashboard's **Server monitor** tab shows the gateway's CPU and memory, open
requests, requests per second, response time and time to first token for the last
hour. The gateway also writes one summary line per minute while busy (every 15
minutes when idle) to its log:

```text
perf 2026-10-31T19:41:00Z cpu_avg=8.7% cpu_max=13.4% (of one core) rss=29.8MiB rss_max=30.6MiB sys_mem=1450/4096MiB sys_cpu=6% active_max=32 slots_max=32/64 db_queue_max=2 req=248 rps=4.13 err=0 rejected=0 p95_max=7016ms ttft_p95_max=4562ms
```

`rejected` counts requests refused because all `MAX_CONCURRENT_REQUESTS` slots were busy.
If it grows, raise that limit (your Go plan's limits still apply).

`tests/loadtest.py` simulates many chat users. Use `--mock` for a free run against a
local fake model; without it, it makes real paid calls. See [Testing](testing.md).

## Gateway keys and models

- Create one **API key** per application or person. Each key can have an expiry
  date, a requests-per-minute limit (`0` means no limit), a list of allowed models,
  and **spending caps** per hour, day and month. Revoking a key blocks new requests
  immediately.
- **Spending caps** limit how much of your Go plan one key can use. Spend is the Go
  allowance plus any cash charged, counted in UTC calendar windows (this hour, today,
  this month). When a key reaches a cap, its requests get `429` with a message naming
  the cap and a `Retry-After` header until the window resets. Requests already running
  when the cap is reached still finish, so a burst can go slightly over. Leave a cap
  empty for no limit.
- **Stats** on each key shows its requests, failures, spend, tokens, response times,
  cap usage with reset times, requests per day (30 days) and per hour (24 hours), and
  a per-model breakdown. **View request logs** opens that key's individual requests.
- Replace the OpenCode Go key in **Setup & settings**. It applies to new requests
  immediately, with no restart.
- Models come from `config/models.json`. To add one, edit the file, run
  `python3 scripts/check_catalog.py`, then click **Reload model catalog** in
  **Setup & settings**. New Go models are never added automatically.

## Updates

- **Windows:** `git pull --ff-only`, then
  `powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\scripts\update-windows.ps1`.
  It downloads the latest release, checks its checksum, keeps a copy of the old
  program in `data\go-router-before-update.exe`, and restarts the gateway.
- **Linux:** see [Update to a new release](setup-linux.md#update-to-a-new-release).

After `git pull` on Windows, run `scripts\secure-windows.ps1` once if the installation
is older than v0.4.1: it locks the folder permissions and moves Caddy and the gateway
from SYSTEM to the LocalService account (switching back automatically if anything
fails). Running it again later is harmless.

Each release is built by GitHub Actions with a signed build-provenance attestation. To
check that a downloaded package was built from this repository:
`gh attestation verify go-router-windows-x86_64.zip --repo AhmedDatori/openrouter-from-opencode`.

`.env`, the database and the admin password are never touched by updates. Newer
versions upgrade the database automatically on first start. After upgrading to
v0.4.0 or later, do not go back to an older version: it cannot create keys in the
upgraded database.

## Backups

Back up `.env`, `config/models.json` and the whole `data/` folder. The database
uses SQLite WAL mode, so copy `go-router.db` together with its `-wal` and `-shm`
files **while the gateway is stopped**, or use SQLite's online backup:

```bash
python3 -c 'import sqlite3; s=sqlite3.connect("data/go-router.db"); d=sqlite3.connect("backup.db"); s.backup(d)'
```

Backups contain the Go key, request logs and key hashes. Store them as privately
as the server itself. To restore, stop the gateway, put the files back, start it.

## Limits and settings

All settings are in `.env` (documented in [.env.example](../.env.example)).
Restart the gateway after editing it.

| Setting | Default | Effect |
|---|---|---|
| `MAX_CONCURRENT_REQUESTS` | 8 | Simultaneous model calls. Extra requests get `429` with `Retry-After: 1`. |
| `MAX_BODY_BYTES` | 20 MiB | Largest request, including base64 images or audio. |
| `MAX_RESPONSE_BYTES` | 16 MiB | Largest non-streamed answer or single stream event. |
| `REQUEST_TIMEOUT_SECONDS` | 600 | Total time per request, including fallbacks. |
| `STREAM_IDLE_TIMEOUT_SECONDS` | 90 | Longest silence allowed inside a stream. |
| `LOG_BODIES` | true | Save request and answer text (media removed). `false` keeps only metadata and cost. |
| `MAX_LOG_BYTES` | 64 KiB | Cap on each saved request and answer. |
| `LOG_RETENTION_DAYS` | 30 | Detailed logs older than this are deleted hourly. Usage totals stay. |
| `CORS_ORIGINS` | empty | Browser origins allowed to call the API. Empty means server-side apps only. |

Keep the reverse proxy's timeouts longer than `REQUEST_TIMEOUT_SECONDS` and its
buffering off. The provided Caddy configurations already do this.

## Troubleshooting

| Symptom | Cause and fix |
|---|---|
| Install or domain script warns that `/healthz` did not answer | Certificate not issued yet. Check the DNS A record points to this server, the provider firewall allows 443 (and 80 unless `-HttpsOnly`), then read the Caddy log. |
| Browser shows a certificate error | Wrong domain in the Caddyfile, or DNS points elsewhere. |
| Dashboard login returns `403` | `PUBLIC_URL` in `.env` does not exactly match the address in the browser. Fix it and restart the gateway. |
| Login works but you are signed out immediately | `COOKIE_SECURE=true` on a plain `http://` address. |
| `429 Too many admin login attempts` | 10 attempts per minute per client. Wait a minute. |
| `401 Invalid API key` | The key was mistyped, revoked or expired. |
| `429 API key hourly/daily/monthly spending cap reached` | The key used its cap. Wait for the reset time in the message, or raise the cap under **API keys > Edit**. |
| `429 Gateway concurrency limit reached` | All slots busy. Retry, or raise `MAX_CONCURRENT_REQUESTS`. |
| `429` or `503` mentioning OpenCode | Go plan limit reached or model unavailable. Respect `Retry-After`. |
| `503 OpenCode Go is not configured` | Save the Go key in **Setup & settings**. |
| Model privacy or region error | Change it in the OpenCode console (Settings > Privacy). The gateway cannot override it. |
| Streamed answers arrive all at once | A proxy is buffering. Use the provided Caddy configuration (`flush_interval -1`). |
| `$0` cost but allowance goes up | Normal: Go plan requests use allowance, not cash. |

For a failed request, open it in **Request logs** to see the model tried, each
attempt's status, timing, usage and the error. Logs never contain the
`Authorization` header or the Go key.
