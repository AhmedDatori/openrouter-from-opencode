# Set up a new Linux server

This guide installs Go Router on a fresh Ubuntu or Debian server and publishes it
at `https://<your-domain>` with a free, automatically renewed HTTPS certificate.
It uses the prebuilt release binary, so nothing is compiled on the server.

A Docker alternative is at the [end of this page](#alternative-docker-compose).

## What you need

| Item | Details |
|---|---|
| Server | Ubuntu 22.04+ or Debian 12+, x86_64 or ARM64. 1 vCPU / 1 GB RAM is enough (the gateway uses about 30 MiB). |
| Access | SSH with a user that can run `sudo`. |
| Domain | A hostname such as `api.example.com`, with a DNS **A record** pointing to the server's public IPv4 address. Turn off any DNS proxy option for the first setup. |
| Ports | Inbound **TCP 80 and 443** allowed by the hosting provider's firewall. Do **not** open 8080. |
| OpenCode Go | An active Go subscription and API key. You paste it in the dashboard at the end. |

Commands below use two shell variables. Set them once in your SSH session
(use `aarch64` on ARM servers):

```bash
DOMAIN=api.example.com
ARCH=x86_64
getent hosts "$DOMAIN"          # must print the server's public IP
```

## 1. Install Caddy and helpers

```bash
sudo apt-get update
sudo apt-get install -y caddy python3 curl
```

Caddy is the HTTPS front door. It starts automatically and renews certificates itself.

## 2. Download the gateway

```bash
cd /tmp
curl -fLO https://github.com/AhmedDatori/openrouter-from-opencode/releases/latest/download/go-router-linux-$ARCH.tar.gz
curl -fLO https://github.com/AhmedDatori/openrouter-from-opencode/releases/latest/download/go-router-linux-$ARCH.tar.gz.sha256
sha256sum -c go-router-linux-$ARCH.tar.gz.sha256      # must print: OK
sudo tar xzf go-router-linux-$ARCH.tar.gz -C /opt     # creates /opt/go-router
```

## 3. Create the configuration and admin password

```bash
cd /opt/go-router
sudo python3 scripts/setup.py --origin "https://$DOMAIN"
sudo ./go-router --check
```

`setup.py` creates `.env` and `data/admin-login.txt` (both readable only by their
owner) with a random admin password. `--check` must print `Configuration valid`.

## 4. Run the gateway as a service

```bash
sudo useradd --system --home /opt/go-router --shell /usr/sbin/nologin go-router
sudo chown -R root:root /opt/go-router
sudo chown -R go-router:go-router /opt/go-router/data /opt/go-router/.env
sudo cp deploy/go-router.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now go-router
curl http://127.0.0.1:8080/readyz                     # must print {"status":"ready"}
```

The service runs as an unprivileged user, can write only to `/opt/go-router/data`,
restarts on failure and starts at boot. The program files stay owned by root.

## 5. Publish it over HTTPS

```bash
printf '%s {\n    reverse_proxy 127.0.0.1:8080 {\n        flush_interval -1\n    }\n}\n' "$DOMAIN" \
  | sudo tee /etc/caddy/Caddyfile
sudo systemctl reload caddy
```

`flush_interval -1` sends streamed answers to clients immediately. If the server
uses `ufw`, allow the web ports (and keep SSH open):

```bash
sudo ufw allow OpenSSH
sudo ufw allow 80,443/tcp
```

## 6. Sign in and connect OpenCode Go

```bash
sudo cat /opt/go-router/data/admin-login.txt
```

1. Open `https://api.example.com/admin` and sign in with that password.
2. Open **Setup & settings**, paste your OpenCode Go API key and click **Save Go key**.
3. Open **API keys** and create one key per application or developer.

## 7. Test from another computer

```bash
curl https://api.example.com/readyz
curl https://api.example.com/api/v1/chat/completions \
  -H "Authorization: Bearer sk-gr-YOUR-APP-KEY" \
  -H "Content-Type: application/json" \
  -d '{"model":"deepseek/deepseek-v4.1-flash","messages":[{"role":"user","content":"Hello"}]}'
```

Give developers the base URL `https://api.example.com/api/v1`, their own key, and a
model ID from `/api/v1/models`. Never share the admin password or the Go key.

## Update to a new release

```bash
ARCH=x86_64        # or aarch64
cd /tmp && rm -rf go-router
curl -fLO https://github.com/AhmedDatori/openrouter-from-opencode/releases/latest/download/go-router-linux-$ARCH.tar.gz
curl -fLO https://github.com/AhmedDatori/openrouter-from-opencode/releases/latest/download/go-router-linux-$ARCH.tar.gz.sha256
sha256sum -c go-router-linux-$ARCH.tar.gz.sha256 && tar xzf go-router-linux-$ARCH.tar.gz
sudo install -m 755 go-router/go-router /opt/go-router/go-router
sudo cp go-router/config/models.json /opt/go-router/config/models.json   # skip if you edited the catalog
sudo systemctl restart go-router
```

`.env`, the database and the admin password are kept. See [Operations](operations.md)
for backups, monitoring and troubleshooting.

## Alternative: Docker Compose

Use this if you prefer containers. Docker **builds the gateway from source**, which
needs about 2 GB of RAM during the build. The included Caddy container handles HTTPS.

```bash
sudo apt-get update
sudo apt-get install -y docker.io docker-compose-v2 git python3
DOMAIN=api.example.com
sudo git clone https://github.com/AhmedDatori/openrouter-from-opencode.git /opt/go-router
cd /opt/go-router
sudo python3 scripts/setup.py --origin "https://$DOMAIN"   # also sets DOMAIN in .env for Caddy
sudo docker compose --profile https up -d --build
sudo cat data/admin-login.txt
```

Then continue with steps 6 and 7 above. Port 80 and 443 must be free on the host.
To update: `sudo git pull --ff-only && sudo docker compose --profile https up -d --build`.
The database lives in the `gateway-data` Docker volume, not in `data/`.
