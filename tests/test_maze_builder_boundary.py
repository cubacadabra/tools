from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path

from cubacadabra.game_builder import GameBuildError, build_game


class MazeBuilderBoundaryTests(unittest.TestCase):
    def _project(self, manifest: dict) -> tuple[Path, tempfile.TemporaryDirectory[str]]:
        directory = tempfile.TemporaryDirectory()
        project = Path(directory.name)
        source = project / "src"
        source.mkdir()
        (source / "main.luau").write_text("return {}\n", encoding="utf-8")
        (project / "manifest.json").write_text(json.dumps(manifest), encoding="utf-8")
        return project, directory

    def test_python_builder_rejects_world_maze_declarations(self) -> None:
        project, directory = self._project({
            "id": "maze-test",
            "version": "0.3.0",
            "sdkVersion": "0.4.0",
            "worlds": {"maze": {"maze": {"width": 4, "height": 4}}},
        })
        with directory:
            with self.assertRaisesRegex(GameBuildError, "native Rust builder"):
                build_game(
                    source_root=project / "src",
                    manifest_path=project / "manifest.json",
                    output=project / "build",
                )

    def test_python_builder_rejects_top_level_maze_declarations(self) -> None:
        project, directory = self._project({
            "id": "maze-test",
            "version": "0.3.0",
            "sdkVersion": "0.4.0",
            "maze": {"width": 4, "height": 4},
        })
        with directory:
            with self.assertRaisesRegex(GameBuildError, "native Rust builder"):
                build_game(
                    source_root=project / "src",
                    manifest_path=project / "manifest.json",
                    output=project / "build",
                )

    def test_maze_101_requires_the_native_builder(self) -> None:
        project = Path(__file__).parents[2] / "examples" / "maze-101"
        with tempfile.TemporaryDirectory() as directory:
            with self.assertRaisesRegex(GameBuildError, "native Rust builder"):
                build_game(
                    source_root=project / "src",
                    manifest_path=project / "manifest.json",
                    output=Path(directory) / "package",
                )


if __name__ == "__main__":
    unittest.main()
