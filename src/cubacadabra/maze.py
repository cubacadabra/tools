"""Expand bounded procedural maze declarations into package world content.

Maze walls and ground are emitted as built-in terrain fills; interactions and
checkpoints remain ordinary Cubacadabra world content.
"""

from __future__ import annotations

import copy
import math
import random
from collections import deque
from typing import Any


MAX_MAZE_WIDTH = 12
MAX_MAZE_HEIGHT = 12
MAX_MAZE_CELLS = MAX_MAZE_WIDTH * MAX_MAZE_HEIGHT
TERRAIN_MATERIALS = {"grass", "ground", "dirt", "rock", "sand", "mud", "snow"}

_DIRECTIONS = (
    ("north", 0, -1, "south"),
    ("east", 1, 0, "west"),
    ("south", 0, 1, "north"),
    ("west", -1, 0, "east"),
)


class MazeBuildError(ValueError):
    """A procedural maze declaration is invalid or exceeds package limits."""


def _integer(config: dict[str, Any], key: str, default: int, *, minimum: int, maximum: int) -> int:
    value = config.get(key, default)
    if isinstance(value, bool) or not isinstance(value, int) or not minimum <= value <= maximum:
        raise MazeBuildError(f"manifest.maze.{key} must be an integer between {minimum} and {maximum}")
    return value


def _number(config: dict[str, Any], key: str, default: float, *, minimum: float, maximum: float) -> float:
    value = config.get(key, default)
    if isinstance(value, bool) or not isinstance(value, (int, float)) or not minimum <= value <= maximum:
        raise MazeBuildError(f"manifest.maze.{key} must be between {minimum} and {maximum}")
    return float(value)


def _cell(config: dict[str, Any], key: str, default: tuple[int, int], width: int, height: int) -> tuple[int, int]:
    value = config.get(key, list(default))
    if (
        not isinstance(value, list)
        or len(value) != 2
        or any(isinstance(item, bool) or not isinstance(item, int) for item in value)
        or not (0 <= value[0] < width and 0 <= value[1] < height)
    ):
        raise MazeBuildError(
            f"manifest.maze.{key} must be a two-item cell coordinate inside the maze"
        )
    return value[0], value[1]


def _origin(config: dict[str, Any], width: int, height: int, cell_size: float) -> tuple[float, float, float]:
    default = [-(width * cell_size) / 2, 0, -(height * cell_size) / 2]
    value = config.get("origin", default)
    if (
        not isinstance(value, list)
        or len(value) != 3
        or any(isinstance(item, bool) or not isinstance(item, (int, float)) for item in value)
    ):
        raise MazeBuildError("manifest.maze.origin must contain three finite numbers")
    if any(not math.isfinite(float(item)) for item in value):
        raise MazeBuildError("manifest.maze.origin must contain three finite numbers")
    return float(value[0]), float(value[1]), float(value[2])


def _carve(width: int, height: int, seed: int) -> dict[tuple[int, int], dict[str, bool]]:
    cells = {
        (x, y): {name: True for name, _, _, _ in _DIRECTIONS}
        for y in range(height)
        for x in range(width)
    }
    rng = random.Random(seed)
    visited = {(0, 0)}
    stack = [(0, 0)]

    while stack:
        x, y = stack[-1]
        choices = [
            (name, x + dx, y + dy, opposite)
            for name, dx, dy, opposite in _DIRECTIONS
            if (x + dx, y + dy) not in visited
            and 0 <= x + dx < width
            and 0 <= y + dy < height
        ]
        if not choices:
            stack.pop()
            continue
        name, next_x, next_y, opposite = rng.choice(choices)
        cells[(x, y)][name] = False
        cells[(next_x, next_y)][opposite] = False
        visited.add((next_x, next_y))
        stack.append((next_x, next_y))
    return cells


def _path(
    cells: dict[tuple[int, int], dict[str, bool]],
    start: tuple[int, int],
    finish: tuple[int, int],
) -> list[tuple[int, int]]:
    queue = deque([start])
    previous: dict[tuple[int, int], tuple[int, int] | None] = {start: None}
    while queue:
        cell = queue.popleft()
        if cell == finish:
            break
        x, y = cell
        for name, dx, dy, _ in _DIRECTIONS:
            next_cell = (x + dx, y + dy)
            if not cells[cell][name] and next_cell not in previous:
                previous[next_cell] = cell
                queue.append(next_cell)
    if finish not in previous:
        raise MazeBuildError("manifest.maze generated a maze without a start-to-finish path")
    result = []
    cursor: tuple[int, int] | None = finish
    while cursor is not None:
        result.append(cursor)
        cursor = previous[cursor]
    result.reverse()
    return result


def _position(origin: tuple[float, float, float], cell_size: float, cell: tuple[int, int], y: float) -> list[float]:
    return [
        origin[0] + (cell[0] + 0.5) * cell_size,
        origin[1] + y,
        origin[2] + (cell[1] + 0.5) * cell_size,
    ]


def _maze_effects() -> dict[str, Any]:
    return {
        "version": 1,
        "templates": {
            "maze-coin": {
                "duration": 1,
                "nodes": [
                    {
                        "shape": "cylinder",
                        "position": [0, 0.5, 0],
                        "size": [0.55, 0.12, 1],
                        "color": "butter",
                        "opacity": 0.9,
                        "visibleStates": ["available"],
                        "animation": {"orbitSpeed": 1.8, "bobAmount": 0.12, "bobSpeed": 2.2},
                    },
                    {
                        "shape": "ring",
                        "position": [0, 0.18, 0],
                        "size": [1.3, 0.08, 1],
                        "color": "$interaction",
                        "opacity": 0.55,
                        "visibleStates": ["available", "collected"],
                    },
                ],
            },
            "maze-finish": {
                "duration": 1,
                "nodes": [
                    {
                        "shape": "ring",
                        "position": [0, 0.12, 0],
                        "size": [3.2, 0.16, 1],
                        "color": "$interaction",
                        "opacity": 0.85,
                        "visibleStates": ["available", "reached"],
                        "animation": {"pulseAmount": 0.08, "pulseSpeed": 2.5},
                    },
                    {
                        "shape": "box",
                        "position": [0, 1.0, 0],
                        "size": [0.24, 1.8, 0.24],
                        "color": "$interaction",
                        "opacity": 0.9,
                        "visibleStates": ["available", "reached"],
                        "count": 3,
                        "animation": {"orbitRadius": 1.0, "orbitSpeed": 0.5, "bobAmount": 0.12},
                    },
                ],
            },
            "maze-finish-burst": {
                "duration": 1.5,
                "nodes": [
                    {
                        "shape": "ring",
                        "position": [0, 0.55, 0],
                        "size": [0.8, 0.12, 1],
                        "color": "signal",
                        "animation": {"expandAmount": 4.5, "fade": True},
                    },
                    {
                        "shape": "sphere",
                        "position": [0, 1.0, 0],
                        "size": [0.25, 1, 1],
                        "color": "butter",
                        "count": 12,
                        "animation": {"orbitRadius": 1.4, "orbitSpeed": 3.5, "radialAmount": 4.8, "fade": True},
                    },
                ],
            },
        },
    }


def _expand_world(world: dict[str, Any]) -> bool:
    config = world.get("maze")
    if config is None:
        return False
    if not isinstance(config, dict):
        raise MazeBuildError("manifest.worlds.<id>.maze must be an object")

    width = _integer(config, "width", 10, minimum=2, maximum=MAX_MAZE_WIDTH)
    height = _integer(config, "height", 10, minimum=2, maximum=MAX_MAZE_HEIGHT)
    cell_size = _number(config, "cellSize", 8, minimum=3, maximum=32)
    wall_height = _number(config, "wallHeight", 7, minimum=2, maximum=24)
    wall_thickness = _number(config, "wallThickness", 0.7, minimum=0.2, maximum=3)
    seed = _integer(config, "seed", 1, minimum=0, maximum=0xFFFFFFFF)
    start = _cell(config, "start", (0, 0), width, height)
    finish = _cell(config, "finish", (width - 1, height - 1), width, height)
    origin = _origin(config, width, height, cell_size)
    cells = _carve(width, height, seed)
    route = _path(cells, start, finish)

    terrain_config = config.get("terrain", {})
    if not isinstance(terrain_config, dict):
        raise MazeBuildError("manifest.maze.terrain must be an object")
    terrain_cell_size = _number(terrain_config, "cellSize", 0.5, minimum=0.5, maximum=8)
    wall_material = terrain_config.get("wallMaterial", "grass")
    floor_material = terrain_config.get("floorMaterial", "ground")
    for key, material in (("wallMaterial", wall_material), ("floorMaterial", floor_material)):
        if not isinstance(material, str) or material.removeprefix("builtin:").lower() not in TERRAIN_MATERIALS:
            raise MazeBuildError(
                f"manifest.maze.terrain.{key} must be a built-in terrain material"
            )
    wall_material = wall_material.removeprefix("builtin:").lower()
    floor_material = floor_material.removeprefix("builtin:").lower()

    blocks = list(world.get("blocks", []))
    terrain = world.get("terrain", {})
    if not isinstance(terrain, dict):
        raise MazeBuildError("manifest.worlds.<id>.terrain must be an object")
    operations = list(terrain.get("operations", []))
    terrain["cellSize"] = terrain_cell_size
    terrain["hideDefaultGround"] = True
    operations.append({
        "operation": "fill",
        "shape": "block",
        "position": [
            origin[0] + width * cell_size / 2,
            origin[1] - 1.0,
            origin[2] + height * cell_size / 2,
        ],
        "size": [width * cell_size, 2.0, height * cell_size],
        "material": floor_material.lower(),
    })
    for (x, y), cell in cells.items():
        cell_x = origin[0] + x * cell_size
        cell_z = origin[2] + y * cell_size
        if cell["north"]:
            operations.append({"operation": "fill", "shape": "block", "position": [cell_x + cell_size / 2, origin[1] + wall_height / 2, cell_z], "size": [cell_size + wall_thickness, wall_height, wall_thickness], "material": wall_material.lower()})
        if cell["west"]:
            operations.append({"operation": "fill", "shape": "block", "position": [cell_x, origin[1] + wall_height / 2, cell_z + cell_size / 2], "size": [wall_thickness, wall_height, cell_size + wall_thickness], "material": wall_material.lower()})
        if x == width - 1 and cell["east"]:
            operations.append({"operation": "fill", "shape": "block", "position": [cell_x + cell_size, origin[1] + wall_height / 2, cell_z + cell_size / 2], "size": [wall_thickness, wall_height, cell_size + wall_thickness], "material": wall_material.lower()})
        if y == height - 1 and cell["south"]:
            operations.append({"operation": "fill", "shape": "block", "position": [cell_x + cell_size / 2, origin[1] + wall_height / 2, cell_z + cell_size], "size": [cell_size + wall_thickness, wall_height, wall_thickness], "material": wall_material.lower()})

    interactions = list(world.get("interactions", []))
    finish_id = str(config.get("finishId", "maze-finish"))
    interactions.append({
        "id": finish_id,
        "kind": "finish",
        "label": "EXIT",
        "position": _position(origin, cell_size, finish, 0),
        "radius": min(cell_size * 0.4, 3.2),
        "color": config.get("finishColor", "signal"),
        "visual": "maze-finish",
    })

    collectibles = config.get("collectibles", {})
    if not isinstance(collectibles, dict):
        raise MazeBuildError("manifest.maze.collectibles must be an object")
    collectible_count = _integer(collectibles, "count", 12, minimum=0, maximum=64)
    collectible_cells = [cell for cell in route[1:-1]]
    collectible_cells.extend(cell for cell in cells if cell not in (start, finish) and cell not in collectible_cells)
    random.Random(seed ^ 0xC0FFEE).shuffle(collectible_cells)
    for index, cell in enumerate(collectible_cells[:collectible_count], start=1):
        interactions.append({
            "id": f"maze-coin-{index:02d}",
            "kind": "collectible",
            "label": "COIN",
            "position": _position(origin, cell_size, cell, 0),
            "radius": min(cell_size * 0.32, 2.4),
            "color": collectibles.get("color", "butter"),
            "visual": "maze-coin",
        })

    checkpoint_every = _integer(config, "checkpointEvery", 0, minimum=0, maximum=64)
    checkpoints = list(world.get("checkpoints", []))
    if checkpoint_every:
        for index in range(checkpoint_every, len(route) - 1, checkpoint_every):
            cell = route[index]
            checkpoints.append({
                "id": f"maze-checkpoint-{index // checkpoint_every}",
                "position": _position(origin, cell_size, cell, 0),
                "radius": min(cell_size * 0.35, 2.7),
            })
    world["blocks"] = blocks
    terrain["operations"] = operations
    world["terrain"] = terrain
    world["interactions"] = interactions
    world["checkpoints"] = checkpoints
    settings = dict(world.get("world", {}))
    extent = max(width, height) * cell_size + cell_size * 2
    settings["groundSize"] = max(float(settings.get("groundSize", 0)), extent)
    settings["gridSize"] = max(float(settings.get("gridSize", 0)), extent)
    settings["gridDivisions"] = max(width, height) * 2
    settings["spawn"] = _position(origin, cell_size, start, 1.5)
    settings["showSpawnPad"] = False
    world["world"] = settings
    world.pop("maze", None)
    return True


def expand_manifest_mazes(manifest: dict[str, Any]) -> dict[str, Any]:
    """Return a manifest with each bounded ``world.maze`` expanded."""

    resolved = copy.deepcopy(manifest)
    worlds = resolved.get("worlds", {})
    if not isinstance(worlds, dict):
        raise MazeBuildError("manifest.worlds must be an object")
    generated_maze = False
    for world in worlds.values():
        if isinstance(world, dict):
            generated_maze = _expand_world(world) or generated_maze
    if isinstance(resolved.get("maze"), dict):
        generated_maze = _expand_world(resolved) or generated_maze
    if generated_maze:
        if resolved.get("effects") is None:
            resolved["effects"] = _maze_effects()
        elif isinstance(resolved.get("effects"), dict) and "source" not in resolved["effects"]:
            effects = resolved["effects"]
            templates = effects.setdefault("templates", {})
            if isinstance(templates, dict):
                templates.update(_maze_effects()["templates"])
        generated = resolved.get("generated", {})
        if not isinstance(generated, dict):
            raise MazeBuildError("manifest.generated must be an object")
        resolved["generated"] = {**generated, "mazeFormat": 1}
    return resolved
