import copy
import json
import os
import tempfile
from datetime import datetime

import fcntl


DEFAULT_PRODUCTS = [
    ("SKU-1001", "Coffee Beans 1kg", 14.99, 18),
    ("SKU-1002", "Black Tea Box", 6.49, 34),
    ("SKU-1003", "Ceramic Mug", 8.95, 20),
    ("SKU-1004", "Chocolate Bar", 2.25, 55),
]


def get_db_path(base_dir):
    return os.path.join(base_dir, "data", "pos_data.json")


def ensure_directories(base_dir):
    os.makedirs(os.path.join(base_dir, "data"), exist_ok=True)


def utcnow():
    return datetime.utcnow().isoformat()


def seed_state():
    created_at = utcnow()
    products = []
    for index, item in enumerate(DEFAULT_PRODUCTS, start=1):
        sku, name, price, stock = item
        products.append(
            {
                "id": index,
                "sku": sku,
                "name": name,
                "price_cents": int(round(price * 100)),
                "stock_qty": stock,
                "active": 1,
                "created_at": created_at,
            }
        )
    return {
        "next_ids": {
            "products": len(products) + 1,
            "inventory_movements": 1,
            "sales": 1,
            "sale_items": 1,
        },
        "products": products,
        "inventory_movements": [],
        "sales": [],
        "sale_items": [],
    }


def initialize_database(db_path):
    if os.path.exists(db_path):
        return
    _write_atomic(db_path, seed_state())


def _lock_path(db_path):
    return db_path + ".lock"


def _read_unlocked(db_path):
    with open(db_path, "r") as handle:
        return json.load(handle)


def _write_atomic(db_path, state):
    directory = os.path.dirname(db_path)
    fd, temp_path = tempfile.mkstemp(prefix="pos-", suffix=".tmp", dir=directory)
    try:
        with os.fdopen(fd, "w") as handle:
            json.dump(state, handle, indent=2, sort_keys=True)
            handle.flush()
            os.fsync(handle.fileno())
        os.replace(temp_path, db_path)
    finally:
        if os.path.exists(temp_path):
            os.unlink(temp_path)


def _read_state_locked(db_path):
    with open(_lock_path(db_path), "a+") as lock_handle:
        fcntl.flock(lock_handle.fileno(), fcntl.LOCK_SH)
        try:
            return _read_unlocked(db_path)
        finally:
            fcntl.flock(lock_handle.fileno(), fcntl.LOCK_UN)


def _update_state_locked(db_path, updater):
    with open(_lock_path(db_path), "a+") as lock_handle:
        fcntl.flock(lock_handle.fileno(), fcntl.LOCK_EX)
        try:
            state = _read_unlocked(db_path)
            result = updater(state)
            _write_atomic(db_path, state)
            return result
        finally:
            fcntl.flock(lock_handle.fileno(), fcntl.LOCK_UN)


def list_products(db_path):
    state = _read_state_locked(db_path)
    return sorted(copy.deepcopy(state["products"]), key=lambda item: item["name"].lower())


def get_product(db_path, product_id):
    state = _read_state_locked(db_path)
    for product in state["products"]:
        if product["id"] == product_id:
            return copy.deepcopy(product)
    return None


def add_product(db_path, sku, name, price_cents, stock_qty):
    if stock_qty < 0:
        raise ValueError("Opening stock cannot be negative")
    if price_cents <= 0:
        raise ValueError("Price must be greater than zero")

    def updater(state):
        for product in state["products"]:
            if product["sku"].lower() == sku.lower():
                raise ValueError("SKU already exists")
        product = {
            "id": state["next_ids"]["products"],
            "sku": sku,
            "name": name,
            "price_cents": price_cents,
            "stock_qty": stock_qty,
            "active": 1,
            "created_at": utcnow(),
        }
        state["next_ids"]["products"] += 1
        state["products"].append(product)

    _update_state_locked(db_path, updater)


def update_product_stock(db_path, product_id, change_qty, reason):
    def updater(state):
        for product in state["products"]:
            if product["id"] == product_id:
                new_stock = product["stock_qty"] + change_qty
                if new_stock < 0:
                    raise ValueError("Stock cannot be negative")
                product["stock_qty"] = new_stock
                movement = {
                    "id": state["next_ids"]["inventory_movements"],
                    "product_id": product_id,
                    "product_name": product["name"],
                    "sku": product["sku"],
                    "change_qty": change_qty,
                    "reason": reason,
                    "created_at": utcnow(),
                }
                state["next_ids"]["inventory_movements"] += 1
                state["inventory_movements"].append(movement)
                return
        raise ValueError("Product not found")

    _update_state_locked(db_path, updater)


def get_inventory_movements(db_path):
    state = _read_state_locked(db_path)
    rows = copy.deepcopy(state["inventory_movements"])
    rows.sort(key=lambda item: item["created_at"], reverse=True)
    return rows[:25]


def create_sale(db_path, cart_items, payment_method, amount_tendered_cents, tax_rate):
    if not cart_items:
        raise ValueError("Cart cannot be empty")

    def updater(state):
        subtotal_cents = sum(item["price_cents"] * item["quantity"] for item in cart_items)
        tax_cents = int(round(subtotal_cents * tax_rate))
        total_cents = subtotal_cents + tax_cents

        if amount_tendered_cents < total_cents:
            raise ValueError("Amount tendered is less than total")

        indexed = {product["id"]: product for product in state["products"]}
        for item in cart_items:
            product = indexed.get(item["id"])
            if product is None:
                raise ValueError("Product not found")
            if item["quantity"] <= 0:
                raise ValueError("Quantity must be greater than zero")
            if product["stock_qty"] < item["quantity"]:
                raise ValueError("Insufficient stock for {0}".format(item["name"]))

        created_at = utcnow()
        receipt_number = "R-{0}".format(datetime.utcnow().strftime("%Y%m%d%H%M%S%f"))
        sale = {
            "id": state["next_ids"]["sales"],
            "receipt_number": receipt_number,
            "subtotal_cents": subtotal_cents,
            "tax_cents": tax_cents,
            "total_cents": total_cents,
            "payment_method": payment_method,
            "amount_tendered_cents": amount_tendered_cents,
            "change_due_cents": amount_tendered_cents - total_cents,
            "created_at": created_at,
        }
        state["next_ids"]["sales"] += 1
        state["sales"].append(sale)

        for item in cart_items:
            product = indexed[item["id"]]
            line_total = item["price_cents"] * item["quantity"]
            sale_item = {
                "id": state["next_ids"]["sale_items"],
                "sale_id": sale["id"],
                "product_id": item["id"],
                "product_name": item["name"],
                "sku": item["sku"],
                "unit_price_cents": item["price_cents"],
                "quantity": item["quantity"],
                "line_total_cents": line_total,
            }
            state["next_ids"]["sale_items"] += 1
            state["sale_items"].append(sale_item)
            product["stock_qty"] -= item["quantity"]
            state["inventory_movements"].append(
                {
                    "id": state["next_ids"]["inventory_movements"],
                    "product_id": item["id"],
                    "product_name": item["name"],
                    "sku": item["sku"],
                    "change_qty": -item["quantity"],
                    "reason": "Sale {0}".format(receipt_number),
                    "created_at": created_at,
                }
            )
            state["next_ids"]["inventory_movements"] += 1
        return sale["id"]

    return _update_state_locked(db_path, updater)


def get_sale(db_path, sale_id):
    state = _read_state_locked(db_path)
    sale = None
    for item in state["sales"]:
        if item["id"] == sale_id:
            sale = copy.deepcopy(item)
            break
    if sale is None:
        return None
    sale["items"] = [
        copy.deepcopy(item)
        for item in state["sale_items"]
        if item["sale_id"] == sale_id
    ]
    return sale


def list_recent_sales(db_path, limit=10):
    state = _read_state_locked(db_path)
    rows = copy.deepcopy(state["sales"])
    rows.sort(key=lambda item: item["created_at"], reverse=True)
    return rows[:limit]


def get_report_summary(db_path):
    state = _read_state_locked(db_path)
    sale_count = len(state["sales"])
    revenue_cents = sum(item["total_cents"] for item in state["sales"])
    tax_cents = sum(item["tax_cents"] for item in state["sales"])
    top_index = {}
    for item in state["sale_items"]:
        key = item["product_id"]
        if key not in top_index:
            top_index[key] = {
                "product_name": item["product_name"],
                "sku": item["sku"],
                "units_sold": 0,
                "revenue_cents": 0,
            }
        top_index[key]["units_sold"] += item["quantity"]
        top_index[key]["revenue_cents"] += item["line_total_cents"]
    top_products = list(top_index.values())
    top_products.sort(key=lambda item: (-item["units_sold"], -item["revenue_cents"], item["product_name"]))
    low_stock = [
        copy.deepcopy(item)
        for item in state["products"]
        if item["stock_qty"] <= 5
    ]
    low_stock.sort(key=lambda item: (item["stock_qty"], item["name"].lower()))
    return {
        "sale_count": sale_count,
        "revenue_cents": revenue_cents,
        "tax_cents": tax_cents,
        "top_products": top_products[:5],
        "low_stock": low_stock,
    }
