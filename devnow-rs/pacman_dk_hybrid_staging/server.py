import argparse
import json
import os
import posixpath
from urllib.parse import unquote
from http.server import HTTPServer, SimpleHTTPRequestHandler
from socketserver import ThreadingMixIn


ROOT_DIR = os.path.dirname(os.path.abspath(__file__))
STATIC_DIR = os.path.join(ROOT_DIR, "static")


class ThreadedHTTPServer(ThreadingMixIn, HTTPServer):
    daemon_threads = True


class HybridRequestHandler(SimpleHTTPRequestHandler):
    server_version = "HybridArcade/1.0"

    def end_headers(self):
        self.send_header("Cache-Control", "no-store")
        SimpleHTTPRequestHandler.end_headers(self)

    def translate_path(self, path):
        path = path.split("?", 1)[0]
        path = path.split("#", 1)[0]
        trailing_slash = path.rstrip().endswith("/")
        path = posixpath.normpath(unquote(path))
        segments = [segment for segment in path.split("/") if segment and segment not in (".", "..")]
        resolved = STATIC_DIR
        for segment in segments:
            resolved = os.path.join(resolved, segment)
        if trailing_slash:
            resolved = os.path.join(resolved, "")
        return resolved

    def do_GET(self):
        if self.path == "/health":
            payload = json.dumps({"status": "ok"}).encode("utf-8")
            self.send_response(200)
            self.send_header("Content-Type", "application/json; charset=utf-8")
            self.send_header("Content-Length", str(len(payload)))
            self.end_headers()
            self.wfile.write(payload)
            return

        if self.path in ("/", "/index.html"):
            self.path = "/index.html"

        SimpleHTTPRequestHandler.do_GET(self)


def build_handler():
    return HybridRequestHandler


def run_server(host="127.0.0.1", port=8011):
    server = ThreadedHTTPServer((host, port), build_handler())
    print("Serving Pac-Man / Donkey Kong hybrid on http://{0}:{1}".format(host, port))
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        server.server_close()


def main():
    parser = argparse.ArgumentParser(description="Run the Pac-Man / Donkey Kong hybrid arcade game.")
    parser.add_argument("--host", default="127.0.0.1", help="Host interface to bind.")
    parser.add_argument("--port", type=int, default=8011, help="Port to serve on.")
    args = parser.parse_args()
    run_server(host=args.host, port=args.port)


if __name__ == "__main__":
    main()
