"use strict";
const $ = (s) => document.querySelector(s);
const esc = (s) =>
  String(s ?? "").replace(
    /[&<>"']/g,
    (c) =>
      ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[
        c
      ],
  );
const icons = {
  grid: "M3 3h7v7H3zM14 3h7v7h-7zM3 14h7v7H3zM14 14h7v7h-7z",
  key: "M15 7a5 5 0 1 1-4.6 7L3 21H1v-3l7-7A5 5 0 0 1 15 7Z",
  logs: "M8 3H4v18h16V3h-4M8 1h8v5H8zM8 11h8M8 15h6",
  models: "m12 2 10 5-10 5L2 7l10-5Zm-10 10 10 5 10-5M2 17l10 5 10-5",
  settings: "M4 7h16M4 17h16M8 4v6M16 14v6",
  refresh:
    "M20 7v5h-5M4 17v-5h5M5 8a8 8 0 0 1 13-3l2 2M19 16a8 8 0 0 1-13 3l-2-2",
  arrow: "M5 12h14M14 7l5 5-5 5",
  plus: "M12 5v14M5 12h14",
  copy: "M9 9h12v12H9zM15 5V1H1v14h4",
  logout: "M9 3H3v18h6M13 7l5 5-5 5M7 12h11",
  clock: "M12 3a9 9 0 1 1 0 18 9 9 0 0 1 0-18ZM12 7v5l3 2",
  tokens: "m12 2 9 5v10l-9 5-9-5V7l9-5Zm0 10v10M3 7l9 5 9-5",
  money: "M12 2v20M17 5H9a4 4 0 0 0 0 8h6a4 4 0 0 1 0 8H5",
  close: "M6 6l12 12M18 6 6 18",
  download: "M12 3v12M7 10l5 5 5-5M3 17v4h18v-4",
  check: "m5 12 4 4L19 6",
  search: "M10 3a7 7 0 1 1 0 14 7 7 0 0 1 0-14Zm5 12 6 6",
};
const icon = (name, size = 16) =>
  `<svg width="${size}" height="${size}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="${icons[name] || icons.grid}"/></svg>`;
const num = (n) => Number(n || 0).toLocaleString();
const money = (n) =>
  n == null
    ? "—"
    : new Intl.NumberFormat("en-US", {
        style: "currency",
        currency: "USD",
        minimumFractionDigits: 2,
        maximumFractionDigits: 9,
      }).format(Number(n));
const ms = (n) =>
  n == null
    ? "—"
    : Number(n) >= 1000
      ? (Number(n) / 1000).toFixed(3) + " s"
      : Number(n) + " ms";
const when = (s) =>
  s
    ? new Date(s).toLocaleString(undefined, {
        month: "short",
        day: "numeric",
        hour: "2-digit",
        minute: "2-digit",
      })
    : "—";
let csrf = "",
  tab = "overview",
  cache = {},
  page = 1,
  logKey = "",
  logModel = "",
  logErrors = false,
  modelSearch = "";
async function api(path, options = {}) {
  const headers = {
    "Content-Type": "application/json",
    ...(options.method && options.method !== "GET"
      ? { "X-CSRF-Token": csrf }
      : {}),
  };
  const r = await fetch("/admin/api" + path, {
    credentials: "same-origin",
    ...options,
    headers: { ...headers, ...options.headers },
  });
  const v = await r.json();
  if (!r.ok) {
    if (r.status === 401 && path !== "/login") {
      loginPage();
      throw new Error("Your admin session expired. Sign in again.");
    }
    throw new Error(v.error?.message || "Request failed");
  }
  return v;
}
function toast(message, error = false) {
  const t = $("#toast");
  t.textContent = message;
  t.className = "show" + (error ? " error" : "");
  clearTimeout(toast.timer);
  toast.timer = setTimeout(() => (t.className = ""), 4500);
}
function closeModal() {
  $("#modal").close();
  $("#modal").innerHTML = "";
}
function modal(content) {
  const el = $("#modal");
  el.innerHTML = `<button class="icon-btn close" data-action="close" aria-label="Close dialog">${icon("close")}</button>${content}`;
  if (!el.open) el.showModal();
}
function brand() {
  return `<div class="brand"><span class="brandmark">${icon("arrow", 21)}</span>Go Router<small>ADMIN</small></div>`;
}
function loginPage(error = "") {
  csrf = "";
  cache = {};
  $("#app").innerHTML =
    `<div class="login"><section class="login-art">${brand()}<h1>One gateway.<br>Your entire workflow.</h1><p>Your models, API keys, and usage.<br>A small gateway with everything in one place.</p><div class="mini-code">POST /api/v1/chat/completions\n\n{\n  "model": "deepseek/deepseek-v4.1-flash",\n  "messages": [{ "role": "user", "content": "Hello" }]\n}</div><footer>Powered by Rust · Connected to OpenCode Go</footer></section><section class="login-box"><form id="login-form" class="login-form"><h2>Welcome back</h2><p>Sign in to manage your gateway.</p>${error ? `<div class="error-box">${esc(error)}</div>` : ""}<div class="formfield"><label for="password">Admin password</label><input id="password" name="password" type="password" required autocomplete="current-password" placeholder="Enter your admin password"></div><button class="btn primary" type="submit">Sign in ${icon("arrow")}</button><p class="spacer"><small>Your password is set in the server’s environment configuration.</small></p></form></section></div>`;
}
const names = {
  overview: "Overview",
  keys: "API keys",
  logs: "Request logs",
  models: "Models",
  setup: "Setup & settings",
};
const navicons = {
  overview: "grid",
  keys: "key",
  logs: "logs",
  models: "models",
  setup: "settings",
};
function shell() {
  $("#app").innerHTML =
    `<div class="shell"><aside class="sidebar">${brand()}<div class="navlabel">Workspace</div><nav class="nav" aria-label="Admin navigation">${Object.entries(
      names,
    )
      .map(
        ([k, v]) =>
          `<button data-tab="${k}" class="${tab === k ? "active" : ""}">${icon(navicons[k])}${v}</button>`,
      )
      .join(
        "",
      )}</nav><div class="sidebar-bottom"><div class="local-badge"><span class="dot"></span>One process. Your infrastructure.</div><div class="profile"><span class="avatar">AD</span><div><strong>Administrator</strong><small>Gateway owner</small></div><button class="icon-btn" data-action="logout" title="Sign out" aria-label="Sign out">${icon("logout")}</button></div></div></aside><section class="main"><header class="topbar"><span>Workspace <span class="muted">/</span> <strong>${names[tab]}</strong></span><span class="pill"><span class="dot"></span>Go key configured</span><button class="icon-btn mobile-logout" data-action="logout" title="Sign out" aria-label="Sign out">${icon("logout")}</button></header><div class="content" id="content"></div></section></div>`;
}
function heading(title, sub, action = "") {
  return `<div class="heading"><div><h1>${title}</h1><p>${sub}</p></div><div class="actions">${action}<button class="btn" data-action="refresh">${icon("refresh")}Refresh</button></div></div>`;
}
function table(rows, full = false) {
  if (!rows.length)
    return '<div class="empty">No requests yet. Your first API call will appear here.</div>';
  return `<div class="tablewrap"><table><thead><tr><th>Model / request</th><th>Status</th><th>Tokens</th><th>Cash / allowance</th><th>Time</th>${full ? "<th>API key</th>" : ""}<th>Created</th></tr></thead><tbody>${rows
    .map((r) => {
      const u = r.usage || {},
        tokens =
          (u.prompt_tokens ?? u.input_tokens ?? 0) +
          (u.completion_tokens ?? u.output_tokens ?? 0),
        ok = r.status < 400 && r.outcome === "completed";
      return `<tr class="clickable" data-log="${esc(r.id)}"><td><strong>${esc(r.model || r.path)}</strong><br><small class="mono">${esc(r.id.slice(0, 17))}…</small></td><td><span class="status ${ok ? "" : "error"}">${ok ? "Completed" : esc(r.outcome === "client_disconnected" ? "Disconnected" : r.status >= 400 ? r.status : r.outcome)}</span></td><td>${r.usage ? num(tokens) : "—"}</td><td>${money(u.cost)}<br><small>${money(u.opencode?.allowance)} allowance</small></td><td><span title="${r.duration_ms} milliseconds">${ms(r.duration_ms)}</span></td>${full ? `<td>${esc(cache.keys?.find((k) => k.id === r.key_id)?.name || "—")}</td>` : ""}<td>${when(r.created_at)}</td></tr>`;
    })
    .join("")}</tbody></table></div>`;
}
function chart(daily) {
  const data = [];
  for (let i = 6; i >= 0; i--) {
    const date = new Date();
    date.setUTCDate(date.getUTCDate() - i);
    const iso = date.toISOString().slice(0, 10);
    data.push({
      date: iso,
      requests: daily.find((d) => d.date === iso)?.requests || 0,
    });
  }
  const max = Math.max(1, ...data.map((d) => d.requests));
  const bars = data
    .map((d, i) => {
      const height = (d.requests / max) * 140;
      return `<g><rect x="${i * 80 + 14}" y="${155 - height}" width="46" height="${Math.max(height, 2)}" rx="5" fill="${i === 6 ? "#215a3d" : "#d8e9df"}"><title>${d.date}: ${d.requests} requests</title></rect><text x="${i * 80 + 37}" y="${Math.max(10, 148 - height)}" text-anchor="middle" fill="#8c9f94" font-size="10">${d.requests || ""}</text></g>`;
    })
    .join("");
  return `<div class="chart"><svg viewBox="0 0 560 164" preserveAspectRatio="none" aria-label="Inference requests per day over seven days" role="img">${bars}</svg>${data.every((d) => !d.requests) ? '<div class="chart-empty">Waiting for your first request</div>' : ""}</div><div class="chart-labels">${data.map((d) => `<span>${new Date(d.date + "T12:00:00Z").toLocaleDateString(undefined, { weekday: "short" })}</span>`).join("")}</div>`;
}
async function overview() {
  const [stats, logs] = await Promise.all([
    api("/stats"),
    api("/logs?limit=6"),
  ]);
  cache.stats = stats;
  const t = stats.totals;
  const tokens = t.prompt_tokens + t.completion_tokens;
  $("#content").innerHTML =
    heading(
      "Gateway overview",
      "A clear view of your requests, usage, and costs.",
    ) +
    `<div class="stats"><div class="stat featured"><div class="stat-top">Total requests ${icon("arrow")}</div><div class="stat-value">${num(t.requests)}</div><div class="stat-foot">${num(t.errors)} failed or interrupted</div></div><div class="stat"><div class="stat-top">Actual cash charged ${icon("money")}</div><div class="stat-value">${money(t.cash_usd)}</div><div class="stat-foot">Reported by OpenCode Go</div></div><div class="stat"><div class="stat-top">Tokens processed ${icon("tokens")}</div><div class="stat-value">${num(tokens)}</div><div class="stat-foot">${num(t.prompt_tokens)} input · ${num(t.completion_tokens)} output</div></div><div class="stat"><div class="stat-top">Average completion ${icon("clock")}</div><div class="stat-value">${t.requests ? ms(t.avg_duration_ms) : "—"}</div><div class="stat-foot">Full request duration · includes streaming</div></div></div><div class="grid2"><section class="panel"><div class="panelhead"><h2>Request activity</h2><small>Last 7 days · UTC</small></div>${chart(stats.daily)}</section><section class="panel"><div class="panelhead"><h2>Understand your costs</h2><small>Lifetime totals</small></div><div class="usage-list"><div class="usage-item"><div>Cash charged<small>Extra money charged per request</small></div><strong>${money(t.cash_usd)}</strong></div><div class="usage-item"><div>Go allowance consumed<small>Usage deducted from your plan</small></div><strong>${money(t.allowance_usd)}</strong></div><div class="usage-item"><div>API list value<small>Model price before plan coverage</small></div><strong>${money(t.list_usd)}</strong></div></div><div class="usage-total"><span>Fixed monthly subscription</span><strong>${money(cache.settings.monthly_subscription_usd)}</strong></div><div class="info-note">Included Go requests can charge $0 while consuming allowance. ${num(t.usage_missing)} request(s) have no usage receipt.</div></section></div><section class="panel"><div class="panelhead"><h2>Recent requests</h2><button class="btn small" data-tab="logs">View all ${icon("arrow", 13)}</button></div>${table(logs.data)}</section><p class="footnote">Cost and usage totals are retained independently of the ${cache.settings.retention_days}-day detailed log window. Output tokens include reasoning when reported by the upstream.</p>`;
}
async function keysPage() {
  cache.keys = (await api("/keys")).data;
  $("#content").innerHTML =
    heading(
      "API keys",
      "Create separate keys for your applications and control their access.",
      `<button class="btn primary" data-action="create-key">${icon("plus")}Create key</button>`,
    ) +
    `<section class="panel"><div class="panelhead"><h2>Your application keys</h2><small>${cache.keys.length} keys · secrets shown once</small></div>${cache.keys.length ? `<div class="tablewrap"><table><thead><tr><th>Name</th><th>Status</th><th>Rate limit</th><th>Models</th><th>Created</th><th>Actions</th></tr></thead><tbody>${cache.keys.map((k) => `<tr><td><div class="keyname"><span class="key-icon">${icon("key", 14)}</span><div><strong>${esc(k.name)}</strong><small class="mono">${esc(k.prefix)}…</small></div></div></td><td><span class="status ${k.disabled ? "off" : ""}">${k.disabled ? "Revoked" : k.expires_at && k.expires_at * 1000 < Date.now() ? "Expired" : "Active"}</span></td><td>${k.rpm ? k.rpm + " / min" : "Unlimited"}</td><td>${k.allowed_models.length ? k.allowed_models.length + " selected" : "All Go models"}</td><td>${when(k.created_at)}</td><td><button class="btn small" data-edit-key="${esc(k.id)}">Edit</button> <button class="btn small danger" data-revoke="${esc(k.id)}" ${k.disabled ? "disabled" : ""}>Revoke</button></td></tr>`).join("")}</tbody></table></div>` : '<div class="empty">Create your first API key to connect an application.</div>'}</section><p class="footnote">Keys are cryptographically random and stored as hashes. Revocation prevents new requests; requests already running are allowed to finish.</p>`;
}
function keyForm(id) {
  const key = cache.keys?.find((k) => k.id === id),
    models = cache.models || [];
  modal(
    `<h2>${key ? "Edit API key" : "Create an API key"}</h2><form id="key-form" data-id="${esc(id || "")}"><div class="formfield"><label for="key-name">Application name</label><input id="key-name" name="name" required maxlength="120" placeholder="e.g. Invoice backend" value="${esc(key?.name || "")}"></div><div class="detail-grid"><div class="formfield"><label for="key-rpm">Requests per minute</label><input id="key-rpm" name="rpm" type="number" min="0" max="100000" value="${key?.rpm ?? 60}"><small>0 means unlimited at the gateway. Go limits still apply.</small></div><div class="formfield"><label for="key-expiry">Expiration (optional)</label><input id="key-expiry" name="expiry" type="datetime-local" value="${key?.expires_at ? new Date(key.expires_at * 1000 - new Date().getTimezoneOffset() * 60000).toISOString().slice(0, 16) : ""}"></div></div><div class="formfield"><label for="key-models">Allowed models (optional)</label><select id="key-models" name="models" multiple>${models.map((m) => `<option value="${esc(m.id)}" ${key?.allowed_models.includes(m.id) ? "selected" : ""}>${esc(m.name)}</option>`).join("")}</select><small>No selection allows all Go models. Hold Ctrl / Cmd to select multiple.</small></div><div class="modal-actions"><button class="btn" type="button" data-action="close">Cancel</button><button class="btn primary" type="submit">${key ? "Save changes" : "Create key"}</button></div></form>`,
  );
}
async function logsPage() {
  const q = new URLSearchParams({ page: String(page), limit: "25" });
  if (logKey) q.set("key_id", logKey);
  if (logModel) q.set("model", logModel);
  if (logErrors) q.set("errors", "true");
  const logs = await api("/logs?" + q);
  cache.logPage = logs;
  $("#content").innerHTML =
    heading(
      "Request logs",
      "Inspect payloads, tokens, errors, and timing for every API request.",
      `<button class="btn" data-action="export">${icon("download")}Export</button>`,
    ) +
    `<section class="panel"><div class="filters"><select id="filter-key" aria-label="Filter by API key"><option value="">All API keys</option>${cache.keys.map((k) => `<option value="${esc(k.id)}" ${logKey === k.id ? "selected" : ""}>${esc(k.name)}</option>`).join("")}</select><select id="filter-model" aria-label="Filter by model"><option value="">All models</option>${cache.models.map((m) => `<option value="${esc(m.id)}" ${logModel === m.id ? "selected" : ""}>${esc(m.name)}</option>`).join("")}</select><label><input id="filter-errors" type="checkbox" ${logErrors ? "checked" : ""}>Errors only</label></div>${table(logs.data, true)}<div class="pagination"><span>${num(logs.total)} requests · page ${page} of ${Math.max(1, Math.ceil(logs.total / 25))}</span><div class="actions"><button class="btn small" data-page="${page - 1}" ${page <= 1 ? "disabled" : ""}>Previous</button><button class="btn small" data-page="${page + 1}" ${page * 25 >= logs.total ? "disabled" : ""}>Next</button></div></div></section><p class="footnote">Stored for ${cache.settings.retention_days} days. ${cache.settings.log_bodies ? "Payload capture is enabled; large payloads are capped and media data is omitted." : "Payload capture is disabled; only request metadata and usage are stored."} No authorization headers are logged.</p>`;
}
async function logDetail(id) {
  const r = await api("/logs/" + encodeURIComponent(id));
  const u = r.usage;
  modal(
    `<h2>Request details</h2><small class="mono">${esc(r.id)}</small><div class="detail-grid spacer"><div class="settings-list"><div><span>Model</span><strong>${esc(r.model || "—")}</strong></div><div><span>Status</span><strong>${r.status} · ${esc(r.outcome)}</strong></div><div><span>Duration</span><strong>${ms(r.duration_ms)} (${r.duration_ms} ms)</strong></div><div><span>First byte / token</span><strong>${ms(r.first_byte_ms)} / ${ms(r.first_token_ms)}</strong></div></div><div class="settings-list"><div><span>Created</span><strong>${when(r.created_at)}</strong></div><div><span>Cash charged</span><strong>${money(u?.cost)}</strong></div><div><span>Go allowance</span><strong>${money(u?.opencode?.allowance)}</strong></div><div><span>List value</span><strong>${money(u?.opencode?.list_cost)}</strong></div></div></div>${r.error ? `<div class="error-box spacer">${esc(r.error)}</div>` : ""}<div class="detail-section"><h3>Usage receipt</h3><pre>${esc(JSON.stringify(u, null, 2) || "No usage receipt was returned.")}</pre></div><div class="detail-section"><h3>Request payload</h3><pre>${esc(JSON.stringify(r.request, null, 2))}</pre></div><div class="detail-section"><h3>Response payload / stream events</h3><pre>${esc(JSON.stringify(r.response, null, 2))}</pre></div><details class="detail-section"><summary>Routing attempts</summary><pre>${esc(JSON.stringify(r.attempts, null, 2))}</pre></details>`,
  );
}
function renderModels() {
  const models = cache.models.filter((m) =>
    (m.id + " " + m.name).toLowerCase().includes(modelSearch.toLowerCase()),
  );
  const grid = $("#model-cards");
  if (!grid) return;
  grid.innerHTML =
    models
      .map(
        (m) =>
          `<article class="model-card"><h3>${esc(m.name)}</h3><span class="mono">${esc(m.id)}</span><div class="tags">${m.input_modalities.map((v) => `<span class="tag">${esc(v)} input</span>`).join("")}${m.trains_on_data ? '<span class="tag privacy">Training data use</span>' : ""}</div><small>List value per 1M tokens</small><div class="model-price"><div>${money(Number(m.pricing.prompt) * 1e6)}<small>Input</small></div><div>${money(Number(m.pricing.completion) * 1e6)}<small>Output</small></div></div><div class="model-bottom"><small>${num(m.context_length)} context</small><button class="btn small" data-copy="${esc(m.id)}">${icon("copy", 12)}Copy ID</button></div></article>`,
      )
      .join("") || '<div class="empty">No matching models.</div>';
}
function modelsPage() {
  $("#content").innerHTML =
    heading(
      "Go model catalog",
      "Only models configured for the OpenCode Go plan.",
      `<input id="model-search" class="search" placeholder="Search models…" aria-label="Search models" value="${esc(modelSearch)}">`,
    ) +
    `<p class="inline-notice">Prices are list values; actual cash charges and allowance deductions come from the upstream receipt. Availability, regions, and training permissions depend on your Go workspace.</p><div class="model-grid" id="model-cards"></div><p class="footnote">Catalog is a reviewed snapshot. Update config/models.json and reload it without restarting the server. No OpenCode CLI is required at runtime.</p>`;
  renderModels();
}
function setupPage() {
  const s = cache.settings,
    url = location.origin + "/api/v1";
  const example = `from openai import OpenAI\n\nclient = OpenAI(\n    base_url="${url}",\n    api_key="YOUR_GATEWAY_KEY",\n)\n\nresult = client.chat.completions.create(\n    model="${s.default_model}",\n    messages=[{"role": "user", "content": "Hello"}],\n)\nprint(result.choices[0].message.content)`;
  $("#content").innerHTML =
    heading(
      "Setup & settings",
      "Connect your backend and inspect the gateway configuration.",
    ) +
    `<div class="setup-grid"><section class="panel"><h2>Connect your application</h2><p>Replace your OpenRouter base URL and API key. Choose a model from this gateway’s Go catalog.</p><div class="endpoint"><code>${esc(url)}</code><button class="icon-btn" data-copy="${esc(url)}" aria-label="Copy API base URL">${icon("copy")}</button></div><h3 class="spacer">Python · OpenAI SDK</h3><pre>${esc(example)}</pre><p class="footnote">Chat Completions, streaming, tool calls, structured outputs, Responses and Anthropic Messages are forwarded to Go. OpenRouter-only hosted services and stateful Responses are not available.</p></section><section class="panel"><h2>Server configuration</h2><div class="settings-list">${[
      ["Version", s.version],
      ["Default model", s.default_model],
      ["Concurrent requests", s.max_concurrent_requests],
      ["Request body limit", (s.max_body_bytes / 1048576).toFixed(0) + " MiB"],
      ["Request timeout", s.request_timeout_seconds + " s"],
      ["Stream idle timeout", s.stream_idle_timeout_seconds + " s"],
      ["Payload logging", s.log_bodies ? "Enabled" : "Disabled"],
      ["Log payload cap", num(s.max_log_bytes) + " bytes"],
      ["Log retention", s.retention_days + " days"],
      ["Secure cookies", s.cookie_secure ? "Enabled" : "Disabled (local HTTP)"],
      ["Monthly subscription", money(s.monthly_subscription_usd)],
      ["Configured models", s.catalog_models],
    ]
      .map(
        ([k, v]) =>
          `<div><span>${esc(k)}</span><strong>${esc(v)}</strong></div>`,
      )
      .join(
        "",
      )}</div><p class="footnote">Change server settings in .env and restart. The upstream credential is never exposed by the admin API.</p><div class="actions"><button class="btn" data-action="reload-models">${icon("refresh")}Reload model catalog</button><button class="btn" data-action="admin-events">Admin activity</button></div></section></div>`;
}
async function navigate(next = tab) {
  tab = next;
  shell();
  $("#content").innerHTML = '<div class="empty">Loading your workspace…</div>';
  try {
    if (!cache.settings || !cache.models || !cache.keys) {
      const [s, m, k] = await Promise.all([
        api("/settings"),
        api("/models"),
        api("/keys"),
      ]);
      cache.settings = s;
      cache.models = m.data;
      cache.keys = k.data;
    }
    if (tab === "overview") await overview();
    else if (tab === "keys") await keysPage();
    else if (tab === "logs") await logsPage();
    else if (tab === "models") modelsPage();
    else setupPage();
  } catch (e) {
    if (csrf)
      $("#content").innerHTML =
        `<div class="app-error">${esc(e.message)} <button class="btn small" data-action="refresh">Retry</button></div>`;
  }
}
async function copy(text) {
  try {
    await navigator.clipboard.writeText(text);
    toast("Copied to clipboard");
  } catch {
    modal(`<h2>Copy this value</h2><pre>${esc(text)}</pre>`);
  }
}
async function action(name, el) {
  switch (name) {
    case "close":
      closeModal();
      break;
    case "refresh":
      cache.settings = null;
      await navigate();
      break;
    case "logout":
      await api("/session", { method: "DELETE" });
      loginPage();
      break;
    case "create-key":
      keyForm();
      break;
    case "admin-events": {
      const r = await api("/events");
      modal(
        `<h2>Admin activity</h2><pre>${esc(JSON.stringify(r.data, null, 2))}</pre>`,
      );
      break;
    }
    case "reload-models": {
      const r = await api("/models/reload", { method: "POST", body: "{}" });
      cache.models = null;
      toast(`Reloaded ${r.models} models`);
      await navigate();
      break;
    }
    case "export": {
      const rows = [];
      for (let p = 1; p <= 25; p++) {
        const q = new URLSearchParams({ page: String(p), limit: "200" });
        if (logKey) q.set("key_id", logKey);
        if (logModel) q.set("model", logModel);
        if (logErrors) q.set("errors", "true");
        const r = await api("/logs?" + q);
        rows.push(...r.data);
        if (rows.length >= r.total) break;
      }
      const blob = new Blob([JSON.stringify(rows, null, 2)], {
          type: "application/json",
        }),
        url = URL.createObjectURL(blob),
        a = document.createElement("a");
      a.href = url;
      a.download = "go-router-logs.json";
      a.click();
      URL.revokeObjectURL(url);
      toast(`Exported ${rows.length} log summaries (maximum 5000 per export)`);
      break;
    }
  }
}
document.addEventListener("click", async (e) => {
  const el = e.target.closest("button,[data-log]");
  if (!el) return;
  try {
    if (el.dataset.tab) {
      await navigate(el.dataset.tab);
      return;
    }
    if (el.dataset.action) {
      await action(el.dataset.action, el);
      return;
    }
    if (el.dataset.copy) {
      await copy(el.dataset.copy);
      return;
    }
    if (el.dataset.log) {
      await logDetail(el.dataset.log);
      return;
    }
    if (el.dataset.editKey) {
      keyForm(el.dataset.editKey);
      return;
    }
    if (el.dataset.revoke) {
      if (
        !confirm("Revoke this API key? Applications using it will lose access.")
      )
        return;
      await api("/keys/" + el.dataset.revoke, { method: "DELETE" });
      toast("API key revoked");
      await keysPage();
      return;
    }
    if (el.dataset.page) {
      page = Number(el.dataset.page);
      await logsPage();
    }
  } catch (error) {
    toast(error.message, true);
  }
});
document.addEventListener("submit", async (e) => {
  e.preventDefault();
  const form = e.target;
  const button = form.querySelector("button[type=submit]");
  if (button) button.disabled = true;
  try {
    if (form.id === "login-form") {
      const data = await api("/login", {
        method: "POST",
        body: JSON.stringify({
          password: form.elements.namedItem("password").value,
        }),
      });
      csrf = data.csrf_token;
      await navigate("overview");
    }
    if (form.id === "key-form") {
      const expiry = form.elements.namedItem("expiry").value;
      const body = {
        name: form.elements.namedItem("name").value,
        rpm: Number(form.elements.namedItem("rpm").value),
        allowed_models: [
          ...form.elements.namedItem("models").selectedOptions,
        ].map((o) => o.value),
        expires_at: expiry
          ? Math.floor(new Date(expiry).getTime() / 1000)
          : null,
      };
      const id = form.dataset.id;
      const r = await api("/keys" + (id ? "/" + id : ""), {
        method: id ? "PATCH" : "POST",
        body: JSON.stringify(body),
      });
      if (id) {
        closeModal();
        toast("API key updated");
      } else {
        modal(
          `<h2>Your API key is ready</h2><p class="keywarning">Copy this key now. It won’t be shown again.</p><div class="secret">${esc(r.key)}</div><div class="modal-actions"><button class="btn" data-action="close">Done</button><button class="btn primary" data-copy="${esc(r.key)}">${icon("copy")}Copy key</button></div>`,
        );
      }
      await keysPage();
    }
  } catch (error) {
    if (form.id === "login-form") loginPage(error.message);
    else toast(error.message, true);
  } finally {
    if (button) button.disabled = false;
  }
});
document.addEventListener("change", async (e) => {
  try {
    if (e.target.id === "filter-key") {
      logKey = e.target.value;
      page = 1;
      await logsPage();
    }
    if (e.target.id === "filter-model") {
      logModel = e.target.value;
      page = 1;
      await logsPage();
    }
    if (e.target.id === "filter-errors") {
      logErrors = e.target.checked;
      page = 1;
      await logsPage();
    }
  } catch (error) {
    toast(error.message, true);
  }
});
document.addEventListener("input", (e) => {
  if (e.target.id === "model-search") {
    modelSearch = e.target.value;
    renderModels();
  }
});
$("#modal").addEventListener("click", (e) => {
  if (e.target === $("#modal")) {
    const r = e.target.getBoundingClientRect();
    if (
      e.clientX < r.left ||
      e.clientX > r.right ||
      e.clientY < r.top ||
      e.clientY > r.bottom
    )
      closeModal();
  }
});
async function boot() {
  try {
    const r = await api("/session");
    csrf = r.csrf_token;
    await navigate();
  } catch {
    loginPage();
  }
}
setInterval(() => {
  if (
    csrf &&
    !document.hidden &&
    !$("#modal").open &&
    (tab === "overview" || tab === "logs")
  ) {
    (tab === "overview" ? overview() : logsPage()).catch(() => {});
  }
}, 15000);
boot();
