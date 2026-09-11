from __future__ import annotations

import hashlib
import json
import tempfile
import threading
import unittest
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import unquote, urlparse

from cubacadabra.local_r2_setup import LocalR2SetupError, setup_local_r2


class LocalR2SetupTests(unittest.TestCase):
    def test_setup_uploads_runtime_and_source_and_is_idempotent(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            digest = hashlib.sha256(b"pack").hexdigest()
            pack = root / "runtime/morphs/sha256" / digest[:2] / f"{digest}.morphpack"
            source = root / "source/morphs/example/source.glb"
            pack.parent.mkdir(parents=True)
            source.parent.mkdir(parents=True)
            pack.write_bytes(b"pack")
            source.write_bytes(b"glb")
            objects: dict[str, bytes] = {}
            puts: list[str] = []

            class Handler(BaseHTTPRequestHandler):
                def log_message(self, format: str, *args: object) -> None:
                    return

                def do_GET(self) -> None:  # noqa: N802 - stdlib handler API
                    path = urlparse(self.path).path
                    if path.endswith("/r2/buckets"):
                        self._send_json({"success": True, "result": ["prod"]})
                    elif "/objects/" in path:
                        key = unquote(path.split("/objects/", 1)[1])
                        if key not in objects:
                            self.send_error(404)
                        else:
                            self.send_response(200)
                            self.send_header("Content-Length", str(len(objects[key])))
                            self.end_headers()
                            self.wfile.write(objects[key])
                    elif path.endswith("/objects"):
                        self._send_json({
                            "success": True,
                            "result": [{"key": key, "size": len(value)} for key, value in objects.items()],
                            "result_info": {"is_truncated": False},
                        })
                    else:
                        self.send_error(404)

                def do_PUT(self) -> None:  # noqa: N802 - stdlib handler API
                    key = unquote(urlparse(self.path).path.split("/objects/", 1)[1])
                    size = int(self.headers.get("Content-Length", "0"))
                    objects[key] = self.rfile.read(size)
                    puts.append(key)
                    self._send_json({"success": True, "result": {"key": key, "size": size}})

                def _send_json(self, payload: dict[str, object]) -> None:
                    body = json.dumps(payload).encode()
                    self.send_response(200)
                    self.send_header("Content-Type", "application/json")
                    self.send_header("Content-Length", str(len(body)))
                    self.end_headers()
                    self.wfile.write(body)

            server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
            thread = threading.Thread(target=server.serve_forever, daemon=True)
            thread.start()
            try:
                endpoint = f"http://127.0.0.1:{server.server_port}"
                first = setup_local_r2(root, endpoint=endpoint)
                second = setup_local_r2(root, endpoint=endpoint)
            finally:
                server.shutdown()
                thread.join()
                server.server_close()

            self.assertEqual(first.uploaded, 2)
            self.assertEqual(second.skipped, 2)
            self.assertEqual(set(puts), {pack.relative_to(root).as_posix(), source.relative_to(root).as_posix()})

    def test_rejects_runtime_pack_with_wrong_content_hash(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            pack = root / "runtime/morphs/sha256/aa" / ("a" * 64 + ".morphpack")
            pack.parent.mkdir(parents=True)
            pack.write_bytes(b"wrong")
            with self.assertRaises(LocalR2SetupError):
                setup_local_r2(root, client=_NoNetworkClient())

    def test_dry_run_validates_without_putting(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            (root / "source/example.txt").parent.mkdir(parents=True)
            (root / "source/example.txt").write_text("source", encoding="utf-8")
            client = _DryRunClient()
            result = setup_local_r2(root, dry_run=True, client=client)
            self.assertEqual(result.uploaded, 0)
            self.assertEqual(result.files, 1)


class _NoNetworkClient:
    def list_buckets(self) -> list[str]:
        raise AssertionError("fixture validation should happen before network access")


class _DryRunClient:
    def list_buckets(self) -> list[str]:
        return ["prod"]

    def list_objects(self, bucket: str) -> list[object]:
        return []

    def put_object(self, *args: object, **kwargs: object) -> None:
        raise AssertionError("dry-run must not write objects")
