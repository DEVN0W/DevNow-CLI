import os
import sqlite3
from contextlib import contextmanager
from datetime import datetime


SCHEMA = """
CREATE TABLE IF NOT EXISTS products (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    sku TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    price_cents INTEGER NOT NULL CHECK(price_cents >= 0),
    stock_qty INTEGER NOT NULL CHECK(stock_qty >= 0),
    created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS orders (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    created_at TEXT NOT NULL,
    total_cents INTEGER NOT NULL CHECK(total_cents >= 0),
    payment_method TEXT NOT NULL,
    note TEXT NOT NULL DEFAULT ''
);

CREATE TABLE IF NOT EXISTS order_items (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    order_id INTEGER NOT NULL,
    product_id INTEGER NOT NULL,
    product_name TEXT NOT NULL,
    unit_price_cents INTEGER NOT NULL CHECK(unit_price_cents >= 0),
    quantity INTEGER NOT NULL CHECK(quantity > 0),
    line_total_cents INTEGER NOT NULL CHECK(line_total_cents >= 0),
    FOREIGN KEY(order_id) REFERENCES orders(id),
    FOREIGN KEY(product_id) REFERENCES products(id)
);
"""


SEED_PRODUCTS = (
    ("COF-001", "House Coffee", 350, 40),
    ("TEA-001", "Green Tea", 320, 25),
    ("SND-001", "Ham Sandwich", 650, 18),
    ("CKE-001", "Cheesecake Slice", 480, 10),
)


class Database(object):
    def __init__(self, path):
        self.path = path

    @contextmanager
    def connect(self):
        directory = os.path.dirname(self.path)
        if directory and not os.path.exists(directory):
            os.makedirs(directory)
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
            count = connection.execute("SELECT COUNT(*) FROM products").fetchone()[0]
            if count == 0:
                now = datetime.utcnow().isoformat()
                connection.executemany(
                    """
                    INSERT INTO products (sku, name, price_cents, stock_qty, created_at)
                    VALUES (?, ?, ?, ?, ?)
                    """,
                    [(sku, name, price, stock, now) for sku, name, price, stock in SEED_PRODUCTS],
                )

    def list_products(self):
        with self.connect() as connection:
            rows = connection.execute(
                """
                SELECT id, sku, name, price_cents, stock_qty
                FROM products
                ORDER BY name ASC
                """
            ).fetchall()
            return [dict(row) for row in rows]

    def get_product(self, product_id):
        with self.connect() as connection:
            row = connection.execute(
                """
                SELECT id, sku, name, price_cents, stock_qty
                FROM products
                WHERE id = ?
                """,
                (product_id,),
            ).fetchone()
            return dict(row) if row else None

    def add_product(self, sku, name, price_cents, stock_qty):
        now = datetime.utcnow().isoformat()
        with self.connect() as connection:
            connection.execute(
                """
                INSERT INTO products (sku, name, price_cents, stock_qty, created_at)
                VALUES (?, ?, ?, ?, ?)
                """,
                (sku, name, price_cents, stock_qty, now),
            )

    def update_stock(self, product_id, quantity_delta):
        with self.connect() as connection:
            row = connection.execute(
                "SELECT stock_qty FROM products WHERE id = ?",
                (product_id,),
            ).fetchone()
            if row is None:
                raise ValueError("Product not found")
            new_stock = row["stock_qty"] + quantity_delta
            if new_stock < 0:
                raise ValueError("Insufficient stock")
            connection.execute(
                "UPDATE products SET stock_qty = ? WHERE id = ?",
                (new_stock, product_id),
            )

    def create_order(self, items, payment_method, note):
        total_cents = sum(item["line_total_cents"] for item in items)
        timestamp = datetime.utcnow().isoformat()
        with self.connect() as connection:
            for item in items:
                stock_row = connection.execute(
                    "SELECT stock_qty FROM products WHERE id = ?",
                    (item["product_id"],),
                ).fetchone()
                if stock_row is None:
                    raise ValueError("Product not found")
                if stock_row["stock_qty"] < item["quantity"]:
                    raise ValueError("Insufficient stock for %s" % item["name"])
            order_cursor = connection.execute(
                """
                INSERT INTO orders (created_at, total_cents, payment_method, note)
                VALUES (?, ?, ?, ?)
                """,
                (timestamp, total_cents, payment_method, note),
            )
            order_id = order_cursor.lastrowid
            for item in items:
                connection.execute(
                    """
                    INSERT INTO order_items (
                        order_id, product_id, product_name, unit_price_cents, quantity, line_total_cents
                    )
                    VALUES (?, ?, ?, ?, ?, ?)
                    """,
                    (
                        order_id,
                        item["product_id"],
                        item["name"],
                        item["price_cents"],
                        item["quantity"],
                        item["line_total_cents"],
                    ),
                )
                connection.execute(
                    "UPDATE products SET stock_qty = stock_qty - ? WHERE id = ?",
                    (item["quantity"], item["product_id"]),
                )
        return order_id

    def list_orders(self):
        with self.connect() as connection:
            orders = connection.execute(
                """
                SELECT id, created_at, total_cents, payment_method, note
                FROM orders
                ORDER BY datetime(created_at) DESC, id DESC
                """
            ).fetchall()
            results = []
            for order in orders:
                items = connection.execute(
                    """
                    SELECT product_name, quantity, unit_price_cents, line_total_cents
                    FROM order_items
                    WHERE order_id = ?
                    ORDER BY id ASC
                    """,
                    (order["id"],),
                ).fetchall()
                order_dict = dict(order)
                order_dict["items"] = [dict(item) for item in items]
                results.append(order_dict)
            return results

    def daily_summary(self):
        today = datetime.utcnow().date().isoformat()
        with self.connect() as connection:
            row = connection.execute(
                """
                SELECT COUNT(*) AS order_count, COALESCE(SUM(total_cents), 0) AS total_cents
                FROM orders
                WHERE substr(created_at, 1, 10) = ?
                """,
                (today,),
            ).fetchone()
            return dict(row)
