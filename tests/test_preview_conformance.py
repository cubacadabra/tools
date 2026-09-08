from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path

from cubacadabra.game_builder import build_game


ROOT = Path(__file__).resolve().parents[2]
GUIDE = ROOT / "tools/docs/cubacadabra-game-developer-guide-preview-0.3.md"


class PreviewConformanceTests(unittest.TestCase):
    def test_capability_probe_covers_the_documented_game_api(self) -> None:
        game = ROOT / "third-game"
        source = (game / "src/main.luau").read_text(encoding="utf-8")
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
            self.assertIn(marker, source + generated, marker)
            self.assertIn(marker, guide, marker)

    def test_preview_packages_use_one_compatible_version(self) -> None:
        for game_id in ("first-game", "second-game", "third-game"):
            manifest = json.loads(
                (ROOT / game_id / "manifest.json").read_text(encoding="utf-8")
            )
            self.assertEqual(manifest["version"], "0.3.0", game_id)
            self.assertEqual(manifest["sdkVersion"], "0.3.0", game_id)


if __name__ == "__main__":
    unittest.main()
