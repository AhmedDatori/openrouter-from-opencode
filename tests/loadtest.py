#!/usr/bin/env python3
"""Concurrent chat load test for Go Router. Standard library only.

Each virtual user holds a multi-turn conversation: it sends a message, streams the
reply, "reads" for a few seconds, then sends the next message with the history.

Real gateway (uses Go allowance; create a test key with rate limit 0 first):
  GATEWAY_KEY=sk-gr-... GATEWAY_ADMIN_PASSWORD=... \
    python3 tests/loadtest.py --url https://gateway.example.com --users 50 --duration 300

Offline gateway-overhead run (local release binary + simulated model, no allowance):
  cargo build --release && python3 tests/loadtest.py --mock --users 50 --duration 300

With an admin password the script restarts the dashboard measurement at the start
and prints the gateway's own CPU / memory / latency summary at the end.
"""
import argparse
import http.client
import http.cookies
import http.server
import json
import os
import pathlib
import random
import secrets
import socket
import ssl
import statistics
import subprocess
import sys
import tempfile
import threading
import time
import urllib.parse

ROOT = pathlib.Path(__file__).resolve().parents[1]
PROMPTS = [
    "Hi! Can you explain what an API gateway does in simple terms?",
    "Give me three tips to write clearer emails at work.",
    "What is the difference between RAM and storage?",
    "Summarize the plot of a classic adventure novel in a short paragraph.",
    "Write a short friendly reply to a customer asking about delivery times.",
    "How do I calculate a 15% discount on 240?",
    "Suggest a healthy breakfast I can make in five minutes.",
    "Explain recursion with a small Python example.",
    "What should I check before signing a rental contract?",
    "Translate 'thank you for your patience' into French, Spanish and Arabic.",
    "Can you make that shorter?",
    "Thanks. Now give me one more example.",
    "Why is that the case?",
    "Turn that into a bullet list.",
]


def pct(values, p):
    if not values:
        return None
    ordered = sorted(values)
    rank = max(1, min(len(ordered), int(-(-p * len(ordered) // 100))))
    return round(ordered[rank - 1])


class Client:
    def __init__(self, base):
        u = urllib.parse.urlsplit(base)
        self.https = u.scheme == "https"
        self.host = u.hostname
        self.port = u.port or (443 if self.https else 80)
        prefix = u.path.rstrip("/")
        if prefix.endswith("/api/v1"):
            prefix = prefix[: -len("/api/v1")]
        self.prefix = prefix
        self.ctx = ssl.create_default_context() if self.https else None

    def connect(self, timeout=660):
        if self.https:
            return http.client.HTTPSConnection(self.host, self.port, timeout=timeout, context=self.ctx)
        return http.client.HTTPConnection(self.host, self.port, timeout=timeout)

    def json(self, method, path, body=None, headers=None, timeout=20):
        conn = self.connect(timeout)
        try:
            raw = json.dumps(body).encode() if body is not None else None
            conn.request(method, self.prefix + path, body=raw, headers={"Content-Type": "application/json", **(headers or {})})
            r = conn.getresponse()
            data = r.read()
            try:
                value = json.loads(data)
            except ValueError:
                value = data.decode(errors="replace")
            return r.status, r.headers, value
        finally:
            conn.close()


class Admin:
    """Reads the gateway's own monitor through the admin API (optional)."""

    def __init__(self, client, password):
        self.client = client
        status, headers, value = client.json("POST", "/admin/api/login", {"password": password})
        if status != 200:
            raise SystemExit(f"Admin login failed ({status}): {value}")
        cookie = http.cookies.SimpleCookie(headers["Set-Cookie"])
        self.headers = {"Cookie": "go_admin=" + cookie["go_admin"].value, "X-CSRF-Token": value["csrf_token"]}

    def reset(self):
        status, _, value = self.client.json("POST", "/admin/api/metrics/reset", {}, self.headers)
        if status != 200:
            raise SystemExit(f"This gateway has no monitor endpoint ({status}); update to v0.3.0 or newer. {value}")

    def metrics(self):
        status, _, value = self.client.json("GET", "/admin/api/metrics?since=" + str(int(time.time() * 1000)), None, self.headers)
        return value if status == 200 else None


class Stats:
    def __init__(self):
        self.lock = threading.Lock()
        self.results = []
        self.active = 0
        self.peak_active = 0

    def begin(self):
        with self.lock:
            self.active += 1
            self.peak_active = max(self.peak_active, self.active)

    def end(self, result):
        with self.lock:
            self.active -= 1
            self.results.append(result)

    def window(self, since):
        with self.lock:
            return [r for r in self.results if r["end"] >= since]


def chat_once(client, key, body):
    """Streams one chat completion. Returns timing and outcome."""
    start = time.perf_counter()
    out = {"start": time.time(), "status": 0, "ttft_ms": None, "chars": 0, "error": None, "usage": None, "text": ""}
    conn = client.connect()
    try:
        conn.request(
            "POST",
            client.prefix + "/api/v1/chat/completions",
            body=json.dumps(body).encode(),
            headers={"Content-Type": "application/json", "Authorization": "Bearer " + key, "X-Title": "Go Router load test"},
        )
        r = conn.getresponse()
        out["status"] = r.status
        if r.status != 200:
            raw = r.read(4096).decode(errors="replace")
            try:
                out["error"] = json.loads(raw)["error"]["message"]
            except (ValueError, KeyError, TypeError):
                out["error"] = raw[:300]
            return out
        parts = []
        while True:
            line = r.readline()
            if not line:
                break
            line = line.strip()
            if not line.startswith(b"data: "):
                continue
            data = line[6:]
            if data == b"[DONE]":
                break
            frame = json.loads(data)
            if "error" in frame:
                out["error"] = str(frame["error"].get("message", frame["error"]))
                break
            if frame.get("usage"):
                out["usage"] = frame["usage"]
            for choice in frame.get("choices") or []:
                text = (choice.get("delta") or {}).get("content") or ""
                if text:
                    if out["ttft_ms"] is None:
                        out["ttft_ms"] = (time.perf_counter() - start) * 1000
                    parts.append(text)
        out["text"] = "".join(parts)
        out["chars"] = len(out["text"])
        if not out["text"] and not out["error"]:
            out["error"] = "Empty reply"
    except Exception as e:  # network errors are results, not crashes
        out["error"] = f"{type(e).__name__}: {e}"
    finally:
        conn.close()
        out["total_ms"] = (time.perf_counter() - start) * 1000
        out["end"] = time.time()
    return out


def user(n, args, client, stats, stop_at):
    rng = random.Random(n)
    time.sleep(rng.uniform(0, args.ramp))
    history = [{"role": "system", "content": "You are a helpful assistant. Keep answers under 150 words."}]
    while time.time() < stop_at:
        history.append({"role": "user", "content": rng.choice(PROMPTS)})
        body = {"model": args.model, "messages": history, "stream": True, "max_tokens": args.max_tokens}
        stats.begin()
        result = chat_once(client, args.key, body)
        result["user"] = n
        stats.end({k: v for k, v in result.items() if k != "text"})
        if result["error"] or result["status"] != 200:
            history.pop()
            time.sleep(min(10, 2 + rng.random() * 3))
            continue
        history.append({"role": "assistant", "content": result["text"]})
        # Keep the system prompt and the most recent turns, like a chat app would.
        if len(history) > 1 + args.history_turns * 2:
            history = history[:1] + history[-args.history_turns * 2 :]
        time.sleep(rng.uniform(args.think_min, args.think_max))


def summarize(results, seconds):
    ok = [r for r in results if r["status"] == 200 and not r["error"]]
    errors = {}
    for r in results:
        if r["status"] != 200 or r["error"]:
            label = f"{r['status']} {r['error'] or ''}".strip()[:120]
            errors[label] = errors.get(label, 0) + 1
    total = [r["total_ms"] for r in ok]
    ttft = [r["ttft_ms"] for r in ok if r["ttft_ms"] is not None]
    tokens_in = sum((r["usage"] or {}).get("prompt_tokens", 0) for r in ok)
    tokens_out = sum((r["usage"] or {}).get("completion_tokens", 0) for r in ok)
    return {
        "requests": len(results),
        "succeeded": len(ok),
        "failed": len(results) - len(ok),
        "requests_per_second": round(len(results) / max(seconds, 1), 2),
        "response_ms": {"p50": pct(total, 50), "p95": pct(total, 95), "p99": pct(total, 99), "max": round(max(total)) if total else None, "mean": round(statistics.mean(total)) if total else None},
        "first_token_ms": {"p50": pct(ttft, 50), "p95": pct(ttft, 95), "p99": pct(ttft, 99), "max": round(max(ttft)) if ttft else None},
        "tokens": {"input": tokens_in, "output": tokens_out},
        "errors": dict(sorted(errors.items(), key=lambda kv: -kv[1])),
    }


class MockModel(http.server.BaseHTTPRequestHandler):
    """Simulated model: waits before the first token, then streams tokens at a fixed rate."""

    protocol_version = "HTTP/1.1"
    ttft = 0.6
    tokens_per_second = 60
    tokens = 220

    def log_message(self, *a):
        pass

    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.send_header("Connection", "close")
        self.end_headers()
        cid = "chat_" + secrets.token_hex(6)
        prompt_tokens = sum(len(str(m.get("content", ""))) // 4 for m in body.get("messages", []))
        n = int(self.tokens * random.uniform(0.6, 1.4))
        try:
            time.sleep(self.ttft * random.uniform(0.7, 1.5))
            for i in range(n):
                frame = {"id": cid, "object": "chat.completion.chunk", "created": int(time.time()), "model": body["model"], "choices": [{"index": 0, "delta": {"content": "word "}, "finish_reason": None}]}
                self.wfile.write(b"data: " + json.dumps(frame).encode() + b"\n\n")
                self.wfile.flush()
                time.sleep(1 / self.tokens_per_second)
            usage = {"prompt_tokens": prompt_tokens, "completion_tokens": n, "total_tokens": prompt_tokens + n, "cost": 0, "opencode": {"billing_source": "go", "allowance": 0.0001, "list_cost": 0.0001}}
            final = {"id": cid, "object": "chat.completion.chunk", "created": int(time.time()), "model": body["model"], "choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}], "usage": usage}
            self.wfile.write(b"data: " + json.dumps(final).encode() + b"\n\ndata: [DONE]\n\n")
            self.wfile.flush()
        except (BrokenPipeError, ConnectionResetError):
            pass
        self.close_connection = True


def start_mock(args, tmp):
    http.server.ThreadingHTTPServer.daemon_threads = True
    up = http.server.ThreadingHTTPServer(("127.0.0.1", 0), MockModel)
    up.request_queue_size = 256
    threading.Thread(target=up.serve_forever, daemon=True).start()
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        port = s.getsockname()[1]
    password = "loadtest-only-password-" + secrets.token_hex(8)
    env = {
        **os.environ,
        "BIND": f"127.0.0.1:{port}",
        "DATABASE_PATH": tmp + "/loadtest.db",
        "GO_BASE_URL": f"http://127.0.0.1:{up.server_port}",
        "OPENCODE_GO_API_KEY": "mock-upstream-key-not-real",
        "ADMIN_PASSWORD": password,
        "MODEL_CATALOG_PATH": str(ROOT / "config/models.json"),
        "MAX_CONCURRENT_REQUESTS": str(max(64, args.users * 2)),
        "PUBLIC_URL": f"http://127.0.0.1:{port}",
    }
    log = open(tmp + "/server.log", "wb")
    proc = subprocess.Popen([args.binary], cwd=tmp, env=env, stdout=log, stderr=log)
    base = f"http://127.0.0.1:{port}"
    client = Client(base)
    for _ in range(100):
        try:
            if client.json("GET", "/healthz", timeout=1)[0] == 200:
                break
        except OSError:
            time.sleep(0.1)
    else:
        raise SystemExit("Local gateway did not start:\n" + pathlib.Path(tmp + "/server.log").read_text())
    admin = Admin(client, password)
    status, _, made = client.json("POST", "/admin/api/keys", {"name": "Load test", "rpm": 0}, admin.headers)
    assert status == 200, made
    return base, made["key"], password, proc, up, log


def main():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--url", help="Gateway base URL, e.g. https://gateway.example.com")
    p.add_argument("--key", default=os.environ.get("GATEWAY_KEY"), help="Gateway API key (or GATEWAY_KEY)")
    p.add_argument("--admin-password", default=os.environ.get("GATEWAY_ADMIN_PASSWORD"), help="Optional; reads the server monitor (or GATEWAY_ADMIN_PASSWORD)")
    p.add_argument("--users", type=int, default=50)
    p.add_argument("--duration", type=int, default=300, help="Seconds")
    p.add_argument("--ramp", type=float, default=20, help="Spread user start times over this many seconds")
    p.add_argument("--think-min", type=float, default=3)
    p.add_argument("--think-max", type=float, default=10)
    p.add_argument("--history-turns", type=int, default=6)
    p.add_argument("--model", default="deepseek/deepseek-v4.1-flash")
    p.add_argument("--max-tokens", type=int, default=400)
    p.add_argument("--mock", action="store_true", help="Run a local gateway against a simulated model")
    p.add_argument("--binary", default=str(ROOT / "target/release/go-router"))
    p.add_argument("--out", help="Write the JSON report here")
    args = p.parse_args()

    tmp = proc = up = log = None
    if args.mock:
        tmp = tempfile.mkdtemp(prefix="go-router-loadtest-")
        args.url, args.key, args.admin_password, proc, up, log = start_mock(args, tmp)
    if not args.url or not args.key:
        p.error("--url and --key (or GATEWAY_KEY) are required unless --mock is used")
    client = Client(args.url)
    admin = Admin(client, args.admin_password) if args.admin_password else None
    if admin:
        admin.reset()

    stats = Stats()
    started = time.time()
    stop_at = started + args.duration
    print(f"{args.users} users chatting for {args.duration}s against {args.url} ({args.model})", flush=True)
    threads = [threading.Thread(target=user, args=(i, args, client, stats, stop_at), daemon=True) for i in range(args.users)]
    for t in threads:
        t.start()
    server_peak = {"cpu_core": 0, "rss": 0}
    try:
        while any(t.is_alive() for t in threads):
            time.sleep(15)
            now = time.time()
            recent = stats.window(now - 15)
            ok = [r for r in recent if not r["error"] and r["status"] == 200]
            line = (
                f"[{now - started:5.0f}s] in flight {stats.active:3d} | last 15s: {len(recent):4d} req "
                f"{len(recent) - len(ok):3d} failed | p50 {pct([r['total_ms'] for r in ok], 50)} ms "
                f"p95 {pct([r['total_ms'] for r in ok], 95)} ms | first token p95 {pct([r['ttft_ms'] for r in ok if r['ttft_ms']], 95)} ms"
            )
            if admin:
                m = admin.metrics()
                c = (m or {}).get("current") or {}
                server_peak["cpu_core"] = max(server_peak["cpu_core"], c.get("cpu_core", 0))
                server_peak["rss"] = max(server_peak["rss"], c.get("rss", 0))
                line += f" | server CPU {c.get('cpu_core', 0):.1f}% of 1 core, RAM {c.get('rss', 0) / 1048576:.1f} MiB"
            print(line, flush=True)
            if now > stop_at + 120:
                print("Some requests are still running two minutes after the end; stopping.", flush=True)
                break
    except KeyboardInterrupt:
        print("Interrupted; summarizing what finished.", flush=True)
    elapsed = time.time() - started
    report = {
        "date_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime(started)),
        "target": "local mock model" if args.mock else args.url,
        "model": args.model,
        "users": args.users,
        "duration_s": args.duration,
        "think_s": [args.think_min, args.think_max],
        "client_peak_in_flight": stats.peak_active,
        "client": summarize(stats.results, elapsed),
    }
    if admin:
        time.sleep(6)  # let the monitor take its final sample
        m = admin.metrics()
        if m:
            s = m["summary"]
            report["server"] = {
                "cpu_count": m["cpu_count"],
                "max_concurrent_requests": m.get("max_concurrent"),
                "gateway_cpu_avg_pct_of_one_core": round(s["cpu_core_avg"], 2),
                "gateway_cpu_peak_pct_of_one_core": round(s["cpu_core_max"], 2),
                "gateway_rss_start_mib": round(s["rss_first"] / 1048576, 1),
                "gateway_rss_peak_mib": round(s["rss_max"] / 1048576, 1),
                "server_ram_used_peak_mib": round(s["sys_used_max"] / 1048576),
                "server_ram_total_mib": round((m.get("current") or {}).get("sys_total", 0) / 1048576),
                "peak_open_requests": s["active_max"],
                "peak_model_slots": s["slots_max"],
                "rejected_at_concurrency_limit": s["rejected"],
                "peak_db_queue": s["db_queue_max"],
                "gateway_response_ms": {"p50": s["p50"], "p95": s["p95"], "p99": s["p99"]},
                "gateway_first_token_ms": {"p50": s["ttft50"], "p95": s["ttft95"], "p99": s["ttft99"]},
            }
    if proc:
        proc.terminate()
        proc.wait(timeout=10)
        up.shutdown()
        log.close()
    text = json.dumps(report, indent=2)
    print(text)
    out = args.out or str(ROOT / "data" / f"loadtest-{time.strftime('%Y%m%d-%H%M%S')}.json")
    pathlib.Path(out).parent.mkdir(parents=True, exist_ok=True)
    pathlib.Path(out).write_text(text + "\n")
    print("Report saved to", out)


if __name__ == "__main__":
    main()
