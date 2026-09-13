#!/usr/bin/env python3
"""Build and load every supported game through the shared platform paths."""

from __future__ import annotations

import json
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
    studio_root = WORKSPACE_ROOT / "studio"
    web_root = WORKSPACE_ROOT / "web"
    browser_bindings = web_root / "public/wasm/renderer/cubacadabra_renderer.js"
    browser_wasm = web_root / "public/wasm/renderer/cubacadabra_renderer_bg.wasm"
    if missing:
        print("Missing supported game projects:", *missing, sep="\n  ", file=sys.stderr)
        return 1
    if (
        not rust_root.is_dir()
        or not studio_root.is_dir()
        or not browser_bindings.is_file()
        or not browser_wasm.is_file()
    ):
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

        fixture = TOOLS_ROOT / "tests/fixtures/conformance-game"
        fixture_output = Path(directory) / "conformance-game"
        build_game(
            source_root=fixture / "src",
            manifest_path=fixture / "manifest.json",
            output=fixture_output,
        )

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
        if browser.returncode:
            return browser.returncode

        trace_args = [str(fixture_output)]
        native_trace = subprocess.run(
            [
                "cargo", "run", "--quiet", "--manifest-path", str(rust_root / "Cargo.toml"),
                "-p", "cubacadabra-client", "--no-default-features",
                "--bin", "validate_game_packages", "--", "--trace", *trace_args,
            ],
            cwd=rust_root,
            check=False,
            capture_output=True,
            text=True,
        )
        if native_trace.returncode:
            print(native_trace.stdout, end="")
            print(native_trace.stderr, end="", file=sys.stderr)
            return native_trace.returncode

        browser_trace = subprocess.run(
            [
                "node", str(web_root / "scripts/validate_game_packages.mjs"), "--trace",
                str(browser_bindings), str(browser_wasm), *trace_args,
            ],
            cwd=web_root,
            check=False,
            capture_output=True,
            text=True,
        )
        if browser_trace.returncode:
            print(browser_trace.stdout, end="")
            print(browser_trace.stderr, end="", file=sys.stderr)
            return browser_trace.returncode

        def trace_from(result: subprocess.CompletedProcess[str], label: str) -> object:
            for line in result.stdout.splitlines():
                if line.startswith("CONFORMANCE_TRACE "):
                    return json.loads(line.removeprefix("CONFORMANCE_TRACE "))
            raise RuntimeError(f"{label} did not emit a conformance trace")

        native_result = trace_from(native_trace, "native validator")
        browser_result = trace_from(browser_trace, "browser validator")
        if native_result != browser_result:
            print("Native and browser conformance traces differ:", file=sys.stderr)
            print(
                json.dumps(
                    {"native": native_result, "browser": browser_result},
                    indent=2,
                ),
                file=sys.stderr,
            )
            return 1
        print("native and browser behavior trace matched")

        for project in PROJECTS:
            studio = subprocess.run(
                [
                    "cargo", "run", "--quiet", "--manifest-path", str(studio_root / "Cargo.toml"),
                    "--", "--validate-project", str(project),
                ],
                cwd=studio_root,
                check=False,
            )
            if studio.returncode:
                return studio.returncode
        return 0


if __name__ == "__main__":
    raise SystemExit(main())
