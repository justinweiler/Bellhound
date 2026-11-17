#!/usr/bin/env python3
"""Exercise packaged simulation fixtures and MCP discovery without network access."""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import subprocess
import sys


EXPECTED_TOOLS = {
    "bellhound_status",
    "bellhound_dial",
    "bellhound_wait",
    "bellhound_speak",
    "bellhound_hangup",
}


def verify_simulation(binary: Path, root: Path) -> None:
    commands = "\n".join(
        json.dumps(
            {
                "command": "speak",
                "text": f"Packaged simulation response {sequence}.",
                "responding_to_sequence": sequence,
            },
            separators=(",", ":"),
        )
        for sequence in (1, 2, 3)
    ) + "\n"
    result = subprocess.run(
        [
            str(binary),
            "--config",
            str(root / "bellhound.example.toml"),
            "simulate",
            "--script",
            str(root / "examples/crocodile-call.jsonl"),
        ],
        input=commands,
        capture_output=True,
        text=True,
        timeout=20,
    )
    if result.returncode:
        raise RuntimeError(f"packaged simulation failed with exit {result.returncode}")
    events = [json.loads(line) for line in result.stdout.splitlines() if line.strip()]
    utterances = [event.get("sequence") for event in events if event.get("event") == "utterance"]
    spoken = [event.get("responding_to_sequence") for event in events if event.get("event") == "spoken"]
    if utterances != [1, 2, 3] or spoken != [1, 2, 3]:
        raise RuntimeError("packaged simulation did not complete all three turns")
    if not any(
        event.get("event") == "ended" and event.get("reason") == "simulation_complete"
        for event in events
    ):
        raise RuntimeError("packaged simulation did not emit terminal completion")


def mcp_request(process: subprocess.Popen[str], payload: dict[str, object]) -> dict[str, object]:
    assert process.stdin is not None and process.stdout is not None
    process.stdin.write(json.dumps(payload, separators=(",", ":")) + "\n")
    process.stdin.flush()
    line = process.stdout.readline()
    if not line:
        raise RuntimeError("packaged MCP process closed before responding")
    return json.loads(line)


def verify_mcp_discovery(binary: Path, root: Path) -> None:
    env = os.environ.copy()
    env.pop("BELLHOUND_MCP_STATUS_ONLY_RELAY_TOKEN", None)
    env.pop("BELLHOUND_MCP_STATUS_ONLY_TELNYX_KEY", None)
    env["RUST_LOG"] = "error"
    process = subprocess.Popen(
        [
            str(binary),
            "--config",
            str(root / "examples/mcp-status-only.toml"),
            "mcp",
        ],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        env=env,
    )
    try:
        initialized = mcp_request(
            process,
            {
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {
                    "protocolVersion": "2025-11-25",
                    "capabilities": {},
                    "clientInfo": {"name": "bellhound-package-check", "version": "1.0.0"},
                },
            },
        )
        if initialized.get("result", {}).get("serverInfo", {}).get("name") != "bellhound":
            raise RuntimeError("packaged MCP initialize returned unexpected server identity")
        assert process.stdin is not None
        process.stdin.write(
            json.dumps(
                {"jsonrpc": "2.0", "method": "notifications/initialized"},
                separators=(",", ":"),
            )
            + "\n"
        )
        process.stdin.flush()
        listed = mcp_request(
            process,
            {"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}},
        )
        tools = {item.get("name") for item in listed.get("result", {}).get("tools", [])}
        if tools != EXPECTED_TOOLS:
            raise RuntimeError(f"packaged MCP tool set mismatch: {sorted(tools)}")
    finally:
        if process.stdin is not None:
            process.stdin.close()
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.terminate()
            try:
                process.wait(timeout=2)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=2)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--root", type=Path, required=True)
    args = parser.parse_args()
    binary = args.binary.resolve()
    root = args.root.resolve()
    verify_simulation(binary, root)
    verify_mcp_discovery(binary, root)
    print("verified packaged simulation and MCP discovery")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
