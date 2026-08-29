import sqlite3
from pathlib import Path


SCHEMA = """
CREATE TABLE IF NOT EXISTS products (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    sku TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    price_cents INTEGER NOT NULL CHECK(price_cents >= 0),
    stock INTEGER NOT NULL CHECK(stock >= 0),
    active INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS sales (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    subtotal_cents INTEGER NOT NULL CHECK(subtotal_cents >= 0),
    tax_cents INTEGER NOT NULL CHECK(tax_cents >= 0),
    total_cents INTEGER NOT NULL CHECK(total_cents >= 0),
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS sale_items (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    sale_id INTEGER NOT NULL REFERENCES sales(id) ON DELETE CASCADE,
    product_id INTEGER NOT NULL REFERENCES products(id),
    product_name TEXT NOT NULL,
    quantity INTEGER NOT NULL CHECK(quantity > 0),
    unit_price_cents INTEGER NOT NULL CHECK(unit_price_cents >= 0),
    line_total_cents INTEGER NOT NULL CHECK(line_total_cents >= 0)
);
"""

DEMO_PRODUCTS = (
    ("COF-001", "Drip Coffee", 350, 40),
    ("TEA-001", "Tea", 275, 25),
    ("SND-001", "Ham Sandwich", 699, 15),
    ("CKE-001", "Cheesecake Slice", 525, 10),
)


def connect(database_path):
    connection = sqlite3.connect(str(database_path))
    connection.row_factory = sqlite3.Row
    connection.execute("PRAGMA foreign_keys = ON")
    return connection


def initialize_database(database_path):
    database_path = Path(database_path)
    database_path.parent.mkdir(parents=True, exist_ok=True)
    with connect(database_path) as connection:
        connection.executescript(SCHEMA)
        product_count = connection.execute(
            "SELECT COUNT(*) AS count FROM products"
        ).fetchone()["count"]
        if product_count == 0:
            connection.executemany(
                """
                INSERT INTO products (sku, name, price_cents, stock)
                VALUES (?, ?, ?, ?)
                """,
                DEMO_PRODUCTS,
            )
        connection.commit()
