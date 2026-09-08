from __future__ import annotations

import io
import json
import tempfile
import threading
import unittest
import zipfile
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

from cubacadabra.examples_uploader import upload_examples


class ExampleUploadTests(unittest.TestCase):
    def test_bumps_builds_and_uploads_examples_with_one_session(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            examples = root / "examples"
            for game_id, version in (
                ("the-wild-west", "0.3.6"),
                ("survival-101", "0.3.1"),
                ("adventure-101", "0.4.0"),
            ):
                project = examples / game_id
                (project / "src").mkdir(parents=True)
                (project / "manifest.json").write_text(
                    json.dumps({"id": game_id, "version": version}),
                    encoding="utf-8",
                )
                (project / "src/main.luau").write_text("return {}\n", encoding="utf-8")

            requests: list[tuple[str, str | None, dict[str, object]]] = []

            class Handler(BaseHTTPRequestHandler):
                def log_message(self, format: str, *args: object) -> None:
                    return

                def do_POST(self) -> None:  # noqa: N802 - stdlib handler API
                    length = int(self.headers["Content-Length"] or 0)
                    body = self.rfile.read(length)
                    if self.path == "/auth/email":
                        self.send_response(200)
                        self.send_header(
                            "Set-Cookie",
                            "cubacadabra_session=test-session; Path=/; Secure",
                        )
                        response = {"user": {"email": "play-review@cubacadabra.com"}}
                    elif self.path == "/cubes/upload":
                        with zipfile.ZipFile(io.BytesIO(body)) as archive:
                            manifest = json.loads(archive.read("manifest.json"))
                        requests.append((self.path, self.headers.get("Cookie"), manifest))
                        self.send_response(201)
                        response = {
                            "ok": True,
                            "cube": {"version": str(manifest["version"])},
                        }
                    else:
                        self.send_response(404)
                        response = {"error": "not_found"}
                    payload = json.dumps(response).encode("utf-8")
                    self.send_header("Content-Type", "application/json")
                    self.send_header("Content-Length", str(len(payload)))
                    self.end_headers()
                    self.wfile.write(payload)

            server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
            thread = threading.Thread(target=server.serve_forever, daemon=True)
            thread.start()
            try:
                plans, results = upload_examples(
                    examples_dir=examples,
                    build_dir=root / "build",
                    zip_dir=root / "archives",
                    backend_url=f"http://127.0.0.1:{server.server_port}",
                )
            finally:
                server.shutdown()
                thread.join()
                server.server_close()

            self.assertEqual([plan.version for plan in plans], ["0.3.7", "0.3.2", "0.4.1"])
            self.assertEqual([result.version for result in results], ["0.3.7", "0.3.2", "0.4.1"])
            self.assertEqual(len(requests), 3)
            self.assertEqual(
                [request[1] for request in requests],
                ["cubacadabra_session=test-session"] * 3,
            )
            self.assertEqual(
                json.loads((examples / "the-wild-west/manifest.json").read_text())["version"],
                "0.3.7",
            )
            self.assertTrue((root / "archives/the-wild-west.zip").exists())
            self.assertTrue((root / "archives/survival-101.zip").exists())
            self.assertTrue((root / "archives/adventure-101.zip").exists())
