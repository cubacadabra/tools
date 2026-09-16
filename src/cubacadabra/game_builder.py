"""Build portable Cubacadabra game packages.

Game source uses ordinary Luau modules, for example::

    local Document = require("./ui/document")

The builder resolves and bundles those modules into one deterministic Luau
entry chunk, then copies the manifest and optional assets into a client-ready
package.
"""

from __future__ import annotations

import hashlib
import json
import math
import os
import posixpath
import re
import shutil
import tempfile
import wave
import zipfile
from dataclasses import dataclass
from pathlib import Path, PurePosixPath

from .maze import MazeBuildError, expand_manifest_mazes
from .package_contract import is_valid_game_id


INCLUDE_RE = re.compile(r'^\s*--\s*@include(?:\s|$)')
SDK_REQUIRE_PREFIX = "@cubacadabra/"
SDK_REQUIRE_RE = re.compile(r"^@cubacadabra/[a-z0-9-]+$")
SDK_MODULES = {
    "@cubacadabra/disclosure": "disclosure.luau",
    "@cubacadabra/obby": "obby.luau",
    "@cubacadabra/survival": "survival.luau",
    "@cubacadabra/cycle": "cycle.luau",
    "@cubacadabra/shared-state": "shared-state.luau",
}
AUDIO_ID_RE = re.compile(r"^[A-Za-z0-9._-]{1,64}$")
AUDIO_PATH_RE = re.compile(
    r"^assets/(?:[A-Za-z0-9_-][A-Za-z0-9._-]*/)*"
    r"[A-Za-z0-9_-][A-Za-z0-9._-]*\.wav$"
)
IMAGE_ID_RE = re.compile(r"^[A-Za-z0-9._-]{1,64}$")
IMAGE_PATH_RE = re.compile(
    r"^assets/(?:[A-Za-z0-9_-][A-Za-z0-9._-]*/)*"
    r"[A-Za-z0-9_-][A-Za-z0-9._-]*\.(?:jpg|jpeg|png)$",
    re.IGNORECASE,
)
EFFECTS_SOURCE_RE = re.compile(
    r"^(?:[A-Za-z0-9_-][A-Za-z0-9._-]*/)*"
    r"[A-Za-z0-9_-][A-Za-z0-9._-]*\.json$"
)
SEMVER_RE = re.compile(
    r"^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)"
    r"(?:-[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?"
    r"(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?$"
)
MAX_AUDIO_ASSETS = 64
MAX_AUDIO_ASSET_BYTES = 4 * 1024 * 1024
MAX_IMAGE_ASSETS = 16
MAX_IMAGE_ASSET_BYTES = 8 * 1024 * 1024
TERRAIN_CHUNK_CELLS = 16
MAX_TERRAIN_OPERATIONS = 512
MAX_TERRAIN_CHUNKS = 512
MAX_TERRAIN_SAMPLES = 2_000_000
MAX_TERRAIN_COORDINATE = 4096
BUILTIN_TERRAIN_MATERIALS = {"grass", "ground", "dirt", "rock", "sand", "mud", "snow"}
MATERIAL_ID_RE = re.compile(r"^[A-Za-z0-9._-]{1,64}$")
PREVIEW_SDK_VERSION = "0.3.0"
TERRAIN_SDK_VERSION = "0.4.0"
SUPPORTED_SDK_VERSIONS = frozenset((PREVIEW_SDK_VERSION, TERRAIN_SDK_VERSION))
SDK_SOURCE_ID = "cubacadabra-preview-sdk"
BUILD_MARKER = ".cubacadabra-build"
BUILD_MARKER_CONTENT = "cubacadabra-game-package-v1\n"


class GameBuildError(ValueError):
    """An input project cannot be turned into a valid game package."""


def _canonical_sdk_root() -> Path:
    """Return the SDK shipped with this version of the build tools.

    ``.luaurc`` is intentionally not consulted here. It is an editor/type
    checking configuration file, not a release dependency resolver.
    """

    return Path(__file__).with_name("sdk").resolve()


def _read_sdk_module(module_name: str) -> str:
    if not SDK_REQUIRE_RE.fullmatch(module_name):
        raise GameBuildError(
            "Cubacadabra SDK requires must use "
            'require("@cubacadabra/<module-name>")'
        )
    sdk_file = SDK_MODULES.get(module_name)
    if sdk_file is None:
        raise GameBuildError(f"unknown Cubacadabra SDK module: {module_name}")
    sdk_path = _canonical_sdk_root() / sdk_file

    try:
        source = sdk_path.read_text(encoding="utf-8")
    except UnicodeDecodeError as error:
        raise GameBuildError(f"{module_name}: SDK source is not valid UTF-8") from error
    return source


@dataclass(frozen=True)
class GameBuildResult:
    """The useful outputs and metadata produced by a successful build."""

    game_id: str
    version: int | str
    output: Path
    zip_path: Path | None


@dataclass(frozen=True)
class _LuauModule:
    """One source module and its build-time require routing table."""

    module_id: str
    source: str
    routes: dict[str, str]


def _read_luau_source(path: Path, source_root: Path) -> str:
    relative = path.relative_to(source_root).as_posix()
    try:
        return path.read_text(encoding="utf-8")
    except UnicodeDecodeError as error:
        raise GameBuildError(f"{relative}: source is not valid UTF-8") from error


def _long_bracket_end(source: str, offset: int) -> tuple[str, int] | None:
    """Return a Luau long-bracket closing token and content offset."""

    if offset >= len(source) or source[offset] != "[":
        return None
    cursor = offset + 1
    while cursor < len(source) and source[cursor] == "=":
        cursor += 1
    if cursor >= len(source) or source[cursor] != "[":
        return None
    equals = source[offset + 1 : cursor]
    return "]" + equals + "]", cursor + 1


def _skip_space_and_comments(source: str, offset: int) -> int:
    """Skip whitespace and comments while parsing a require expression."""

    cursor = offset
    while cursor < len(source):
        if source[cursor].isspace():
            cursor += 1
            continue
        if source.startswith("--", cursor):
            long_comment = _long_bracket_end(source, cursor + 2)
            if long_comment is not None:
                closing, content_offset = long_comment
                end = source.find(closing, content_offset)
                cursor = len(source) if end < 0 else end + len(closing)
            else:
                newline = source.find("\n", cursor + 2)
                cursor = len(source) if newline < 0 else newline + 1
            continue
        break
    return cursor


def _require_specifiers(source: str, module_name: str) -> tuple[str, ...]:
    """Find global require calls without mistaking comments or strings for code."""

    specifiers: list[str] = []
    cursor = 0
    previous_token = ""
    while cursor < len(source):
        if source.startswith("--", cursor):
            long_comment = _long_bracket_end(source, cursor + 2)
            if long_comment is not None:
                closing, content_offset = long_comment
                end = source.find(closing, content_offset)
                cursor = len(source) if end < 0 else end + len(closing)
            else:
                newline = source.find("\n", cursor + 2)
                cursor = len(source) if newline < 0 else newline + 1
            continue

        character = source[cursor]
        if character in ('"', "'", "`"):
            quote = character
            cursor += 1
            while cursor < len(source):
                if source[cursor] == "\\":
                    cursor += 2
                elif source[cursor] == quote:
                    cursor += 1
                    break
                else:
                    cursor += 1
            previous_token = "string"
            continue

        long_string = _long_bracket_end(source, cursor)
        if long_string is not None:
            closing, content_offset = long_string
            end = source.find(closing, content_offset)
            cursor = len(source) if end < 0 else end + len(closing)
            previous_token = "string"
            continue

        if character.isalpha() or character == "_":
            end = cursor + 1
            while end < len(source) and (source[end].isalnum() or source[end] == "_"):
                end += 1
            identifier = source[cursor:end]
            if identifier != "require" or previous_token in (".", ":"):
                previous_token = identifier
                cursor = end
                continue

            call = _skip_space_and_comments(source, end)
            if call >= len(source) or source[call] != "(":
                previous_token = identifier
                cursor = end
                continue
            argument = _skip_space_and_comments(source, call + 1)
            if argument >= len(source) or source[argument] not in ('"', "'"):
                raise GameBuildError(
                    f"{module_name}: require paths must be static quoted strings"
                )
            quote = source[argument]
            value_start = argument + 1
            value_end = value_start
            while value_end < len(source) and source[value_end] != quote:
                if source[value_end] == "\\":
                    raise GameBuildError(
                        f"{module_name}: require paths cannot contain escapes"
                    )
                value_end += 1
            if value_end >= len(source):
                raise GameBuildError(f"{module_name}: unterminated require path")
            close = _skip_space_and_comments(source, value_end + 1)
            if close >= len(source) or source[close] != ")":
                raise GameBuildError(
                    f"{module_name}: require must contain exactly one string path"
                )
            specifiers.append(source[value_start:value_end])
            previous_token = ")"
            cursor = close + 1
            continue

        if not character.isspace():
            previous_token = character
        cursor += 1

    return tuple(dict.fromkeys(specifiers))


def _resolve_local_module(
    specifier: str,
    requiring_path: Path,
    source_root: Path,
) -> Path:
    relative = requiring_path.relative_to(source_root).as_posix()
    if not specifier.startswith(("./", "../")):
        raise GameBuildError(
            f"{relative}: require path must start with './', '../', or '@cubacadabra/'"
        )
    if "\\" in specifier:
        raise GameBuildError(f"{relative}: require paths must use forward slashes")

    requiring_module = PurePosixPath(relative)
    unresolved = posixpath.normpath(
        (requiring_module.parent / PurePosixPath(specifier)).as_posix()
    )
    if unresolved == ".." or unresolved.startswith("../") or unresolved.startswith("/"):
        raise GameBuildError(f"{relative}: require must stay inside src/")

    unresolved_path = PurePosixPath(unresolved)
    if unresolved_path.suffix in (".luau", ".lua"):
        candidates = [source_root.joinpath(*unresolved_path.parts)]
    else:
        candidates = [
            source_root.joinpath(*PurePosixPath(unresolved + suffix).parts)
            for suffix in (".luau", ".lua")
        ] + [
            source_root.joinpath(*unresolved_path.parts, filename)
            for filename in ("init.luau", "init.lua")
        ]
    matches = [candidate for candidate in candidates if candidate.is_file()]
    if not matches:
        raise GameBuildError(f"{relative}: required module not found: {specifier}")
    if len(matches) > 1:
        choices = ", ".join(path.relative_to(source_root).as_posix() for path in matches)
        raise GameBuildError(
            f"{relative}: required module is ambiguous: {specifier} ({choices})"
        )

    match = matches[0]
    try:
        match.resolve().relative_to(source_root.resolve())
    except ValueError as error:
        raise GameBuildError(f"{relative}: require must stay inside src/") from error
    return match


def _bundle_luau_modules(
    entry: Path,
    source_root: Path,
    sdk_dependencies: dict[str, dict[str, str]] | None = None,
) -> str:
    modules: dict[str, _LuauModule] = {}

    def visit_local(path: Path, stack: tuple[str, ...]) -> str:
        module_id = path.relative_to(source_root).as_posix()
        return visit(module_id, _read_luau_source(path, source_root), path, stack)

    def visit_sdk(module_id: str, stack: tuple[str, ...]) -> str:
        source = _read_sdk_module(module_id)
        if sdk_dependencies is not None:
            sdk_dependencies[module_id] = {
                "source": SDK_SOURCE_ID,
                "sha256": hashlib.sha256(source.encode("utf-8")).hexdigest(),
            }
        return visit(module_id, source, None, stack)

    def visit(
        module_id: str,
        source: str,
        path: Path | None,
        stack: tuple[str, ...],
    ) -> str:
        if module_id in stack:
            chain = " -> ".join((*stack, module_id))
            raise GameBuildError(f"cyclic Luau require: {chain}")
        if module_id in modules:
            return module_id

        for line_number, line in enumerate(source.splitlines(), start=1):
            if INCLUDE_RE.match(line):
                raise GameBuildError(
                    f"{module_id}:{line_number}: @include is no longer supported; "
                    "use a Luau require() and return a value from the module"
                )

        routes: dict[str, str] = {}
        next_stack = (*stack, module_id)
        for specifier in _require_specifiers(source, module_id):
            if specifier.startswith(SDK_REQUIRE_PREFIX):
                dependency_id = visit_sdk(specifier, next_stack)
            else:
                if path is None:
                    raise GameBuildError(
                        f"{module_id}: SDK modules cannot require game source modules"
                    )
                dependency = _resolve_local_module(specifier, path, source_root)
                dependency_id = visit_local(dependency, next_stack)
            routes[specifier] = dependency_id

        modules[module_id] = _LuauModule(module_id, source, routes)
        return module_id

    entry_id = visit_local(entry, ())
    lines = [
        "local __modules = {}",
        "local __routes = {}",
        "local __cache = {}",
        "local __loading = {}",
        "",
    ]
    for module_id in sorted(modules):
        module = modules[module_id]
        encoded_id = json.dumps(module_id)
        lines.append(f"-- begin module: {module_id}")
        lines.append(f"__routes[{encoded_id}] = {{")
        for specifier, dependency_id in sorted(module.routes.items()):
            lines.append(
                f"    [{json.dumps(specifier)}] = {json.dumps(dependency_id)},"
            )
        lines.append("}")
        lines.append(f"__modules[{encoded_id}] = function(require)")
        lines.append(module.source)
        lines.append("end")
        lines.append(f"-- end module: {module_id}")
        lines.append("")

    lines.extend(
        [
            "local function __require(module_id)",
            "    local cached = __cache[module_id]",
            "    if cached ~= nil then",
            "        return cached",
            "    end",
            "    if __loading[module_id] then",
            '        error("cyclic bundled require: " .. module_id)',
            "    end",
            "    local loader = __modules[module_id]",
            "    if loader == nil then",
            '        error("bundled module not found: " .. module_id)',
            "    end",
            "    local routes = __routes[module_id]",
            "    local function module_require(path)",
            "        local dependency_id = routes[path]",
            "        if dependency_id == nil then",
            '            error("undeclared bundled require from " .. module_id .. ": " .. tostring(path))',
            "        end",
            "        return __require(dependency_id)",
            "    end",
            "    __loading[module_id] = true",
            "    local result = loader(module_require)",
            "    __loading[module_id] = nil",
            "    if result == nil then",
            '        error("bundled module returned nil: " .. module_id)',
            "    end",
            "    __cache[module_id] = result",
            "    return result",
            "end",
            "",
            f"return __require({json.dumps(entry_id)})",
        ]
    )
    return "\n".join(lines)


def _manifest_value(manifest: dict[str, object], key: str) -> object:
    value = manifest.get(key)
    if value is None:
        raise GameBuildError(f"manifest.{key} must be present")
    return value


def _resolve_effects_source(
    manifest: dict[str, object],
    project_root: Path,
) -> dict[str, object]:
    effects = manifest.get("effects")
    if not isinstance(effects, dict) or "source" not in effects:
        return manifest
    if set(effects) != {"source"}:
        raise GameBuildError(
            "manifest.effects.source cannot be combined with inline effect fields"
        )

    source_value = effects["source"]
    if not isinstance(source_value, str) or not EFFECTS_SOURCE_RE.fullmatch(source_value):
        raise GameBuildError(
            "manifest.effects.source must be a relative JSON path inside the game project"
        )
    source_path = project_root.joinpath(*PurePosixPath(source_value).parts)
    try:
        source_path.resolve().relative_to(project_root.resolve())
    except ValueError as error:
        raise GameBuildError(
            "manifest.effects.source must stay inside the game project"
        ) from error
    if not source_path.is_file():
        raise GameBuildError(
            f"manifest.effects.source was not found: {source_value}"
        )
    try:
        resolved_effects = json.loads(source_path.read_text(encoding="utf-8"))
    except UnicodeDecodeError as error:
        raise GameBuildError(
            f"manifest.effects.source is not valid UTF-8: {source_value}"
        ) from error
    except json.JSONDecodeError as error:
        raise GameBuildError(
            f"manifest.effects.source is not valid JSON: {error.msg}"
        ) from error
    if not isinstance(resolved_effects, dict):
        raise GameBuildError("manifest.effects.source must contain a JSON object")

    resolved_manifest = dict(manifest)
    resolved_manifest["effects"] = resolved_effects
    return resolved_manifest


def _validate_output(output: Path, source_root: Path) -> None:
    """Prevent a package output from deleting or containing the source tree."""

    try:
        output.relative_to(source_root)
    except ValueError:
        try:
            source_root.relative_to(output)
        except ValueError:
            return
    raise GameBuildError(
        "output directory cannot overlap the source directory in either direction: "
        f"{output}"
    )


def _validate_existing_output(output: Path) -> None:
    """Only allow replacement of a directory previously written by this builder."""

    if not output.exists():
        return
    if not output.is_dir():
        raise GameBuildError(f"output exists and is not a directory: {output}")
    marker = output / BUILD_MARKER
    try:
        marker_content = marker.read_text(encoding="utf-8")
    except (OSError, UnicodeError) as error:
        raise GameBuildError(
            "refusing to replace existing output without a Cubacadabra build marker: "
            f"{output}"
        ) from error
    if marker_content != BUILD_MARKER_CONTENT:
        raise GameBuildError(
            "refusing to replace existing output with an invalid Cubacadabra build "
            f"marker: {output}"
        )


def _temporary_directory(parent: Path, prefix: str) -> Path:
    """Create a temporary directory beside a final output path."""

    return Path(tempfile.mkdtemp(prefix=prefix, dir=parent))


def _replace_output(staging: Path, output: Path) -> None:
    """Install a complete staging directory while retaining rollback coverage."""

    backup: Path | None = None
    try:
        if output.exists():
            backup = _temporary_directory(output.parent, f".{output.name}.old-")
            backup.rmdir()
            output.rename(backup)
        staging.rename(output)
    except OSError:
        if backup is not None and not output.exists() and backup.exists():
            backup.rename(output)
        raise
    else:
        if backup is not None:
            shutil.rmtree(backup)


def _write_archive(output: Path, zip_path: Path) -> Path:
    """Write an archive beside its destination so the destination stays intact."""

    if zip_path.exists() and zip_path.is_dir():
        raise GameBuildError(f"zip path exists and is a directory: {zip_path}")
    archive_fd, archive_name = tempfile.mkstemp(
        prefix=f".{zip_path.name}.", suffix=".tmp", dir=zip_path.parent
    )
    os.close(archive_fd)
    archive_temp = Path(archive_name)
    try:
        with zipfile.ZipFile(archive_temp, "w", compression=zipfile.ZIP_DEFLATED) as archive:
            for path in sorted(output.rglob("*")):
                if path.is_file() and path.name != BUILD_MARKER:
                    archive.write(path, path.relative_to(output).as_posix())
    except BaseException:
        archive_temp.unlink(missing_ok=True)
        raise
    return archive_temp


def _validate_audio_assets(manifest: dict[str, object], project_root: Path) -> None:
    assets = manifest.get("assets")
    if assets is None:
        return
    if not isinstance(assets, dict):
        raise GameBuildError("manifest.assets must be an object")
    audio = assets.get("audio")
    if audio is None:
        return
    if not isinstance(audio, dict):
        raise GameBuildError("manifest.assets.audio must be an object")
    if len(audio) > MAX_AUDIO_ASSETS:
        raise GameBuildError(f"manifest.assets.audio cannot contain more than {MAX_AUDIO_ASSETS} sounds")

    for audio_id, definition in audio.items():
        if not isinstance(audio_id, str) or not AUDIO_ID_RE.fullmatch(audio_id):
            raise GameBuildError(
                "manifest.assets.audio ids must be 1–64 ASCII letters, numbers, dots, "
                "dashes, or underscores"
            )
        if not isinstance(definition, dict):
            raise GameBuildError(f"manifest.assets.audio.{audio_id} must be an object")
        path_value = definition.get("path")
        if not isinstance(path_value, str) or not path_value:
            raise GameBuildError(f"manifest.assets.audio.{audio_id}.path must be a non-empty string")
        relative_path = PurePosixPath(path_value)
        if not AUDIO_PATH_RE.fullmatch(path_value):
            raise GameBuildError(
                f"manifest.assets.audio.{audio_id}.path must stay inside assets/"
            )
        audio_path = project_root.joinpath(*relative_path.parts)
        if not audio_path.is_file():
            raise GameBuildError(
                f"manifest.assets.audio.{audio_id}.path was not found: {path_value}"
            )
        try:
            audio_path.resolve().relative_to((project_root / "assets").resolve())
        except ValueError as error:
            raise GameBuildError(
                f"manifest.assets.audio.{audio_id}.path must stay inside assets/"
            ) from error
        if audio_path.stat().st_size > MAX_AUDIO_ASSET_BYTES:
            raise GameBuildError(
                f"manifest.assets.audio.{audio_id}.path exceeds 4 MiB: {path_value}"
            )
        try:
            with wave.open(str(audio_path), "rb") as wav:
                if (
                    wav.getcomptype() != "NONE"
                    or wav.getsampwidth() != 2
                    or wav.getframerate() != 48_000
                    or wav.getnchannels() not in (1, 2)
                ):
                    raise GameBuildError(
                        f"manifest.assets.audio.{audio_id}.path must be 48 kHz, "
                        "16-bit PCM WAV with one or two channels"
                    )
        except (EOFError, wave.Error) as error:
            raise GameBuildError(
                f"manifest.assets.audio.{audio_id}.path is not a valid WAV file"
            ) from error
        volume = definition.get("volume", 1)
        if (
            not isinstance(volume, (int, float))
            or isinstance(volume, bool)
            or not 0 <= volume <= 1
        ):
            raise GameBuildError(
                f"manifest.assets.audio.{audio_id}.volume must be between 0 and 1"
            )


def _validate_image_assets(manifest: dict[str, object], project_root: Path) -> None:
    assets = manifest.get("assets")
    if assets is None:
        return
    if not isinstance(assets, dict):
        raise GameBuildError("manifest.assets must be an object")
    images = assets.get("images")
    if images is None:
        return
    if not isinstance(images, dict):
        raise GameBuildError("manifest.assets.images must be an object")
    if len(images) > MAX_IMAGE_ASSETS:
        raise GameBuildError(
            f"manifest.assets.images cannot contain more than {MAX_IMAGE_ASSETS} image"
        )

    for image_id, definition in images.items():
        if not isinstance(image_id, str) or not IMAGE_ID_RE.fullmatch(image_id):
            raise GameBuildError(
                "manifest.assets.images ids must be 1–64 ASCII letters, numbers, dots, "
                "dashes, or underscores"
            )
        if not isinstance(definition, dict):
            raise GameBuildError(f"manifest.assets.images.{image_id} must be an object")
        path_value = definition.get("path")
        if not isinstance(path_value, str) or not IMAGE_PATH_RE.fullmatch(path_value):
            raise GameBuildError(
                f"manifest.assets.images.{image_id}.path must reference a JPG, JPEG, "
                "or PNG inside assets/"
            )
        image_path = project_root.joinpath(*PurePosixPath(path_value).parts)
        if not image_path.is_file():
            raise GameBuildError(
                f"manifest.assets.images.{image_id}.path was not found: {path_value}"
            )
        try:
            image_path.resolve().relative_to((project_root / "assets").resolve())
        except ValueError as error:
            raise GameBuildError(
                f"manifest.assets.images.{image_id}.path must stay inside assets/"
            ) from error
        if image_path.stat().st_size > MAX_IMAGE_ASSET_BYTES:
            raise GameBuildError(
                f"manifest.assets.images.{image_id}.path exceeds 8 MiB: {path_value}"
            )


def _validate_world_materials(manifest: dict[str, object]) -> None:
    assets = manifest.get("assets")
    images = assets.get("images", {}) if isinstance(assets, dict) else {}
    worlds = manifest.get("worlds", {})
    world_definitions = [manifest]
    if isinstance(worlds, dict):
        world_definitions.extend(world for world in worlds.values() if isinstance(world, dict))

    for world in world_definitions:
        materials = world.get("materials")
        if materials is None:
            continue
        if not isinstance(materials, dict):
            raise GameBuildError("world.materials must be an object")
        for material_id, definition in materials.items():
            if not isinstance(material_id, str) or not MATERIAL_ID_RE.fullmatch(material_id):
                raise GameBuildError(
                    "world.materials ids must be 1–64 ASCII letters, numbers, dots, "
                    "dashes, or underscores"
                )
            if not isinstance(definition, dict):
                raise GameBuildError(f"world.materials.{material_id} must be an object")
            image_id = definition.get("image")
            if not isinstance(image_id, str) or image_id not in images:
                raise GameBuildError(
                    f"world.materials.{material_id}.image must reference a declared image asset"
                )
            for field in ("tileU", "tileV"):
                value = definition.get(field, 8)
                if (
                    isinstance(value, bool)
                    or not isinstance(value, (int, float))
                    or not math.isfinite(value)
                    or value <= 0
                    or value > 4096
                ):
                    raise GameBuildError(
                        f"world.materials.{material_id}.{field} must be between 0 and 4096"
                    )

        material_ids = set(materials)
        ground_material = world.get("groundMaterial")
        if ground_material is not None and ground_material not in material_ids:
            raise GameBuildError("world.groundMaterial must reference a declared material")
        blocks = world.get("blocks", [])
        if isinstance(blocks, list):
            for block in blocks:
                if isinstance(block, dict) and block.get("material") not in (None, *material_ids):
                    raise GameBuildError("world.blocks.material must reference a declared material")


def _validate_world_terrain(manifest: dict[str, object]) -> None:
    worlds = manifest.get("worlds", {})
    world_definitions = [manifest]
    if isinstance(worlds, dict):
        world_definitions.extend(world for world in worlds.values() if isinstance(world, dict))

    terrain_operations_found = False
    for world in world_definitions:
        terrain = world.get("terrain")
        if terrain is None:
            continue
        if not isinstance(terrain, dict):
            raise GameBuildError("world.terrain must be an object")
        cell_size = terrain.get("cellSize", 0.5)
        if (
            isinstance(cell_size, bool)
            or not isinstance(cell_size, (int, float))
            or not math.isfinite(cell_size)
            or not 0.5 <= cell_size <= 8
        ):
            raise GameBuildError("world.terrain.cellSize must be between 0.5 and 8")
        operations = terrain.get("operations", [])
        if not isinstance(operations, list):
            raise GameBuildError("world.terrain.operations must be an array")
        if len(operations) > MAX_TERRAIN_OPERATIONS:
            raise GameBuildError(
                f"world.terrain.operations cannot contain more than {MAX_TERRAIN_OPERATIONS} operations"
            )
        if not isinstance(terrain.get("hideDefaultGround", False), bool):
            raise GameBuildError("world.terrain.hideDefaultGround must be a boolean")
        if not isinstance(terrain.get("materialArt", True), bool):
            raise GameBuildError("world.terrain.materialArt must be a boolean")
        if not operations:
            continue
        terrain_operations_found = True

        parsed_operations = []
        for index, operation in enumerate(operations):
            field = f"world.terrain.operations[{index}]"
            if not isinstance(operation, dict):
                raise GameBuildError(f"{field} must be an object")
            mode = operation.get("operation", "fill")
            shape = operation.get("shape", "block")
            if mode not in ("fill", "carve", "paint"):
                raise GameBuildError(f"{field}.operation must be fill, carve, or paint")
            if shape not in ("block", "box", "ball", "sphere"):
                raise GameBuildError(f"{field}.shape must be block or ball")
            material = operation.get("material", "")
            if mode in ("fill", "paint") and (
                not isinstance(material, str)
                or material.removeprefix("builtin:").lower() not in BUILTIN_TERRAIN_MATERIALS
            ):
                raise GameBuildError(f"{field}.material must be a built-in terrain material")
            position = operation.get("position")
            if (
                not isinstance(position, list)
                or len(position) != 3
                or any(
                    isinstance(value, bool)
                    or not isinstance(value, (int, float))
                    or not math.isfinite(value)
                    or abs(value) > MAX_TERRAIN_COORDINATE
                    for value in position
                )
            ):
                raise GameBuildError(f"{field}.position must contain three finite in-bounds numbers")
            if shape in ("block", "box"):
                size = operation.get("size")
                if (
                    not isinstance(size, list)
                    or len(size) != 3
                    or any(
                        isinstance(value, bool)
                        or not isinstance(value, (int, float))
                        or not math.isfinite(value)
                        or value <= 0
                        for value in size
                    )
                ):
                    raise GameBuildError(f"{field}.size must contain three positive finite numbers")
                bounds = [
                    [position[axis] - size[axis] / 2 for axis in range(3)],
                    [position[axis] + size[axis] / 2 for axis in range(3)],
                ]
            else:
                radius = operation.get("radius")
                if (
                    isinstance(radius, bool)
                    or not isinstance(radius, (int, float))
                    or not math.isfinite(radius)
                    or radius <= 0
                ):
                    raise GameBuildError(f"{field}.radius must be positive and finite")
                bounds = [
                    [position[axis] - radius for axis in range(3)],
                    [position[axis] + radius for axis in range(3)],
                ]
                minimum_feature = radius * 2
            if shape in ("block", "box"):
                minimum_feature = min(size)
            if minimum_feature < cell_size:
                raise GameBuildError(f"{field} feature size must be at least terrain.cellSize")
            if any(abs(value) > MAX_TERRAIN_COORDINATE for side in bounds for value in side):
                raise GameBuildError(f"{field} bounds exceed the supported terrain world limits")
            parsed_operations.append((mode, bounds))

        chunks = set()
        chunk_world_size = cell_size * TERRAIN_CHUNK_CELLS
        for mode, bounds in parsed_operations:
            if mode != "fill":
                continue
            first = [
                math.floor((bounds[0][axis] - cell_size) / chunk_world_size)
                for axis in range(3)
            ]
            end = [
                math.ceil((bounds[1][axis] + cell_size) / chunk_world_size)
                for axis in range(3)
            ]
            for z in range(first[2], end[2]):
                for y in range(first[1], end[1]):
                    for x in range(first[0], end[0]):
                        chunks.add((x, y, z))
                        if len(chunks) > MAX_TERRAIN_CHUNKS:
                            raise GameBuildError(
                                f"world.terrain exceeds the {MAX_TERRAIN_CHUNKS}-chunk limit"
                            )
        sample_count = len(chunks) * (TERRAIN_CHUNK_CELLS + 1) ** 3
        if sample_count > MAX_TERRAIN_SAMPLES:
            raise GameBuildError(
                f"world.terrain exceeds the {MAX_TERRAIN_SAMPLES}-sample limit"
            )

    if terrain_operations_found and manifest.get("sdkVersion") != TERRAIN_SDK_VERSION:
        raise GameBuildError(
            f"world.terrain requires manifest.sdkVersion {TERRAIN_SDK_VERSION}"
        )


def _sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def build_game(
    *,
    source_root: Path,
    manifest_path: Path,
    output: Path,
    zip_path: Path | None = None,
) -> GameBuildResult:
    """Build one portable game package from a project's source and manifest."""

    source_root = source_root.resolve()
    manifest_path = manifest_path.resolve()
    output = output.resolve()
    if not source_root.is_dir():
        raise GameBuildError(f"source directory not found: {source_root}")
    entry = source_root / "main.luau"
    if not entry.is_file():
        raise GameBuildError(f"Luau entry point not found: {entry}")
    if not manifest_path.is_file():
        raise GameBuildError(f"manifest not found: {manifest_path}")
    _validate_output(output, source_root)

    try:
        manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    except UnicodeDecodeError as error:
        raise GameBuildError(f"manifest is not valid UTF-8: {manifest_path}") from error
    except json.JSONDecodeError as error:
        raise GameBuildError(f"manifest is not valid JSON: {error.msg}") from error
    if not isinstance(manifest, dict):
        raise GameBuildError("manifest must contain a JSON object")
    try:
        manifest = expand_manifest_mazes(manifest)
    except MazeBuildError as error:
        raise GameBuildError(str(error)) from error
    manifest = _resolve_effects_source(manifest, manifest_path.parent)

    game_id = _manifest_value(manifest, "id")
    version = _manifest_value(manifest, "version")
    if not is_valid_game_id(game_id):
        raise GameBuildError(
            "manifest.id must use lowercase letters, numbers, and single dashes"
        )

    package = manifest.get("package")
    if package is None:
        package = {"formatVersion": 3, "entry": "game.luau"}
        manifest["package"] = package
    elif not isinstance(package, dict) or package.get("entry") != "game.luau":
        raise GameBuildError("manifest.package.entry must be 'game.luau'")

    display_name = manifest.get("displayName")
    if display_name is None:
        manifest["displayName"] = game_id
    elif (
        not isinstance(display_name, str)
        or not display_name.strip()
        or len(display_name.strip()) > 120
    ):
        raise GameBuildError(
            "manifest.displayName must be a non-empty string of at most 120 characters"
        )
    legacy_version = isinstance(version, int) and not isinstance(version, bool) and version >= 1
    semantic_version = isinstance(version, str) and SEMVER_RE.fullmatch(version) is not None
    if not legacy_version and not semantic_version:
        raise GameBuildError("manifest.version must be SemVer or a legacy positive integer")
    sdk_version = manifest.get("sdkVersion")
    if sdk_version is not None:
        if not isinstance(sdk_version, str) or SEMVER_RE.fullmatch(sdk_version) is None:
            raise GameBuildError("manifest.sdkVersion must be a SemVer string")
        if sdk_version not in SUPPORTED_SDK_VERSIONS:
            raise GameBuildError(
                f"manifest.sdkVersion {sdk_version!r} is unsupported; "
                f"this builder supports {', '.join(sorted(SUPPORTED_SDK_VERSIONS))}"
            )
    _validate_audio_assets(manifest, manifest_path.parent)
    _validate_image_assets(manifest, manifest_path.parent)
    _validate_world_materials(manifest)
    _validate_world_terrain(manifest)

    resolved_zip = zip_path.resolve() if zip_path is not None else None
    if resolved_zip is not None:
        if resolved_zip.is_relative_to(output):
            raise GameBuildError("zip path cannot be inside the package output directory")
        if resolved_zip.is_relative_to(source_root):
            raise GameBuildError("zip path cannot be inside the source directory")
        resolved_zip.parent.mkdir(parents=True, exist_ok=True)

    _validate_existing_output(output)
    output.parent.mkdir(parents=True, exist_ok=True)
    staging: Path | None = _temporary_directory(output.parent, f".{output.name}.staging-")
    archive_temp: Path | None = None
    try:
        sdk_dependencies: dict[str, dict[str, str]] = {}
        generated_script = (
            "-- GENERATED FILE: do not edit; edit src/ and run cubacadabra build-game.\n"
            f"-- game: {game_id}\n"
            f"-- version: {version}\n\n"
            + _bundle_luau_modules(entry, source_root, sdk_dependencies)
            + "\n"
        )
        (staging / "game.luau").write_text(generated_script, encoding="utf-8")
        (staging / "manifest.json").write_text(
            json.dumps(manifest, indent=2) + "\n",
            encoding="utf-8",
        )

        assets = manifest_path.parent / "assets"
        if assets.is_dir():
            shutil.copytree(
                assets,
                staging / "assets",
                ignore=shutil.ignore_patterns(".DS_Store"),
            )

        payload_files = sorted(
            path.relative_to(staging).as_posix()
            for path in staging.rglob("*")
            if path.is_file()
        )
        package_info = {
            "formatVersion": package.get("formatVersion", 3),
            "id": game_id,
            "version": version,
            "runtime": {"api": sdk_version or PREVIEW_SDK_VERSION},
            "dependencies": sdk_dependencies,
            "dependencyPolicy": "canonical-toolchain",
            "entry": "game.luau",
            "manifest": "manifest.json",
            "files": payload_files,
            "sha256": {
                name: _sha256_file(staging / name)
                for name in payload_files
            },
        }
        (staging / "package.json").write_text(
            json.dumps(package_info, indent=2) + "\n",
            encoding="utf-8",
        )
        (staging / BUILD_MARKER).write_text(BUILD_MARKER_CONTENT, encoding="utf-8")

        if resolved_zip is not None:
            archive_temp = _write_archive(staging, resolved_zip)

        _replace_output(staging, output)
        staging = None
        if archive_temp is not None:
            os.replace(archive_temp, resolved_zip)
            archive_temp = None
    finally:
        if staging is not None and staging.exists():
            shutil.rmtree(staging)
        if archive_temp is not None:
            archive_temp.unlink(missing_ok=True)
    return GameBuildResult(
        game_id=game_id,
        version=version,
        output=output,
        zip_path=resolved_zip,
    )
