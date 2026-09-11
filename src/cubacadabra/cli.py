"""Command-line entry point for the Cubacadabra developer tools."""

from __future__ import annotations

import argparse
import sys
from collections.abc import Sequence
from pathlib import Path

from . import __version__
from .examples_uploader import (
    DEFAULT_BACKEND_URL,
    DEFAULT_REVIEW_EMAIL,
    PRODUCTION_BACKEND_URL,
    ExampleUploadError,
    default_password,
    upload_examples,
)
from .game_builder import GameBuildError, build_game
from .game_creator import GameCreateError, create_game
from .local_r2_setup import (
    DEFAULT_BUCKET,
    DEFAULT_ENDPOINT,
    LocalR2SetupError,
    default_starter_set,
    setup_local_r2,
)


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
        help=(
            "Source directory relative to the project (default: src); "
            "an absolute game directory is also accepted."
        ),
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

    upload_parser = commands.add_parser(
        "upload-examples",
        help="Bump, build, and upload both example games.",
        description=(
            "Bump the patch version in both example manifests, build their "
            "portable ZIP packages, sign in with the review account, and "
            "upload them to the cube backend."
        ),
        epilog=(
            "Examples:\n"
            "  cubacadabra upload-examples\n"
            "  cubacadabra upload-examples --target production\n"
            "  cubacadabra upload-examples --no-bump  # retry an existing build"
        ),
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    upload_parser.add_argument(
        "--examples-dir",
        type=Path,
        default=Path("../examples"),
        metavar="DIR",
        help="Examples repository directory (default: ../examples).",
    )
    upload_parser.add_argument(
        "--build-dir",
        type=Path,
        default=Path("/tmp/cubacadabra-examples"),
        metavar="DIR",
        help="Package build directory (default: /tmp/cubacadabra-examples).",
    )
    upload_parser.add_argument(
        "--zip-dir",
        type=Path,
        default=Path(".."),
        metavar="DIR",
        help="Directory for the two ZIP archives (default: ..).",
    )
    upload_parser.add_argument(
        "--target",
        choices=("local", "production"),
        default="local",
        help=(
            "Backend target: local uses http://127.0.0.1:8787; production "
            "uses https://api.cubacadabra.com (default: local)."
        ),
    )
    upload_parser.add_argument(
        "--backend-url",
        default=None,
        metavar="URL",
        help="Override the backend URL selected by --target.",
    )
    upload_parser.add_argument(
        "--email",
        default=DEFAULT_REVIEW_EMAIL,
        help=f"Review account email (default: {DEFAULT_REVIEW_EMAIL}).",
    )
    upload_parser.add_argument(
        "--password",
        default=None,
        help="Review account password (default: CUBACADABRA_REVIEW_PASSWORD or testing).",
    )
    upload_parser.add_argument(
        "--no-bump",
        action="store_true",
        help="Keep current manifest versions; useful for retrying an upload.",
    )
    upload_parser.set_defaults(handler=_run_upload_examples)

    setup_parser = commands.add_parser(
        "setup-local",
        help="Seed Wrangler local R2 from the checked-in morph starter set.",
        description=(
            "Upload the starter-set runtime and source morph objects to a "
            "Wrangler Local Explorer R2 bucket. This does not apply D1 migrations."
        ),
    )
    setup_parser.add_argument(
        "--starter-set",
        type=Path,
        default=default_starter_set(),
        metavar="DIR",
        help="Starter set directory (default: the tools checkout's starter-set).",
    )
    setup_parser.add_argument(
        "--endpoint",
        default=DEFAULT_ENDPOINT,
        help=f"Wrangler dev endpoint (default: {DEFAULT_ENDPOINT}).",
    )
    setup_parser.add_argument(
        "--bucket",
        default=DEFAULT_BUCKET,
        help=f"Local R2 bucket name (default: {DEFAULT_BUCKET}).",
    )
    setup_parser.add_argument(
        "--dry-run",
        action="store_true",
        help="Validate and show uploads without writing R2 objects.",
    )
    setup_parser.set_defaults(handler=_run_setup_local)

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


def _source_path(project: Path, value: Path) -> Path:
    """Resolve a source path, accepting either a source dir or project root."""

    source = _project_path(project, value).resolve()
    if not (source / "main.luau").is_file() and (source / "src/main.luau").is_file():
        return source / "src"
    return source


def _run_build_game(args: argparse.Namespace) -> int:
    project = args.project.resolve()
    source_dir = _source_path(project, args.source_dir)
    manifest = _project_path(project, args.manifest).resolve()
    if args.manifest == Path("manifest.json") and not manifest.is_file():
        for candidate in (source_dir / "manifest.json", source_dir.parent / "manifest.json"):
            if candidate.is_file():
                manifest = candidate.resolve()
                break
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


def _run_upload_examples(args: argparse.Namespace) -> int:
    backend_url = args.backend_url or (
        PRODUCTION_BACKEND_URL if args.target == "production" else DEFAULT_BACKEND_URL
    )
    try:
        plans, results = upload_examples(
            examples_dir=args.examples_dir,
            build_dir=args.build_dir,
            zip_dir=args.zip_dir,
            backend_url=backend_url,
            email=args.email,
            password=args.password if args.password is not None else default_password(),
            bump_versions=not args.no_bump,
        )
    except (ExampleUploadError, OSError) as error:
        print(f"cubacadabra upload-examples failed: {error}", file=sys.stderr)
        return 1

    for plan, result in zip(plans, results):
        if plan.previous_version != plan.version:
            print(f"Bumped {plan.game_id}: {plan.previous_version} -> {plan.version}")
        state = "already uploaded" if result.already_exists else "uploaded"
        print(f"{state} {result.game_id} v{result.version} -> {result.zip_path}")
    return 0


def _run_setup_local(args: argparse.Namespace) -> int:
    try:
        result = setup_local_r2(
            args.starter_set,
            endpoint=args.endpoint,
            bucket=args.bucket,
            dry_run=args.dry_run,
        )
    except (LocalR2SetupError, OSError) as error:
        print(f"cubacadabra setup-local failed: {error}", file=sys.stderr)
        return 1

    action = "would seed" if args.dry_run else "seeded"
    print(
        f"{action} {args.bucket}: files={result.files} "
        f"uploaded={result.uploaded} skipped={result.skipped} "
        f"bytes_uploaded={result.bytes_uploaded}"
    )
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
