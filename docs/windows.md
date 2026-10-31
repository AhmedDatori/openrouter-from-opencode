# Windows Server: first deployment from an Arch/Linux client

The `go-router-windows-x86_64.zip` package contains a native Windows executable,
the model catalog, setup scripts, and documentation. No Rust, Docker, Python,
Node, OpenCode CLI, GPU, or separate database is required on the server.
Use Windows Server 2019 or later, x64. Building from Linux uses the GNU target;
the resulting executable imports only Windows system DLLs.

The first deployment below uses an SSH tunnel. The gateway listens only on
`127.0.0.1:8080` on Windows, and the tunnel exposes it on `127.0.0.1:8080` on your
own computer. All tunnel traffic is encrypted. This is sufficient to test your
own backends without a public domain or inbound port 8080. The tunnel must stay
open while your local applications use the API.

## 1. Enable SSH on Windows

Inside RDP, open **Windows PowerShell as Administrator**:

```powershell
Add-WindowsCapability -Online -Name OpenSSH.Server~~~~0.0.1.0
Start-Service sshd
Set-Service -Name sshd -StartupType Automatic
```

Microsoft's installation normally creates the `OpenSSH-Server-In-TCP` firewall
rule. Verify that the provider's external firewall also permits TCP port 22.
For a first deployment, restrict the Windows rule to your own public IP:

```powershell
Set-NetFirewallRule -Name OpenSSH-Server-In-TCP -RemoteAddress YOUR_ARCH_PUBLIC_IP
```

Your Arch machine's public IP can be displayed with `curl -4 https://api.ipify.org`.
Update this rule if your IP changes. Do not disable the firewall or change the
existing RDP rule. If installation reports a restart is required, arrange that
with the server owner first.

Reference: [Microsoft OpenSSH setup](https://learn.microsoft.com/en-us/windows-server/administration/openssh/openssh_install_firstuse).

## 2. Transfer and extract the package

Download the ZIP from the private GitHub release to your local computer, then:

```bash
scp go-router-windows-x86_64.zip administrator@SERVER_IP:go-router-windows-x86_64.zip
```

Use the Windows administrator password when prompted. Verify the SSH host-key
fingerprint on first use against the server's own fingerprint:

```powershell
& "$env:WINDIR\System32\OpenSSH\ssh-keygen.exe" -lf C:\ProgramData\ssh\ssh_host_ed25519_key.pub
```

Back in the Windows PowerShell window:

```powershell
Expand-Archive "$env:USERPROFILE\go-router-windows-x86_64.zip" -DestinationPath C:\
Set-Location C:\go-router
```

The executable must now be `C:\go-router\go-router.exe`. You can compare the ZIP's
`Get-FileHash -Algorithm SHA256` result with the published `.sha256` asset.

## 3. Configure the gateway and start automatically

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\scripts\setup-windows.ps1
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\scripts\install-windows-task.ps1
Invoke-RestMethod http://127.0.0.1:8080/readyz
notepad .\data\admin-login.txt
```

Setup asks privately for the **OpenCode Go API key**, generates an admin password,
and protects the installation directory for the current account, Administrators,
and SYSTEM. It never overwrites an existing `.env`. The execution-policy override
applies only to that PowerShell invocation; no machine policy is changed.

The `GoRouter` scheduled task runs at startup as SYSTEM, survives RDP disconnection,
and restarts on failure up to three times. It is a background startup task, not
a Windows Service Control Manager service. Output is appended to `data\server.log`.
Task registration requires Administrator. The task's startup check uses port 8080;
if you customize the bind address, verify readiness at that configured port.

`data\admin-login.txt` contains your private dashboard password. Application API
keys are generated separately inside the dashboard. Do not put your Go upstream
key into an application or commit `.env` or `data` to GitHub.

## 4. Connect from Arch/Linux

```bash
ssh -N -o ExitOnForwardFailure=yes -o ServerAliveInterval=30 \
  -L 127.0.0.1:8080:127.0.0.1:8080 administrator@SERVER_IP
```

Keep that terminal open. In your local browser, open
`http://127.0.0.1:8080/admin` and enter the generated admin password. Create an
application key, then configure your backend with:

```text
Base URL: http://127.0.0.1:8080/api/v1
API key: the application key created in the dashboard
```

The port must be free on your local machine. If you choose another local port,
update `PUBLIC_URL` in the Windows `.env` to that exact browser origin, then
restart the task. `127.0.0.1` inside a Docker container or on another computer is
that environment's own loopback, not your Arch host; arrange forwarding there too.

## Operations and next deployment stage

```powershell
Get-ScheduledTask -TaskName GoRouter
Get-Content C:\go-router\data\server.log -Tail 50
Stop-ScheduledTask -TaskName GoRouter
Start-ScheduledTask -TaskName GoRouter
```

Stopping a scheduled task is a forced stop; active requests can be interrupted.
For upgrades/backups, stop it first, copy the entire `data` directory plus `.env`
and the model catalog, replace the executable, then start the task again. Do not
copy only a live SQLite `.db` file: WAL files are part of the database state.
`server.log` is an append-only process log; archive it periodically while stopped.
The structured request-log retention setting does not rotate this process log.

For a permanent public URL, put an HTTPS reverse proxy in front and set the exact
`PUBLIC_URL` and `COOKIE_SECURE=true`. Keep the gateway on loopback. See
`deployment.md` for proxy configuration. Sharing client keys does not multiply
Go subscription allowances; OpenCode's upstream intended-use guidance still applies.

## Build from source

Install Rust 1.88+ and a suitable Windows C toolchain (MSVC build tools and Windows
SDK for the default MSVC target), then run `cargo build --locked --release`.
Copy `target\release\go-router.exe` beside `.env.example`, `config`, `scripts`, and
the documentation. A Windows release workflow is supplied; hosted CI may be
unavailable when the GitHub account has a billing restriction.
