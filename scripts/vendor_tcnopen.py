#!/usr/bin/env python3
"""Refresh or verify TauTerm's vendored TCNOpen TRDP 3.0.0.0 snapshot.

This is a maintainer tool. Normal TauTerm builds never download TCNOpen.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import shutil
import sys
import tempfile
import urllib.request
import zipfile
from pathlib import Path

VERSION = "3.0.0.0"
RELEASE_URL = (
    "https://sourceforge.net/projects/tcnopen/files/TRDP/"
    f"{VERSION}/{VERSION}.zip/download"
)

ROOT = Path(__file__).resolve().parent.parent
VENDOR = ROOT / "src-tauri" / "vendor" / "tcnopen"
SOURCE_JSON = VENDOR / "SOURCE.json"

VENDORED_FILES = [
    "src/api/iec61375-2-3.h",
    "src/api/tau_ctrl.h",
    "src/api/tau_ctrl_types.h",
    "src/api/tau_dnr.h",
    "src/api/tau_dnr_types.h",
    "src/api/tau_marshall.h",
    "src/api/tau_so_if.h",
    "src/api/tau_tti.h",
    "src/api/tau_tti_types.h",
    "src/api/tau_xml.h",
    "src/api/trdp-config.xsd",
    "src/api/trdp_if_light.h",
    "src/api/trdp_serviceRegistry.h",
    "src/api/trdp_tsn_def.h",
    "src/api/trdp_types.h",
    "src/common/tau_cstinfo.c",
    "src/common/tau_cstinfo.h",
    "src/common/tau_ctrl.c",
    "src/common/tau_dnr.c",
    "src/common/tau_marshall.c",
    "src/common/tau_so_if.c",
    "src/common/tau_tti.c",
    "src/common/tau_xml.c",
    "src/common/tlc_if.c",
    "src/common/tlc_if.h",
    "src/common/tlm_if.c",
    "src/common/tlp_if.c",
    "src/common/trdp_dllmain.c",
    "src/common/trdp_mdcom.c",
    "src/common/trdp_mdcom.h",
    "src/common/trdp_pdcom.c",
    "src/common/trdp_pdcom.h",
    "src/common/trdp_pdindex.c",
    "src/common/trdp_pdindex.h",
    "src/common/trdp_private.h",
    "src/common/trdp_stats.c",
    "src/common/trdp_stats.h",
    "src/common/trdp_utils.c",
    "src/common/trdp_utils.h",
    "src/common/trdp_xml.c",
    "src/common/trdp_xml.h",
    "src/vos/api/vos_mem.h",
    "src/vos/api/vos_shared_mem.h",
    "src/vos/api/vos_sock.h",
    "src/vos/api/vos_thread.h",
    "src/vos/api/vos_types.h",
    "src/vos/api/vos_utils.h",
    "src/vos/common/vos_mem.c",
    "src/vos/common/vos_utils.c",
    "src/vos/posix/vos_private.h",
    "src/vos/posix/vos_shared_mem.c",
    "src/vos/posix/vos_sock.c",
    "src/vos/posix/vos_sockTSN.c",
    "src/vos/posix/vos_thread.c",
    "src/vos/windows/vos_private.h",
    "src/vos/windows/vos_shared_mem.c",
    "src/vos/windows/vos_sock.c",
    "src/vos/windows/vos_thread.c",
]


def normalize_text(raw: bytes, path: str) -> str:
    try:
        text = raw.decode("utf-8-sig")
    except UnicodeDecodeError as exc:
        raise RuntimeError(f"{path}: expected text source, decode failed: {exc}") from exc
    return text.replace("\r\n", "\n").replace("\r", "\n")


def download_release(target: Path) -> None:
    request = urllib.request.Request(
        RELEASE_URL,
        headers={"User-Agent": "TauTerm-TCNOpen-vendor/1.0"},
    )
    print(f"Downloading official TCNOpen {VERSION} release...")
    with urllib.request.urlopen(request, timeout=120) as response, target.open("wb") as output:
        shutil.copyfileobj(response, output)
    if target.read_bytes()[:4] != b"PK\x03\x04":
        raise RuntimeError(
            "SourceForge response is not a ZIP archive. "
            "Retry later or pass --archive /path/to/3.0.0.0.zip."
        )


def archive_sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def locate_prefix(archive: zipfile.ZipFile) -> str:
    suffix = "src/api/trdp_if_light.h"
    matches = [name for name in archive.namelist() if name.endswith(suffix)]
    if len(matches) != 1:
        raise RuntimeError(
            f"Expected exactly one {suffix} in official ZIP, found {len(matches)}"
        )
    return matches[0][: -len(suffix)]


def release_files(archive_path: Path) -> dict[str, str]:
    result: dict[str, str] = {}
    with zipfile.ZipFile(archive_path) as archive:
        prefix = locate_prefix(archive)
        names = set(archive.namelist())
        for relative in VENDORED_FILES:
            member = prefix + relative
            if member not in names:
                raise RuntimeError(f"Official archive is missing {relative}")
            result[relative] = normalize_text(archive.read(member), relative)
    return result


def verify_version(files: dict[str, str]) -> None:
    private = files["src/common/trdp_private.h"]
    expected = {
        "TRDP_VERSION": "3",
        "TRDP_RELEASE": "0",
        "TRDP_UPDATE": "0",
        "TRDP_EVOLUTION": "0",
    }
    for macro, value in expected.items():
        marker = f"#define {macro}"
        matching = [
            line
            for line in private.splitlines()
            if line.strip().startswith(marker)
        ]
        if not matching or matching[0].split()[-1] != value:
            raise RuntimeError(
                f"Official source is not the expected {VERSION}: "
                f"{macro} should be {value}"
            )


def check_snapshot(files: dict[str, str]) -> bool:
    ok = True
    for relative, expected in files.items():
        path = VENDOR / relative
        if not path.is_file():
            print(f"MISSING {relative}")
            ok = False
            continue
        actual = path.read_text(encoding="utf-8-sig").replace("\r\n", "\n").replace("\r", "\n")
        if actual != expected:
            print(f"DIFF    {relative}")
            ok = False
    return ok


def update_snapshot(files: dict[str, str], sha256: str) -> None:
    for relative, text in files.items():
        path = VENDOR / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding="utf-8", newline="\n")

    metadata = json.loads(SOURCE_JSON.read_text(encoding="utf-8"))
    metadata["archive_sha256"] = sha256
    SOURCE_JSON.write_text(
        json.dumps(metadata, indent=2, ensure_ascii=False) + "\n",
        encoding="utf-8",
        newline="\n",
    )


def main() -> int:
    parser = argparse.ArgumentParser()
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument(
        "--check",
        action="store_true",
        help="compare the committed snapshot with the official release (default)",
    )
    mode.add_argument(
        "--update",
        action="store_true",
        help="replace vendored files from the official release",
    )
    parser.add_argument(
        "--archive",
        type=Path,
        help="use an existing official 3.0.0.0.zip instead of downloading",
    )
    args = parser.parse_args()

    with tempfile.TemporaryDirectory(prefix="tauterm-tcnopen-") as temp_dir:
        archive_path = args.archive.resolve() if args.archive else Path(temp_dir) / f"{VERSION}.zip"
        if not args.archive:
            download_release(archive_path)
        if not archive_path.is_file():
            raise RuntimeError(f"Archive not found: {archive_path}")

        sha256 = archive_sha256(archive_path)
        files = release_files(archive_path)
        verify_version(files)
        print(f"TCNOpen {VERSION} archive SHA-256: {sha256}")

        if args.update:
            update_snapshot(files, sha256)
            print(f"Updated {len(files)} vendored files under {VENDOR}")
            return 0

        if check_snapshot(files):
            print(f"Verified {len(files)} vendored files against the official release.")
            return 0
        print("Vendored TCNOpen snapshot differs from the official release.", file=sys.stderr)
        return 1


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except Exception as exc:
        print(f"vendor_tcnopen.py: {exc}", file=sys.stderr)
        raise SystemExit(2)
