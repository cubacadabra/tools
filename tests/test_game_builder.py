from __future__ import annotations

import json
import tempfile
import unittest
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
