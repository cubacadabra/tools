"""Build deterministic Morph release manifests from authored source files."""

from __future__ import annotations

import hashlib
import json
import os
import subprocess
import tempfile
from dataclasses import dataclass
from pathlib import Path


class MorphReleaseError(Exception):
    """Raised when a Morph release cannot be built or validated."""


@dataclass(frozen=True)
class MorphReleaseResult:
    release_id: str
    lock_path: Path
    runtime_root: Path
    assets: int
    packs: int


def write_release_sql(lock_path: Path, output: Path | None = None, channel: str = "production") -> Path:
    """Turn a generated catalog lock into an idempotent D1 update."""

    lock = _read_json(lock_path)
    release_id = str(lock["release"])
    lock_sha = str(lock["lockSha256"])
    lines = [_insert("morph_catalog", {
        "channel": channel,
        "release_id": release_id,
        "catalog_hash": lock_sha,
        "catalog_json": _canonical_json(lock),
    }, replace=True)]
    destination = (output or lock_path.with_suffix(".sql")).expanduser().resolve()
    destination.parent.mkdir(parents=True, exist_ok=True)
    destination.write_text("\n".join(lines) + "\n", encoding="utf-8")
    return destination


def build_morph_release(
    starter_set: Path,
    *,
    output: Path | None = None,
    compiler_manifest: Path | None = None,
    source_commit: str | None = None,
) -> MorphReleaseResult:
    root = starter_set.expanduser().resolve()
    source_root = root / "source/morphs"
    if not source_root.is_dir():
        raise MorphReleaseError(f"Morph source directory does not exist: {source_root}")
    generated = (output or root.parent / ".cubacadabra/generated/morphs").expanduser().resolve()
    runtime_root = generated / "runtime"
    runtime_root.mkdir(parents=True, exist_ok=True)
    manifest_path = generated / "catalog.lock.json"
    compiler_manifest = compiler_manifest or root.parents[1] / "studio/crates/morph_authoring/Cargo.toml"

    source_catalog = _read_json(root / "catalog.json") if (root / "catalog.json").is_file() else None
    source_specs = source_catalog.get("assets", []) if source_catalog else [
        {"source": path.relative_to(root).as_posix()} for path in sorted(source_root.rglob("*.morph.json"))
    ]
    entries: dict[str, dict[str, object]] = {}
    packs = 0
    for spec in source_specs:
        if not isinstance(spec, dict) or not isinstance(spec.get("source"), str):
            raise MorphReleaseError("catalog assets must contain source paths")
        sidecar = (root / str(spec["source"])).resolve()
        if not sidecar.is_file() or not sidecar.is_relative_to(root):
            raise MorphReleaseError(f"catalog references missing source manifest: {sidecar}")
        source = _read_json(sidecar)
        asset = source.get("asset")
        if not isinstance(asset, dict) or not isinstance(asset.get("id"), str):
            raise MorphReleaseError(f"{sidecar} has no valid asset definition")
        asset_id = str(asset["id"])
        if asset_id in entries:
            raise MorphReleaseError(f"duplicate catalog asset: {asset_id}")
        if spec.get("id") is not None and spec.get("id") != asset_id:
            raise MorphReleaseError(f"catalog ID {spec.get('id')} does not match {asset_id}")
        geometry = asset.get("source", {}).get("geometry") if isinstance(asset.get("source"), dict) else None
        if not isinstance(geometry, str):
            raise MorphReleaseError(f"{sidecar} has no source.geometry")
        glb = sidecar.parent / geometry
        if not glb.is_file():
            raise MorphReleaseError(f"{sidecar} references missing GLB: {glb}")
        pack = runtime_root / "morphs/sha256/pending" / f"{asset_id.replace(':', '_').replace('/', '_')}.morphpack"
        pack.parent.mkdir(parents=True, exist_ok=True)
        _compile(sidecar, glb, pack, compiler_manifest)
        digest = hashlib.sha256(pack.read_bytes()).hexdigest()
        final = runtime_root / f"morphs/sha256/{digest[:2]}/{digest}.morphpack"
        final.parent.mkdir(parents=True, exist_ok=True)
        if final != pack:
            final.write_bytes(pack.read_bytes())
            pack.unlink()
        definition = _entry(asset)
        definition["delivery"] = "morphpack"
        definition["artifact"] = {"sha256": digest, "bytes": final.stat().st_size}
        entries[asset_id] = definition
        packs += 1

    # Procedural definitions remain first-class catalog entries, but have an
    # explicit delivery type instead of empty hash columns.
    builtin_ref = source_catalog.get("builtins") if source_catalog else None
    builtin_catalog = (root / builtin_ref).resolve() if isinstance(builtin_ref, str) else root.parents[1] / "rust/assets/characters/morph_catalog.json"
    if builtin_catalog.is_file():
        builtins = _read_json(builtin_catalog)
        excluded_kinds = set(source_catalog.get("excludeBuiltinKinds", []) if source_catalog else [])
        for asset in builtins.get("assets", []):
            if not isinstance(asset, dict) or not isinstance(asset.get("id"), str):
                continue
            asset_id = str(asset["id"])
            if asset_id not in entries and asset.get("kind") not in excluded_kinds:
                entries[asset_id] = _entry(asset) | {"delivery": "builtin", "artifact": None}

    # Starter recipes are authored alongside parts and travel in the same
    # immutable release. Their order is the curated character-select order.
    presets = []
    for spec in source_catalog.get("presets", []) if source_catalog else []:
        if not isinstance(spec, dict) or not isinstance(spec.get("source"), str):
            raise MorphReleaseError("catalog presets must contain source paths")
        path = (root / spec["source"]).resolve()
        if not path.is_file() or not path.is_relative_to(root):
            raise MorphReleaseError(f"catalog references missing preset: {path}")
        preset = _read_json(path)
        thumbnail = preset.get("thumbnail")
        if thumbnail:
            image = (path.parent / thumbnail).resolve()
            if not image.is_file() or not image.is_relative_to(root) or image.suffix != ".png":
                raise MorphReleaseError(f"invalid preset thumbnail: {image}")
            image_bytes = image.read_bytes()
            digest = hashlib.sha256(image_bytes).hexdigest()
            target = runtime_root / f"morphs/thumbnails/sha256/{digest[:2]}/{digest}.png"
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(image_bytes)
            preset["thumbnail"] = f"/morphs/thumbnails/sha256/{digest[:2]}/{digest}.png"
        presets.append(preset)

    commit = source_commit or os.environ.get("CUBACADABRA_SOURCE_COMMIT") or _git_commit(root)
    body = {
        "schemaVersion": 2,
        "sourceCommit": commit,
        "compiler": "cubacadabra-morph-authoring@0.1.0",
        "assets": [entries[key] for key in sorted(entries)],
        "presets": presets,
    }
    lock_sha = _canonical_hash(body)
    release_id = f"{commit[:12]}-{lock_sha[:12]}"
    lock = {"release": release_id, "lockSha256": lock_sha, **body}
    manifest_path.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile(mode="w", suffix=".json", dir=generated, delete=False, encoding="utf-8") as pending:
        pending.write(_canonical_json(lock) + "\n")
        pending_path = Path(pending.name)
    command = ["cargo", "run", "--quiet", "--manifest-path", str(compiler_manifest),
               "--bin", "morph_catalog_validate", "--", str(pending_path)]
    try:
        subprocess.run(command, check=True, capture_output=True, text=True)
        pending_path.replace(manifest_path)
    except (OSError, subprocess.CalledProcessError) as error:
        raise MorphReleaseError(f"Invalid Morph release: {getattr(error, 'stderr', '') or error}") from error
    finally:
        pending_path.unlink(missing_ok=True)
    return MorphReleaseResult(release_id, manifest_path, runtime_root, len(entries), packs)


def _entry(asset: dict[str, object]) -> dict[str, object]:
    return {
        "id": asset["id"],
        "kind": asset.get("kind", ""),
        "name": asset.get("displayName", asset["id"]),
        "base": (asset.get("supportedBases") or [asset["id"]])[0],
        "rigProfile": asset.get("rigProfile", "cuba:rig/biped15.v1"),
        "slots": asset.get("occupiedSlots", []),
        "tags": asset.get("coverage", []),
        "capabilities": asset.get("requiredCapabilities", []),
        "definition": asset,
    }


def _read_json(path: Path) -> dict[str, object]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise MorphReleaseError(f"could not read JSON {path}: {error}") from error
    if not isinstance(value, dict):
        raise MorphReleaseError(f"JSON root must be an object: {path}")
    return value


def _canonical_json(value: object) -> str:
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False)


def _canonical_hash(value: object) -> str:
    return hashlib.sha256(_canonical_json(value).encode("utf-8")).hexdigest()


def _sql(value: object) -> str:
    if value is None:
        return "NULL"
    if isinstance(value, bool):
        return "1" if value else "0"
    if isinstance(value, int):
        return str(value)
    return "'" + str(value).replace("'", "''") + "'"


def _insert(table: str, values: dict[str, object], *, replace: bool = False) -> str:
    columns = ", ".join(values)
    payload = ", ".join(_sql(value) for value in values.values())
    prefix = "INSERT OR REPLACE" if replace else "INSERT OR IGNORE"
    return f"{prefix} INTO {table} ({columns}) VALUES ({payload});"


def _git_commit(root: Path) -> str:
    try:
        return subprocess.run(
            ["git", "-C", str(root.parent), "rev-parse", "HEAD"],
            check=True,
            capture_output=True,
            text=True,
        ).stdout.strip()
    except (OSError, subprocess.CalledProcessError):
        return "uncommitted"


def _compile(manifest: Path, glb: Path, output: Path, compiler_manifest: Path) -> None:
    command = [
        "cargo", "run", "--quiet", "--manifest-path", str(compiler_manifest),
        "--bin", "morph_compile", "--", str(manifest), str(glb), str(output),
    ]
    try:
        subprocess.run(command, check=True, cwd=compiler_manifest.parent.parent.parent, capture_output=True, text=True)
    except (OSError, subprocess.CalledProcessError) as error:
        detail = getattr(error, "stderr", "") or str(error)
        raise MorphReleaseError(f"Morph compiler failed for {manifest}: {detail.strip()}") from error
