#!/usr/bin/env python3
import argparse
import os

import app


ThreadedHTTPServer = app.ThreadedHTTPServer
GAME_CONFIG = app.GAME_CONFIG
ROOT_DIR = os.path.dirname(os.path.abspath(__file__))
STATIC_DIR = app.STATIC_DIR


def api_payload_for_path(path):
    if path == "/health":
        return 200, {"status": "ok", "game": GAME_CONFIG["title"]}
    return app.api_payload_for_path(path)


def build_handler():
    return app.GameRequestHandler


def run_server(host="127.0.0.1", port=8011):
    server = ThreadedHTTPServer((host, port), build_handler())
    print("Serving Maze Climber on http://{0}:{1}".format(host, port))
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        server.server_close()


def main():
    parser = argparse.ArgumentParser(description="Run the Maze Climber arcade game.")
    parser.add_argument("--host", default=os.environ.get("HOST", "127.0.0.1"))
    parser.add_argument("--port", type=int, default=int(os.environ.get("PORT", "8011")))
    args = parser.parse_args()
    run_server(host=args.host, port=args.port)


if __name__ == "__main__":
    main()
