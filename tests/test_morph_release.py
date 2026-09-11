from __future__ import annotations

import hashlib
import json
import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from cubacadabra.morph_release import MorphReleaseError, build_morph_release


class MorphReleaseTests(unittest.TestCase):
    def fixture(self, root: Path) -> dict:
        source = root / "source/morphs/base/person"
        source.mkdir(parents=True)
        (source / "person.glb").write_bytes(b"glb")
        asset = {"id": "cuba:base/person.v1", "kind": "base", "source": {"geometry": "person.glb"}}
        (source / "person.morph.json").write_text(json.dumps({"asset": asset}))
        (root / "presets").mkdir()
        (root / "presets/person.png").write_bytes(b"thumbnail pixels")
        preset = {"id": "cuba:preset/person-17.v1", "displayName": "Person 17",
                  "base": asset["id"], "parts": [], "face": "cuba:face/neutral.v1",
                  "parameters": {"skin": "#a86f50"}, "thumbnail": "person.png"}
        (root / "presets/person.json").write_text(json.dumps(preset))
        (root / "builtins.json").write_text(json.dumps({"assets": [
            {"id": "cuba:old-outfit.v1", "kind": "outfit"},
            {"id": "cuba:face/neutral.v1", "kind": "face"},
        ]}))
        (root / "catalog.json").write_text(json.dumps({
            "assets": [{"source": "source/morphs/base/person/person.morph.json"}],
            "presets": [{"source": "presets/person.json"}],
            "builtins": "builtins.json", "excludeBuiltinKinds": ["outfit"],
        }))
        return preset

    def build(self, root: Path):
        def compile_asset(manifest, glb, output, compiler): output.write_bytes(b"compiled pack")
        with patch("cubacadabra.morph_release._compile", side_effect=compile_asset):
            return build_morph_release(root, output=root / "generated", compiler_manifest=root / "compiler/Cargo.toml", source_commit="a" * 40)

    def test_complete_presets_and_thumbnails_are_in_the_deterministic_release(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            preset = self.fixture(root)
            with patch("cubacadabra.morph_release.subprocess.run") as validator:
                first = self.build(root)
                second = self.build(root)
            self.assertEqual(first.release_id, second.release_id)
            self.assertIn("morph_catalog_validate", validator.call_args.args[0])
            lock = json.loads(first.lock_path.read_text())
            self.assertEqual(len(lock["assets"]), 2)
            actual = lock["presets"][0]
            self.assertEqual(actual["parameters"], preset["parameters"])
            self.assertEqual(actual["face"], preset["face"])
            digest = hashlib.sha256(b"thumbnail pixels").hexdigest()
            self.assertEqual(actual["thumbnail"], f"/morphs/thumbnails/sha256/{digest[:2]}/{digest}.png")
            self.assertTrue((first.runtime_root / actual["thumbnail"].lstrip("/")).is_file())

    def test_failed_validation_preserves_the_previous_release(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.fixture(root)
            generated = root / "generated"
            generated.mkdir()
            lock = generated / "catalog.lock.json"
            lock.write_text("previous valid release")
            with patch("cubacadabra.morph_release.subprocess.run", side_effect=subprocess.CalledProcessError(1, "validator", stderr="unknown preset part")):
                with self.assertRaisesRegex(MorphReleaseError, "unknown preset part"):
                    self.build(root)
            self.assertEqual(lock.read_text(), "previous valid release")
