# Public HTTPS API on Windows Server

This deployment runs entirely on the Windows server. Clients connect directly
to `https://api.example.com/api/v1` with their own gateway API keys. Your personal
computer, RDP session, and SSH tunnel are not required for normal operation.
The gateway uses OpenCode Go remotely; it does not run an AI model locally.

## Prerequisites

- Windows Server 2019 or later, x64, with administrator access.
- A Go subscription and upstream API key.
- A public DNS hostname, for example `api.example.com`, **or a public IPv4 address**.
- For a hostname, a DNS A record pointing it at your server's public IPv4 address.
  Remove an incorrect AAAA record if the hostname has one. For the first test,
  use direct/DNS-only mode if your DNS provider offers a proxy option.
- Inbound TCP 80 and 443 allowed by the hosting provider's firewall. The setup
  script adds Windows firewall rules for those ports. Port 8080 remains private.
- Ports 80 and 443 not already occupied by another server, such as IIS.

### Without a domain: use the public server IP

No DNS purchase or configuration is needed. With the current Windows package,
replace the publishing command below with:

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\scripts\publish-windows.ps1 -Address 168.119.149.240
```

Replace the example IP with your own server's IP. The resulting URLs are
`https://168.119.149.240/admin` and `https://168.119.149.240/api/v1`.
The helper explicitly selects Let's Encrypt's production ACME endpoint and
`shortlived` profile, rather than Caddy's default locally trusted IP certificate.
It also sets `default_sni` for clients that omit SNI when using an IP URL.
Public IP certificates are valid for 160 hours; Caddy handles automatic renewal.
The IP must remain assigned to this server, and incoming TCP 80/443 must reach it
through both provider-level firewalls/NAT and Windows Firewall. Clients use normal
TLS verification. There is no need to install a private CA or disable verification.

Reference: [Let's Encrypt public IP certificates](https://letsencrypt.org/2026/01/15/6day-and-ip-general-availability).

### Port 80 already in use

If another application or the Windows System/HTTP.sys listener owns port 80,
and port 443 is available, add `-HttpsOnly`:

```powershell
Set-Location C:\go-router
git pull --ff-only
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\scripts\publish-windows.ps1 -Address 168.119.149.240 -HttpsOnly
```

This mode checks/opens only TCP 443, disables Caddy's HTTP redirects and HTTP ACME
challenge, and validates certificates using TLS-ALPN on TCP 443. It does not stop
or reconfigure the existing port-80 listener. Use `https://` explicitly in clients.
Incoming TCP 443 must reach Caddy directly, including TLS-ALPN traffic, through
the hosting provider's firewall/NAT. It must also remain reachable for renewals.
This option works with both a public IP and a DNS hostname. It is an installation
option; an existing GoRouterCaddy service should be reconfigured rather than reinstalled.

See [Caddy ACME challenge options](https://caddyserver.com/docs/caddyfile/directives/tls#issuers)
and [automatic HTTPS modes](https://caddyserver.com/docs/caddyfile/options#auto-https).

## Install from the Windows desktop

### Clone and install without GitHub sign-in

Install [Git for Windows](https://git-scm.com/install/windows). Open **Windows
PowerShell as Administrator**, then run these commands on a fresh installation:

```powershell
$ErrorActionPreference = 'Stop'
git clone https://github.com/AhmedDatori/openrouter-from-opencode.git C:\go-router
if ($LASTEXITCODE -ne 0) { throw 'Clone failed. Stop here and inspect the error.' }
Set-Location C:\go-router
$packageDir = Join-Path $env:TEMP 'go-router-release'
New-Item -ItemType Directory -Path $packageDir -Force | Out-Null
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
$release = 'https://github.com/AhmedDatori/openrouter-from-opencode/releases/download/v0.2.0'
$zip = Join-Path $packageDir 'go-router-windows-x86_64.zip'
Invoke-WebRequest -UseBasicParsing "$release/go-router-windows-x86_64.zip" -OutFile $zip
Invoke-WebRequest -UseBasicParsing "$release/go-router-windows-x86_64.zip.sha256" -OutFile "$zip.sha256"
$expected = (Get-Content "$zip.sha256" -Raw).Trim().Split(' ')[0]
if ((Get-FileHash $zip -Algorithm SHA256).Hash.ToLowerInvariant() -ne $expected) {
    throw 'Download checksum mismatch.'
}
Expand-Archive $zip -DestinationPath "$packageDir/unpacked" -Force
Copy-Item "$packageDir/unpacked/go-router/go-router.exe" .\go-router.exe
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\scripts\publish-windows.ps1 -Address 168.119.149.240
```

Use your own server IP in the last line. The binary is prebuilt: GitHub CLI,
GitHub sign-in, Rust, and a C compiler are not needed. If `C:\go-router` already
exists, use/update that installation instead of deleting its private configuration.
Follow steps 4-6 below to retrieve the admin password, verify public access, and
create application keys. Provider-level TCP 80/443 access is still required.

### Install by downloading the ZIP in a browser

1. In the server's browser, open the project's public
   [release](https://github.com/AhmedDatori/openrouter-from-opencode/releases/tag/v0.2.0).
   No GitHub sign-in is required.
   Download **go-router-windows-x86_64.zip** into Downloads. Do not download the
   Linux archive. Alternatively, transfer this ZIP by SCP or another trusted
   file-transfer method; SSH is for transferring/administering only.
2. Open **Windows PowerShell as Administrator**. If this is the first install:

   ```powershell
   Expand-Archive "$env:USERPROFILE\Downloads\go-router-windows-x86_64.zip" -DestinationPath C:\
   Set-Location C:\go-router
   powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\scripts\publish-windows.ps1 -Domain api.example.com
   ```

   Replace `api.example.com` with your actual DNS hostname. Setup generates a dashboard password without asking for a Go key. Existing `.env` credentials
   are preserved when upgrading an existing local setup to a public origin;
   their previous configuration is backed up privately inside `data`.

   If `C:\go-router` already exists, update only the tracked scripts from the new
   archive, or copy `scripts\publish-windows.ps1` into its scripts folder. Do not
   overwrite `.env` or `data`; stop the gateway before replacing its executable.
3. Setup downloads Caddy 2.11.7 from its official GitHub release, verifies the
   pinned SHA512 checksum, retries transient download failures up to three times,
   validates its
   configuration, sets `PUBLIC_URL=https://your-domain`, sets secure cookies,
   and installs the `GoRouterCaddy` Windows service. The gateway is started as the
   `GoRouter` startup task. Both start automatically and survive RDP disconnection.

   If an older installer fails downloading from `caddyserver.com/api/download`,
   run `git pull --ff-only` and retry the publishing command. The current installer
   uses the standard official GitHub release ZIP instead. It does not disable TLS
   certificate verification. A pre-existing `caddy\caddy.exe` is reused.
4. Read the dashboard login details on the server:

   ```powershell
   notepad C:\go-router\data\admin-login.txt
   ```

5. From another machine, verify public access:

   ```bash
   curl --fail https://api.example.com/readyz
   ```

   Caddy certificate issuance can take a little time after correct DNS and ports
   become reachable. A successful script invocation does not prove public reachability.
6. Open `https://api.example.com/admin`, sign in with the generated admin password,
   and open **Setup & settings** to save your OpenCode Go API key. It takes effect
   immediately without a restart. Then create a separate application key for each
   person or backend. Share the
   base URL and that person's key, not the dashboard password or Go upstream key.

## Client example

```javascript
import OpenAI from "openai";

const client = new OpenAI({
  baseURL: "https://api.example.com/api/v1",
  apiKey: process.env.GO_ROUTER_API_KEY,
});

const response = await client.chat.completions.create({
  model: "deepseek/deepseek-v4.1-flash",
  messages: [{role: "user", content: "Hello"}],
});
console.log(response.choices[0].message.content);
```

Use models returned by `/api/v1/models`. Server backends work without CORS; browser
applications need allowed browser origins configured in `CORS_ORIGINS` and must
consider whether exposing their application key to end users is appropriate.
The compatibility matrix still applies; unsupported OpenRouter platform products
are not enabled by deploying publicly. Every client shares the same Go subscription
allowances, privacy settings, availability, and upstream intended-use conditions.

## Operations

### Upgrade to the server monitor (v0.3.0)

Use Administrator PowerShell:

```powershell
Set-Location C:\go-router
git pull --ff-only
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\scripts\update-windows.ps1
```

The dashboard gains a **Server monitor** tab. A `perf ...` summary line is appended to
`data\server.log` every minute while requests are flowing. To check it:

```powershell
Get-Content .\data\server.log -Tail 20 | Select-String perf
```

### Upgrade to dashboard-managed Go keys (v0.2.0)

On an existing clone, use Administrator PowerShell:

```powershell
Set-Location C:\go-router
git pull --ff-only
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\scripts\update-windows.ps1
```

This downloads and verifies v0.2.0, backs up the previous executable inside `data`,
and replaces only the executable. It preserves `.env`, SQLite, and the existing
dashboard password. A running gateway startup task is stopped and restarted.
In-flight requests can be interrupted. Caddy is left running.

If first setup previously stopped at the Go-key prompt or configuration check,
finish installation without entering a Go key in the console:

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\scripts\publish-windows.ps1 -Address 168.119.149.240 -HttpsOnly
notepad .\data\admin-login.txt
```

Use `-HttpsOnly` when port 80 is occupied. Then sign in at
`https://168.119.149.240/admin`, open **Setup & settings**, paste the Go API key into
the masked field, and choose **Save Go key**. The server accepts new model requests
immediately; no restart is needed. Saving validates the key's format; the next
actual model request verifies it with OpenCode. Without a key, authenticated
inference requests return a descriptive `503`, while the dashboard remains usable.
An existing Caddy service means publishing is already installed; do not reinstall it.

Windows configuration/catalog paths resolve relative to `go-router.exe`, including
startup tasks launched from System32. Invalid `.env` parsing now fails with a
redacted format/encoding message instead of silently ignoring the file.

```powershell
Get-Service GoRouterCaddy
Get-ScheduledTask -TaskName GoRouter
Get-Content C:\go-router\data\caddy\process.log -Tail 50
Get-Content C:\go-router\data\server.log -Tail 50
Restart-Service GoRouterCaddy
```

Certificates are kept privately in `data\caddy\storage` and renewed by Caddy. Back
up the complete private data directory. Caddy process logs rotate at 10 MiB, with
five rolled files retained. Caddy's administrative API is disabled; after editing
its Caddyfile, validate the configuration and restart the service. The gateway's
dashboard remains the only application dashboard.

If the publishing script reports an existing Caddy service, inspect/update that
deployment instead of reinstalling it. If initial issuance fails, check DNS,
provider-level port rules, and Caddy's process log; do not disable TLS verification
in clients. A domain working only from inside the server is not a complete test.

References: [Caddy automatic HTTPS](https://caddyserver.com/docs/automatic-https),
[Caddy Windows services](https://caddyserver.com/docs/running#windows-service).
