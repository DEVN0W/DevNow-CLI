from wsgiref.simple_server import make_server

from app.pos_app import POSApplication


def main():
    app = POSApplication(base_dir=".")
    server = make_server("127.0.0.1", 8000, app)
    print("POS system running at http://127.0.0.1:8000")
    server.serve_forever()


if __name__ == "__main__":
    main()

