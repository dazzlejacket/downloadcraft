#!/usr/bin/env python3
"""Check that no upstream or fork symbol was lost in the merge.

The v0.9.8 merge was resolved by hand, and one hand-written replacement
silently dropped four upstream functions while another dropped a fork
helper. This compares the inventory of public items (fn / struct / enum /
const / static / trait / impl / mod) in every source file against BOTH
sides of the merge -- the upstream tag and the fork branch -- and reports
what is missing.

A symbol that was deliberately replaced is listed in
`scripts/merge_symbol_allowlist.txt` as `file|symbol`, one per line. It is
reported as replaced instead of missing, so the list stays a record of the
intentional changes rather than a way to silence a real loss.

Usage:
    python scripts/check_merge_symbols.py --repo <worktree> --base v0.9.8
"""
from __future__ import annotations

import argparse
import re
import subprocess
import sys
from pathlib import Path

ITEM_RE = re.compile(
    r"^\s*(?:pub(?:\([^)]*\))?\s+)?"
    r"(?:async\s+)?"
    r"(fn|struct|enum|const|static|trait|impl|mod)\s+([A-Za-z_][A-Za-z0-9_]*)",
    re.MULTILINE,
)

ALLOWLIST = "scripts/merge_symbol_allowlist.txt"


def inventory(text: str) -> set[str]:
    out = set()
    for kind, name in ITEM_RE.findall(text):
        out.add(f"{kind} {name}")
    return out


def git(repo: Path, *args: str) -> str:
    return subprocess.run(
        ["git", *args], cwd=repo, capture_output=True, text=True,
        encoding="utf-8", errors="replace", check=True,
    ).stdout


def load_allowlist(repo: Path) -> set[tuple[str, str]]:
    path = repo / ALLOWLIST
    allowed: set[tuple[str, str]] = set()
    if not path.exists():
        return allowed
    for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        if "|" not in line:
            print(f"{ALLOWLIST}: bad line {line!r} (expected file|symbol)")
            continue
        rel, symbol = line.split("|", 1)
        allowed.add((rel.strip().replace("\\", "/"), symbol.strip()))
    return allowed


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--repo", default=".")
    ap.add_argument("--base", default="v0.9.8")
    args = ap.parse_args()
    repo = Path(args.repo).resolve()
    allowed = load_allowlist(repo)

    missing_total = 0
    replaced_total = 0

    for label, ref in (("upstream", args.base), ("fork", "HEAD")):
        files = git(repo, "ls-tree", "-r", "--name-only", ref, "--", "src/").split()
        for rel in files:
            other = git(repo, "show", f"{ref}:{rel}")
            path = repo / rel
            if not path.exists():
                print(f"{rel}: FILE MISSING (the {label} file is not in the merge)")
                missing_total += 1
                continue
            merged = path.read_text(encoding="utf-8", errors="replace")
            other_items = inventory(other)
            got_items = inventory(merged)
            missing = sorted(i for i in other_items if i not in got_items)
            for item in missing:
                kind, _, name = item.partition(" ")
                if (rel.replace("\\", "/"), name) in allowed:
                    print(f"{rel}: {label} `{name}` was replaced on purpose")
                    replaced_total += 1
                    continue
                print(f"{rel}: {label} item not found in the merged file: {item}")
                missing_total += 1

    print()
    if missing_total == 0:
        print(f"RESULT: no symbol was lost ({replaced_total} replacement(s) on record)")
    else:
        print(f"RESULT: {missing_total} symbol(s) missing")
    return 1 if missing_total else 0


if __name__ == "__main__":
    sys.exit(main())
