#!/usr/bin/env python3
"""Refresh or verify TauTerm's vendored TCNOpen TRDP 3.0.0.0 source.

The committed vendor tree is derived deterministically from the official release:
1. normalize official text files to UTF-8/LF;
2. apply the ordered downstream patch series declared in SOURCE.json.

Normal TauTerm builds use the already-patched committed source and never download TCNOpen.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
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
PATCH_DIR = VENDOR / "patches"

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

_HUNK_RE = re.compile(
    r"^@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@(?: .*)?$"
)


def normalize_text(raw: bytes, path: str) -> str:
    try:
        text = raw.decode("utf-8-sig")
    except UnicodeDecodeError as exc:
        raise RuntimeError(f"{path}: expected text source, decode failed: {exc}") from exc
    return text.replace("\r\n", "\n").replace("\r", "\n")


def source_metadata() -> dict:
    return json.loads(SOURCE_JSON.read_text(encoding="utf-8"))


def patch_series() -> list[Path]:
    metadata = source_metadata()
    entries = metadata.get("downstream_patches")
    if not isinstance(entries, list) or not entries:
        raise RuntimeError("SOURCE.json must declare a non-empty downstream_patches list")

    series: list[Path] = []
    declared: set[Path] = set()
    for entry in entries:
        if not isinstance(entry, dict) or not isinstance(entry.get("file"), str):
            raise RuntimeError("Each downstream_patches entry must contain a string file path")
        relative = Path(entry["file"])
        path = (VENDOR / relative).resolve()
        try:
            path.relative_to(VENDOR.resolve())
        except ValueError as exc:
            raise RuntimeError(f"Patch path escapes vendor root: {relative}") from exc
        if path.suffix != ".patch" or not path.is_file():
            raise RuntimeError(f"Declared downstream patch is missing: {relative}")
        if path in declared:
            raise RuntimeError(f"Duplicate downstream patch declaration: {relative}")
        declared.add(path)
        series.append(path)

    actual = {path.resolve() for path in PATCH_DIR.glob("*.patch")}
    if actual != declared:
        missing = sorted(str(path.relative_to(VENDOR.resolve())) for path in declared - actual)
        extra = sorted(str(path.relative_to(VENDOR.resolve())) for path in actual - declared)
        raise RuntimeError(
            "SOURCE.json downstream patch list does not match patches directory; "
            f"missing={missing}, extra={extra}"
        )
    return series


def _parse_patch(patch_path: Path) -> list[tuple[str, list[tuple[int, int, list[str]]]]]:
    lines = patch_path.read_text(encoding="utf-8").splitlines()
    result: list[tuple[str, list[tuple[int, int, list[str]]]]] = []
    index = 0

    while index < len(lines):
        if not lines[index].startswith("--- "):
            index += 1
            continue

        old_name = lines[index][4:].split("\t", 1)[0]
        index += 1
        if index >= len(lines) or not lines[index].startswith("+++ "):
            raise RuntimeError(f"{patch_path}: missing +++ header")
        new_name = lines[index][4:].split("\t", 1)[0]
        index += 1

        if not old_name.startswith("a/") or not new_name.startswith("b/"):
            raise RuntimeError(f"{patch_path}: patch paths must use a/ and b/ prefixes")
        old_relative = old_name[2:]
        new_relative = new_name[2:]
        if old_relative != new_relative:
            raise RuntimeError(f"{patch_path}: renames are not supported")
        if old_relative not in VENDORED_FILES:
            raise RuntimeError(f"{patch_path}: patch targets non-vendored file {old_relative}")

        hunks: list[tuple[int, int, list[str]]] = []
        while index < len(lines) and not lines[index].startswith("--- "):
            match = _HUNK_RE.match(lines[index])
            if not match:
                if lines[index].strip():
                    raise RuntimeError(
                        f"{patch_path}: unsupported patch line outside hunk: {lines[index]}"
                    )
                index += 1
                continue

            old_start = int(match.group(1))
            new_start = int(match.group(3))
            index += 1
            body: list[str] = []
            while index < len(lines):
                line = lines[index]
                if line.startswith("@@ ") or line.startswith("--- "):
                    break
                if line == r"\ No newline at end of file":
                    index += 1
                    continue
                if not line or line[0] not in " +-":
                    raise RuntimeError(f"{patch_path}: unsupported hunk line: {line}")
                body.append(line)
                index += 1
            hunks.append((old_start, new_start, body))

        result.append((old_relative, hunks))

    if not result:
        raise RuntimeError(f"{patch_path}: no file patches found")
    return result


def _apply_file_hunks(
    text: str,
    hunks: list[tuple[int, int, list[str]]],
    *,
    reverse: bool,
    label: str,
) -> str:
    had_newline = text.endswith("\n")
    source = text.splitlines()
    offset = 0

    for old_start, new_start, body in hunks:
        before: list[str] = []
        after: list[str] = []
        for line in body:
            marker, payload = line[0], line[1:]
            if marker in " -":
                before.append(payload)
            if marker in " +":
                after.append(payload)

        if reverse:
            before, after = after, before
            expected_start = new_start - 1
        else:
            expected_start = old_start - 1

        position = expected_start + offset
        actual = source[position : position + len(before)]
        if actual != before:
            raise RuntimeError(
                f"{label}: patch context mismatch at line {expected_start + 1}; "
                f"expected {before!r}, found {actual!r}"
            )
        source[position : position + len(before)] = after
        offset += len(after) - len(before)

    result = "\n".join(source)
    if had_newline:
        result += "\n"
    return result


def apply_patch(
    files: dict[str, str],
    patch_path: Path,
    *,
    reverse: bool = False,
) -> None:
    parsed = _parse_patch(patch_path)
    for relative, hunks in parsed:
        if relative not in files:
            raise RuntimeError(f"{patch_path}: missing target in source set: {relative}")
        files[relative] = _apply_file_hunks(
            files[relative],
            hunks,
            reverse=reverse,
            label=f"{patch_path.name}:{relative}",
        )


def apply_downstream_patches(files: dict[str, str]) -> None:
    for patch_path in patch_series():
        apply_patch(files, patch_path)


def verify_patch_roundtrip() -> bool:
    current: dict[str, str] = {}
    for relative in VENDORED_FILES:
        path = VENDOR / relative
        if not path.is_file():
            print(f"MISSING {relative}")
            return False
        current[relative] = path.read_text(encoding="utf-8-sig").replace(
            "\r\n", "\n"
        ).replace("\r", "\n")

    pristine = dict(current)
    series = patch_series()
    for patch_path in reversed(series):
        apply_patch(pristine, patch_path, reverse=True)

    roundtrip = dict(pristine)
    for patch_path in series:
        apply_patch(roundtrip, patch_path)

    if roundtrip != current:
        print("Downstream patch round-trip did not reproduce committed vendor tree.", file=sys.stderr)
        return False

    print(
        f"Verified {len(series)} downstream patches round-trip cleanly against "
        f"{len(VENDORED_FILES)} committed vendor files."
    )
    return True


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
        actual = path.read_text(encoding="utf-8-sig").replace(
            "\r\n", "\n"
        ).replace("\r", "\n")
        if actual != expected:
            print(f"DIFF    {relative}")
            ok = False
    return ok


def update_snapshot(files: dict[str, str], sha256: str) -> None:
    for relative, text in files.items():
        path = VENDOR / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding="utf-8", newline="\n")

    metadata = source_metadata()
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
        help="compare committed vendor files with official release + downstream patches (default)",
    )
    mode.add_argument(
        "--update",
        action="store_true",
        help="refresh vendored files from official release and apply downstream patches",
    )
    mode.add_argument(
        "--check-patches",
        action="store_true",
        help="offline verification that the declared patch series exactly round-trips the committed vendor tree",
    )
    parser.add_argument(
        "--archive",
        type=Path,
        help="use an existing official 3.0.0.0.zip instead of downloading",
    )
    args = parser.parse_args()

    if args.check_patches:
        return 0 if verify_patch_roundtrip() else 1

    with tempfile.TemporaryDirectory(prefix="tauterm-tcnopen-") as temp_dir:
        archive_path = (
            args.archive.resolve()
            if args.archive
            else Path(temp_dir) / f"{VERSION}.zip"
        )
        if not args.archive:
            download_release(archive_path)
        if not archive_path.is_file():
            raise RuntimeError(f"Archive not found: {archive_path}")

        sha256 = archive_sha256(archive_path)
        files = release_files(archive_path)
        verify_version(files)
        apply_downstream_patches(files)
        print(f"TCNOpen {VERSION} archive SHA-256: {sha256}")
        print(f"Applied {len(patch_series())} downstream patches.")

        if args.update:
            update_snapshot(files, sha256)
            print(f"Updated {len(files)} patched vendor files under {VENDOR}")
            return 0

        if check_snapshot(files):
            print(
                f"Verified {len(files)} vendor files against the official release "
                "plus downstream patch series."
            )
            return 0
        print(
            "Vendored TCNOpen tree differs from official release + downstream patches.",
            file=sys.stderr,
        )
        return 1


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except Exception as exc:
        print(f"vendor_tcnopen.py: {exc}", file=sys.stderr)
        raise SystemExit(2)
