import hashlib
import hmac
import os
import secrets
from cgi import FieldStorage
from html import escape
from http.cookies import SimpleCookie
from urllib.parse import parse_qs
from wsgiref.simple_server import make_server
from wsgiref.util import setup_testing_defaults

from .database import Database


def money(cents):
    return "$%0.2f" % (float(cents) / 100.0)


def to_int(value, default=0):
    try:
        return int(value)
    except (TypeError, ValueError):
        return default


class POSApplication(object):
    def __init__(self, db_path, secret_key=None):
        self.db = Database(db_path)
        self.db.initialize()
        self.secret_key = (secret_key or "dev-secret").encode("utf-8")
        self.sessions = {}

    def __call__(self, environ, start_response):
        setup_testing_defaults(environ)
        method = environ.get("REQUEST_METHOD", "GET").upper()
        path = environ.get("PATH_INFO", "/")
        session_id, session, set_cookie = self._load_session(environ)

        try:
            if method == "GET" and path == "/":
                status, headers, body = self.index(session)
            elif method == "GET" and path == "/orders":
                status, headers, body = self.orders(session)
            elif method == "POST" and path == "/products":
                status, headers, body = self.create_product(environ, session)
            elif method == "POST" and path == "/cart/add":
                status, headers, body = self.add_to_cart(environ, session)
            elif method == "POST" and path == "/cart/update":
                status, headers, body = self.update_cart(environ, session)
            elif method == "POST" and path == "/checkout":
                status, headers, body = self.checkout(environ, session)
            elif method == "GET" and path == "/assets/style.css":
                status, headers, body = self.style()
            else:
                status, headers, body = self.not_found()
        except ValueError as exc:
            session["flash"] = str(exc)
            status, headers, body = self.redirect("/")

        if set_cookie:
            headers.append(("Set-Cookie", set_cookie))
        start_response(status, headers)
        return [body.encode("utf-8")]

    def _load_session(self, environ):
        cookie = SimpleCookie(environ.get("HTTP_COOKIE", ""))
        morsel = cookie.get("pos_session")
        if morsel and morsel.value in self.sessions:
            session_id = morsel.value
            session = self.sessions[session_id]
            return session_id, session, None

        session_id = secrets.token_hex(16)
        session = {
            "cart": {},
            "csrf_token": self._sign_token(secrets.token_hex(16)),
            "flash": "",
        }
        self.sessions[session_id] = session
        cookie = SimpleCookie()
        cookie["pos_session"] = session_id
        cookie["pos_session"]["path"] = "/"
        cookie["pos_session"]["httponly"] = True
        return session_id, session, cookie.output(header="").strip()

    def _sign_token(self, token):
        digest = hmac.new(self.secret_key, token.encode("utf-8"), hashlib.sha256).hexdigest()
        return "%s:%s" % (token, digest)

    def _check_csrf(self, session, form):
        submitted = form.get("csrf_token", [""])[0]
        expected = session.get("csrf_token", "")
        if not submitted or not hmac.compare_digest(submitted, expected):
            raise ValueError("Invalid CSRF token")

    def parse_form(self, environ):
        if environ.get("CONTENT_TYPE", "").startswith("application/x-www-form-urlencoded"):
            size = to_int(environ.get("CONTENT_LENGTH", "0"), 0)
            payload = environ["wsgi.input"].read(size).decode("utf-8")
            return parse_qs(payload, keep_blank_values=True)
        storage = FieldStorage(fp=environ["wsgi.input"], environ=environ, keep_blank_values=True)
        data = {}
        for key in storage.keys():
            data[key] = [storage.getvalue(key)]
        return data

    def get_cart_items(self, session):
        cart = session["cart"]
        items = []
        for product_id_text, quantity in sorted(cart.items()):
            product = self.db.get_product(int(product_id_text))
            if not product:
                continue
            quantity = max(0, int(quantity))
            if quantity == 0:
                continue
            items.append(
                {
                    "product_id": product["id"],
                    "sku": product["sku"],
                    "name": product["name"],
                    "price_cents": product["price_cents"],
                    "quantity": quantity,
                    "line_total_cents": product["price_cents"] * quantity,
                    "stock_qty": product["stock_qty"],
                }
            )
        return items

    def base_page(self, title, content, session):
        flash = session.pop("flash", "")
        flash_html = '<p class="flash">%s</p>' % escape(flash) if flash else ""
        return """<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>{title}</title>
  <link rel="stylesheet" href="/assets/style.css">
</head>
<body>
  <header class="hero">
    <div>
      <p class="eyebrow">POS System Web</p>
      <h1>{title}</h1>
      <p class="subtle">Counter-ready sales, inventory, and checkout in one local app.</p>
    </div>
    <nav>
      <a href="/">Register</a>
      <a href="/orders">Orders</a>
    </nav>
  </header>
  {flash}
  <main>{content}</main>
</body>
</html>""".format(title=escape(title), content=content, flash=flash_html)

    def index(self, session):
        products = self.db.list_products()
        cart_items = self.get_cart_items(session)
        summary = self.db.daily_summary()
        total_cents = sum(item["line_total_cents"] for item in cart_items)
        product_rows = []
        for product in products:
            low_stock = " low" if product["stock_qty"] <= 5 else ""
            product_rows.append(
                """
                <tr>
                  <td>{sku}</td>
                  <td>{name}</td>
                  <td>{price}</td>
                  <td class="{low_class}">{stock}</td>
                  <td>
                    <form method="post" action="/cart/add" class="inline-form">
                      <input type="hidden" name="csrf_token" value="{csrf}">
                      <input type="hidden" name="product_id" value="{product_id}">
                      <input type="number" name="quantity" min="1" max="{stock}" value="1" required>
                      <button type="submit">Add</button>
                    </form>
                  </td>
                </tr>
                """.format(
                    sku=escape(product["sku"]),
                    name=escape(product["name"]),
                    price=money(product["price_cents"]),
                    stock=product["stock_qty"],
                    low_class=low_stock.strip(),
                    csrf=escape(session["csrf_token"]),
                    product_id=product["id"],
                )
            )

        cart_rows = []
        for item in cart_items:
            cart_rows.append(
                """
                <tr>
                  <td>{name}</td>
                  <td>{unit_price}</td>
                  <td>{stock}</td>
                  <td>
                    <form method="post" action="/cart/update" class="inline-form">
                      <input type="hidden" name="csrf_token" value="{csrf}">
                      <input type="hidden" name="product_id" value="{product_id}">
                      <input type="number" name="quantity" min="0" max="{stock}" value="{quantity}" required>
                      <button type="submit">Update</button>
                    </form>
                  </td>
                  <td>{line_total}</td>
                </tr>
                """.format(
                    name=escape(item["name"]),
                    unit_price=money(item["price_cents"]),
                    stock=item["stock_qty"],
                    csrf=escape(session["csrf_token"]),
                    product_id=item["product_id"],
                    quantity=item["quantity"],
                    line_total=money(item["line_total_cents"]),
                )
            )

        content = """
        <section class="stats-grid">
          <article class="stat-card"><span>Orders Today</span><strong>{order_count}</strong></article>
          <article class="stat-card"><span>Revenue Today</span><strong>{daily_total}</strong></article>
          <article class="stat-card"><span>Cart Total</span><strong>{cart_total}</strong></article>
        </section>

        <section class="layout-grid">
          <article class="panel">
            <h2>Inventory</h2>
            <table>
              <thead><tr><th>SKU</th><th>Name</th><th>Price</th><th>Stock</th><th>Add</th></tr></thead>
              <tbody>{product_rows}</tbody>
            </table>
          </article>

          <article class="panel">
            <h2>Current Cart</h2>
            <table>
              <thead><tr><th>Item</th><th>Unit</th><th>Stock</th><th>Qty</th><th>Total</th></tr></thead>
              <tbody>{cart_rows}</tbody>
            </table>
            <form method="post" action="/checkout" class="checkout-form">
              <input type="hidden" name="csrf_token" value="{csrf}">
              <label>Payment Method
                <select name="payment_method">
                  <option value="cash">Cash</option>
                  <option value="card">Card</option>
                  <option value="mixed">Mixed</option>
                </select>
              </label>
              <label>Note
                <input type="text" name="note" maxlength="120" placeholder="Optional order note">
              </label>
              <button type="submit" {disabled}>Checkout {cart_total}</button>
            </form>
          </article>

          <article class="panel">
            <h2>Add Product</h2>
            <form method="post" action="/products" class="stack-form">
              <input type="hidden" name="csrf_token" value="{csrf}">
              <label>SKU<input type="text" name="sku" maxlength="24" required></label>
              <label>Name<input type="text" name="name" maxlength="80" required></label>
              <label>Price<input type="number" name="price_cents" min="0" step="1" required></label>
              <label>Stock<input type="number" name="stock_qty" min="0" step="1" required></label>
              <button type="submit">Save Product</button>
            </form>
          </article>
        </section>
        """.format(
            order_count=summary["order_count"],
            daily_total=money(summary["total_cents"]),
            cart_total=money(total_cents),
            product_rows="".join(product_rows) or '<tr><td colspan="5">No products yet.</td></tr>',
            cart_rows="".join(cart_rows) or '<tr><td colspan="5">Cart is empty.</td></tr>',
            csrf=escape(session["csrf_token"]),
            disabled='disabled="disabled"' if not cart_items else "",
        )
        return "200 OK", [("Content-Type", "text/html; charset=utf-8")], self.base_page("Register", content, session)

    def orders(self, session):
        orders = self.db.list_orders()
        order_blocks = []
        for order in orders:
            line_items = "".join(
                [
                    "<li>%s x%s <span>%s</span></li>"
                    % (
                        escape(item["product_name"]),
                        item["quantity"],
                        money(item["line_total_cents"]),
                    )
                    for item in order["items"]
                ]
            )
            order_blocks.append(
                """
                <article class="order-card">
                  <div class="order-head">
                    <h2>Order #{order_id}</h2>
                    <strong>{total}</strong>
                  </div>
                  <p>{created_at} via {payment}</p>
                  <p>{note}</p>
                  <ul>{line_items}</ul>
                </article>
                """.format(
                    order_id=order["id"],
                    total=money(order["total_cents"]),
                    created_at=escape(order["created_at"]),
                    payment=escape(order["payment_method"]),
                    note=escape(order["note"] or "No note"),
                    line_items=line_items,
                )
            )
        content = '<section class="orders-grid">%s</section>' % (
            "".join(order_blocks) or "<p>No orders have been created yet.</p>"
        )
        return "200 OK", [("Content-Type", "text/html; charset=utf-8")], self.base_page("Orders", content, session)

    def create_product(self, environ, session):
        form = self.parse_form(environ)
        self._check_csrf(session, form)
        sku = form.get("sku", [""])[0].strip().upper()
        name = form.get("name", [""])[0].strip()
        price_cents = to_int(form.get("price_cents", ["0"])[0], -1)
        stock_qty = to_int(form.get("stock_qty", ["0"])[0], -1)
        if not sku or not name:
            raise ValueError("SKU and name are required")
        if price_cents < 0 or stock_qty < 0:
            raise ValueError("Price and stock must be non-negative")
        self.db.add_product(sku, name, price_cents, stock_qty)
        session["flash"] = "Product added"
        return self.redirect("/")

    def add_to_cart(self, environ, session):
        form = self.parse_form(environ)
        self._check_csrf(session, form)
        product_id = to_int(form.get("product_id", ["0"])[0], 0)
        quantity = to_int(form.get("quantity", ["1"])[0], 0)
        product = self.db.get_product(product_id)
        if not product:
            raise ValueError("Product not found")
        if quantity <= 0:
            raise ValueError("Quantity must be greater than zero")
        existing = session["cart"].get(str(product_id), 0)
        if existing + quantity > product["stock_qty"]:
            raise ValueError("Quantity exceeds stock")
        session["cart"][str(product_id)] = existing + quantity
        session["flash"] = "Added to cart"
        return self.redirect("/")

    def update_cart(self, environ, session):
        form = self.parse_form(environ)
        self._check_csrf(session, form)
        product_id = to_int(form.get("product_id", ["0"])[0], 0)
        quantity = to_int(form.get("quantity", ["0"])[0], -1)
        product = self.db.get_product(product_id)
        if not product:
            raise ValueError("Product not found")
        if quantity < 0:
            raise ValueError("Quantity cannot be negative")
        if quantity > product["stock_qty"]:
            raise ValueError("Quantity exceeds stock")
        if quantity == 0:
            session["cart"].pop(str(product_id), None)
            session["flash"] = "Removed from cart"
        else:
            session["cart"][str(product_id)] = quantity
            session["flash"] = "Cart updated"
        return self.redirect("/")

    def checkout(self, environ, session):
        form = self.parse_form(environ)
        self._check_csrf(session, form)
        payment_method = form.get("payment_method", ["cash"])[0].strip().lower()
        note = form.get("note", [""])[0].strip()[:120]
        if payment_method not in ("cash", "card", "mixed"):
            raise ValueError("Unsupported payment method")
        items = self.get_cart_items(session)
        if not items:
            raise ValueError("Cart is empty")
        self.db.create_order(items, payment_method, note)
        session["cart"] = {}
        session["flash"] = "Checkout complete"
        return self.redirect("/orders")

    def style(self):
        css = """
        :root {
          --bg: #f7f2ea;
          --panel: #fffdf9;
          --ink: #201a17;
          --accent: #b45b2f;
          --accent-dark: #7a3418;
          --line: #e4d7c8;
          --good: #285943;
          --warn: #b03a2e;
        }
        * { box-sizing: border-box; }
        body {
          margin: 0;
          font-family: Georgia, "Times New Roman", serif;
          color: var(--ink);
          background:
            radial-gradient(circle at top right, rgba(180,91,47,0.14), transparent 25%),
            linear-gradient(180deg, #f2e7da 0%, var(--bg) 35%, #efe9df 100%);
        }
        .hero, main { width: min(1180px, calc(100% - 2rem)); margin: 0 auto; }
        .hero {
          padding: 2rem 0 1rem;
          display: flex;
          gap: 1rem;
          justify-content: space-between;
          align-items: end;
        }
        .hero nav a {
          margin-left: 0.75rem;
          color: var(--accent-dark);
          text-decoration: none;
          font-weight: bold;
        }
        .eyebrow { text-transform: uppercase; letter-spacing: 0.2em; font-size: 0.75rem; color: var(--accent-dark); }
        h1, h2 { margin: 0 0 0.5rem; }
        .subtle { margin: 0; color: #5f5148; }
        .flash {
          width: min(1180px, calc(100% - 2rem));
          margin: 0 auto 1rem;
          padding: 0.85rem 1rem;
          background: #fff4d7;
          border-left: 4px solid var(--accent);
        }
        .stats-grid, .layout-grid, .orders-grid {
          display: grid;
          gap: 1rem;
        }
        .stats-grid {
          grid-template-columns: repeat(auto-fit, minmax(180px, 1fr));
          margin-bottom: 1rem;
        }
        .layout-grid {
          grid-template-columns: 2fr 1.4fr 1fr;
          align-items: start;
          padding-bottom: 2rem;
        }
        .orders-grid {
          grid-template-columns: repeat(auto-fit, minmax(280px, 1fr));
          padding-bottom: 2rem;
        }
        .stat-card, .panel, .order-card {
          background: rgba(255,253,249,0.92);
          border: 1px solid var(--line);
          border-radius: 18px;
          padding: 1rem;
          box-shadow: 0 18px 50px rgba(60, 34, 13, 0.08);
        }
        .stat-card span { display: block; color: #6e5e53; font-size: 0.85rem; }
        .stat-card strong { font-size: 1.7rem; }
        table { width: 100%; border-collapse: collapse; font-size: 0.95rem; }
        th, td { padding: 0.65rem 0.35rem; border-bottom: 1px solid var(--line); text-align: left; vertical-align: top; }
        .inline-form, .checkout-form, .stack-form { display: grid; gap: 0.75rem; }
        .inline-form { grid-template-columns: 1fr auto; align-items: center; }
        input, select, button {
          width: 100%;
          border-radius: 10px;
          border: 1px solid #ccb8a5;
          padding: 0.7rem 0.8rem;
          font: inherit;
          background: #fffdfb;
        }
        button {
          background: linear-gradient(135deg, var(--accent), var(--accent-dark));
          color: #fff;
          border: 0;
          cursor: pointer;
          font-weight: bold;
        }
        button[disabled] { opacity: 0.45; cursor: not-allowed; }
        .low { color: var(--warn); font-weight: bold; }
        .order-head {
          display: flex;
          justify-content: space-between;
          gap: 1rem;
          align-items: baseline;
        }
        ul { padding-left: 1.2rem; margin-bottom: 0; }
        li span { float: right; }
        @media (max-width: 960px) {
          .layout-grid { grid-template-columns: 1fr; }
          .hero { align-items: start; flex-direction: column; }
        }
        """
        return "200 OK", [("Content-Type", "text/css; charset=utf-8")], css

    def redirect(self, location):
        return "303 See Other", [("Location", location)], ""

    def not_found(self):
        return "404 Not Found", [("Content-Type", "text/plain; charset=utf-8")], "Not found"


def create_app(db_path=None, secret_key=None):
    db_path = db_path or os.path.join(os.path.dirname(os.path.dirname(__file__)), "data", "pos.db")
    return POSApplication(db_path, secret_key=secret_key)


def main():
    app = create_app()
    port = int(os.environ.get("PORT", "8080"))
    with make_server("127.0.0.1", port, app) as server:
        print("POS server running on http://127.0.0.1:%s" % port)
        server.serve_forever()
