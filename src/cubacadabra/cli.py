"""Command-line entry point for the Cubacadabra developer tools."""

from __future__ import annotations

import argparse
import sys
from collections.abc import Sequence
from pathlib import Path

from . import __version__
from .game_builder import GameBuildError, build_game
from .game_creator import GameCreateError, create_game


DESCRIPTION = "Tools for building and maintaining Cubacadabra projects."
EPILOG = "More commands will be added here as the Cubacadabra toolset grows."


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="cubacadabra",
        description=DESCRIPTION,
        epilog=EPILOG,
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument(
        "--version",
        action="version",
        version=f"%(prog)s {__version__}",
    )
    parser.add_argument(
        "--create-game",
        action="store_true",
        help="Create a new starter game (use with --title and --path).",
    )
    parser.add_argument("--title", help=argparse.SUPPRESS)
    parser.add_argument("--path", type=Path, help=argparse.SUPPRESS)

    commands = parser.add_subparsers(
        dest="command",
        metavar="COMMAND",
        title="commands",
        description="Run one of the available project tools.",
    )
    build_parser = commands.add_parser(
        "build-game",
        help="Build a portable game package from a game project.",
        description=(
            "Assemble a game's Luau source and manifest into the portable "
            "package consumed by Cubacadabra clients."
        ),
        epilog=(
            "Examples:\n"
            "  cubacadabra build-game\n"
            "  cubacadabra build-game ../first-game --output /tmp/first-game\n"
            "  cubacadabra build-game ../second-game --zip build/second-game.zip"
        ),
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    build_parser.add_argument(
        "project",
        nargs="?",
        type=Path,
        default=Path("."),
        help="Game project directory (default: current directory).",
    )
    build_parser.add_argument(
        "--source",
        dest="source_dir",
        type=Path,
        default=Path("src"),
        metavar="DIR",
        help="Source directory relative to the project (default: src).",
    )
    build_parser.add_argument(
        "--manifest",
        type=Path,
        default=Path("manifest.json"),
        metavar="FILE",
        help="Manifest path relative to the project (default: manifest.json).",
    )
    build_parser.add_argument(
        "--output",
        type=Path,
        default=None,
        metavar="DIR",
        help="Package directory (default: PROJECT/build/package).",
    )
    build_parser.add_argument(
        "--zip",
        dest="zip_path",
        type=Path,
        default=None,
        metavar="FILE",
        help="Also write a distributable ZIP archive.",
    )
    build_parser.set_defaults(handler=_run_build_game)

    create_parser = commands.add_parser(
        "create-game",
        help="Create a new starter game.",
        description="Create a new Cubacadabra game directory and starter files.",
        epilog=(
            "Examples:\n"
            '  cubacadabra create-game --title "The Wild West" --path ~/games\n'
            '  cubacadabra --create-game --title "The Wild West" --path ~/games'
        ),
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    create_parser.add_argument(
        "--title",
        required=True,
        help="Display name for the game.",
    )
    create_parser.add_argument(
        "--path",
        required=True,
        type=Path,
        metavar="DIR",
        help="Directory in which to create the game directory.",
    )
    create_parser.set_defaults(handler=_run_create_game)
    return parser


def _project_path(project: Path, value: Path) -> Path:
    """Resolve a project-relative CLI path without changing explicit paths."""

    if value.is_absolute():
        return value
    return project / value


def _run_build_game(args: argparse.Namespace) -> int:
    project = args.project.resolve()
    source_dir = _project_path(project, args.source_dir).resolve()
    manifest = _project_path(project, args.manifest).resolve()
    if args.output is None:
        output = (project / "build/package").resolve()
    else:
        output = (
            args.output.resolve()
            if args.output.is_absolute()
            else (Path.cwd() / args.output).resolve()
        )
    zip_path = None
    if args.zip_path is not None:
        zip_path = (
            args.zip_path.resolve()
            if args.zip_path.is_absolute()
            else (Path.cwd() / args.zip_path).resolve()
        )

    try:
        result = build_game(
            source_root=source_dir,
            manifest_path=manifest,
            output=output,
            zip_path=zip_path,
        )
    except (GameBuildError, OSError) as error:
        print(f"cubacadabra build-game failed: {error}", file=sys.stderr)
        return 1

    print(f"Built {result.game_id} v{result.version} -> {result.output}")
    if result.zip_path is not None:
        print(f"Wrote package archive -> {result.zip_path}")
    return 0


def _run_create_game(args: argparse.Namespace) -> int:
    try:
        result = create_game(title=args.title, path=args.path)
    except (GameCreateError, OSError) as error:
        print(f"cubacadabra create-game failed: {error}", file=sys.stderr)
        return 1

    print(f"Created {result.display_name} ({result.game_id}) -> {result.project}")
    return 0


def main(argv: Sequence[str] | None = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)
    if args.create_game:
        if args.title is None or args.path is None:
            parser.error("--create-game requires --title and --path")
        return _run_create_game(args)
    if not hasattr(args, "handler"):
        parser.print_help()
        return 0
    return args.handler(args)
