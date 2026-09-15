from __future__ import annotations

import hashlib
import json
import tempfile
import unittest
import wave
import zipfile
from pathlib import Path

from cubacadabra.cli import main
from cubacadabra.game_builder import GameBuildError, build_game
from cubacadabra.game_creator import GameCreateError, create_game


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
            'local document = require("./ui/document")\n'
            'return { document = document }\n',
            encoding="utf-8",
        )
        (self.project / "src/ui/document.luau").write_text(
            "local document = {}\nreturn document\n", encoding="utf-8"
        )
        (self.project / "assets/logo.txt").write_text("asset", encoding="utf-8")

    def tearDown(self) -> None:
        self.temp_dir.cleanup()

    def test_create_game_writes_standard_starter_layout(self) -> None:
        games = self.project / "games"
        result = create_game(title="The Wild West", path=games)

        self.assertEqual(result.game_id, "the-wild-west")
        self.assertEqual(result.project, (games / "the-wild-west").resolve())
        manifest = json.loads((result.project / "manifest.json").read_text())
        self.assertEqual(manifest["id"], "the-wild-west")
        self.assertEqual(manifest["displayName"], "The Wild West")
        self.assertEqual(manifest["version"], "0.3.0")
        self.assertEqual(manifest["sdkVersion"], "0.3.0")
        self.assertEqual(manifest["package"], {"formatVersion": 3, "entry": "game.luau"})
        self.assertEqual(manifest["palette"]["signal"], "#57E5D0")
        self.assertTrue((result.project / "assets").is_dir())
        self.assertTrue((result.project / "assets/audio").is_dir())
        self.assertTrue((result.project / "assets/images").is_dir())
        editor_config = json.loads((result.project / ".luaurc").read_text())
        self.assertEqual(editor_config, {"aliases": {"cubacadabra": ".cubacadabra/sdk"}})
        self.assertTrue((result.project / ".cubacadabra/sdk/shared-state.luau").is_file())
        source = (result.project / "src/main.luau").read_text()
        self.assertIn("api.session:start(\"the-wild-west\"", source)
        for control in (
            'id = "player-joystick"',
            'kind = "joystick"',
            'id = "player-jump"',
            'action = "player.jump"',
            'id = "player-run"',
            'action = "player.run"',
        ):
            self.assertIn(control, source)

    def test_create_game_refuses_to_overwrite(self) -> None:
        games = self.project / "games"
        create_game(title="The Wild West", path=games)

        with self.assertRaises(GameCreateError):
            create_game(title="The Wild West", path=games)

    def test_create_game_rejects_a_title_that_produces_a_short_id(self) -> None:
        with self.assertRaisesRegex(GameCreateError, "between 3 and 64"):
            create_game(title="A", path=self.project / "games")

    def test_create_game_cli_flag(self) -> None:
        games = self.project / "games"

        self.assertEqual(
            main(["--create-game", "--title", "The Wild West", "--path", str(games)]),
            0,
        )
        self.assertTrue((games / "the-wild-west" / "manifest.json").exists())

    def test_created_game_builds_from_its_project_directory(self) -> None:
        games = self.project / "games"
        result = create_game(title="The Wild West", path=games)
        output = self.project / "build/package"

        build_game(
            source_root=result.project / "src",
            manifest_path=result.project / "manifest.json",
            output=output,
        )

        self.assertTrue((output / "game.luau").exists())
        self.assertEqual(
            json.loads((output / "manifest.json").read_text())["id"],
            "the-wild-west",
        )

    def test_build_cli_accepts_an_absolute_game_project_directory(self) -> None:
        games = self.project / "games"
        result = create_game(title="The Wild West", path=games)
        output = self.project / "build/package"

        self.assertEqual(
            main(
                [
                    "build-game",
                    "--source",
                    str(result.project),
                    "--output",
                    str(output),
                ]
            ),
            0,
        )

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
        script = (output / "game.luau").read_text()
        self.assertIn("begin module: ui/document.luau", script)
        self.assertIn('["./ui/document"] = "ui/document.luau"', script)
        self.assertIn('return __require("main.luau")', script)
        built_manifest = json.loads((output / "manifest.json").read_text())
        self.assertEqual(
            built_manifest["package"], {"formatVersion": 3, "entry": "game.luau"}
        )
        self.assertEqual(built_manifest["displayName"], "test-game")
        self.assertTrue((output / "assets/logo.txt").exists())
        package = json.loads((output / "package.json").read_text())
        self.assertEqual(package["files"], ["assets/logo.txt", "game.luau", "manifest.json"])
        for name in package["files"]:
            digest = hashlib.sha256((output / name).read_bytes()).hexdigest()
            self.assertEqual(package["sha256"][name], digest)
        with zipfile.ZipFile(archive) as zip_file:
            self.assertEqual(sorted(zip_file.namelist()), [
                "assets/logo.txt",
                "game.luau",
                "manifest.json",
                "package.json",
            ])

    def test_build_ignores_macos_directory_metadata(self) -> None:
        (self.project / "assets/.DS_Store").write_bytes(b"Finder metadata")
        nested_assets = self.project / "assets/images"
        nested_assets.mkdir()
        (nested_assets / ".DS_Store").write_bytes(b"nested Finder metadata")
        output = self.project / "build/package"

        build_game(
            source_root=self.project / "src",
            manifest_path=self.project / "manifest.json",
            output=output,
        )

        package = json.loads((output / "package.json").read_text())
        self.assertNotIn("assets/.DS_Store", package["files"])
        self.assertNotIn("assets/images/.DS_Store", package["files"])
        self.assertFalse((output / "assets/.DS_Store").exists())
        self.assertFalse((output / "assets/images/.DS_Store").exists())

    def test_bundles_a_cubacadabra_sdk_module(self) -> None:
        (self.project / "src/main.luau").write_text(
            'local CubaSharedState = require("@cubacadabra/shared-state")\n'
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
            "begin module: @cubacadabra/shared-state",
            script,
        )
        self.assertIn("local CubaSharedState = {}", script)
        self.assertIn("return CubaSharedState", script)

    def test_builder_uses_the_project_sdk_alias(self) -> None:
        sdk_root = self.project / ".cubacadabra/sdk"
        sdk_root.mkdir(parents=True)
        (self.project / ".luaurc").write_text(
            json.dumps({"aliases": {"cubacadabra": ".cubacadabra/sdk"}}),
            encoding="utf-8",
        )
        (self.project / "src/main.luau").write_text(
            'return require("@cubacadabra/disclosure")\n',
            encoding="utf-8",
        )
        (sdk_root / "disclosure.luau").write_text(
            "local ProjectSdk = {}\nreturn ProjectSdk\n",
            encoding="utf-8",
        )

        output = self.project / "build/package"
        build_game(
            source_root=self.project / "src",
            manifest_path=self.project / "manifest.json",
            output=output,
        )

        self.assertIn("local ProjectSdk = {}", (output / "game.luau").read_text())

    def test_builder_rejects_a_missing_configured_project_sdk(self) -> None:
        (self.project / ".luaurc").write_text(
            json.dumps({"aliases": {"cubacadabra": ".cubacadabra/sdk"}}),
            encoding="utf-8",
        )
        (self.project / "src/main.luau").write_text(
            'return require("@cubacadabra/disclosure")\n',
            encoding="utf-8",
        )

        with self.assertRaisesRegex(GameBuildError, "does not point to an SDK directory"):
            build_game(
                source_root=self.project / "src",
                manifest_path=self.project / "manifest.json",
                output=self.project / "build/package",
            )

    def test_bundles_the_disclosure_sdk_module(self) -> None:
        (self.project / "src/main.luau").write_text(
            'local CubaDisclosure = require("@cubacadabra/disclosure")\n'
            'return { disclosure = CubaDisclosure }\n',
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
            "begin module: @cubacadabra/disclosure",
            script,
        )
        self.assertIn("local CubaDisclosure = {}", script)

    def test_bundles_the_survival_sdk_module(self) -> None:
        (self.project / "src/main.luau").write_text(
            'local CubaSurvival = require("@cubacadabra/survival")\n'
            'return { survival = CubaSurvival }\n',
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
            "begin module: @cubacadabra/survival",
            script,
        )
        self.assertIn("local CubaSurvival = {}", script)

    def test_rejects_an_unknown_cubacadabra_sdk_module(self) -> None:
        (self.project / "src/main.luau").write_text(
            'local missing = require("@cubacadabra/missing")\nreturn missing\n',
            encoding="utf-8",
        )

        with self.assertRaisesRegex(GameBuildError, "unknown Cubacadabra SDK module"):
            build_game(
                source_root=self.project / "src",
                manifest_path=self.project / "manifest.json",
                output=self.project / "build/package",
            )

    def test_rejects_an_upload_invalid_cube_id(self) -> None:
        (self.project / "manifest.json").write_text(
            json.dumps({"id": "Test Game", "version": 3}), encoding="utf-8"
        )

        with self.assertRaisesRegex(GameBuildError, "manifest.id"):
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

    def test_rejects_require_traversal(self) -> None:
        (self.project / "src/main.luau").write_text(
            'local outside = require("../../outside")\nreturn outside\n',
            encoding="utf-8",
        )

        with self.assertRaisesRegex(GameBuildError, "require must stay inside src"):
            build_game(
                source_root=self.project / "src",
                manifest_path=self.project / "manifest.json",
                output=self.project / "build/package",
            )

    def test_rejects_cyclic_require(self) -> None:
        (self.project / "src/main.luau").write_text(
            'local document = require("./ui/document")\nreturn document\n',
            encoding="utf-8",
        )
        (self.project / "src/ui/document.luau").write_text(
            'local main = require("../main")\nreturn main\n', encoding="utf-8"
        )

        with self.assertRaisesRegex(GameBuildError, "cyclic Luau require"):
            build_game(
                source_root=self.project / "src",
                manifest_path=self.project / "manifest.json",
                output=self.project / "build/package",
            )

    def test_resolves_transitive_requires_relative_to_each_module(self) -> None:
        (self.project / "src/ui/document.luau").write_text(
            'local styles = require("./styles")\nreturn { styles = styles }\n',
            encoding="utf-8",
        )
        (self.project / "src/ui/styles.luau").write_text(
            'return { accent = "#57E5D0" }\n', encoding="utf-8"
        )

        output = self.project / "build/package"
        build_game(
            source_root=self.project / "src",
            manifest_path=self.project / "manifest.json",
            output=output,
        )

        script = (output / "game.luau").read_text()
        self.assertIn("begin module: ui/styles.luau", script)
        self.assertIn('["./styles"] = "ui/styles.luau"', script)

    def test_ignores_require_text_inside_strings_and_comments(self) -> None:
        (self.project / "src/main.luau").write_text(
            '-- require("./missing-line")\n'
            '--[[ require("./missing-block") ]]\n'
            'local example = "require(\\"./missing-string\\")"\n'
            'return { example = example }\n',
            encoding="utf-8",
        )

        build_game(
            source_root=self.project / "src",
            manifest_path=self.project / "manifest.json",
            output=self.project / "build/package",
        )

    def test_rejects_dynamic_require_paths(self) -> None:
        (self.project / "src/main.luau").write_text(
            'local path = "./ui/document"\nreturn require(path)\n',
            encoding="utf-8",
        )

        with self.assertRaisesRegex(GameBuildError, "static quoted strings"):
            build_game(
                source_root=self.project / "src",
                manifest_path=self.project / "manifest.json",
                output=self.project / "build/package",
            )

    def test_rejects_ambiguous_luau_module_paths(self) -> None:
        (self.project / "src/ui/document.lua").write_text(
            "return {}\n", encoding="utf-8"
        )

        with self.assertRaisesRegex(GameBuildError, "required module is ambiguous"):
            build_game(
                source_root=self.project / "src",
                manifest_path=self.project / "manifest.json",
                output=self.project / "build/package",
            )

    def test_rejects_legacy_include_with_migration_guidance(self) -> None:
        (self.project / "src/main.luau").write_text(
            '-- @include "ui/document.luau"\nreturn {}\n', encoding="utf-8"
        )

        with self.assertRaisesRegex(GameBuildError, "@include is no longer supported"):
            build_game(
                source_root=self.project / "src",
                manifest_path=self.project / "manifest.json",
                output=self.project / "build/package",
            )

    def test_rejects_output_inside_source(self) -> None:
        with self.assertRaisesRegex(GameBuildError, "cannot overlap"):
            build_game(
                source_root=self.project / "src",
                manifest_path=self.project / "manifest.json",
                output=self.project / "src/output",
            )

    def test_rejects_output_containing_source(self) -> None:
        with self.assertRaisesRegex(GameBuildError, "cannot overlap"):
            build_game(
                source_root=self.project / "src",
                manifest_path=self.project / "manifest.json",
                output=self.project,
            )

    def test_refuses_to_replace_an_unowned_output_directory(self) -> None:
        output = self.project / "build/package"
        output.mkdir(parents=True)
        sentinel = output / "do-not-delete.txt"
        sentinel.write_text("keep me", encoding="utf-8")

        with self.assertRaisesRegex(GameBuildError, "without a Cubacadabra build marker"):
            build_game(
                source_root=self.project / "src",
                manifest_path=self.project / "manifest.json",
                output=output,
            )

        self.assertEqual(sentinel.read_text(encoding="utf-8"), "keep me")

    def test_failed_build_preserves_the_previous_package(self) -> None:
        output = self.project / "build/package"
        build_game(
            source_root=self.project / "src",
            manifest_path=self.project / "manifest.json",
            output=output,
        )
        previous_script = (output / "game.luau").read_text(encoding="utf-8")

        (self.project / "src/main.luau").write_text(
            'return require("./missing")\n', encoding="utf-8"
        )
        with self.assertRaisesRegex(GameBuildError, "required module not found"):
            build_game(
                source_root=self.project / "src",
                manifest_path=self.project / "manifest.json",
                output=output,
            )

        self.assertEqual((output / "game.luau").read_text(encoding="utf-8"), previous_script)

    def test_replaces_a_previous_package_with_a_builder_marker(self) -> None:
        output = self.project / "build/package"
        build_game(
            source_root=self.project / "src",
            manifest_path=self.project / "manifest.json",
            output=output,
        )
        (self.project / "src/main.luau").write_text(
            'return { rebuilt = true }\n', encoding="utf-8"
        )

        build_game(
            source_root=self.project / "src",
            manifest_path=self.project / "manifest.json",
            output=output,
        )

        self.assertIn("rebuilt = true", (output / "game.luau").read_text(encoding="utf-8"))
        self.assertTrue((output / ".cubacadabra-build").exists())

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

    def test_accepts_the_frozen_preview_sdk_version(self) -> None:
        (self.project / "manifest.json").write_text(
            json.dumps({
                "id": "test-game",
                "version": "0.3.0",
                "sdkVersion": "0.3.0",
            }),
            encoding="utf-8",
        )

        result = build_game(
            source_root=self.project / "src",
            manifest_path=self.project / "manifest.json",
            output=self.project / "build/package",
        )

        self.assertEqual(result.version, "0.3.0")

    def test_rejects_an_unsupported_sdk_version(self) -> None:
        (self.project / "manifest.json").write_text(
            json.dumps({
                "id": "test-game",
                "version": "0.3.0",
                "sdkVersion": "0.4.0",
            }),
            encoding="utf-8",
        )

        with self.assertRaisesRegex(GameBuildError, "sdkVersion.*unsupported"):
            build_game(
                source_root=self.project / "src",
                manifest_path=self.project / "manifest.json",
                output=self.project / "build/package",
            )

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

    def test_copies_declared_image_assets(self) -> None:
        image = self.project / "assets/images/billboard.jpg"
        image.parent.mkdir()
        image.write_bytes(b"jpeg fixture")
        (self.project / "manifest.json").write_text(
            json.dumps({
                "id": "test-game",
                "version": 3,
                "assets": {
                    "images": {
                        "billboard": {"path": "assets/images/billboard.jpg"},
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

        self.assertEqual((output / "assets/images/billboard.jpg").read_bytes(), image.read_bytes())

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
