#!/usr/bin/env python3
"""Validate Bellhound's committed MCP registry descriptor without network access."""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path


SERVER_NAME = "io.github.justinweiler/bellhound"
REPOSITORY_URL = "https://github.com/justinweiler/Bellhound"


def fail(message: str) -> None:
    raise ValueError(message)


def package_version(root: Path, expected: str | None) -> str:
    manifest = root / "Cargo.toml"
    if manifest.exists():
        try:
            import tomllib
        except ModuleNotFoundError as error:  # pragma: no cover - Python < 3.11
            raise RuntimeError("Python 3.11 or newer is required to parse Cargo.toml") from error
        with manifest.open("rb") as handle:
            version = tomllib.load(handle)["package"]["version"]
        if expected is not None and version != expected:
            fail(f"Cargo.toml version {version!r} does not match expected {expected!r}")
        return version
    if expected is None:
        fail("expected version is required when Cargo.toml is not packaged")
    return expected


def validate(root: Path, expected_version: str | None) -> None:
    version = package_version(root, expected_version)
    descriptor = json.loads((root / "server.json").read_text(encoding="utf-8"))

    if descriptor.get("name") != SERVER_NAME:
        fail("unexpected MCP server name")
    if descriptor.get("version") != version:
        fail("server.json version does not match the Bellhound package version")
    if len(descriptor.get("description", "")) > 100:
        fail("MCP description exceeds the registry's 100-character limit")
    repository = descriptor.get("repository", {})
    if repository.get("url") != REPOSITORY_URL or repository.get("source") != "github":
        fail("unexpected repository metadata")

    packages = descriptor.get("packages", [])
    if len(packages) != 1:
        fail("Bellhound must publish exactly one registry package")
    package = packages[0]
    expected_package = {
        "registryType": "cargo",
        "registryBaseUrl": "https://crates.io",
        "identifier": "bellhound",
        "version": version,
        "transport": {"type": "stdio"},
    }
    for key, value in expected_package.items():
        if package.get(key) != value:
            fail(f"unexpected package field {key!r}")

    arguments = package.get("packageArguments", [])
    if len(arguments) != 2:
        fail("Bellhound MCP launch requires the config argument and mcp subcommand")
    config_argument, subcommand = arguments
    variables = config_argument.get("variables", {})
    config_path = variables.get("config_path", {})
    if (
        config_argument.get("type") != "named"
        or config_argument.get("name") != "--config"
        or config_argument.get("value") != "{config_path}"
        or config_path.get("format") != "filepath"
        or config_path.get("isRequired") is not True
    ):
        fail("registry config-path argument does not match Bellhound's CLI contract")
    if subcommand != {"type": "positional", "value": "mcp"}:
        fail("registry subcommand does not launch Bellhound MCP")

    readme = (root / "README.md").read_text(encoding="utf-8")
    if f"mcp-name: {SERVER_NAME}" not in readme:
        fail("README.md is missing the visible Cargo ownership token")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, default=Path("."))
    parser.add_argument("--expected-version")
    args = parser.parse_args()
    try:
        validate(args.root.resolve(), args.expected_version)
    except (KeyError, OSError, TypeError, ValueError, json.JSONDecodeError) as error:
        print(f"registry metadata invalid: {error}", file=sys.stderr)
        return 1
    print(f"registry metadata valid: {SERVER_NAME}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
