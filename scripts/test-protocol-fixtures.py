#!/usr/bin/env python3
"""Self-test TauTerm's reusable protocol fixtures without requiring real hardware."""

from __future__ import annotations

import socket
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
TELNET_SERVER = ROOT / "scripts" / "test-telnet-server.py"


def free_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
        sock.bind(("127.0.0.1", 0))
        return int(sock.getsockname()[1])


def recv_until(sock: socket.socket, needle: bytes, timeout: float = 5.0) -> bytes:
    deadline = time.monotonic() + timeout
    data = bytearray()
    while needle not in data and time.monotonic() < deadline:
        try:
            chunk = sock.recv(4096)
        except socket.timeout:
            continue
        if not chunk:
            break
        data.extend(chunk)
    if needle not in data:
        raise AssertionError(f"did not receive {needle!r}; got {bytes(data)!r}")
    return bytes(data)


def test_telnet_fixture() -> None:
    port = free_port()
    process = subprocess.Popen(
        [sys.executable, str(TELNET_SERVER), str(port)],
        cwd=ROOT,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
    )
    try:
        deadline = time.monotonic() + 8.0
        client = None
        while time.monotonic() < deadline:
            try:
                client = socket.create_connection(("127.0.0.1", port), timeout=0.5)
                break
            except OSError:
                if process.poll() is not None:
                    output = process.stdout.read() if process.stdout else ""
                    raise AssertionError(f"Telnet fixture exited early: {output}")
                time.sleep(0.1)
        if client is None:
            raise AssertionError("Telnet fixture did not start")

        with client:
            client.settimeout(0.5)
            recv_until(client, b"login:")
            client.sendall(b"tester\r")
            recv_until(client, b"Password:")
            client.sendall(b"secret\r")
            recv_until(client, b"tester@mock:~$ ")
            client.sendall(b"uname -a\r")
            recv_until(client, b"Linux mock-telnet")
            client.sendall(b"exit\r")
            recv_until(client, "再见".encode("utf-8"))
    finally:
        process.terminate()
        try:
            process.wait(timeout=3)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=3)


def main() -> None:
    test_telnet_fixture()
    print("Protocol fixture self-test passed.")


if __name__ == "__main__":
    main()
