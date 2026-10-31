# Public HTTPS API on Windows Server

This deployment runs entirely on the Windows server. Clients connect directly
to `https://api.example.com/api/v1` with their own gateway API keys. Your personal
computer, RDP session, and SSH tunnel are not required for normal operation.
The gateway uses OpenCode Go remotely; it does not run an AI model locally.

## Prerequisites

- Windows Server 2019 or later, x64, with administrator access.
- A Go subscription and upstream API key.
- A public DNS hostname, for example `api.example.com`.
- A DNS A record pointing that hostname at your server's public IPv4 address.
  Remove an incorrect AAAA record if the hostname has one. For the first test,
  use direct/DNS-only mode if your DNS provider offers a proxy option.
- Inbound TCP 80 and 443 allowed by the hosting provider's firewall. The setup
  script adds Windows firewall rules for those ports. Port 8080 remains private.
- Ports 80 and 443 not already occupied by another server, such as IIS.

## Install from the Windows desktop

1. In the server's browser, sign in to GitHub and open the private project's
   [release](https://github.com/AhmedDatori/go-router/releases/tag/v0.1.0).
   Download **go-router-windows-x86_64.zip** into Downloads. Do not download the
   Linux archive. Alternatively, transfer this ZIP by SCP or another trusted
   file-transfer method; SSH is for transferring/administering only.
2. Open **Windows PowerShell as Administrator**. If this is the first install:

   ```powershell
   Expand-Archive "$env:USERPROFILE\Downloads\go-router-windows-x86_64.zip" -DestinationPath C:\
   Set-Location C:\go-router
   powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\scripts\publish-windows.ps1 -Domain api.example.com
   ```

   Replace `api.example.com` with your actual DNS hostname. Setup reads the Go
   key privately and generates a dashboard password. Existing `.env` credentials
   are preserved when upgrading an existing local setup to a public origin;
   their previous configuration is backed up privately inside `data`.

   If `C:\go-router` already exists, update only the tracked scripts from the new
   archive, or copy `scripts\publish-windows.ps1` into its scripts folder. Do not
   overwrite `.env` or `data`; stop the gateway before replacing its executable.
3. Setup downloads Caddy from its official HTTPS download endpoint, validates its
   configuration, sets `PUBLIC_URL=https://your-domain`, sets secure cookies,
   and installs the `GoRouterCaddy` Windows service. The gateway is started as the
   `GoRouter` startup task. Both start automatically and survive RDP disconnection.
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
   and create a separate application key for each person or backend. Share the
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
