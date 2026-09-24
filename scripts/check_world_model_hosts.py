#!/usr/bin/env python3
"""Check that every native and browser host exposes world-model registration.

This is intentionally a source-level contract check. The package schema is
shared by the builder and runtime, but each host still has a small adapter that
loads bytes and calls the shared renderer. Keeping those adapters in this
check makes a new host integration fail loudly instead of silently omitting a
declared ``assets.models`` entry.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path


WORKSPACE_ROOT = Path(__file__).resolve().parents[2]


REQUIREMENTS = {
    "rust renderer API": (
        WORKSPACE_ROOT / "rust/src/renderer/device.rs",
        r"register_world_mesh",
    ),
    "desktop registration": (
        WORKSPACE_ROOT / "desktop/src/app.rs",
        r"register_world_mesh",
    ),
    "web package loading": (
        WORKSPACE_ROOT / "web/src/game/loadGamePackage.js",
        r"assets\.models|worldModels",
    ),
    "web registration": (
        WORKSPACE_ROOT / "web/src/app/createGame.js",
        r"registerWorldMesh",
    ),
    "studio registration": (
        WORKSPACE_ROOT / "studio/src/app.rs",
        r"register_world_mesh",
    ),
    "iOS package loading": (
        WORKSPACE_ROOT / "ios_app/cubacadabra/GamePackage.swift",
        r"loadWorldModels",
    ),
    "iOS registration": (
        WORKSPACE_ROOT / "ios_app/cubacadabra/EngineBridge.swift",
        r"engine_renderer_register_world_mesh",
    ),
    "iOS C bridge declaration": (
        WORKSPACE_ROOT / "ios_app/cubacadabra/cubacadabra_engine.h",
        r"engine_renderer_register_world_mesh",
    ),
    "Android package loading": (
        WORKSPACE_ROOT
        / "android_app/app/src/main/java/dev/andrewarrow/cubacadabra/game/GamePackageLoader.kt",
        r"loadWorldModels",
    ),
    "Android registration": (
        WORKSPACE_ROOT
        / "android_app/app/src/main/java/dev/andrewarrow/cubacadabra/game/GameViewModel.kt",
        r"nativeRegisterWorldMesh",
    ),
    "Android JNI bridge": (
        WORKSPACE_ROOT / "android_app/app/src/main/cpp/jni_bridge.c",
        r"nativeRegisterWorldMesh|engine_renderer_register_world_mesh",
    ),
}


def main() -> int:
    failures: list[str] = []
    for label, (path, pattern) in REQUIREMENTS.items():
        if not path.is_file():
            failures.append(f"{label}: missing {path.relative_to(WORKSPACE_ROOT)}")
            continue
        if not re.search(pattern, path.read_text(encoding="utf-8")):
            failures.append(
                f"{label}: {pattern!r} not found in {path.relative_to(WORKSPACE_ROOT)}"
            )

    if failures:
        print("World-model host contract failed:", file=sys.stderr)
        for failure in failures:
            print(f"  - {failure}", file=sys.stderr)
        return 1

    print(f"World-model host contract passed ({len(REQUIREMENTS)} checks).")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
