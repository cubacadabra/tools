"""Build and upload the repository's example games."""

from __future__ import annotations

import json
import os
import re
import urllib.error
import urllib.request
from dataclasses import dataclass
from pathlib import Path
from typing import Any

DEFAULT_REVIEW_EMAIL = "play-review@cubacadabra.com"
DEFAULT_REVIEW_PASSWORD = "testing"
DEFAULT_BACKEND_URL = "http://127.0.0.1:8787"
PRODUCTION_BACKEND_URL = "https://api.cubacadabra.com"
EXAMPLE_NAMES = ("the-wild-west", "survival-101", "adventure-101")
SEMVER_RE = re.compile(
    r"^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)"
    r"(?:-[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?"
    r"(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?$"
)


class ExampleUploadError(RuntimeError):
    """The example build or upload workflow could not be completed."""


@dataclass(frozen=True)
class ExamplePlan:
    """One example project and the package locations used for it."""

    game_id: str
    project: Path
    manifest: Path
    output: Path
    zip_path: Path
    previous_version: str | int
    version: str | int


@dataclass(frozen=True)
class ExampleUploadResult:
    """The result reported by the backend for one uploaded example."""

    game_id: str
    version: str | int
    zip_path: Path
    already_exists: bool


def _next_patch_version(version: object) -> str | int:
    if isinstance(version, int) and not isinstance(version, bool) and version >= 1:
        return version + 1
    if not isinstance(version, str) or SEMVER_RE.fullmatch(version) is None:
        raise ExampleUploadError(
            "manifest.version must be SemVer or a legacy positive integer"
        )

    match = re.match(r"^(\d+)\.(\d+)\.(\d+)", version)
    assert match is not None
    return f"{match.group(1)}.{match.group(2)}.{int(match.group(3)) + 1}"


def _read_manifest(path: Path) -> dict[str, Any]:
    try:
        manifest = json.loads(path.read_text(encoding="utf-8"))
    except FileNotFoundError as error:
        raise ExampleUploadError(f"manifest not found: {path}") from error
    except UnicodeDecodeError as error:
        raise ExampleUploadError(f"manifest is not valid UTF-8: {path}") from error
    except json.JSONDecodeError as error:
        raise ExampleUploadError(f"manifest is not valid JSON: {error.msg}") from error
    if not isinstance(manifest, dict):
        raise ExampleUploadError(f"manifest must contain a JSON object: {path}")
    return manifest


def _write_manifest(path: Path, manifest: dict[str, Any]) -> None:
    path.write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")


def _response_json(response: Any) -> dict[str, Any]:
    try:
        result = json.loads(response.read().decode("utf-8"))
    except (UnicodeDecodeError, json.JSONDecodeError):
        return {}
    return result if isinstance(result, dict) else {}


class BackendClient:
    """Small stdlib-only client for the auth and cube upload endpoints."""

    def __init__(self, base_url: str, timeout: float = 60.0) -> None:
        normalized = base_url.rstrip("/")
        if not normalized.startswith(("http://", "https://")):
            raise ExampleUploadError("backend URL must start with http:// or https://")
        self.base_url = normalized
        self.timeout = timeout
        self._session_cookie: str | None = None

    def _request(
        self,
        path: str,
        *,
        method: str,
        data: bytes,
        content_type: str,
    ) -> tuple[int, dict[str, Any], Any]:
        request = urllib.request.Request(
            f"{self.base_url}{path}",
            data=data,
            method=method,
            headers={
                "Accept": "application/json",
                "Content-Type": content_type,
            },
        )
        if self._session_cookie is not None:
            request.add_header("Cookie", self._session_cookie)

        try:
            response = urllib.request.urlopen(request, timeout=self.timeout)
        except urllib.error.HTTPError as error:
            body = _response_json(error)
            return error.code, body, error
        except (urllib.error.URLError, TimeoutError, OSError) as error:
            raise ExampleUploadError(
                f"could not reach backend at {self.base_url}: {error}"
            ) from error

        return response.status, _response_json(response), response

    def login(self, email: str, password: str) -> None:
        status, body, response = self._request(
            "/auth/email",
            method="POST",
            data=json.dumps({"email": email, "password": password}).encode("utf-8"),
            content_type="application/json",
        )
        try:
            if status != 200:
                error = body.get("error", "login_failed")
                raise ExampleUploadError(
                    f"backend login failed ({status}): {error}"
                )

            cookies = response.headers.get_all("Set-Cookie") or []
            for cookie in cookies:
                cookie_pair = cookie.split(";", 1)[0]
                if cookie_pair.startswith("cubacadabra_session="):
                    self._session_cookie = cookie_pair
                    break
            if self._session_cookie is None:
                raise ExampleUploadError("backend login did not return a session cookie")
        finally:
            response.close()

    def upload(self, zip_path: Path) -> tuple[dict[str, Any], bool]:
        try:
            archive = zip_path.read_bytes()
        except OSError as error:
            raise ExampleUploadError(f"could not read {zip_path}: {error}") from error

        status, body, response = self._request(
            "/cubes/upload",
            method="POST",
            data=archive,
            content_type="application/zip",
        )
        try:
            if status == 201:
                return body, False
            if status == 409 and body.get("error") == "cube_already_exists":
                return body, True
            error = body.get("error", "cube_upload_failed")
            raise ExampleUploadError(
                f"backend upload failed for {zip_path.name} ({status}): {error}"
            )
        finally:
            response.close()


def _plans(
    examples_dir: Path,
    build_dir: Path,
    zip_dir: Path,
    *,
    bump_versions: bool,
) -> list[ExamplePlan]:
    plans: list[ExamplePlan] = []
    for game_id in EXAMPLE_NAMES:
        project = (examples_dir / game_id).resolve()
        manifest_path = project / "manifest.json"
        manifest = _read_manifest(manifest_path)
        manifest_id = manifest.get("id")
        if manifest_id != game_id:
            raise ExampleUploadError(
                f"{manifest_path}: expected manifest.id to be {game_id!r}, "
                f"got {manifest_id!r}"
            )
        version = manifest.get("version")
        next_version = _next_patch_version(version) if bump_versions else version
        if not isinstance(next_version, (int, str)) or isinstance(next_version, bool):
            raise ExampleUploadError(f"{manifest_path}: manifest.version is missing")
        output = (build_dir / game_id).resolve()
        if output.is_relative_to(project):
            raise ExampleUploadError(
                f"build output must not be inside the example project: {output}"
            )
        plans.append(
            ExamplePlan(
                game_id=game_id,
                project=project,
                manifest=manifest_path,
                output=output,
                zip_path=(zip_dir / f"{game_id}.zip").resolve(),
                previous_version=version,
                version=next_version,
            )
        )
    return plans


def upload_examples(
    *,
    examples_dir: Path,
    build_dir: Path,
    zip_dir: Path,
    backend_url: str,
    email: str = DEFAULT_REVIEW_EMAIL,
    password: str = DEFAULT_REVIEW_PASSWORD,
    bump_versions: bool = True,
) -> tuple[list[ExamplePlan], list[ExampleUploadResult]]:
    """Bump, build, authenticate, and upload the example games."""

    plans = _plans(
        examples_dir.resolve(),
        build_dir,
        zip_dir,
        bump_versions=bump_versions,
    )

    client = BackendClient(backend_url)
    client.login(email, password)

    if bump_versions:
        for plan in plans:
            manifest = _read_manifest(plan.manifest)
            manifest["version"] = plan.version
            _write_manifest(plan.manifest, manifest)

    from .game_builder import build_game

    for plan in plans:
        try:
            build_game(
                source_root=plan.project / "src",
                manifest_path=plan.manifest,
                output=plan.output,
                zip_path=plan.zip_path,
            )
        except (OSError, ValueError) as error:
            raise ExampleUploadError(f"could not build {plan.game_id}: {error}") from error

    results: list[ExampleUploadResult] = []
    for plan in plans:
        body, already_exists = client.upload(plan.zip_path)
        cube = body.get("cube")
        reported_version = (
            cube.get("version", plan.version)
            if isinstance(cube, dict)
            else plan.version
        )
        results.append(
            ExampleUploadResult(
                game_id=plan.game_id,
                version=reported_version,
                zip_path=plan.zip_path,
                already_exists=already_exists,
            )
        )
    return plans, results


def default_password() -> str:
    """Allow CI or local overrides without requiring a password argument."""

    return os.environ.get("CUBACADABRA_REVIEW_PASSWORD", DEFAULT_REVIEW_PASSWORD)
