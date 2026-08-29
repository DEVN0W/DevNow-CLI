#!/usr/bin/env python3
import json
import os
from http.server import HTTPServer, SimpleHTTPRequestHandler
from socketserver import ThreadingMixIn
from urllib.parse import urlparse


BASE_DIR = os.path.dirname(os.path.abspath(__file__))
STATIC_DIR = os.path.join(BASE_DIR, "static")

GAME_CONFIG = {
    "title": "Maze Climber",
    "tagline": "A maze chase that breaks into vertical platform runs.",
    "controls": {
        "move": ["Arrow keys", "WASD"],
        "jump": ["Space", "W"],
        "start": ["Enter"],
    },
    "loop": [
        "Clear the maze and collect every pellet.",
        "Climb the platform tower while dodging rolling hazards.",
        "Finish the tower to start a harder maze cycle.",
    ],
}


def api_payload_for_path(path):
    if path == "/api/health":
        return 200, {"status": "ok", "game": GAME_CONFIG["title"]}
    if path == "/api/game-config":
        return 200, GAME_CONFIG
    return None, None


def load_static_text(rel_path):
    full_path = os.path.join(STATIC_DIR, rel_path)
    with open(full_path, "r", encoding="utf-8") as handle:
        return handle.read()


class ThreadedHTTPServer(ThreadingMixIn, HTTPServer):
    daemon_threads = True


class GameRequestHandler(SimpleHTTPRequestHandler):
    server_version = "MazeClimberHTTP/1.0"

    def _send_json(self, payload, status=200):
        data = json.dumps(payload).encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "application/json; charset=utf-8")
        self.send_header("Content-Length", str(len(data)))
        self.send_header("Cache-Control", "no-store")
        self.end_headers()
        self.wfile.write(data)

    def _static_path(self, request_path):
        parsed = urlparse(request_path)
        clean_path = parsed.path
        if clean_path in ("", "/"):
            clean_path = "/index.html"
        rel_path = clean_path.lstrip("/")
        full_path = os.path.normpath(os.path.join(STATIC_DIR, rel_path))
        if not full_path.startswith(STATIC_DIR):
            return os.path.join(STATIC_DIR, "index.html")
        return full_path

    def do_GET(self):
        parsed = urlparse(self.path)
        status, payload = api_payload_for_path(parsed.path)
        if payload is not None:
            self._send_json(payload, status=status)
            return
        return SimpleHTTPRequestHandler.do_GET(self)

    def translate_path(self, path):
        return self._static_path(path)

    def log_message(self, format_string, *args):
        return


def create_server(host, port):
    return ThreadedHTTPServer((host, port), GameRequestHandler)


def main():
    host = os.environ.get("HOST", "127.0.0.1")
    port = int(os.environ.get("PORT", "8000"))
    server = create_server(host, port)
    print("Serving Maze Climber on http://{0}:{1}".format(host, port))
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        server.server_close()


if __name__ == "__main__":
    main()
