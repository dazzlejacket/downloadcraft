#!/usr/bin/env python3
"""Create sanitized representative fixtures from a live DownloadCraft config dir.

The live config can hold real user data (signed URLs, session cookies, proxy
credentials, local paths). This script copies the *shape* of the real files into
a fixtures directory with every secret-bearing value replaced by a sentinel, so
the merge and the migration tests can run against a realistic file without
copying anybody's credentials.

Usage:
    python scripts/make_config_fixtures.py --source <config_dir> --output <dir>

Nothing in the output contains a value copied verbatim from the source.
"""
from __future__ import annotations

import argparse
import json
import os
import re
from pathlib import Path

# Queue-row fields whose value is a credential or a user-identifying path.
SECRET_ROW_KEYS = {
    "cookie": "SENTINEL_COOKIE",
    "username": "SENTINEL_USER",
    "password": "SENTINEL_PASSWORD",
    "extra_headers": "SENTINEL_HEADERS",
    "user_agent": "SENTINEL_UA",
    # A real filename carries the media title and therefore the user's history.
    # Keep the extension (the naming logic cares about it) and lose the rest.
    "filename": "SENTINEL_NAME",
    "description": "SENTINEL_DESCRIPTION",
}

# ui.json keys that can carry a secret or an absolute user path.
SECRET_UI_KEYS = {
    "proxy_url": "SENTINEL_PROXY",
    "user_agent": "SENTINEL_UA",
    "save_dir": "SENTINEL_SAVE_DIR",
    "antivirus_cmd": "SENTINEL_AV_CMD",
    "extract_custom_dir": "SENTINEL_EXTRACT_DIR",
}

URL_RE = re.compile(r"^[a-zA-Z][a-zA-Z0-9+.-]*://")
SECRETISH_KEY_RE = re.compile(r"(token|secret|password|cookie|auth|apikey|api_key)", re.IGNORECASE)


def redact_url(value: str) -> str:
    """Keep the scheme and the shape, lose the host, the path and the query."""
    if not isinstance(value, str) or not URL_RE.match(value):
        return value
    scheme = value.split("://", 1)[0]
    return f"{scheme}://SENTINEL_HOST/SENTINEL_PATH"


def redact_value(value, key: str | None = None):
    if isinstance(value, dict):
        out = {}
        for k, v in value.items():
            if key is not None and SECRETISH_KEY_RE.search(str(k)):
                out[k] = "SENTINEL_SECRET"
            else:
                out[k] = redact_value(v, k)
        return out
    if isinstance(value, list):
        return [redact_value(v, key) for v in value]
    if isinstance(value, str):
        if key in SECRET_ROW_KEYS or key in SECRET_UI_KEYS:
            base = SECRET_ROW_KEYS.get(key) or SECRET_UI_KEYS[key]
            if key == "filename" and isinstance(value, str):
                suffix = os.path.splitext(value)[1]
                return base + suffix
            return base
        if URL_RE.match(value):
            return redact_url(value)
        return value
    return value


def sanitize_queue(src: Path, dest: Path) -> int:
    rows = json.loads(src.read_text(encoding="utf-8-sig"))
    if not isinstance(rows, list):
        raise SystemExit("queue.json is not a JSON array")
    clean = [redact_value(row) for row in rows]
    # Keep a bounded, representative slice: 3 completed, 3 active, 3 failed if present.
    by_status: dict[str, list] = {}
    for row in clean:
        by_status.setdefault(str(row.get("status", "?")), []).append(row)
    sample = []
    for status, group in sorted(by_status.items()):
        sample.extend(group[:3])
    dest.write_text(json.dumps(sample, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    return len(sample)


def sanitize_ui(src: Path, dest: Path) -> int:
    data = json.loads(src.read_text(encoding="utf-8-sig"))
    clean = redact_value(data)
    dest.write_text(json.dumps(clean, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    return len(clean)


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--source", required=True, help="live config dir (e.g. APPDATA/download_craft)")
    ap.add_argument("--output", required=True, help="fixtures dir to write")
    args = ap.parse_args()

    src = Path(args.source)
    out = Path(args.output)
    out.mkdir(parents=True, exist_ok=True)

    written = []
    q = src / "queue.json"
    if q.exists():
        n = sanitize_queue(q, out / "queue.json")
        written.append(f"queue.json ({n} representative rows)")
    u = src / "ui.json"
    if u.exists():
        n = sanitize_ui(u, out / "ui.json")
        written.append(f"ui.json ({n} top-level keys; the app's settings live here too)")

    # A settings.json is not a separate file in this app, but downstream tooling
    # and the migration tests want one. Emit it as a view over the same settings
    # keys, so a future settings.json split has a fixture to compare against.
    ui_out = out / "ui.json"
    if ui_out.exists():
        data = json.loads(ui_out.read_text(encoding="utf-8"))
        settings = {
            k: data[k]
            for k in (
                "save_dir", "proxy_url", "user_agent", "theme", "max_concurrent",
                "connections_default", "max_retries", "timeout_secs", "speed_limit_kbps",
                "finish_action", "close_to_tray", "show_complete", "update_check_on",
                "category_folders", "rename_template", "folder_rules", "antivirus_cmd",
            )
            if k in data
        }
        (out / "settings.json").write_text(
            json.dumps(settings, indent=2, ensure_ascii=False) + "\n", encoding="utf-8"
        )
        written.append(f"settings.json ({len(settings)} settings keys derived from ui.json)")

    print("Wrote sanitized fixtures to", out)
    for line in written:
        print("  -", line)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
