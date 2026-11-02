#!/usr/bin/env python3
"""Create the private .env file and admin password for Linux or Docker installs.

Usage:
    python3 scripts/setup.py                                   # local use: http://localhost:8080
    python3 scripts/setup.py --origin https://api.example.com  # public HTTPS domain

Writes .env (mode 600) and data/admin-login.txt (mode 600). Never overwrites an
existing .env and never prints the password. The OpenCode Go key is added later in
the dashboard (Setup & settings), so it is never typed into a shell.
"""
import argparse
import json
import os
import pathlib
import re
import secrets

ROOT = pathlib.Path(__file__).resolve().parents[1]


def write_private(path: pathlib.Path, text: str) -> None:
    """Create a new file readable only by the current user; fail if it exists."""
    fd = os.open(path, os.O_CREAT | os.O_EXCL | os.O_WRONLY, 0o600)
    with os.fdopen(fd, "w") as f:
        f.write(text)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--origin", default="http://localhost:8080",
                        help="exact URL users open in the browser, without a path")
    parser.add_argument("--bind", default="127.0.0.1:8080",
                        help="listen address; keep it on 127.0.0.1 behind a reverse proxy")
    args = parser.parse_args()

    if not re.fullmatch(r"https?://[^/\s]+", args.origin):
        raise SystemExit("--origin must look like https://api.example.com (no path or trailing slash).")
    env_path = ROOT / ".env"
    if env_path.exists():
        raise SystemExit(".env already exists. Edit it instead; this helper never overwrites credentials.")

    password = secrets.token_urlsafe(32)
    secure = str(args.origin.startswith("https://")).lower()
    text = (ROOT / ".env.example").read_text()
    # JSON string quoting is valid dotenv syntax and avoids shell interpretation.
    for old, new in [
        ("ADMIN_PASSWORD=replace-with-a-long-random-password", "ADMIN_PASSWORD=" + json.dumps(password)),
        ("BIND=127.0.0.1:8080", "BIND=" + args.bind),
        ("PUBLIC_URL=http://localhost:8080", "PUBLIC_URL=" + args.origin),
        ("COOKIE_SECURE=false", "COOKIE_SECURE=" + secure),
    ]:
        if old not in text:
            raise SystemExit(f".env.example is missing the expected line: {old.split('=')[0]}")
        text = text.replace(old, new)
    if args.origin.startswith("https://"):
        text = re.sub(r"(?m)^DOMAIN=.*$", "DOMAIN=" + args.origin.removeprefix("https://"), text)
    write_private(env_path, text)

    data = ROOT / "data"
    data.mkdir(exist_ok=True)
    write_private(data / "admin-login.txt", (
        "Go Router admin\n"
        f"URL: {args.origin}/admin\n"
        f"Password: {password}\n\n"
        f"API base URL: {args.origin}/api/v1\n"
        "Create application keys in the dashboard. Never give the OpenCode Go key to applications.\n"
    ))
    print("Created .env and data/admin-login.txt (both mode 600). No credentials printed.")
    print("Next: start the gateway, sign in, and save your OpenCode Go key in Setup & settings.")


if __name__ == "__main__":
    main()
