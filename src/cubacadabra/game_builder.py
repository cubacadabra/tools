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
import zipfile
from dataclasses import dataclass
from pathlib import Path, PurePosixPath


INCLUDE_RE = re.compile(r'^\s*--\s*@include\s+"([^"]+)"\s*$')


class GameBuildError(ValueError):
    """An input project cannot be turned into a valid game package."""


@dataclass(frozen=True)
class GameBuildResult:
    """The useful outputs and metadata produced by a successful build."""

    game_id: str
    version: int
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


def _validate_output(output: Path, source_root: Path) -> None:
    """Prevent a package output from deleting the source tree."""

    try:
        output.relative_to(source_root)
    except ValueError:
        return
    raise GameBuildError(f"output directory cannot be inside the source directory: {output}")


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

    game_id = _manifest_value(manifest, "id")
    version = _manifest_value(manifest, "version")
    if not isinstance(game_id, str) or not game_id:
        raise GameBuildError("manifest.id must be a non-empty string")
    if not isinstance(version, int) or isinstance(version, bool) or version < 1:
        raise GameBuildError("manifest.version must be a positive integer")

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
    (output / "game.luau").write_text(generated_script, encoding="utf-8", newline="\n")
    shutil.copyfile(manifest_path, output / "manifest.json")

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
        newline="\n",
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
