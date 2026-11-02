#!/usr/bin/env python3
"""Validate config/models.json offline (no credentials, no network).

Run after editing the catalog, before reloading it in the dashboard:
    python3 scripts/check_catalog.py
"""
import json
import pathlib
import sys

ROOT = pathlib.Path(__file__).resolve().parents[1]


def main() -> int:
    models = json.loads((ROOT / "config" / "models.json").read_text())
    names: set[str] = set()
    problems: list[str] = []
    for m in models:
        mid = m.get("id", "<missing id>")
        if not m.get("upstream_id") or "/" in m["upstream_id"]:
            problems.append(f"{mid}: upstream_id must be the raw Go model ID without '/'")
        if m.get("context_length", 0) <= 0 or m.get("max_completion_tokens", 0) <= 0:
            problems.append(f"{mid}: context_length and max_completion_tokens must be positive")
        if set(m.get("output_modalities", [])) != {"text"}:
            problems.append(f"{mid}: only text output is supported")
        for price in ("prompt", "completion"):
            try:
                if float(m["pricing"][price]) < 0:
                    raise ValueError
            except (KeyError, TypeError, ValueError):
                problems.append(f"{mid}: pricing.{price} must be a non-negative number string")
        for name in [mid, *m.get("aliases", [])]:
            if name in names:
                problems.append(f"{mid}: duplicate model ID or alias {name!r}")
            names.add(name)
    if problems:
        print("\n".join(problems), file=sys.stderr)
        return 1
    print(f"Validated {len(models)} Go models and {len(names)} unique IDs/aliases.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
