"""Seed a Wrangler Local Explorer R2 bucket from the morph starter set."""

from __future__ import annotations

import hashlib
import json
import mimetypes
from dataclasses import dataclass
from pathlib import Path
from urllib.error import HTTPError, URLError
from urllib.parse import quote, urlencode, urljoin
from urllib.request import Request, urlopen


DEFAULT_ENDPOINT = "http://127.0.0.1:8787"
DEFAULT_BUCKET = "prod"


class LocalR2SetupError(Exception):
    """Raised when local R2 cannot be safely seeded."""


@dataclass(frozen=True)
class LocalR2SetupResult:
    uploaded: int
    skipped: int
    bytes_uploaded: int
    files: int


@dataclass(frozen=True)
class _RemoteObject:
    key: str
    size: int


class LocalExplorerClient:
    """Small standard-library client for Wrangler's Local Explorer API."""

    def __init__(self, endpoint: str = DEFAULT_ENDPOINT, timeout: float = 30.0) -> None:
        self.endpoint = endpoint.rstrip("/")
        self.timeout = timeout

    @property
    def api_base(self) -> str:
        return self.endpoint + "/cdn-cgi/explorer/api/"

    def list_buckets(self) -> list[str]:
        payload = self._json("GET", "r2/buckets")
        result = payload.get("result") or []
        if isinstance(result, dict):
            result = result.get("buckets") or []
        buckets: list[str] = []
        for item in result:
            if isinstance(item, str):
                buckets.append(item)
            elif isinstance(item, dict):
                name = item.get("name") or item.get("bucket") or item.get("bucket_name")
                if name:
                    buckets.append(str(name))
        return buckets

    def list_objects(self, bucket: str) -> list[_RemoteObject]:
        objects: list[_RemoteObject] = []
        cursor: str | None = None
        while True:
            params: dict[str, str | int] = {"per_page": 1000}
            if cursor:
                params["cursor"] = cursor
            payload = self._json(
                "GET",
                f"r2/buckets/{quote(bucket, safe='')}/objects",
                params=params,
            )
            for item in payload.get("result") or []:
                if isinstance(item, dict) and item.get("key"):
                    objects.append(
                        _RemoteObject(key=str(item["key"]), size=int(item.get("size") or 0))
                    )
            info = payload.get("result_info") or {}
            if not isinstance(info, dict):
                break
            next_cursor = info.get("cursor") or info.get("next_cursor")
            truncated = str(info.get("is_truncated", "false")).lower() == "true"
            if not truncated or not next_cursor:
                break
            cursor = str(next_cursor)
        return objects

    def get_object(self, bucket: str, key: str) -> bytes:
        response = self._request(
            "GET",
            f"r2/buckets/{quote(bucket, safe='')}/objects/{quote(key, safe='')}",
        )
        return response.read()

    def put_object(self, bucket: str, key: str, body: bytes, content_type: str) -> None:
        self._json(
            "PUT",
            f"r2/buckets/{quote(bucket, safe='')}/objects/{quote(key, safe='')}",
            data=body,
            headers={"content-type": content_type},
        )

    def _json(
        self,
        method: str,
        path: str,
        *,
        params: dict[str, str | int] | None = None,
        data: bytes | None = None,
        headers: dict[str, str] | None = None,
    ) -> dict[str, object]:
        response = self._request(method, path, params=params, data=data, headers=headers)
        try:
            payload = json.loads(response.read().decode("utf-8"))
        except json.JSONDecodeError as error:
            raise LocalR2SetupError(f"Local Explorer returned invalid JSON for {path}") from error
        if not isinstance(payload, dict) or payload.get("success") is False:
            raise LocalR2SetupError(f"Local Explorer request failed for {path}: {payload!r}")
        return payload

    def _request(
        self,
        method: str,
        path: str,
        *,
        params: dict[str, str | int] | None = None,
        data: bytes | None = None,
        headers: dict[str, str] | None = None,
    ):
        url = urljoin(self.api_base, path)
        if params:
            url += "?" + urlencode(params)
        try:
            return urlopen(Request(url, data=data, method=method, headers=headers or {}), timeout=self.timeout)
        except HTTPError as error:
            body = error.read().decode("utf-8", errors="replace")
            raise LocalR2SetupError(f"{method} {url} failed with HTTP {error.code}: {body}") from error
        except URLError as error:
            raise LocalR2SetupError(
                f"{method} {url} failed: {error.reason}; is Wrangler dev running?"
            ) from error


def default_starter_set() -> Path:
    """Locate the checked-in fixture directory in a source or editable checkout."""

    return Path(__file__).resolve().parents[2] / "starter-set"


def setup_local_r2(
    starter_set: Path,
    *,
    endpoint: str = DEFAULT_ENDPOINT,
    bucket: str = DEFAULT_BUCKET,
    dry_run: bool = False,
    client: LocalExplorerClient | None = None,
    printer=print,
) -> LocalR2SetupResult:
    root = starter_set.expanduser().resolve()
    if not root.is_dir():
        raise LocalR2SetupError(f"starter set does not exist: {root}")
    files = _fixture_files(root)
    if not files:
        raise LocalR2SetupError(f"starter set has no runtime or source files: {root}")
    for path in files:
        _validate_fixture_path(root, path)

    explorer = client or LocalExplorerClient(endpoint)
    if bucket not in explorer.list_buckets():
        raise LocalR2SetupError(
            f"local R2 bucket {bucket!r} was not found at {endpoint}; start the backend first"
        )
    remote = {item.key: item for item in explorer.list_objects(bucket)}

    uploaded = skipped = bytes_uploaded = 0
    for path in files:
        key = path.relative_to(root).as_posix()
        body = path.read_bytes()
        previous = remote.get(key)
        if previous is not None:
            if hashlib.sha256(explorer.get_object(bucket, key)).digest() == hashlib.sha256(body).digest():
                skipped += 1
                printer(f"skip {key}")
                continue
            if key.startswith("runtime/"):
                raise LocalR2SetupError(
                    f"immutable runtime object differs from starter set: {key}"
                )
        if dry_run:
            printer(f"would upload {key} ({len(body)} bytes)")
            continue
        content_type = mimetypes.guess_type(path.name)[0] or "application/octet-stream"
        explorer.put_object(bucket, key, body, content_type)
        uploaded += 1
        bytes_uploaded += len(body)
        printer(f"upload {key} ({len(body)} bytes)")

    return LocalR2SetupResult(
        uploaded=uploaded,
        skipped=skipped,
        bytes_uploaded=bytes_uploaded,
        files=len(files),
    )


def _fixture_files(root: Path) -> list[Path]:
    files: list[Path] = []
    for directory_name in ("runtime", "source"):
        directory = root / directory_name
        if directory.is_dir():
            files.extend(path for path in directory.rglob("*") if path.is_file())
    return sorted(files)


def _validate_fixture_path(root: Path, path: Path) -> None:
    relative = path.relative_to(root).as_posix()
    if relative.startswith("runtime/morphs/sha256/") and relative.endswith(".morphpack"):
        digest = path.stem
        prefix = Path(relative).parts[3]
        if len(digest) != 64 or any(character not in "0123456789abcdef" for character in digest):
            raise LocalR2SetupError(f"runtime morph pack is not SHA-256 named: {relative}")
        if prefix != digest[:2]:
            raise LocalR2SetupError(f"runtime morph pack prefix does not match hash: {relative}")
        actual = hashlib.sha256(path.read_bytes()).hexdigest()
        if actual != digest:
            raise LocalR2SetupError(
                f"runtime morph pack content hash does not match filename: {relative}"
            )
