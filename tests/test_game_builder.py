from __future__ import annotations

import json
import tempfile
import unittest
import wave
import zipfile
from pathlib import Path

from cubacadabra.game_builder import GameBuildError, build_game


class GameBuilderTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temp_dir = tempfile.TemporaryDirectory()
        self.project = Path(self.temp_dir.name)
        (self.project / "src/ui").mkdir(parents=True)
        (self.project / "assets").mkdir()
        (self.project / "manifest.json").write_text(
            json.dumps({"id": "test-game", "version": 3}), encoding="utf-8"
        )
        (self.project / "src/main.luau").write_text(
            '-- @include "ui/document.luau"\nreturn {}\n', encoding="utf-8"
        )
        (self.project / "src/ui/document.luau").write_text(
            "local document = {}\n", encoding="utf-8"
        )
        (self.project / "assets/logo.txt").write_text("asset", encoding="utf-8")

    def tearDown(self) -> None:
        self.temp_dir.cleanup()

    def test_builds_package_and_zip(self) -> None:
        output = self.project / "build/package"
        archive = self.project / "build/test-game.zip"

        result = build_game(
            source_root=self.project / "src",
            manifest_path=self.project / "manifest.json",
            output=output,
            zip_path=archive,
        )

        self.assertEqual(result.game_id, "test-game")
        self.assertIn("begin include: ui/document.luau", (output / "game.luau").read_text())
        self.assertTrue((output / "assets/logo.txt").exists())
        package = json.loads((output / "package.json").read_text())
        self.assertEqual(package["files"], ["assets/logo.txt", "game.luau", "manifest.json"])
        with zipfile.ZipFile(archive) as zip_file:
            self.assertEqual(sorted(zip_file.namelist()), [
                "assets/logo.txt",
                "game.luau",
                "manifest.json",
                "package.json",
            ])

    def test_expands_a_versioned_cubacadabra_sdk_include(self) -> None:
        (self.project / "src/main.luau").write_text(
            '-- @include "@cubacadabra/shared-state-v1.luau"\n'
            'return { shared = CubaSharedState }\n',
            encoding="utf-8",
        )

        output = self.project / "build/package"
        build_game(
            source_root=self.project / "src",
            manifest_path=self.project / "manifest.json",
            output=output,
        )

        script = (output / "game.luau").read_text()
        self.assertIn(
            "begin SDK include: @cubacadabra/shared-state-v1.luau",
            script,
        )
        self.assertIn("local CubaSharedState = {}", script)

    def test_rejects_an_unknown_cubacadabra_sdk_include(self) -> None:
        (self.project / "src/main.luau").write_text(
            '-- @include "@cubacadabra/missing.luau"\nreturn {}\n',
            encoding="utf-8",
        )

        with self.assertRaisesRegex(GameBuildError, "unknown Cubacadabra SDK include"):
            build_game(
                source_root=self.project / "src",
                manifest_path=self.project / "manifest.json",
                output=self.project / "build/package",
            )

    def test_inlines_a_game_owned_effects_source(self) -> None:
        (self.project / "effects.json").write_text(
            json.dumps({
                "version": 1,
                "templates": {
                    "pulse": {
                        "nodes": [{"shape": "ring", "color": "accent"}],
                    },
                },
            }),
            encoding="utf-8",
        )
        (self.project / "manifest.json").write_text(
            json.dumps({
                "id": "test-game",
                "version": 3,
                "effects": {"source": "effects.json"},
            }),
            encoding="utf-8",
        )

        output = self.project / "build/package"
        build_game(
            source_root=self.project / "src",
            manifest_path=self.project / "manifest.json",
            output=output,
        )

        manifest = json.loads((output / "manifest.json").read_text())
        self.assertEqual(manifest["effects"]["version"], 1)
        self.assertEqual(
            manifest["effects"]["templates"]["pulse"]["nodes"][0]["shape"],
            "ring",
        )
        self.assertNotIn("source", manifest["effects"])

    def test_rejects_effects_source_traversal(self) -> None:
        (self.project / "manifest.json").write_text(
            json.dumps({
                "id": "test-game",
                "version": 3,
                "effects": {"source": "../effects.json"},
            }),
            encoding="utf-8",
        )

        with self.assertRaisesRegex(GameBuildError, "relative JSON path"):
            build_game(
                source_root=self.project / "src",
                manifest_path=self.project / "manifest.json",
                output=self.project / "build/package",
            )

    def test_rejects_include_traversal(self) -> None:
        (self.project / "src/ui/document.luau").write_text(
            '-- @include "../main.luau"\n', encoding="utf-8"
        )

        with self.assertRaisesRegex(GameBuildError, "include must stay inside src"):
            build_game(
                source_root=self.project / "src",
                manifest_path=self.project / "manifest.json",
                output=self.project / "build/package",
            )

    def test_rejects_cyclic_include(self) -> None:
        (self.project / "src/main.luau").write_text(
            '-- @include "ui/document.luau"\n', encoding="utf-8"
        )
        (self.project / "src/ui/document.luau").write_text(
            '-- @include "ui/document.luau"\n', encoding="utf-8"
        )

        with self.assertRaisesRegex(GameBuildError, "cyclic Luau include"):
            build_game(
                source_root=self.project / "src",
                manifest_path=self.project / "manifest.json",
                output=self.project / "build/package",
            )

    def test_rejects_output_inside_source(self) -> None:
        with self.assertRaisesRegex(GameBuildError, "cannot be inside"):
            build_game(
                source_root=self.project / "src",
                manifest_path=self.project / "manifest.json",
                output=self.project / "src/output",
            )

    def test_accepts_a_semantic_release_version(self) -> None:
        (self.project / "manifest.json").write_text(
            json.dumps({"id": "test-game", "version": "0.0.1"}),
            encoding="utf-8",
        )

        result = build_game(
            source_root=self.project / "src",
            manifest_path=self.project / "manifest.json",
            output=self.project / "build/package",
        )

        self.assertEqual(result.version, "0.0.1")

    def test_copies_declared_audio_assets(self) -> None:
        audio = self.project / "assets/audio/chime.wav"
        audio.parent.mkdir()
        with wave.open(str(audio), "wb") as wav:
            wav.setnchannels(1)
            wav.setsampwidth(2)
            wav.setframerate(48_000)
            wav.writeframes(b"\0\0" * 48)
        (self.project / "manifest.json").write_text(
            json.dumps({
                "id": "test-game",
                "version": 3,
                "assets": {
                    "audio": {
                        "chime": {"path": "assets/audio/chime.wav", "volume": 0.8},
                    },
                },
            }),
            encoding="utf-8",
        )

        output = self.project / "build/package"
        build_game(
            source_root=self.project / "src",
            manifest_path=self.project / "manifest.json",
            output=output,
        )

        self.assertEqual((output / "assets/audio/chime.wav").read_bytes(), audio.read_bytes())

    def test_rejects_audio_asset_outside_assets(self) -> None:
        (self.project / "manifest.json").write_text(
            json.dumps({
                "id": "test-game",
                "version": 3,
                "assets": {
                    "audio": {
                        "chime": {"path": "../chime.wav"},
                    },
                },
            }),
            encoding="utf-8",
        )

        with self.assertRaisesRegex(GameBuildError, "must stay inside assets"):
            build_game(
                source_root=self.project / "src",
                manifest_path=self.project / "manifest.json",
                output=self.project / "build/package",
            )

    def test_rejects_missing_audio_asset(self) -> None:
        (self.project / "manifest.json").write_text(
            json.dumps({
                "id": "test-game",
                "version": 3,
                "assets": {
                    "audio": {
                        "chime": {"path": "assets/audio/missing.wav"},
                    },
                },
            }),
            encoding="utf-8",
        )

        with self.assertRaisesRegex(GameBuildError, "was not found"):
            build_game(
                source_root=self.project / "src",
                manifest_path=self.project / "manifest.json",
                output=self.project / "build/package",
            )
