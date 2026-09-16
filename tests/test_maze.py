from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path

from cubacadabra.game_builder import GameBuildError, build_game
from cubacadabra.maze import MazeBuildError, expand_manifest_mazes


class MazeExpansionTests(unittest.TestCase):
    def _manifest(self, **maze):
        return {
            "id": "maze-test",
            "version": "0.3.0",
            "sdkVersion": "0.4.0",
            "worlds": {
                "maze": {
                    "world": {"groundSize": 1, "gridSize": 1},
                    "maze": maze,
                },
            },
        }

    def test_expands_a_deterministic_bounded_maze(self) -> None:
        manifest = self._manifest(width=6, height=5, seed=42, collectibles={"count": 4})

        first = expand_manifest_mazes(manifest)
        second = expand_manifest_mazes(manifest)
        world = first["worlds"]["maze"]

        self.assertEqual(first, second)
        self.assertNotIn("maze", world)
        self.assertEqual(len(world["interactions"]), 5)
        self.assertEqual(len(world["blocks"]), 0)
        self.assertGreater(len(world["terrain"]["operations"]), 1)
        self.assertEqual(world["terrain"]["operations"][0]["material"], "ground")
        self.assertEqual(world["terrain"]["operations"][1]["material"], "grass")
        self.assertTrue(world["terrain"]["hideDefaultGround"])
        self.assertEqual(world["world"]["spawn"][1], 1.5)
        self.assertEqual(first["generated"], {"mazeFormat": 1})

    def test_generates_checkpoints_on_the_solution_route(self) -> None:
        world = expand_manifest_mazes(
            self._manifest(width=5, height=5, seed=9, checkpointEvery=3)
        )["worlds"]["maze"]

        self.assertGreaterEqual(len(world["checkpoints"]), 1)
        self.assertTrue(world["checkpoints"][0]["id"].startswith("maze-checkpoint-"))

    def test_rejects_mazes_larger_than_runtime_bounds(self) -> None:
        with self.assertRaisesRegex(MazeBuildError, "width.*between"):
            expand_manifest_mazes(self._manifest(width=13, height=5))

    def test_rejects_non_builtin_maze_terrain_material(self) -> None:
        with self.assertRaisesRegex(MazeBuildError, "built-in terrain material"):
            expand_manifest_mazes(
                self._manifest(width=4, height=4, terrain={"wallMaterial": "volcanic"})
            )

    def test_accepts_fully_qualified_builtin_terrain_materials(self) -> None:
        world = expand_manifest_mazes(self._manifest(
            width=4,
            height=4,
            terrain={"wallMaterial": "builtin:grass", "floorMaterial": "builtin:ground"},
        ))["worlds"]["maze"]

        self.assertEqual(world["terrain"]["operations"][0]["material"], "ground")
        self.assertTrue(all(
            operation["material"] == "grass"
            for operation in world["terrain"]["operations"][1:]
        ))

    def test_does_not_change_manifests_without_a_maze(self) -> None:
        manifest = {"id": "plain", "version": "0.3.0", "worlds": {"plain": {}}}
        self.assertEqual(expand_manifest_mazes(manifest), manifest)

    def test_builder_emits_generated_maze_content_in_the_package(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            project = Path(directory)
            source = project / "src"
            source.mkdir()
            (source / "main.luau").write_text("return {}\n", encoding="utf-8")
            manifest = self._manifest(width=4, height=4, seed=7, collectibles={"count": 2})
            (project / "manifest.json").write_text(json.dumps(manifest), encoding="utf-8")

            output = project / "build"
            build_game(
                source_root=source,
                manifest_path=project / "manifest.json",
                output=output,
            )

            built = json.loads((output / "manifest.json").read_text(encoding="utf-8"))
            world = built["worlds"]["maze"]
            self.assertNotIn("maze", world)
            self.assertEqual(world["terrain"]["operations"][0]["material"], "ground")
            self.assertTrue(all(
                fill["material"] == "grass"
                for fill in world["terrain"]["operations"][1:]
            ))
            self.assertEqual(len(world["interactions"]), 3)
            self.assertEqual(built["generated"], {"mazeFormat": 1})
            self.assertIn("maze-coin", built["effects"]["templates"])
            package_info = json.loads((output / "package.json").read_text(encoding="utf-8"))
            self.assertEqual(package_info["runtime"]["api"], "0.4.0")

    def test_builder_rejects_terrain_with_an_older_sdk_version(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            project = Path(directory)
            source = project / "src"
            source.mkdir()
            (source / "main.luau").write_text("return {}\n", encoding="utf-8")
            manifest = self._manifest(width=4, height=4, seed=7)
            manifest["sdkVersion"] = "0.3.0"
            (project / "manifest.json").write_text(json.dumps(manifest), encoding="utf-8")

            with self.assertRaisesRegex(GameBuildError, "terrain requires manifest.sdkVersion 0.4.0"):
                build_game(
                    source_root=source,
                    manifest_path=project / "manifest.json",
                    output=project / "build",
                )


if __name__ == "__main__":
    unittest.main()
