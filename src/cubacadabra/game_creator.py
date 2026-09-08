"""Create a new Cubacadabra game starter project."""

from __future__ import annotations

import copy
import json
import re
import shutil
import unicodedata
from dataclasses import dataclass
from pathlib import Path


class GameCreateError(ValueError):
    """The requested game cannot be created safely."""


DEFAULT_PALETTE = {
    "sky": "#151A3F",
    "ground": "#20295D",
    "groundEdge": "#080D26",
    "grid": "#3B4C86",
    "signal": "#57E5D0",
    "hot": "#FF5B85",
    "coral": "#FF8A3D",
    "butter": "#F1E95B",
    "periwinkle": "#9F7BFF",
    "ink": "#0B102B",
    "paper": "#F7F5E9",
}

DEFAULT_WORLD = {
    "groundSize": 70,
    "gridSize": 64,
    "gridDivisions": 32,
    "spawn": [0, 0, 23],
    "showSpawnPad": False,
    "clouds": [
        {"position": [-20, 19, -35], "scale": 0.9},
        {"position": [24, 23, -48], "scale": 1.2},
    ],
}


@dataclass(frozen=True)
class GameCreateResult:
    """The directory and id produced by a successful game creation."""

    game_id: str
    display_name: str
    project: Path


def _game_id(title: str) -> str:
    normalized = unicodedata.normalize("NFKD", title).encode("ascii", "ignore").decode()
    game_id = re.sub(r"[^a-z0-9]+", "-", normalized.lower()).strip("-")
    if not game_id:
        raise GameCreateError("title must contain at least one letter or number")
    return game_id


def _manifest(title: str, game_id: str) -> dict[str, object]:
    palette = copy.deepcopy(DEFAULT_PALETTE)
    world = copy.deepcopy(DEFAULT_WORLD)
    return {
        "id": game_id,
        "version": "0.3.0",
        "sdkVersion": "0.3.0",
        "package": {
            "formatVersion": 3,
            "entry": "game.luau",
        },
        "displayName": title,
        "lobby": False,
        "startWorld": "lobby",
        "launch": {
            "destinationWorld": "starter-world",
            "authoritative": True,
        },
        "scene": {
            "eyebrow": "cubacadabra",
            "title": title,
            "description": "A new Cubacadabra game.",
            "maxPlayers": 18,
        },
        "palette": palette,
        "avatars": {
            "player": {
                "skin": "#E8AE86",
                "shirt": palette["signal"],
                "pants": "#4C3F91",
                "shoes": palette["ink"],
                "character": {
                    "version": 1,
                    "body": "cuba:person.v1",
                    "face": "determined",
                    "outfit": "cuba:everyday-hoodie.v1",
                    "equipment": {},
                    "colors": {
                        "primary": palette["signal"],
                        "secondary": "#4C3F91",
                        "sole": palette["ink"],
                    },
                    "revision": 1,
                },
            },
            "npcs": [],
        },
        "world": world,
        "launchPads": [],
        "blocks": [],
        "worlds": {
            "starter-world": {
                "palette": copy.deepcopy(palette),
                "world": copy.deepcopy(world),
                "blocks": [],
                "signs": [],
                "interactions": [],
            },
        },
    }


def _source(title: str, game_id: str) -> str:
    title_literal = json.dumps(title, ensure_ascii=False)
    id_literal = json.dumps(game_id)
    joystick_background = json.dumps(DEFAULT_PALETTE["ink"] + "C9")
    joystick_border = json.dumps(DEFAULT_PALETTE["signal"] + "55")
    control_background = json.dumps(DEFAULT_PALETTE["ink"] + "F5")
    foreground = json.dumps(DEFAULT_PALETTE["paper"])
    accent = json.dumps(DEFAULT_PALETTE["signal"])
    return (
        "-- Welcome to Cubacadabra. Add your game rules and UI here.\n"
        "local Game = {}\n\n"
        "local function player_controls()\n"
        "    return {\n"
        "        nodes = {\n"
        "            {\n"
        "                id = \"player-joystick\",\n"
        "                kind = \"joystick\",\n"
        "                action = \"player.move\",\n"
        "                layout = { anchor = \"bottomLeft\", width = 120, height = 120, offset = { 20, -24 } },\n"
        "                style = {\n"
        f"                    background = {joystick_background},\n"
        f"                    borderColor = {joystick_border},\n"
        "                    borderWidth = 2,\n"
        "                    cornerRadius = 60,\n"
        f"                    accent = {accent},\n"
        "                },\n"
        "            },\n"
        "            {\n"
        "                id = \"player-jump\",\n"
        "                kind = \"button\",\n"
        "                text = \"JUMP\",\n"
        "                action = \"player.jump\",\n"
        "                layout = { anchor = \"bottomRight\", width = 86, height = 44, offset = { -22, -84 } },\n"
        "                style = {\n"
        f"                    background = {control_background},\n"
        f"                    borderColor = {accent},\n"
        "                    borderWidth = 2,\n"
        "                    cornerRadius = 17,\n"
        f"                    foreground = {foreground},\n"
        f"                    accent = {accent},\n"
        "                    textAlign = \"center\",\n"
        "                    fontSize = 14,\n"
        "                },\n"
        "            },\n"
        "            {\n"
        "                id = \"player-run\",\n"
        "                kind = \"button\",\n"
        "                text = \"RUN\",\n"
        "                action = \"player.run\",\n"
        "                layout = { anchor = \"bottomRight\", width = 86, height = 44, offset = { -22, -30 } },\n"
        "                style = {\n"
        f"                    background = {control_background},\n"
        f"                    borderColor = {accent},\n"
        "                    borderWidth = 2,\n"
        "                    cornerRadius = 17,\n"
        f"                    foreground = {foreground},\n"
        f"                    accent = {accent},\n"
        "                    textAlign = \"center\",\n"
        "                    fontSize = 14,\n"
        "                },\n"
        "            },\n"
        "        },\n"
        "    }\n"
        "end\n\n"
        "function Game.on_start(api)\n"
        "    api.lobby:set_enabled(false)\n"
        f"    api.lobby:set_status({title_literal} .. \" is ready\")\n"
        f"    api.session:start({id_literal}, {{ mode = \"preview\" }})\n"
        "    api.ui:set_document(player_controls())\n"
        "end\n\n"
        "return Game\n"
    )


def create_game(*, title: str, path: Path) -> GameCreateResult:
    """Create a starter game below *path* without overwriting existing files."""

    if not isinstance(title, str) or not title.strip():
        raise GameCreateError("title is required")
    title = title.strip()
    game_id = _game_id(title)
    base_path = Path(path).expanduser().resolve()
    project = base_path / game_id

    if project.exists():
        raise GameCreateError(f"game directory already exists: {project}")

    base_path.mkdir(parents=True, exist_ok=True)
    project.mkdir()
    try:
        (project / "src").mkdir()
        (project / "assets/audio").mkdir(parents=True)
        (project / "assets/images").mkdir(parents=True)
        (project / "manifest.json").write_text(
            json.dumps(_manifest(title, game_id), indent=2) + "\n",
            encoding="utf-8",
        )
        (project / "src/main.luau").write_text(
            _source(title, game_id),
            encoding="utf-8",
        )
    except OSError:
        # The project directory is new and contains only files from this
        # operation, so avoid leaving a misleading half-created project.
        shutil.rmtree(project)
        raise

    return GameCreateResult(game_id=game_id, display_name=title, project=project)
