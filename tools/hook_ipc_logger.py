#!/usr/bin/env python3
r"""Simple Windows-first hook IPC logger for xi.

Default endpoint: \\.\pipe\xi-hook-events
"""

from __future__ import annotations

import argparse
import json
import os
import sys
import time

DEFAULT_ENDPOINT = r"\\.\pipe\xi-hook-events"


def log(msg: str) -> None:
    print(msg, flush=True)


def run_windows(endpoint: str) -> int:
    try:
        import pywintypes  # type: ignore
        import win32file  # type: ignore
        import win32pipe  # type: ignore
    except ImportError:
        log("pywin32 is required on Windows: pip install pywin32")
        return 2

    log(f"listening on {endpoint}")
    while True:
        pipe = win32pipe.CreateNamedPipe(
            endpoint,
            win32pipe.PIPE_ACCESS_INBOUND,
            win32pipe.PIPE_TYPE_BYTE | win32pipe.PIPE_READMODE_BYTE | win32pipe.PIPE_WAIT,
            1,
            65536,
            65536,
            0,
            None,
        )
        try:
            win32pipe.ConnectNamedPipe(pipe, None)
            log("client connected")
            buf = b""
            while True:
                try:
                    _hr, chunk = win32file.ReadFile(pipe, 4096)
                except pywintypes.error:
                    break
                if not chunk:
                    break
                buf += chunk
                while b"\n" in buf:
                    line, buf = buf.split(b"\n", 1)
                    if not line.strip():
                        continue
                    text = line.decode("utf-8", errors="replace")
                    try:
                        event = json.loads(text)
                        point = event.get("point", "?")
                        seq = event.get("seq", "?")
                        session_id = event.get("session_id", "?")
                        tool = event.get("tool")
                        if tool:
                            log(f"[{seq}] {point} session={session_id} tool={tool} {text}")
                        else:
                            log(f"[{seq}] {point} session={session_id} {text}")
                    except json.JSONDecodeError:
                        log(text)
        finally:
            try:
                win32file.CloseHandle(pipe)
            except Exception:
                pass
            log("client disconnected")


def run_unix(endpoint: str) -> int:
    import socket

    if os.path.exists(endpoint):
        os.unlink(endpoint)

    server = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    server.bind(endpoint)
    server.listen(1)
    log(f"listening on {endpoint}")
    try:
        while True:
            conn, _addr = server.accept()
            log("client connected")
            with conn:
                buf = b""
                while True:
                    chunk = conn.recv(4096)
                    if not chunk:
                        break
                    buf += chunk
                    while b"\n" in buf:
                        line, buf = buf.split(b"\n", 1)
                        if line.strip():
                            log(line.decode("utf-8", errors="replace"))
            log("client disconnected")
    finally:
        server.close()
        try:
            os.unlink(endpoint)
        except FileNotFoundError:
            pass


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("endpoint", nargs="?", default=DEFAULT_ENDPOINT)
    args = parser.parse_args()

    if os.name == "nt":
        return run_windows(args.endpoint)
    return run_unix(args.endpoint)


if __name__ == "__main__":
    raise SystemExit(main())
