# Set up a new Windows Server

This guide installs Go Router on a fresh Windows Server and publishes it at
`https://<your-domain>` with a free, automatically renewed HTTPS certificate.
It takes about 15 minutes. No Rust, Docker, Python or compiler is needed.

## What you need

| Item | Details |
|---|---|
| Server | Windows Server 2019 or newer, x64. 2 vCPU / 4 GB RAM is plenty (the gateway itself uses about 30 MiB). |
| Access | Remote Desktop with an Administrator account. |
| Domain | A hostname such as `api.example.com`, with a DNS **A record** pointing to the server's public IPv4 address. Remove any AAAA record. If your DNS has a proxy option (for example Cloudflare's orange cloud), turn it off. |
| Ports | Your hosting provider's firewall must allow inbound **TCP 443**, and **TCP 80** unless port 80 is already used on the server (see step 3). Do **not** open 8080. |
| OpenCode Go | An active Go subscription and an API key from the OpenCode console. You paste it in the dashboard at the end, not in a terminal. |

Check the DNS record from any computer before you start. It must print the
server's IP:

```powershell
Resolve-DnsName api.example.com -Type A
```

## 1. Install Git

Sign in to the server with Remote Desktop and install
[Git for Windows](https://git-scm.com/download/win) with the default options.
It is used to download the scripts now and to update them later.

## 2. Download the project

Open **Windows PowerShell as Administrator** and run:

```powershell
New-Item -ItemType Directory C:\go-router | Out-Null
icacls C:\go-router /inheritance:r /grant:r '*S-1-5-32-544:(OI)(CI)F' '*S-1-5-18:(OI)(CI)F'
git clone https://github.com/AhmedDatori/openrouter-from-opencode.git C:\go-router
Set-Location C:\go-router
```

The `icacls` line makes the folder writable only by Administrators and SYSTEM
**before** anything is downloaded into it, so no other account on the server can
plant or change files that later run with high privileges. No GitHub account is
needed. Keep the folder at `C:\go-router`; the rest of this guide assumes that path.

## 3. Check port 80

```powershell
Get-NetTCPConnection -State Listen -LocalPort 80,443 -ErrorAction SilentlyContinue
```

- **Nothing printed:** ports are free. Use the normal command in step 4.
- **Port 80 is listed** (often IIS or another Windows web service): add `-HttpsOnly`
  in step 4. Go Router will use port 443 only and leave port 80 alone.
- **Port 443 is listed:** another web server already owns HTTPS. Stop here and decide
  with the server owner which program should use it.

## 4. Install

Replace `api.example.com` with your domain:

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\scripts\install-windows.ps1 -Domain api.example.com
# or, if port 80 is in use:
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\scripts\install-windows.ps1 -Domain api.example.com -HttpsOnly
```

The script:

1. Locks the folder permissions, then checks that the domain resolves and the ports are free.
2. Downloads the latest `go-router.exe` from this repository's GitHub releases and
   verifies its SHA-256 checksum.
3. Downloads Caddy 2.11.7 from its official release and verifies the SHA-512 checksums
   of the archive and of `caddy.exe`.
4. Creates `.env` and a random admin password in `data\admin-login.txt`, and
   restricts the folder to Administrators and SYSTEM.
5. Registers the **GoRouter** startup task (the gateway, on `127.0.0.1:8080`).
6. Installs the **GoRouterCaddy** Windows service (HTTPS, with HSTS) and firewall rules
   for Caddy only.
7. Waits until `https://<domain>/healthz` answers with a valid certificate.
8. Runs `secure-windows.ps1`, which switches Caddy and the gateway from SYSTEM to the
   low-privilege **LocalService** account and switches back automatically if they do
   not come up healthy.

It ends with `Installed. Admin: https://api.example.com/admin ...`. If it prints a
warning instead, see [Troubleshooting](operations.md#troubleshooting).

The execution-policy flag applies only to that one command; no system policy changes.

## 5. Sign in and connect OpenCode Go

```powershell
notepad C:\go-router\data\admin-login.txt
```

1. Open `https://api.example.com/admin` and sign in with that password. Save it in a
   password manager, then delete the file: `Remove-Item C:\go-router\data\admin-login.txt`.
2. Open **Setup & settings**, paste your OpenCode Go API key and click **Save Go key**.
   It takes effect immediately.
3. Open **API keys** and create one key per application or developer. The full key
   is shown only once.

## 6. Test from another computer

```bash
curl https://api.example.com/readyz
curl https://api.example.com/api/v1/chat/completions \
  -H "Authorization: Bearer sk-gr-YOUR-APP-KEY" \
  -H "Content-Type: application/json" \
  -d '{"model":"deepseek/deepseek-v4.1-flash","messages":[{"role":"user","content":"Hello"}]}'
```

The request appears under **Request logs** in the dashboard with its cost.

## What to give developers

```text
Base URL: https://api.example.com/api/v1
API key:  sk-gr-...   (their own key from the dashboard)
Model:    deepseek/deepseek-v4.1-flash   (any ID from /api/v1/models)
```

Never share the admin password or the OpenCode Go key. Send keys privately.

## After installation

| Task | Command (Administrator PowerShell in `C:\go-router`) |
|---|---|
| Update to the latest release | `git pull --ff-only` then `powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\scripts\update-windows.ps1` |
| Change the domain | `powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\scripts\set-domain-windows.ps1 -Domain new.example.com` |
| Change the admin password | add `-NewPassword` to the command above, with the current domain |
| Re-apply permissions and least-privilege accounts | `powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\scripts\secure-windows.ps1` |
| Status | `Get-ScheduledTask GoRouter; Get-Service GoRouterCaddy` |
| Logs | `Get-Content .\data\server.log -Tail 50` and `Get-Content .\data\caddy\process.log -Tail 50` |

More in [Operations](operations.md): backups, monitoring, limits and troubleshooting.

## Harden the server

The gateway is only one part of the server's security. Also:

- Use a strong Administrator password and remove accounts you don't recognize.
- Allow Remote Desktop (TCP 3389) only from your own IP addresses in the hosting
  provider's firewall.
- Keep Windows Update enabled.
