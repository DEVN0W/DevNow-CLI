import sqlite3
from decimal import Decimal, InvalidOperation, ROUND_HALF_UP

from pos.db import connect


TAX_RATE = Decimal("0.08")


class POSService:
    def __init__(self, database_path):
        self.database_path = database_path

    def list_products(self):
        with connect(self.database_path) as connection:
            rows = connection.execute(
                """
                SELECT id, sku, name, price_cents, stock, active, created_at
                FROM products
                ORDER BY name COLLATE NOCASE ASC
                """
            ).fetchall()
        return [dict(row) for row in rows]

    def get_product(self, product_id):
        with connect(self.database_path) as connection:
            row = connection.execute(
                """
                SELECT id, sku, name, price_cents, stock, active, created_at
                FROM products
                WHERE id = ?
                """,
                (product_id,),
            ).fetchone()
        return dict(row) if row else None

    def create_product(self, sku, name, price_text, stock_text):
        sku = (sku or "").strip().upper()
        name = (name or "").strip()
        price_cents = money_to_cents(price_text)
        stock = parse_non_negative_int(stock_text, "Stock")

        if not sku:
            raise ValueError("SKU is required.")
        if not name:
            raise ValueError("Product name is required.")

        with connect(self.database_path) as connection:
            try:
                connection.execute(
                    """
                    INSERT INTO products (sku, name, price_cents, stock)
                    VALUES (?, ?, ?, ?)
                    """,
                    (sku, name, price_cents, stock),
                )
            except sqlite3.IntegrityError as exc:
                if "UNIQUE constraint failed" in str(exc):
                    raise ValueError("SKU already exists.")
                raise
            connection.commit()

    def get_cart_items(self, cart):
        normalized = []
        subtotal_cents = 0
        for product_id_text, quantity in cart.items():
            product_id = int(product_id_text)
            product = self.get_product(product_id)
            if not product or not product["active"]:
                continue
            safe_quantity = max(0, int(quantity))
            if safe_quantity == 0:
                continue
            line_total_cents = product["price_cents"] * safe_quantity
            subtotal_cents += line_total_cents
            normalized.append(
                {
                    "product_id": product["id"],
                    "sku": product["sku"],
                    "name": product["name"],
                    "stock": product["stock"],
                    "quantity": safe_quantity,
                    "price_cents": product["price_cents"],
                    "line_total_cents": line_total_cents,
                }
            )
        tax_cents = calculate_tax(subtotal_cents)
        return {
            "items": normalized,
            "subtotal_cents": subtotal_cents,
            "tax_cents": tax_cents,
            "total_cents": subtotal_cents + tax_cents,
        }

    def create_sale(self, cart):
        cart_snapshot = self.get_cart_items(cart)
        if not cart_snapshot["items"]:
            raise ValueError("Cart is empty.")

        with connect(self.database_path) as connection:
            try:
                for item in cart_snapshot["items"]:
                    current = connection.execute(
                        "SELECT stock, name FROM products WHERE id = ?",
                        (item["product_id"],),
                    ).fetchone()
                    if current is None:
                        raise ValueError("One of the products no longer exists.")
                    if current["stock"] < item["quantity"]:
                        raise ValueError(
                            "Insufficient stock for {name}.".format(
                                name=current["name"]
                            )
                        )

                cursor = connection.execute(
                    """
                    INSERT INTO sales (subtotal_cents, tax_cents, total_cents)
                    VALUES (?, ?, ?)
                    """,
                    (
                        cart_snapshot["subtotal_cents"],
                        cart_snapshot["tax_cents"],
                        cart_snapshot["total_cents"],
                    ),
                )
                sale_id = cursor.lastrowid

                for item in cart_snapshot["items"]:
                    connection.execute(
                        """
                        INSERT INTO sale_items (
                            sale_id,
                            product_id,
                            product_name,
                            quantity,
                            unit_price_cents,
                            line_total_cents
                        )
                        VALUES (?, ?, ?, ?, ?, ?)
                        """,
                        (
                            sale_id,
                            item["product_id"],
                            item["name"],
                            item["quantity"],
                            item["price_cents"],
                            item["line_total_cents"],
                        ),
                    )
                    connection.execute(
                        """
                        UPDATE products
                        SET stock = stock - ?
                        WHERE id = ?
                        """,
                        (item["quantity"], item["product_id"]),
                    )
            except Exception:
                connection.rollback()
                raise
            connection.commit()

        return sale_id

    def list_sales(self):
        with connect(self.database_path) as connection:
            sales = connection.execute(
                """
                SELECT id, subtotal_cents, tax_cents, total_cents, created_at
                FROM sales
                ORDER BY id DESC
                LIMIT 50
                """
            ).fetchall()
            sale_rows = [dict(row) for row in sales]
            for sale in sale_rows:
                items = connection.execute(
                    """
                    SELECT product_name, quantity, unit_price_cents, line_total_cents
                    FROM sale_items
                    WHERE sale_id = ?
                    ORDER BY id ASC
                    """,
                    (sale["id"],),
                ).fetchall()
                sale["items"] = [dict(item) for item in items]
        return sale_rows

    def dashboard_summary(self):
        with connect(self.database_path) as connection:
            metrics = connection.execute(
                """
                SELECT
                    (SELECT COUNT(*) FROM products WHERE active = 1) AS active_products,
                    (SELECT COALESCE(SUM(stock), 0) FROM products WHERE active = 1) AS units_in_stock,
                    (SELECT COUNT(*) FROM sales) AS total_sales,
                    (SELECT COALESCE(SUM(total_cents), 0) FROM sales) AS revenue_cents
                """
            ).fetchone()
        return dict(metrics)


def parse_non_negative_int(value, label):
    try:
        number = int(str(value).strip())
    except ValueError:
        raise ValueError("{label} must be a whole number.".format(label=label))
    if number < 0:
        raise ValueError("{label} cannot be negative.".format(label=label))
    return number


def money_to_cents(value):
    try:
        decimal_value = Decimal(str(value).strip())
    except (InvalidOperation, ValueError):
        raise ValueError("Price must be a valid amount.")
    if decimal_value < 0:
        raise ValueError("Price cannot be negative.")
    quantized = decimal_value.quantize(Decimal("0.01"), rounding=ROUND_HALF_UP)
    return int(quantized * 100)


def calculate_tax(subtotal_cents):
    subtotal = Decimal(subtotal_cents) / Decimal("100")
    tax = (subtotal * TAX_RATE).quantize(Decimal("0.01"), rounding=ROUND_HALF_UP)
    return int(tax * 100)
