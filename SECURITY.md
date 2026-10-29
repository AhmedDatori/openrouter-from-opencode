# Security

Report vulnerabilities privately to the repository owner. Include the version,
configuration (without secrets), reproduction steps, and expected behavior.

- Serve public deployments over HTTPS. Set `PUBLIC_URL=https://your-domain` and
  `COOKIE_SECURE=true`. The default HTTP bind is loopback for local use.
- Client API keys and admin session tokens are high-entropy random secrets.
  Only SHA-256 hashes are stored. Admin passwords are verified with Argon2id.
- The dashboard uses HttpOnly, SameSite=Strict cookies, a per-session CSRF token,
  origin validation, a restrictive CSP, and no external JavaScript or fonts.
- The upstream Go key stays server-side. Never give it to downstream clients.
  Rotate it in OpenCode if leaked, then update `.env` and restart this service.
- API keys can be revoked, scoped to models, given an expiry, and rate limited.
  All keys share your actual Go subscription and its limits; they are not
  independent subscriptions. Revocation stops new calls, not active streams.
- Request/response logging can contain business or personal information.
  It defaults to enabled, with media data removed, a 64 KiB capture cap, and
  30-day detailed retention. Set `LOG_BODIES=false` if you only need metadata.
  Lifetime accounting aggregates remain; retention does not erase billing totals.
- Keep `.env`, `data/`, SQLite files, and backups private. They are ignored by Git
  and excluded from Docker build contexts. Database backups contain key hashes,
  admin sessions, and captured payloads. Do not expose the data folder via a web server.
- OpenCode workspace privacy settings and regional restrictions still apply.
  The gateway does not bypass them, free-tier limits, or upstream authentication.
- Do not let untrusted users edit `GO_BASE_URL` or `config/models.json`.
