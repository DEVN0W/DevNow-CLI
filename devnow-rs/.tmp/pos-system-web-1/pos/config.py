from pathlib import Path


BASE_DIR = Path(__file__).resolve().parent.parent
DATABASE_PATH = BASE_DIR / "pos.db"
HOST = "127.0.0.1"
PORT = 8000
