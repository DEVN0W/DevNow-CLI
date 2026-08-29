import hashlib
import html
import hmac
import io
import json
import os
import secrets
import time
from http import cookies
from urllib.parse import parse_qs

from . import database


def money(cents):
    return "${0:.2f}".format(cents / 100.0)


def html_escape(text):
    return html.escape("" if text is None else str(text), quote=True)


def parse_post(environ):
    length = int(environ.get("CONTENT_LENGTH") or 0)
    payload = environ["wsgi.input"].read(length) if length else b""
    content_type = environ.get("CONTENT_TYPE", "")
    if "application/x-www-form-urlencoded" in content_type:
        parsed = parse_qs(payload.decode("utf-8"), keep_blank_values=True)
        return {key: values[0] for key, values in parsed.items()}
    if "application/json" in content_type:
        return json.loads(payload.decode("utf-8") or "{}")
    return {}


def redirect(location, messages=None):
    headers = [("Location", location)]
    if messages is not None:
        jar = cookies.SimpleCookie()
        jar["flash"] = json.dumps(messages)
        jar["flash"]["path"] = "/"
        headers.append(("Set-Cookie", jar.output(header="").strip()))
    return "302 Found", headers, b""


class POSApplication(object):
    def __init__(self, base_dir):
        self.base_dir = base_dir
        self.db_path = database.get_db_path(base_dir)
        database.ensure_directories(base_dir)
        database.initialize_database(self.db_path)
        self.tax_rate = 0.07
        self.store_name = "Corner Shop POS"
        self.admin_pin = self.load_or_create_secret("admin_pin.txt", digits=12)
        self.session_secret = self.load_or_create_secret("session_secret.txt", hex_bytes=32)
        self.login_attempts = {}

    def load_or_create_secret(self, filename, digits=None, hex_bytes=None):
        path = os.path.join(self.base_dir, "data", filename)
        if os.path.exists(path):
            try:
                os.chmod(path, 0o600)
            except OSError:
                pass
            with open(path, "r") as handle:
                return handle.read().strip()
        if digits is not None:
            minimum = 10 ** (digits - 1)
            value = str(secrets.randbelow(9 * minimum) + minimum)
        else:
            value = secrets.token_hex(hex_bytes or 32)
        fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
        with os.fdopen(fd, "w") as handle:
            handle.write(value)
        return value

    def client_identifier(self, environ):
        return environ.get("REMOTE_ADDR") or "local"

    def login_is_throttled(self, environ):
        client_id = self.client_identifier(environ)
        attempt = self.login_attempts.get(client_id)
        if not attempt:
            return False
        if attempt["count"] < 5:
            return False
        return (time.time() - attempt["last_failure"]) < 300

    def record_login_failure(self, environ):
        client_id = self.client_identifier(environ)
        attempt = self.login_attempts.get(client_id, {"count": 0, "last_failure": 0})
        attempt["count"] += 1
        attempt["last_failure"] = time.time()
        self.login_attempts[client_id] = attempt

    def clear_login_failures(self, environ):
        client_id = self.client_identifier(environ)
        if client_id in self.login_attempts:
            del self.login_attempts[client_id]

    def sign_session(self, subject):
        return hmac.new(
            self.session_secret.encode("utf-8"),
            subject.encode("utf-8"),
            hashlib.sha256,
        ).hexdigest()

    def get_session_data(self, environ):
        raw_cookie = environ.get("HTTP_COOKIE", "")
        jar = cookies.SimpleCookie()
        jar.load(raw_cookie)
        if "session" not in jar:
            return None
        raw_value = jar["session"].value
        if ":" not in raw_value:
            return None
        payload, signature = raw_value.split(":", 1)
        expected = self.sign_session(payload)
        if not hmac.compare_digest(signature, expected):
            return None
        if "|" not in payload:
            return None
        subject, csrf_token = payload.split("|", 1)
        if subject != "staff" or not csrf_token:
            return None
        return {"subject": subject, "csrf_token": csrf_token}

    def is_authenticated(self, environ):
        return self.get_session_data(environ) is not None

    def require_auth(self, environ):
        return environ.get("PATH_INFO") not in ("/login", "/health", "/static/app.css") and not self.is_authenticated(environ)

    def csrf_token(self, environ):
        session = self.get_session_data(environ)
        return session["csrf_token"] if session else ""

    def csrf_input(self, environ):
        return '<input type="hidden" name="csrf_token" value="{0}">'.format(
            html_escape(self.csrf_token(environ))
        )

    def validate_csrf(self, environ, form):
        submitted = (form.get("csrf_token") or "").strip()
        expected = self.csrf_token(environ)
        if not submitted or not expected or not hmac.compare_digest(submitted, expected):
            raise ValueError("Invalid CSRF token")

    def __call__(self, environ, start_response):
        method = environ["REQUEST_METHOD"].upper()
        path = environ.get("PATH_INFO", "/")

        try:
            if self.require_auth(environ):
                status, headers, body = redirect("/login")
            elif path == "/":
                status, headers, body = self.handle_dashboard(environ)
            elif path == "/static/app.css":
                status, headers, body = self.static_response("app.css")
            elif path == "/login":
                status, headers, body = self.handle_login(environ, method)
            elif path == "/logout":
                status, headers, body = self.handle_logout()
            elif path == "/products":
                status, headers, body = self.handle_products(environ, method)
            elif path == "/inventory":
                status, headers, body = self.handle_inventory(environ, method)
            elif path == "/checkout":
                status, headers, body = self.handle_checkout(environ, method)
            elif path.startswith("/receipt/"):
                status, headers, body = self.handle_receipt(environ)
            elif path == "/reports":
                status, headers, body = self.handle_reports(environ)
            elif path == "/health":
                status, headers, body = self.handle_health()
            else:
                status, headers, body = self.render_page(
                    "Not Found",
                    "<p>The requested page was not found.</p>",
                    status="404 Not Found",
                )
        except ValueError as exc:
            status, headers, body = self.render_page(
                "Error",
                "<p>{0}</p>".format(html_escape(str(exc))),
                status="400 Bad Request",
            )

        start_response(status, headers)
        return [body]

    def get_flash_messages(self, environ):
        raw_cookie = environ.get("HTTP_COOKIE", "")
        jar = cookies.SimpleCookie()
        jar.load(raw_cookie)
        if "flash" not in jar:
            return [], []
        try:
            messages = json.loads(jar["flash"].value)
        except ValueError:
            messages = []
        clear_headers = [("Set-Cookie", "flash=; Path=/; Max-Age=0")]
        return messages, clear_headers

    def navigation(self):
        return (
            '<nav class="nav">'
            '<a href="/">Dashboard</a>'
            '<a href="/products">Products</a>'
            '<a href="/inventory">Inventory</a>'
            '<a href="/checkout">Checkout</a>'
            '<a href="/reports">Reports</a>'
            '<a href="/logout">Logout</a>'
            "</nav>"
        )

    def render_page(self, title, content, status="200 OK", extra_headers=None, environ=None):
        extra_headers = extra_headers or []
        flash_html = ""
        clear_headers = []
        if environ is not None:
            flashes, clear_headers = self.get_flash_messages(environ)
            if flashes:
                flash_html = "".join(
                    [
                        '<div class="flash {0}">{1}</div>'.format(
                            html_escape(item.get("level", "info")),
                            html_escape(item.get("text", "")),
                        )
                        for item in flashes
                    ]
                )

        body = """<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>{title}</title>
  <link rel="stylesheet" href="/static/app.css">
</head>
<body>
  <header class="hero">
    <div>
      <p class="eyebrow">Retail Control Panel</p>
      <h1>{store_name}</h1>
      <p>Manage products, capture sales, and review store performance from one local app.</p>
    </div>
  </header>
  {nav}
  <main class="page">
    {flash_html}
    <section class="panel">
      <h2>{title}</h2>
      {content}
    </section>
  </main>
</body>
</html>
""".format(
            title=html_escape(title),
            store_name=html_escape(self.store_name),
            nav=self.navigation(),
            flash_html=flash_html,
            content=content,
        )
        headers = [("Content-Type", "text/html; charset=utf-8")] + clear_headers + extra_headers
        return status, headers, body.encode("utf-8")

    def static_response(self, filename):
        static_path = os.path.join(self.base_dir, "app", "static", filename)
        if not os.path.exists(static_path):
            return self.render_page("Missing Asset", "<p>Static asset not found.</p>", status="404 Not Found")
        with open(static_path, "rb") as handle:
            body = handle.read()
        content_type = "text/css; charset=utf-8" if filename.endswith(".css") else "application/octet-stream"
        return "200 OK", [("Content-Type", content_type)], body

    def handle_dashboard(self, environ):
        products = database.list_products(self.db_path)
        recent_sales = database.list_recent_sales(self.db_path, limit=5)
        report = database.get_report_summary(self.db_path)
        low_stock_rows = "".join(
            [
                "<tr><td>{sku}</td><td>{name}</td><td>{stock}</td></tr>".format(
                    sku=html_escape(item["sku"]),
                    name=html_escape(item["name"]),
                    stock=item["stock_qty"],
                )
                for item in report["low_stock"]
            ]
        ) or '<tr><td colspan="3">No low-stock products.</td></tr>'
        sales_rows = "".join(
            [
                "<tr><td>{receipt}</td><td>{method}</td><td>{total}</td><td>{created}</td></tr>".format(
                    receipt=html_escape(item["receipt_number"]),
                    method=html_escape(item["payment_method"].title()),
                    total=money(item["total_cents"]),
                    created=html_escape(item["created_at"][:19].replace("T", " ")),
                )
                for item in recent_sales
            ]
        ) or '<tr><td colspan="4">No sales yet.</td></tr>'
        content = """
<div class="grid three">
  <article class="stat"><span>Products</span><strong>{product_count}</strong></article>
  <article class="stat"><span>Sales</span><strong>{sale_count}</strong></article>
  <article class="stat"><span>Revenue</span><strong>{revenue}</strong></article>
</div>
<div class="grid two">
  <section>
    <h3>Recent Sales</h3>
    <table>
      <thead><tr><th>Receipt</th><th>Payment</th><th>Total</th><th>Created</th></tr></thead>
      <tbody>{sales_rows}</tbody>
    </table>
  </section>
  <section>
    <h3>Low Stock</h3>
    <table>
      <thead><tr><th>SKU</th><th>Name</th><th>Stock</th></tr></thead>
      <tbody>{low_stock_rows}</tbody>
    </table>
  </section>
</div>
""".format(
            product_count=len(products),
            sale_count=report["sale_count"],
            revenue=money(report["revenue_cents"]),
            sales_rows=sales_rows,
            low_stock_rows=low_stock_rows,
        )
        return self.render_page("Dashboard", content, environ=environ)

    def handle_login(self, environ, method):
        if method == "POST":
            form = parse_post(environ)
            if self.login_is_throttled(environ):
                raise ValueError("Too many failed login attempts. Wait 5 minutes and try again.")
            submitted_pin = (form.get("pin") or "").strip()
            if submitted_pin != self.admin_pin:
                self.record_login_failure(environ)
                raise ValueError("Invalid staff PIN")
            self.clear_login_failures(environ)
            payload = "staff|{0}".format(secrets.token_hex(16))
            session_value = "{0}:{1}".format(payload, self.sign_session(payload))
            status, headers, body = redirect(
                "/",
                [{"level": "success", "text": "Signed in successfully."}],
            )
            headers.append(
                (
                    "Set-Cookie",
                    "session={0}; Path=/; HttpOnly; SameSite=Strict".format(session_value),
                )
            )
            return status, headers, body

        content = """
<div class="login-card">
  <p class="muted">Enter the staff passcode from <code>data/admin_pin.txt</code> to access the POS.</p>
  <form method="post" class="stack">
    <label>Staff Passcode<input name="pin" type="password" required></label>
    <button type="submit">Sign In</button>
  </form>
</div>
"""
        return self.render_page("Staff Login", content, environ=environ)

    def handle_logout(self):
        return "302 Found", [
            ("Location", "/login"),
            ("Set-Cookie", "session=; Path=/; Max-Age=0"),
            ("Set-Cookie", "flash=; Path=/; Max-Age=0"),
        ], b""

    def handle_products(self, environ, method):
        if method == "POST":
            form = parse_post(environ)
            self.validate_csrf(environ, form)
            sku = (form.get("sku") or "").strip()
            name = (form.get("name") or "").strip()
            price = float(form.get("price") or "0")
            stock_qty = int(form.get("stock_qty") or "0")
            if not sku or not name or price <= 0:
                raise ValueError("SKU, product name, and a positive price are required")
            if stock_qty < 0:
                raise ValueError("Opening stock cannot be negative")
            database.add_product(self.db_path, sku, name, int(round(price * 100)), stock_qty)
            return redirect(
                "/products",
                [{"level": "success", "text": "Product created successfully."}],
            )

        rows = "".join(
            [
                "<tr><td>{sku}</td><td>{name}</td><td>{price}</td><td>{stock}</td></tr>".format(
                    sku=html_escape(item["sku"]),
                    name=html_escape(item["name"]),
                    price=money(item["price_cents"]),
                    stock=item["stock_qty"],
                )
                for item in database.list_products(self.db_path)
            ]
        )
        content = """
<div class="grid two">
  <section>
    <h3>Add Product</h3>
    <form method="post" class="stack">
      {csrf_input}
      <label>SKU<input name="sku" required></label>
      <label>Name<input name="name" required></label>
      <label>Price<input type="number" name="price" min="0.01" step="0.01" required></label>
      <label>Opening Stock<input type="number" name="stock_qty" min="0" step="1" value="0" required></label>
      <button type="submit">Save Product</button>
    </form>
  </section>
  <section>
    <h3>Catalog</h3>
    <table>
      <thead><tr><th>SKU</th><th>Name</th><th>Price</th><th>Stock</th></tr></thead>
      <tbody>{rows}</tbody>
    </table>
  </section>
</div>
""".format(rows=rows, csrf_input=self.csrf_input(environ))
        return self.render_page("Products", content, environ=environ)

    def handle_inventory(self, environ, method):
        if method == "POST":
            form = parse_post(environ)
            self.validate_csrf(environ, form)
            product_id = int(form.get("product_id") or "0")
            change_qty = int(form.get("change_qty") or "0")
            reason = (form.get("reason") or "").strip() or "Manual adjustment"
            if product_id <= 0 or change_qty == 0:
                raise ValueError("Select a product and enter a non-zero stock adjustment")
            database.update_product_stock(self.db_path, product_id, change_qty, reason)
            return redirect(
                "/inventory",
                [{"level": "success", "text": "Inventory updated."}],
            )

        products = database.list_products(self.db_path)
        options = "".join(
            [
                '<option value="{id}">{name} ({sku})</option>'.format(
                    id=item["id"],
                    name=html_escape(item["name"]),
                    sku=html_escape(item["sku"]),
                )
                for item in products
            ]
        )
        movement_rows = "".join(
            [
                "<tr><td>{created}</td><td>{sku}</td><td>{name}</td><td>{qty}</td><td>{reason}</td></tr>".format(
                    created=html_escape(item["created_at"][:19].replace("T", " ")),
                    sku=html_escape(item["sku"]),
                    name=html_escape(item["product_name"]),
                    qty=item["change_qty"],
                    reason=html_escape(item["reason"]),
                )
                for item in database.get_inventory_movements(self.db_path)
            ]
        ) or '<tr><td colspan="5">No adjustments recorded.</td></tr>'
        content = """
<div class="grid two">
  <section>
    <h3>Adjust Stock</h3>
    <form method="post" class="stack">
      {csrf_input}
      <label>Product<select name="product_id" required>{options}</select></label>
      <label>Quantity Change<input type="number" name="change_qty" step="1" required></label>
      <label>Reason<input name="reason" value="Stock count update"></label>
      <button type="submit">Apply Adjustment</button>
    </form>
  </section>
  <section>
    <h3>Recent Movements</h3>
    <table>
      <thead><tr><th>Created</th><th>SKU</th><th>Name</th><th>Qty</th><th>Reason</th></tr></thead>
      <tbody>{movement_rows}</tbody>
    </table>
  </section>
</div>
""".format(options=options, movement_rows=movement_rows, csrf_input=self.csrf_input(environ))
        return self.render_page("Inventory", content, environ=environ)

    def handle_checkout(self, environ, method):
        products = database.list_products(self.db_path)
        query = parse_qs(environ.get("QUERY_STRING", ""))
        cart = []
        for item in products:
            qty_key = "qty_{0}".format(item["id"])
            quantity = int(query.get(qty_key, ["0"])[0] or "0")
            if quantity > 0:
                cart.append(
                    {
                        "id": item["id"],
                        "sku": item["sku"],
                        "name": item["name"],
                        "price_cents": item["price_cents"],
                        "quantity": quantity,
                    }
                )
        subtotal_cents = sum(item["price_cents"] * item["quantity"] for item in cart)
        tax_cents = int(round(subtotal_cents * self.tax_rate))
        total_cents = subtotal_cents + tax_cents

        if method == "POST":
            form = parse_post(environ)
            self.validate_csrf(environ, form)
            cart = []
            for item in products:
                quantity = int(form.get("qty_{0}".format(item["id"])) or "0")
                if quantity > 0:
                    cart.append(
                        {
                            "id": item["id"],
                            "sku": item["sku"],
                            "name": item["name"],
                            "price_cents": item["price_cents"],
                            "quantity": quantity,
                        }
                    )
            tendered = float(form.get("amount_tendered") or "0")
            sale_id = database.create_sale(
                self.db_path,
                cart,
                (form.get("payment_method") or "cash").strip().lower(),
                int(round(tendered * 100)),
                self.tax_rate,
            )
            return redirect(
                "/receipt/{0}".format(sale_id),
                [{"level": "success", "text": "Sale completed."}],
            )

        product_rows = "".join(
            [
                """
<tr>
  <td>{sku}</td>
  <td>{name}</td>
  <td>{price}</td>
  <td>{stock}</td>
  <td><input type="number" min="0" max="{stock}" step="1" name="qty_{id}" value="{qty}"></td>
</tr>
""".format(
                    sku=html_escape(item["sku"]),
                    name=html_escape(item["name"]),
                    price=money(item["price_cents"]),
                    stock=item["stock_qty"],
                    id=item["id"],
                    qty=([cart_item["quantity"] for cart_item in cart if cart_item["id"] == item["id"]] or [0])[0],
                )
                for item in products
            ]
        )
        cart_rows = "".join(
            [
                "<tr><td>{name}</td><td>{qty}</td><td>{line}</td></tr>".format(
                    name=html_escape(item["name"]),
                    qty=item["quantity"],
                    line=money(item["price_cents"] * item["quantity"]),
                )
                for item in cart
            ]
        ) or '<tr><td colspan="3">Select quantities and submit the form to build a sale.</td></tr>'
        content = """
<div class="grid two">
  <section>
    <h3>Build Cart</h3>
    <form method="post" class="stack">
      {csrf_input}
      <table>
        <thead><tr><th>SKU</th><th>Name</th><th>Price</th><th>Stock</th><th>Qty</th></tr></thead>
        <tbody>{product_rows}</tbody>
      </table>
      <label>Payment Method
        <select name="payment_method">
          <option value="cash">Cash</option>
          <option value="card">Card</option>
          <option value="mobile">Mobile</option>
        </select>
      </label>
      <label>Amount Tendered<input type="number" name="amount_tendered" min="0" step="0.01" value="{total_value}" required></label>
      <button type="submit">Complete Sale</button>
    </form>
  </section>
  <section>
    <h3>Cart Summary</h3>
    <table>
      <thead><tr><th>Item</th><th>Qty</th><th>Total</th></tr></thead>
      <tbody>{cart_rows}</tbody>
    </table>
    <div class="totals">
      <p><span>Subtotal</span><strong>{subtotal}</strong></p>
      <p><span>Tax</span><strong>{tax}</strong></p>
      <p><span>Total</span><strong>{total}</strong></p>
    </div>
    <p class="muted">Tip: enter quantities, then submit. For cash sales, set tendered above the total to calculate change.</p>
  </section>
</div>
""".format(
            product_rows=product_rows,
            cart_rows=cart_rows,
            subtotal=money(subtotal_cents),
            tax=money(tax_cents),
            total=money(total_cents),
            total_value="{0:.2f}".format(total_cents / 100.0),
            csrf_input=self.csrf_input(environ),
        )
        return self.render_page("Checkout", content, environ=environ)

    def handle_receipt(self, environ):
        try:
            sale_id = int(environ.get("PATH_INFO", "/0").split("/")[-1])
        except ValueError:
            return self.render_page("Receipt Not Found", "<p>Receipt not found.</p>", status="404 Not Found")
        sale = database.get_sale(self.db_path, sale_id)
        if sale is None:
            return self.render_page("Receipt Not Found", "<p>Receipt not found.</p>", status="404 Not Found")
        item_rows = "".join(
            [
                "<tr><td>{name}</td><td>{qty}</td><td>{price}</td><td>{line}</td></tr>".format(
                    name=html_escape(item["product_name"]),
                    qty=item["quantity"],
                    price=money(item["unit_price_cents"]),
                    line=money(item["line_total_cents"]),
                )
                for item in sale["items"]
            ]
        )
        content = """
<div class="receipt">
  <p><strong>Receipt:</strong> {receipt}</p>
  <p><strong>Created:</strong> {created}</p>
  <p><strong>Payment:</strong> {payment}</p>
  <table>
    <thead><tr><th>Item</th><th>Qty</th><th>Unit</th><th>Total</th></tr></thead>
    <tbody>{item_rows}</tbody>
  </table>
  <div class="totals">
    <p><span>Subtotal</span><strong>{subtotal}</strong></p>
    <p><span>Tax</span><strong>{tax}</strong></p>
    <p><span>Total</span><strong>{total}</strong></p>
    <p><span>Tendered</span><strong>{tendered}</strong></p>
    <p><span>Change Due</span><strong>{change}</strong></p>
  </div>
  <p><a href="/checkout">Start another sale</a></p>
</div>
""".format(
            receipt=html_escape(sale["receipt_number"]),
            created=html_escape(sale["created_at"][:19].replace("T", " ")),
            payment=html_escape(sale["payment_method"].title()),
            item_rows=item_rows,
            subtotal=money(sale["subtotal_cents"]),
            tax=money(sale["tax_cents"]),
            total=money(sale["total_cents"]),
            tendered=money(sale["amount_tendered_cents"]),
            change=money(sale["change_due_cents"]),
        )
        return self.render_page("Receipt", content, environ=environ)

    def handle_reports(self, environ):
        report = database.get_report_summary(self.db_path)
        top_rows = "".join(
            [
                "<tr><td>{name}</td><td>{sku}</td><td>{units}</td><td>{revenue}</td></tr>".format(
                    name=html_escape(item["product_name"]),
                    sku=html_escape(item["sku"]),
                    units=item["units_sold"],
                    revenue=money(item["revenue_cents"]),
                )
                for item in report["top_products"]
            ]
        ) or '<tr><td colspan="4">No sales yet.</td></tr>'
        content = """
<div class="grid three">
  <article class="stat"><span>Total Sales</span><strong>{count}</strong></article>
  <article class="stat"><span>Revenue</span><strong>{revenue}</strong></article>
  <article class="stat"><span>Tax Collected</span><strong>{tax}</strong></article>
</div>
<section>
  <h3>Top Products</h3>
  <table>
    <thead><tr><th>Name</th><th>SKU</th><th>Units Sold</th><th>Revenue</th></tr></thead>
    <tbody>{top_rows}</tbody>
  </table>
</section>
""".format(
            count=report["sale_count"],
            revenue=money(report["revenue_cents"]),
            tax=money(report["tax_cents"]),
            top_rows=top_rows,
        )
        return self.render_page("Reports", content, environ=environ)

    def handle_health(self):
        return "200 OK", [("Content-Type", "text/plain; charset=utf-8")], b"ok"


def build_test_environ(path="/", method="GET", body="", content_type="application/x-www-form-urlencoded"):
    encoded = body.encode("utf-8")
    return {
        "REQUEST_METHOD": method,
        "PATH_INFO": path,
        "QUERY_STRING": "",
        "CONTENT_TYPE": content_type,
        "CONTENT_LENGTH": str(len(encoded)),
        "wsgi.input": io.BytesIO(encoded),
        "HTTP_COOKIE": "",
    }
