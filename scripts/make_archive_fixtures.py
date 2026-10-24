#!/usr/bin/env python3
"""Build the small ZIP fixtures the archive tests use.

Everything here is generated: no third-party archive is committed, and the
fixtures carry no DRM, no personal data and nothing but plain text.

Run from the repository root:

    python scripts/make_archive_fixtures.py
"""
import pathlib
import zipfile

ROOT = pathlib.Path(__file__).resolve().parents[1]
FIX = ROOT / "tests" / "fixtures"


def write_zip(name, entries):
    path = FIX / name
    with zipfile.ZipFile(path, "w", zipfile.ZIP_DEFLATED) as z:
        for entry_name, data in entries:
            z.writestr(entry_name, data)
    return path


def main():
    FIX.mkdir(parents=True, exist_ok=True)

    # A plain archive with a folder and two files.
    write_zip(
        "sample.zip",
        [
            ("hello.txt", b"hello from the zip fixture\n"),
            ("folder/nested.txt", b"nested file\n"),
            ("folder/deeper/leaf.txt", b"leaf\n"),
        ],
    )

    # An archive whose entries try to leave the destination folder.
    write_zip(
        "traversal.zip",
        [
            ("../escape.txt", b"this one must never be written\n"),
            ("safe.txt", b"this one is fine\n"),
            ("a/../../escape2.txt", b"this one must never be written either\n"),
        ],
    )

    # An archive with a name Windows would not accept.
    write_zip(
        "reserved-names.zip",
        [
            ("CON", b"device name\n"),
            ("ok.txt", b"fine\n"),
        ],
    )

    for p in sorted(FIX.glob("*.zip")):
        print(p, p.stat().st_size)


if __name__ == "__main__":
    main()
