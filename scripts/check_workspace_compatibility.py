#!/usr/bin/env python3
"""Build and load every supported game through the shared platform paths."""

from __future__ import annotations

import subprocess
import sys
import tempfile
from pathlib import Path

TOOLS_ROOT = Path(__file__).resolve().parents[1]
WORKSPACE_ROOT = TOOLS_ROOT.parent
sys.path.insert(0, str(TOOLS_ROOT / "src"))

from cubacadabra.game_builder import build_game  # noqa: E402


PROJECTS = (
    WORKSPACE_ROOT / "first-game",
    WORKSPACE_ROOT / "second-game",
    WORKSPACE_ROOT / "third-game",
    WORKSPACE_ROOT / "examples/adventure-101",
    WORKSPACE_ROOT / "examples/survival-101",
    WORKSPACE_ROOT / "examples/the-wild-west",
)


def main() -> int:
    missing = [path for path in PROJECTS if not (path / "manifest.json").is_file()]
    rust_root = WORKSPACE_ROOT / "rust"
    web_root = WORKSPACE_ROOT / "web"
    browser_bindings = web_root / "public/wasm/renderer/cubacadabra_renderer.js"
    browser_wasm = web_root / "public/wasm/renderer/cubacadabra_renderer_bg.wasm"
    if missing:
        print("Missing supported game projects:", *missing, sep="\n  ", file=sys.stderr)
        return 1
    if not rust_root.is_dir() or not browser_bindings.is_file() or not browser_wasm.is_file():
        print(
            "The compatibility workspace requires rust/ and a built web renderer. "
            "Run rust/scripts/build_web_renderer.sh --debug first.",
            file=sys.stderr,
        )
        return 1

    with tempfile.TemporaryDirectory(prefix="cubacadabra-compatibility-") as directory:
        package_paths = []
        for project in PROJECTS:
            output = Path(directory) / project.name
            build_game(
                source_root=project / "src",
                manifest_path=project / "manifest.json",
                output=output,
            )
            package_paths.append(output)

        native = subprocess.run(
            [
                "cargo", "run", "--quiet", "--manifest-path", str(rust_root / "Cargo.toml"),
                "-p", "cubacadabra-client", "--no-default-features",
                "--bin", "validate_game_packages", "--",
                *(str(path) for path in package_paths),
            ],
            cwd=rust_root,
            check=False,
        )
        if native.returncode:
            return native.returncode

        browser = subprocess.run(
            [
                "node", str(web_root / "scripts/validate_game_packages.mjs"),
                str(browser_bindings), str(browser_wasm),
                *(str(path) for path in package_paths),
            ],
            cwd=web_root,
            check=False,
        )
        return browser.returncode


if __name__ == "__main__":
    raise SystemExit(main())
