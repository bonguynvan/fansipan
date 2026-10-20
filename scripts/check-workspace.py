#!/usr/bin/env python3
"""Fails when a crate under crates/ is not a member of the workspace.

`cargo build/clippy/test --workspace` never touches a crate left out of
the workspace, so the build looks green while that crate is broken
and nothing says so. Run from anywhere: python scripts/check-workspace.py
"""

import json
import os
import subprocess
import sys
from collections.abc import Iterator
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
# Vendored or generated trees that may hold manifests which are not ours.
SKIP = {"node_modules", "target", ".git"}


def crates_on_disk() -> Iterator[Path]:
    for directory, subdirectories, files in os.walk(ROOT / "crates"):
        subdirectories[:] = [d for d in subdirectories if d not in SKIP]
        if "Cargo.toml" in files:
            yield (Path(directory) / "Cargo.toml").resolve()


def workspace_members() -> set[Path]:
    metadata = subprocess.run(
        ["cargo", "metadata", "--no-deps", "--format-version", "1"],
        cwd=ROOT,
        check=True,
        capture_output=True,
        text=True,
    ).stdout
    return {Path(package["manifest_path"]).resolve() for package in json.loads(metadata)["packages"]}


def main() -> int:
    members = workspace_members()
    on_disk = set(crates_on_disk())
    missing = sorted(on_disk - members)
    for manifest in missing:
        print(f"not a workspace member: {manifest.relative_to(ROOT).as_posix()}", file=sys.stderr)
    if missing:
        print("add each to [workspace] members in Cargo.toml", file=sys.stderr)
        return 1
    print(f"all {len(on_disk)} crates under crates/ are workspace members")
    return 0


if __name__ == "__main__":
    sys.exit(main())
