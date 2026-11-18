# Set up in IIS Manager

Use this guide on a Windows Server that already runs websites in IIS. Go Router becomes
one more IIS website: IIS starts it, restarts it if it stops, and serves it over HTTPS
with a certificate that is already installed. Everything is done in File Explorer, a
browser and IIS Manager; no PowerShell is needed.

The examples use `ai.example.com`; replace it with your domain.

## What you need

| Item | Details |
|---|---|
| Domain | A DNS **A record** for `ai.example.com` pointing to this server's IP. |
| Certificate | A certificate in IIS that covers the domain, for example a `*.example.com` wildcard. It appears in **IIS Manager > server > Server Certificates**. |
| ASP.NET Core Module | Part of the free **ASP.NET Core Hosting Bundle**. See step 1. |
| OpenCode Go | An active Go subscription and API key. |

## 1. Check the ASP.NET Core Module

Open **IIS Manager**, click the **server name** in the left tree, and open **Modules**.

- **`AspNetCoreModuleV2` is in the list:** continue with step 2.
- **It is missing:** download the latest **ASP.NET Core Hosting Bundle** from
  <https://dotnet.microsoft.com/download/dotnet> (choose the newest version, then
  *ASP.NET Core Runtime > Windows > Hosting Bundle*), install it, then click the server
  name in IIS Manager and click **Restart** on the right.

> **Why:** this Microsoft module lets IIS start a program and forward web requests to it.
> Servers that already host .NET websites usually have it.

## 2. Download Go Router

1. In a browser on the server, open
   <https://github.com/AhmedDatori/openrouter-from-opencode/releases/latest> and download
   **`go-router-windows-x86_64.zip`**.
2. Right-click the zip > **Properties** > tick **Unblock** (if shown) > **OK**.
3. Right-click the zip > **Extract All...** > extract to **`C:\`**. This creates
   **`C:\go-router`**. If a `C:\go-router` folder already exists from an earlier attempt
   and holds no `.env` or `data` you need, delete it first.

> **Why:** the release contains the ready program (`go-router.exe`), the model list, and
> the IIS configuration file `iis\web.config`. Nothing needs to be built.

## 3. Create the settings file

1. Open `C:\go-router`. Copy **`.env.example`** and rename the copy to **`.env`**
   (File Explorer may show it as `.env.example - Copy`; rename it to exactly `.env`).
2. Right-click `.env` > **Open with** > **Notepad**, and change these three lines:

   ```text
   ADMIN_PASSWORD=choose-a-long-password-of-at-least-16-characters
   PUBLIC_URL=https://ai.example.com
   COOKIE_SECURE=true
   ```

3. **File > Save**. Keep the name `.env` (not `.env.txt`).

> **Why:** `ADMIN_PASSWORD` protects the dashboard, and `PUBLIC_URL` must be the exact
> address people open, or the dashboard rejects sign-in as a cross-site request.

## 4. Lock down the folder

1. Right-click **`C:\go-router`** > **Properties** > **Security** > **Advanced**.
2. Click **Disable inheritance** > **Convert inherited permissions into explicit permissions**.
3. Remove every entry except **SYSTEM** and **Administrators** (both *Full control*).
4. Click **Add** > **Select a principal** > type **`LOCAL SERVICE`** > **Check Names** >
   **OK**. Choose **Read & execute** and **OK**.
5. Click **OK** to close the windows.
6. Create a folder **`C:\go-router\data`**. Right-click it > **Properties** > **Security** >
   **Edit** > **Add** > **`LOCAL SERVICE`** > tick **Modify** > **OK**.

> **Why:** the gateway runs as the low-privilege **LOCAL SERVICE** account. It can read the
> program and settings and write only its database in `data`. Only administrators can
> change the program, so no other account on the server can replace it.

## 5. Create the application pool

1. In IIS Manager, click **Application Pools** > **Add Application Pool...**
   - Name: **`GoRouter`**
   - .NET CLR version: **No Managed Code**
   - Managed pipeline mode: **Integrated**
   - Click **OK**.
2. Select **GoRouter** > **Advanced Settings...** (right side) and set:

   | Setting | Value |
   |---|---|
   | General > **Start Mode** | `AlwaysRunning` |
   | Process Model > **Identity** | Built-in account: `LocalService` |
   | Process Model > **Idle Time-out (minutes)** | `0` |
   | Recycling > **Regular Time Interval (minutes)** | `0` |

   Click **OK**.

> **Why:** Go Router is not a .NET program. A dedicated pool with the LocalService account
> keeps it separate from your other sites. Idle shutdown and timed recycling would cut
> answers that are still streaming, so both are turned off.

## 6. Create the website

In IIS Manager, right-click **Sites** > **Add Website...**

| Field | Value |
|---|---|
| Site name | `GoRouter` |
| Application pool | **Select...** > `GoRouter` |
| Physical path | `C:\go-router\iis` |
| Binding type | `https` |
| IP address | `All Unassigned` |
| Port | `443` |
| Host name | `ai.example.com` |
| Require Server Name Indication | ticked |
| SSL certificate | your certificate for the domain (for example `*.example.com`) |
| Start Website immediately | ticked |

Click **OK**.

> **Why:** the host name and Server Name Indication make IIS send only `ai.example.com` to
> this site, so your other websites on port 443 are unaffected. The site folder contains
> only `web.config`, so IIS can never serve `.env` or the database.

## 7. Test it

Open **`https://ai.example.com/healthz`** in a browser. It must show
`{"status":"ok","version":"..."}`. The first request after a start can take a second.

Then open **`https://ai.example.com/admin`**, sign in with the `ADMIN_PASSWORD` you chose,
and:

1. **Setup & settings** > paste your OpenCode Go API key > **Save Go key**.
2. **API keys** > create one key per application or developer (with spending caps if you
   want). The full key is shown only once.

Developers use the base URL `https://ai.example.com/api/v1`, their own key, and a model
ID such as `deepseek/deepseek-v4.1-flash`.

## Managing it in IIS Manager

| Task | Where |
|---|---|
| Stop or start | **Sites > GoRouter > Stop / Start**, or the **GoRouter** application pool |
| Restart after editing `.env` | **Application Pools > GoRouter > Recycle** |
| Gateway logs | `C:\go-router\data\logs\` (newest `stdout_*.log`) |
| Request logs, usage, keys | the dashboard at `/admin` |
| Renewed certificate | **Sites > GoRouter > Bindings... > https > Edit** > pick the new certificate |
| Startup errors | Event Viewer > Windows Logs > Application, source *IIS AspNetCore Module V2* |

## Updating to a new release

1. Download the new **`go-router-windows-x86_64.zip`** (step 2), unblock it, and extract it
   to a temporary folder such as `C:\go-router-new`.
2. In IIS Manager, **Application Pools > GoRouter > Stop**.
3. Copy **`go-router.exe`** and the **`config`** folder from the new release over the ones
   in `C:\go-router` (replace files). Do not touch `.env` or `data`.
4. **Application Pools > GoRouter > Start**, then open `/healthz` to see the new version.

Your keys, settings, password and request history are kept.

## If something goes wrong

| What you see | Cause and fix |
|---|---|
| **HTTP 500.19** | The ASP.NET Core Module is missing (step 1). |
| **HTTP 502.5** or **500.30** | `go-router.exe` could not start. Read the newest file in `C:\go-router\data\logs\`. Usually `.env` is missing, misnamed (`.env.txt`), or `ADMIN_PASSWORD` is shorter than 16 characters. |
| **Sign-in says "origin"** | `PUBLIC_URL` in `.env` does not exactly match the address in the browser. Fix it, then recycle the pool. |
| **Certificate warning** | The binding uses the wrong certificate (step 6). |
| The default IIS page or another site appears | The host name in the binding is wrong, or DNS points elsewhere. |
