"""Build portable Cubacadabra game packages.

Game source modules can include one another with a directive such as::

    -- @include "ui/document.luau"

The builder expands those directives into one deterministic Luau entry chunk,
then copies the manifest and optional assets into a client-ready package.
"""

from __future__ import annotations

import json
import re
import shutil
import wave
import zipfile
from dataclasses import dataclass
from pathlib import Path, PurePosixPath


INCLUDE_RE = re.compile(r'^\s*--\s*@include\s+"([^"]+)"\s*$')
SDK_INCLUDE_PREFIX = "@cubacadabra/"
SDK_INCLUDE_RE = re.compile(r"^@cubacadabra/[a-z0-9-]+\.luau$")
SDK_INCLUDES = {
    "@cubacadabra/disclosure-v1.luau": "disclosure.luau",
    "@cubacadabra/obby-v1.luau": "obby.luau",
    "@cubacadabra/survival-v1.luau": "survival.luau",
    "@cubacadabra/shared-state-v1.luau": "shared-state.luau",
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
MAX_IMAGE_ASSETS = 1
MAX_IMAGE_ASSET_BYTES = 8 * 1024 * 1024
PREVIEW_SDK_VERSION = "0.3.0"


class GameBuildError(ValueError):
    """An input project cannot be turned into a valid game package."""


def _read_sdk_include(include_value: str) -> str:
    if not SDK_INCLUDE_RE.fullmatch(include_value):
        raise GameBuildError(
            "Cubacadabra SDK includes must use "
            '"@cubacadabra/<module-name>.luau"'
        )
    sdk_file = SDK_INCLUDES.get(include_value)
    if sdk_file is None:
        raise GameBuildError(f"unknown Cubacadabra SDK include: {include_value}")
    sdk_path = Path(__file__).with_name("sdk") / sdk_file

    try:
        source = sdk_path.read_text(encoding="utf-8")
    except UnicodeDecodeError as error:
        raise GameBuildError(f"{include_value}: SDK source is not valid UTF-8") from error

    for line_number, line in enumerate(source.splitlines(), start=1):
        if re.match(r"^return(?:\s|$)", line):
            raise GameBuildError(
                f"{include_value}:{line_number}: SDK modules share the entry "
                "chunk and cannot contain a top-level return"
            )
        if INCLUDE_RE.match(line):
            raise GameBuildError(
                f"{include_value}:{line_number}: SDK modules cannot include other files"
            )
    return source


@dataclass(frozen=True)
class GameBuildResult:
    """The useful outputs and metadata produced by a successful build."""

    game_id: str
    version: int | str
    output: Path
    zip_path: Path | None


def _read_source_file(
    path: Path,
    source_root: Path,
    stack: tuple[Path, ...],
) -> str:
    relative = path.relative_to(source_root).as_posix()
    if path in stack:
        chain = " -> ".join(item.relative_to(source_root).as_posix() for item in (*stack, path))
        raise GameBuildError(f"cyclic Luau include: {chain}")

    lines: list[str] = []
    try:
        source = path.read_text(encoding="utf-8")
    except UnicodeDecodeError as error:
        raise GameBuildError(f"{relative}: source is not valid UTF-8") from error

    for line_number, line in enumerate(source.splitlines(), start=1):
        if stack and re.match(r"^return(?:\s|$)", line):
            raise GameBuildError(
                f"{relative}:{line_number}: included files share the entry chunk "
                "and cannot contain a top-level return"
            )

        match = INCLUDE_RE.match(line)
        if not match:
            lines.append(line)
            continue

        if match.group(1).startswith(SDK_INCLUDE_PREFIX):
            include_name = match.group(1)
            lines.append(f"-- begin SDK include: {include_name}")
            lines.append(_read_sdk_include(include_name))
            lines.append(f"-- end SDK include: {include_name}")
            continue

        include_value = PurePosixPath(match.group(1))
        if (
            not match.group(1)
            or include_value.is_absolute()
            or ".." in include_value.parts
        ):
            raise GameBuildError(f"{relative}:{line_number}: include must stay inside src/")

        include_path = source_root.joinpath(*include_value.parts)
        try:
            include_path = include_path.resolve().relative_to(source_root.resolve())
        except ValueError as error:
            raise GameBuildError(
                f"{relative}:{line_number}: include must stay inside src/"
            ) from error
        include_path = source_root / include_path
        if not include_path.is_file():
            raise GameBuildError(
                f"{relative}:{line_number}: included file not found: {include_value}"
            )

        lines.append(f"-- begin include: {include_value}")
        lines.append(_read_source_file(include_path, source_root, (*stack, path)))
        lines.append(f"-- end include: {include_value}")

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
    """Prevent a package output from deleting the source tree."""

    try:
        output.relative_to(source_root)
    except ValueError:
        return
    raise GameBuildError(f"output directory cannot be inside the source directory: {output}")


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
    manifest = _resolve_effects_source(manifest, manifest_path.parent)

    game_id = _manifest_value(manifest, "id")
    version = _manifest_value(manifest, "version")
    if not isinstance(game_id, str) or not game_id:
        raise GameBuildError("manifest.id must be a non-empty string")
    legacy_version = isinstance(version, int) and not isinstance(version, bool) and version >= 1
    semantic_version = isinstance(version, str) and SEMVER_RE.fullmatch(version) is not None
    if not legacy_version and not semantic_version:
        raise GameBuildError("manifest.version must be SemVer or a legacy positive integer")
    sdk_version = manifest.get("sdkVersion")
    if sdk_version is not None:
        if not isinstance(sdk_version, str) or SEMVER_RE.fullmatch(sdk_version) is None:
            raise GameBuildError("manifest.sdkVersion must be a SemVer string")
        if sdk_version != PREVIEW_SDK_VERSION:
            raise GameBuildError(
                f"manifest.sdkVersion {sdk_version!r} is unsupported; "
                f"this builder supports {PREVIEW_SDK_VERSION}"
            )
    _validate_audio_assets(manifest, manifest_path.parent)
    _validate_image_assets(manifest, manifest_path.parent)

    if output.exists():
        if not output.is_dir():
            raise GameBuildError(f"output exists and is not a directory: {output}")
        shutil.rmtree(output)
    output.mkdir(parents=True)

    generated_script = (
        "-- GENERATED FILE: do not edit; edit src/ and run cubacadabra build-game.\n"
        f"-- game: {game_id}\n"
        f"-- version: {version}\n\n"
        + _read_source_file(entry, source_root, ())
        + "\n"
    )
    (output / "game.luau").write_text(generated_script, encoding="utf-8")
    (output / "manifest.json").write_text(
        json.dumps(manifest, indent=2) + "\n",
        encoding="utf-8",
    )

    assets = manifest_path.parent / "assets"
    if assets.is_dir():
        shutil.copytree(assets, output / "assets")

    payload_files = sorted(
        path.relative_to(output).as_posix()
        for path in output.rglob("*")
        if path.is_file()
    )
    package_info = {
        "formatVersion": 1,
        "id": game_id,
        "version": version,
        "entry": "game.luau",
        "manifest": "manifest.json",
        "files": payload_files,
    }
    (output / "package.json").write_text(
        json.dumps(package_info, indent=2) + "\n",
        encoding="utf-8",
    )

    resolved_zip = zip_path.resolve() if zip_path is not None else None
    if resolved_zip is not None:
        resolved_zip.parent.mkdir(parents=True, exist_ok=True)
        if resolved_zip.exists():
            resolved_zip.unlink()
        with zipfile.ZipFile(resolved_zip, "w", compression=zipfile.ZIP_DEFLATED) as archive:
            for path in sorted(output.rglob("*")):
                if path.is_file():
                    archive.write(path, path.relative_to(output).as_posix())

    return GameBuildResult(
        game_id=game_id,
        version=version,
        output=output,
        zip_path=resolved_zip,
    )
