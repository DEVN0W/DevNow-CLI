import io
import json
import os
from datetime import datetime

from .security import hash_password


def utc_now():
    return datetime.utcnow().strftime("%Y-%m-%d %H:%M:%S")


class Database(object):
    def __init__(self, path):
        self.path = path
        directory = os.path.dirname(path)
        if directory and not os.path.exists(directory):
            os.makedirs(directory)

    def initialize(self):
        if not os.path.exists(self.path):
            self.write_data(
                {
                    "users": [],
                    "products": [],
                    "sales": [],
                    "inventory_movements": [],
                    "next_ids": {"users": 1, "products": 1, "sales": 1, "inventory_movements": 1},
                }
            )
        data = self.read_data()
        changed = False
        if not data["users"]:
            self._append_user(data, "admin", "admin123", "admin")
            self._append_user(data, "cashier", "cashier123", "cashier")
            changed = True
        if not data["products"]:
            now = utc_now()
            for name, sku, price_cents, stock in [
                ("Coffee Beans 1kg", "CF-100", 1499, 20),
                ("Tea Box 40ct", "TE-200", 899, 30),
                ("Blueberry Muffin", "MF-300", 425, 18),
            ]:
                product_id = self.next_id(data, "products")
                data["products"].append(
                    {
                        "id": product_id,
                        "name": name,
                        "sku": sku,
                        "price_cents": price_cents,
                        "stock_quantity": stock,
                        "created_at": now,
                    }
                )
            changed = True
        if changed:
            self.write_data(data)

    def _append_user(self, data, username, password, role):
        user_id = self.next_id(data, "users")
        data["users"].append(
            {
                "id": user_id,
                "username": username,
                "password_hash": hash_password(password),
                "role": role,
                "created_at": utc_now(),
            }
        )

    def read_data(self):
        with io.open(self.path, "r", encoding="utf-8") as handle:
            return json.load(handle)

    def write_data(self, data):
        temp_path = self.path + ".tmp"
        with io.open(temp_path, "w", encoding="utf-8") as handle:
            json.dump(data, handle, indent=2, sort_keys=True)
        os.replace(temp_path, self.path)

    def next_id(self, data, bucket):
        next_value = data["next_ids"][bucket]
        data["next_ids"][bucket] += 1
        return next_value

    def get_user(self, username):
        data = self.read_data()
        for user in data["users"]:
            if user["username"] == username:
                return user
        return None

    def list_products(self):
        data = self.read_data()
        return sorted(data["products"], key=lambda row: row["name"])

    def get_product(self, product_id):
        data = self.read_data()
        for product in data["products"]:
            if product["id"] == product_id:
                return product
        return None

    def add_product(self, name, sku, price_cents, stock_quantity):
        data = self.read_data()
        for product in data["products"]:
            if product["sku"] == sku:
                raise ValueError("SKU already exists.")
        data["products"].append(
            {
                "id": self.next_id(data, "products"),
                "name": name,
                "sku": sku,
                "price_cents": price_cents,
                "stock_quantity": stock_quantity,
                "created_at": utc_now(),
            }
        )
        self.write_data(data)

    def adjust_stock(self, product_id, change_amount, reason):
        data = self.read_data()
        product = None
        for row in data["products"]:
            if row["id"] == product_id:
                product = row
                break
        if product is None:
            raise ValueError("Product not found.")
        next_stock = product["stock_quantity"] + change_amount
        if next_stock < 0:
            raise ValueError("Stock cannot drop below zero.")
        product["stock_quantity"] = next_stock
        data["inventory_movements"].append(
            {
                "id": self.next_id(data, "inventory_movements"),
                "product_id": product_id,
                "change_amount": change_amount,
                "reason": reason,
                "created_at": utc_now(),
            }
        )
        self.write_data(data)

    def list_sales(self):
        data = self.read_data()
        return sorted(data["sales"], key=lambda row: row["id"], reverse=True)

    def get_sale(self, sale_id):
        data = self.read_data()
        for sale in data["sales"]:
            if sale["id"] == sale_id:
                return sale, sale["items"]
        return None, []

    def create_sale(self, cashier, payment_method, cart_rows):
        data = self.read_data()
        current = dict((row["id"], row) for row in data["products"])
        for row in cart_rows:
            if row["quantity"] <= 0:
                raise ValueError("Cart quantity must be positive.")
            product = current.get(row["product_id"])
            if product is None:
                raise ValueError("Product no longer exists.")
            if product["stock_quantity"] < row["quantity"]:
                raise ValueError("Insufficient stock for %s." % row["name"])

        sale_id = self.next_id(data, "sales")
        total = sum(row["price_cents"] * row["quantity"] for row in cart_rows)
        items = []
        for row in cart_rows:
            line_total = row["price_cents"] * row["quantity"]
            current[row["product_id"]]["stock_quantity"] -= row["quantity"]
            items.append(
                {
                    "product_id": row["product_id"],
                    "product_name": row["name"],
                    "sku": row["sku"],
                    "quantity": row["quantity"],
                    "price_cents": row["price_cents"],
                    "line_total_cents": line_total,
                }
            )
            data["inventory_movements"].append(
                {
                    "id": self.next_id(data, "inventory_movements"),
                    "product_id": row["product_id"],
                    "change_amount": -row["quantity"],
                    "reason": "sale #%s" % sale_id,
                    "created_at": utc_now(),
                }
            )
        data["sales"].append(
            {
                "id": sale_id,
                "cashier": cashier,
                "total_cents": total,
                "payment_method": payment_method,
                "created_at": utc_now(),
                "items": items,
            }
        )
        self.write_data(data)
        return sale_id
