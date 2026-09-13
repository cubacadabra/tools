from __future__ import annotations

import json
import hashlib
import tempfile
import unittest
from pathlib import Path

from cubacadabra.game_builder import build_game


ROOT = Path(__file__).resolve().parents[2]
GUIDE = ROOT / "tools/docs/cubacadabra-game-developer-guide-preview-0.3.md"


class PreviewConformanceTests(unittest.TestCase):
    SUPPORTED_GAME_PROJECTS = (
        ROOT / "first-game",
        ROOT / "second-game",
        ROOT / "third-game",
        ROOT / "examples/adventure-101",
        ROOT / "examples/survival-101",
        ROOT / "examples/the-wild-west",
    )

    def test_all_supported_game_sources_use_and_build_with_the_shared_pipeline(self) -> None:
        for project in self.SUPPORTED_GAME_PROJECTS:
            with self.subTest(project=project.name):
                source = project / "src/main.luau"
                self.assertNotIn("@include", source.read_text(encoding="utf-8"))
                with tempfile.TemporaryDirectory() as directory:
                    output = Path(directory) / project.name
                    result = build_game(
                        source_root=project / "src",
                        manifest_path=project / "manifest.json",
                        output=output,
                    )
                    package = json.loads((output / "package.json").read_text(encoding="utf-8"))
                    self.assertEqual(result.game_id, package["id"])
                    self.assertEqual(set(package["files"]), set(package["sha256"]))
                    for name in package["files"]:
                        self.assertEqual(
                            package["sha256"][name],
                            hashlib.sha256((output / name).read_bytes()).hexdigest(),
                        )

    def test_capability_probe_covers_the_documented_game_api(self) -> None:
        game = ROOT / "third-game"
        source = (game / "src/main.luau").read_text(encoding="utf-8")
        manifest_source = (game / "manifest.json").read_text(encoding="utf-8")
        guide = GUIDE.read_text(encoding="utf-8")

        documented_and_exercised = [
            "function Game.on_start",
            "function Game.on_tick",
            "function Game.on_interaction",
            "function Game.on_network_message",
            "function Game.on_ui_event",
            "function Game.on_launch",
            "api.lobby:set_enabled",
            "api.lobby:set_status",
            "api.session:start",
            "api.ui:set_document",
            "api.ui:clear",
            "api.ui:set_text",
            "api.ui:set_value",
            "api.ui:set_checked",
            "api.ui:set_visible",
            "api.interactions:get_state",
            "api.network:publish",
            "api.network:set_state",
            "api.network:compare_set_state",
            "api.audio:play",
            "api.effects:set_state",
            "api.effects:play",
            "billboards",
            "CubaSharedState.create",
            ":start(api)",
            ":dispatch(api",
            ":receive(api",
            ":update(api",
            "CubaDisclosure.create",
            ":sync(api)",
            ":set_open(api",
            ":handle(api",
        ]

        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "package"
            build_game(
                source_root=game / "src",
                manifest_path=game / "manifest.json",
                output=output,
            )
            generated = (output / "game.luau").read_text(encoding="utf-8")

        for marker in documented_and_exercised:
            self.assertIn(marker, source + generated + manifest_source, marker)
            self.assertIn(marker, guide, marker)

        manifest = json.loads((game / "manifest.json").read_text(encoding="utf-8"))
        self.assertEqual(
            manifest["assets"]["images"]["billboard"]["path"],
            "assets/images/billboard.jpg",
        )
        self.assertEqual(manifest["worlds"]["probe-arena"]["billboards"][0]["image"], "billboard")

    def test_preview_packages_use_one_compatible_version(self) -> None:
        for game_id in ("first-game", "second-game", "third-game"):
            manifest = json.loads(
                (ROOT / game_id / "manifest.json").read_text(encoding="utf-8")
            )
            self.assertEqual(manifest["version"], "0.3.0", game_id)
            self.assertEqual(manifest["sdkVersion"], "0.3.0", game_id)

    def test_game_workspaces_map_the_sdk_alias_to_real_luau_modules(self) -> None:
        for workspace in ("first-game", "second-game", "third-game", "examples"):
            config = json.loads((ROOT / workspace / ".luaurc").read_text())
            sdk_root = (ROOT / workspace / config["aliases"]["cubacadabra"]).resolve()
            self.assertEqual(sdk_root, (ROOT / "tools/src/cubacadabra/sdk").resolve())
            for module in ("shared-state", "disclosure", "survival", "cycle", "obby"):
                self.assertTrue((sdk_root / f"{module}.luau").is_file())


if __name__ == "__main__":
    unittest.main()
