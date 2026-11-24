GO ROUTER - INSTALL IN IIS (Windows x64)
======================================

Ready-to-run program. No Rust, Node, Python, CLI, or build tools needed.
Use File Explorer, Notepad, a browser, and IIS Manager. No shell commands.
Configured address: @ORIGIN@

ONE-TIME SERVER REQUIREMENT
In IIS Manager > server > Modules, check for AspNetCoreModuleV2.
If missing, install Microsoft's ASP.NET Core Hosting Bundle:
https://dotnet.microsoft.com/permalink/dotnetcore-current-windows-runtime-bundle-installer
Install it AFTER IIS. Restart the server if requested; on a shared server,
arrange the IIS/service restart with its administrator. A .NET Framework
installation alone does not provide this module.

1. UNZIP
Right-click the ZIP > Properties > Unblock (if shown) > OK, then Extract All.
Put the included go-router folder at C:\go-router (or any local folder).
Keep its files and subfolders together. Use a new folder for a new install.

2. SETTINGS (ALREADY INCLUDED)
Open C:\go-router\.env in Notepad. Save the value after ADMIN_PASSWORD=
in your password manager: this is the generated dashboard password.
If using a different domain, change PUBLIC_URL to its exact HTTPS address,
for example https://api.yourcompany.com (no /admin or /api/v1 suffix).
Keep COOKIE_SECURE=true. No API key is needed to start the website.

3. APPLICATION POOL
IIS Manager > Application Pools > Add Application Pool:
  Name: GoRouter
  .NET CLR version: No Managed Code
  Pipeline: Integrated
Select GoRouter > Advanced Settings:
  Enable 32-Bit Applications: False
  Start Mode: AlwaysRunning
  Identity: ApplicationPoolIdentity (the default)
  Idle Time-out (minutes): 0
  Regular Time Interval (minutes): 0
  Maximum Worker Processes: 1 (the default)

4. FOLDER ACCESS
In File Explorer, right-click C:\go-router > Properties > Security > Edit > Add.
Select the LOCAL SERVER in Locations, type IIS AppPool\GoRouter > Check Names.
Grant Read & execute. On C:\go-router\data, grant the SAME account Modify.
Apply permissions to the folder, subfolders and files.
Do not give the pool Modify access to the executable or .env.

5. ADD THE WEBSITE
IIS Manager > Sites > Add Website:
  Site name: GoRouter
  Application pool: GoRouter
  Physical path: C:\go-router\iis       <-- select the iis subfolder
  Binding: https / port 443
  Host name: @HOST@
  Require Server Name Indication: checked
  SSL certificate: a valid certificate for that host
DNS for the host must point to this server, and HTTPS/443 must be reachable.

Select the site > Authentication > Anonymous Authentication > Enable.
Then Edit > Application pool identity > OK. Disable Windows/Basic
Authentication for this site if enabled. Do not change other sites.
If Authentication is not visible, open IIS Manager as an administrator.

6. OPEN AND FINISH
Open @ORIGIN@/healthz - it should show "status":"ok".
Open @ORIGIN@/admin and sign in with ADMIN_PASSWORD from .env.
In Setup & settings, paste your active OpenCode Go API key and Save Go key.
In API keys, create a key for the application that will use the gateway.
API base URL: @ORIGIN@/api/v1
Default model: deepseek/deepseek-v4.1-flash

The website starts immediately; model answers need your active Go API key.
Your provider's subscription and model availability apply.
Before sharing to end users, verify one real model response with your key.

MANAGE / UPDATE
Start/stop/recycle using the GoRouter application pool in IIS Manager.
After changing .env, recycle the pool. Active streams can be interrupted.
Logs: C:\go-router\data\logs\stdout_*.log
Back up .env and data securely. This ZIP contains a password; share privately.
To update: stop the pool, back up .env and data, replace ONLY go-router.exe,
config\models.json and iis\web.config, then start the pool.
Keep your existing .env and data; do not extract a new ZIP over an active install.
For another server, choose a different ADMIN_PASSWORD (16+ characters).

IF IT DOES NOT START
500.19: read the error detail. Usually missing AspNetCoreModuleV2, a locked
IIS configuration section, or insufficient access to the site folder.
502.5: inspect data\logs and Event Viewer > Windows Logs > Application
(IIS AspNetCore Module V2). Check folder permissions and .env filename.
401.3: set Anonymous Authentication to Application pool identity (step 5).
Sign-in "origin" error: PUBLIC_URL must exactly match the browser address.
Different site opens: check hostname, certificate, SNI and DNS (step 5).
API 503 "not configured": save your OpenCode Go key in the dashboard.

Use a dedicated IIS website at the domain root, not a virtual subfolder.
The separate iis folder keeps .env, the program and database outside the web root.
No URL Rewrite, ARR, separate Windows service, or scheduled task is needed.
