import json
import os
import sqlite3
from contextlib import contextmanager
from datetime import datetime


SCHEMA = """
CREATE TABLE IF NOT EXISTS products (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    sku TEXT NOT NULL UNIQUE,
    price_cents INTEGER NOT NULL CHECK (price_cents >= 0),
    stock_qty INTEGER NOT NULL CHECK (stock_qty >= 0),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS sales (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    subtotal_cents INTEGER NOT NULL,
    tax_cents INTEGER NOT NULL,
    total_cents INTEGER NOT NULL,
    cash_received_cents INTEGER NOT NULL,
    change_cents INTEGER NOT NULL,
    created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS sale_items (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    sale_id INTEGER NOT NULL,
    product_id INTEGER NOT NULL,
    product_name TEXT NOT NULL,
    sku TEXT NOT NULL,
    unit_price_cents INTEGER NOT NULL,
    quantity INTEGER NOT NULL,
    line_total_cents INTEGER NOT NULL,
    FOREIGN KEY (sale_id) REFERENCES sales(id),
    FOREIGN KEY (product_id) REFERENCES products(id)
);
"""


def utc_now():
    return datetime.utcnow().replace(microsecond=0).isoformat() + "Z"


def ensure_directory(path):
    directory = os.path.dirname(path)
    if directory and not os.path.exists(directory):
        os.makedirs(directory)


class Database(object):
    def __init__(self, path):
        self.path = path
        ensure_directory(path)
        self.initialize()

    @contextmanager
    def connect(self):
        connection = sqlite3.connect(self.path)
        connection.row_factory = sqlite3.Row
        try:
            yield connection
            connection.commit()
        finally:
            connection.close()

    def initialize(self):
        with self.connect() as connection:
            connection.executescript(SCHEMA)

    def seed_defaults(self):
        with self.connect() as connection:
            existing = connection.execute("SELECT COUNT(*) AS count FROM products").fetchone()["count"]
            if existing:
                return
            now = utc_now()
            rows = [
                ("Coffee Beans", "SKU-COFFEE", 1299, 18, now, now),
                ("Bagel", "SKU-BAGEL", 399, 30, now, now),
                ("Orange Juice", "SKU-JUICE", 549, 12, now, now),
            ]
            connection.executemany(
                """
                INSERT INTO products (name, sku, price_cents, stock_qty, created_at, updated_at)
                VALUES (?, ?, ?, ?, ?, ?)
                """,
                rows,
            )

    def list_products(self):
        with self.connect() as connection:
            rows = connection.execute(
                "SELECT * FROM products ORDER BY name COLLATE NOCASE ASC"
            ).fetchall()
            return [dict(row) for row in rows]

    def get_product(self, product_id):
        with self.connect() as connection:
            row = connection.execute(
                "SELECT * FROM products WHERE id = ?", (product_id,)
            ).fetchone()
            return dict(row) if row else None

    def create_product(self, name, sku, price_cents, stock_qty):
        now = utc_now()
        with self.connect() as connection:
            try:
                connection.execute(
                    """
                    INSERT INTO products (name, sku, price_cents, stock_qty, created_at, updated_at)
                    VALUES (?, ?, ?, ?, ?, ?)
                    """,
                    (name, sku, price_cents, stock_qty, now, now),
                )
            except sqlite3.IntegrityError:
                raise ValueError("SKU already exists.")

    def update_stock(self, product_id, quantity_delta):
        with self.connect() as connection:
            row = connection.execute(
                "SELECT stock_qty FROM products WHERE id = ?", (product_id,)
            ).fetchone()
            if not row:
                raise ValueError("Product not found.")
            new_qty = row["stock_qty"] + quantity_delta
            if new_qty < 0:
                raise ValueError("Insufficient stock.")
            connection.execute(
                "UPDATE products SET stock_qty = ?, updated_at = ? WHERE id = ?",
                (new_qty, utc_now(), product_id),
            )

    def daily_summary(self):
        with self.connect() as connection:
            row = connection.execute(
                """
                SELECT
                    COUNT(*) AS sale_count,
                    COALESCE(SUM(total_cents), 0) AS gross_cents,
                    COALESCE(SUM(change_cents), 0) AS change_cents
                FROM sales
                WHERE substr(created_at, 1, 10) = substr(?, 1, 10)
                """,
                (utc_now(),),
            ).fetchone()
            return dict(row)

    def sales_history(self, limit=20):
        with self.connect() as connection:
            sale_rows = connection.execute(
                "SELECT * FROM sales ORDER BY id DESC LIMIT ?",
                (limit,),
            ).fetchall()
            sales = []
            for sale in sale_rows:
                items = connection.execute(
                    "SELECT * FROM sale_items WHERE sale_id = ? ORDER BY id ASC",
                    (sale["id"],),
                ).fetchall()
                payload = dict(sale)
                payload["items"] = [dict(item) for item in items]
                sales.append(payload)
            return sales

    def record_sale(self, cart_items, cash_received_cents, tax_rate):
        if not cart_items:
            raise ValueError("Cart is empty.")
        subtotal_cents = sum(item["line_total_cents"] for item in cart_items)
        tax_cents = int(round(subtotal_cents * tax_rate))
        total_cents = subtotal_cents + tax_cents
        if cash_received_cents < total_cents:
            raise ValueError("Cash received is less than the order total.")
        change_cents = cash_received_cents - total_cents
        now = utc_now()
        with self.connect() as connection:
            for item in cart_items:
                stock_row = connection.execute(
                    "SELECT stock_qty FROM products WHERE id = ?",
                    (item["product_id"],),
                ).fetchone()
                if not stock_row:
                    raise ValueError("Product not found during checkout.")
                if stock_row["stock_qty"] < item["quantity"]:
                    raise ValueError("Not enough stock for %s." % item["product_name"])
            sale_cursor = connection.execute(
                """
                INSERT INTO sales (
                    subtotal_cents, tax_cents, total_cents, cash_received_cents, change_cents, created_at
                ) VALUES (?, ?, ?, ?, ?, ?)
                """,
                (subtotal_cents, tax_cents, total_cents, cash_received_cents, change_cents, now),
            )
            sale_id = sale_cursor.lastrowid
            for item in cart_items:
                connection.execute(
                    """
                    INSERT INTO sale_items (
                        sale_id, product_id, product_name, sku, unit_price_cents, quantity, line_total_cents
                    ) VALUES (?, ?, ?, ?, ?, ?, ?)
                    """,
                    (
                        sale_id,
                        item["product_id"],
                        item["product_name"],
                        item["sku"],
                        item["unit_price_cents"],
                        item["quantity"],
                        item["line_total_cents"],
                    ),
                )
                connection.execute(
                    """
                    UPDATE products
                    SET stock_qty = stock_qty - ?, updated_at = ?
                    WHERE id = ?
                    """,
                    (item["quantity"], utc_now(), item["product_id"]),
                )
        return {
            "sale_id": sale_id,
            "subtotal_cents": subtotal_cents,
            "tax_cents": tax_cents,
            "total_cents": total_cents,
            "cash_received_cents": cash_received_cents,
            "change_cents": change_cents,
            "created_at": now,
        }
