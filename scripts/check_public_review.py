#!/usr/bin/env python3
"""Audit a sibling checkout's public-review hygiene using tracked source only."""

import argparse
from pathlib import Path
import re
import subprocess
import sys
from urllib.parse import unquote

MAX_JSON_BYTES = 4_000_000
DOC_LINK = re.compile(r"https://github.com/cubacadabra/docs/(?:blob|tree)/main/([^\s)\"<>#]+)")
RETIRED_REPOSITORY = re.compile(r"https://github.com/cubacadabra/(?:first-game|second-game|third-game)(?:/|\b)")


def git_files(repo, *args):
    return [Path(value) for value in subprocess.check_output(
        ["git", "-C", str(repo), *args, "-z"]
    ).decode().split("\0") if value]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[2])
    root = parser.parse_args().root.resolve()
    docs = root / "docs"
    errors = []
    checked = 0
    for name in ("first-game", "second-game", "third-game"):
        if (root / name).exists():
            errors.append(f"Retired source checkout still present: {name}; games belong in examples.")
    for repo in sorted(root.iterdir()):
        if repo.name.startswith(".") or not (repo / ".git").exists():
            continue
        checked += 1
        if not (repo / "LICENSE").is_file():
            errors.append(f"{repo.name}: missing root LICENSE")
        tracked = git_files(repo, "ls-files")
        changed = set(git_files(repo, "diff", "--name-only", "--diff-filter=AM", "HEAD"))
        changed.update(git_files(repo, "ls-files", "--others", "--exclude-standard"))
        for filename in sorted(changed):
            path = repo / filename
            if path.is_file() and path.suffix == ".json" and path.stat().st_size > MAX_JSON_BYTES:
                errors.append(f"{repo.name}/{filename}: changed JSON exceeds {MAX_JSON_BYTES} bytes")
        for filename in sorted(set(tracked) | changed):
            path = repo / filename
            if not path.is_file() or path.suffix not in {".md", ".html"}:
                continue
            # Retained source-disposition records describe historical locations.
            if filename.as_posix().startswith("docs/") or filename.as_posix() == "reference/migration-sources.md" or repo.name == "deployed":
                continue
            text = path.read_text()
            for target in DOC_LINK.findall(text):
                if not (docs / unquote(target)).exists():
                    errors.append(f"{repo.name}/{filename}: broken canonical docs link {target}")
            if RETIRED_REPOSITORY.search(text):
                errors.append(f"{repo.name}/{filename}: links to a retired game repository")
        for filename in tracked:
            if filename.name in {".env", "keystore.properties", "upload-key.jks"} or filename.suffix.lower() in {".p12", ".p8", ".jks", ".keystore"}:
                errors.append(f"{repo.name}/{filename}: tracked credential material")
    if not checked:
        errors.append("No sibling Git repositories found.")
    if errors:
        print("Public review checks failed:", file=sys.stderr)
        print("\n".join(f"- {error}" for error in errors), file=sys.stderr)
        return 1
    print(f"Public review checks passed ({checked} repositories; changed JSON, licenses, links, credential filenames).")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
