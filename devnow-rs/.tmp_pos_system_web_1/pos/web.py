import io
import os
import secrets
from html import escape
from http import cookies
from urllib.parse import parse_qs
from wsgiref.simple_server import make_server

from .database import Database
from .security import sign_data, unsign_data, verify_password


def money(cents):
    return "${:,.2f}".format(float(cents) / 100.0)


def html_escape(value):
    return escape("" if value is None else str(value), quote=True)


def read_post(environ):
    if "pos.form_data" in environ:
        return environ["pos.form_data"]
    try:
        size = int(environ.get("CONTENT_LENGTH", "0") or "0")
    except ValueError:
        size = 0
    body = environ["wsgi.input"].read(size) if size > 0 else b""
    data = parse_qs(body.decode("utf-8"), keep_blank_values=True)
    environ["pos.form_data"] = data
    return data


class Response(object):
    def __init__(self, body="", status="200 OK", headers=None):
        if isinstance(body, str):
            body = body.encode("utf-8")
        self.body = body
        self.status = status
        self.headers = headers or [("Content-Type", "text/html; charset=utf-8")]


class POSApplication(object):
    def __init__(self, db_path, secret_key):
        self.db = Database(db_path)
        self.db.initialize()
        self.secret_key = secret_key

    def __call__(self, environ, start_response):
        response = self.dispatch(environ)
        headers = list(response.headers)
        headers.append(("Content-Length", str(len(response.body))))
        start_response(response.status, headers)
        return [response.body]

    def dispatch(self, environ):
        path = environ.get("PATH_INFO", "/")
        method = environ.get("REQUEST_METHOD", "GET").upper()
        session = self.load_session(environ)

        if path == "/":
            if session.get("username"):
                return self.redirect("/dashboard", session=session)
            return Response(self.render_page("Login", self.login_form()))

        if path == "/login" and method == "POST":
            data = read_post(environ)
            username = first_value(data, "username")
            password = first_value(data, "password")
            user = self.db.get_user(username)
            if not user or not verify_password(password, user["password_hash"]):
                return Response(self.render_page("Login", self.login_form("Invalid credentials.")), status="401 Unauthorized")
            session = {
                "username": user["username"],
                "role": user["role"],
                "csrf": secrets.token_hex(16),
                "cart": [],
            }
            return self.redirect("/dashboard", session=session)

        if path == "/logout":
            return self.redirect("/", session={})

        user = session.get("username")
        if not user:
            return self.redirect("/")

        if path == "/dashboard":
            return Response(self.render_page("Dashboard", self.dashboard_view(session)))

        if path == "/products":
            return Response(self.render_page("Products", self.products_view(session, "")))

        if path == "/products/add" and method == "POST":
            csrf_error = self.require_csrf(environ, session)
            if csrf_error:
                return csrf_error
            if session.get("role") != "admin":
                return Response(self.render_page("Forbidden", self.message_box("Admin role required.")), status="403 Forbidden")
            data = read_post(environ)
            try:
                self.db.add_product(
                    first_value(data, "name").strip(),
                    first_value(data, "sku").strip().upper(),
                    parse_price(first_value(data, "price")),
                    int(first_value(data, "stock")),
                )
                return self.redirect("/products", session=session)
            except Exception as exc:
                return Response(self.render_page("Products", self.products_view(session, "Could not add product: %s" % exc)), status="400 Bad Request")

        if path == "/products/stock" and method == "POST":
            csrf_error = self.require_csrf(environ, session)
            if csrf_error:
                return csrf_error
            if session.get("role") != "admin":
                return Response(self.render_page("Forbidden", self.message_box("Admin role required.")), status="403 Forbidden")
            data = read_post(environ)
            try:
                self.db.adjust_stock(
                    int(first_value(data, "product_id")),
                    int(first_value(data, "change_amount")),
                    "manual adjustment",
                )
                return self.redirect("/products", session=session)
            except Exception as exc:
                return Response(self.render_page("Products", self.products_view(session, "Stock update failed: %s" % exc)), status="400 Bad Request")

        if path == "/cart":
            return Response(self.render_page("Cart", self.cart_view(session, "")))

        if path == "/cart/add" and method == "POST":
            csrf_error = self.require_csrf(environ, session)
            if csrf_error:
                return csrf_error
            data = read_post(environ)
            cart = list(session.get("cart", []))
            product_id = int(first_value(data, "product_id"))
            quantity = int(first_value(data, "quantity"))
            merge_cart_item(cart, product_id, quantity)
            session["cart"] = normalize_cart(cart)
            return self.redirect("/cart", session=session)

        if path == "/cart/remove" and method == "POST":
            csrf_error = self.require_csrf(environ, session)
            if csrf_error:
                return csrf_error
            data = read_post(environ)
            product_id = int(first_value(data, "product_id"))
            session["cart"] = [item for item in session.get("cart", []) if item["product_id"] != product_id]
            return self.redirect("/cart", session=session)

        if path == "/checkout" and method == "POST":
            csrf_error = self.require_csrf(environ, session)
            if csrf_error:
                return csrf_error
            data = read_post(environ)
            payment_method = first_value(data, "payment_method") or "cash"
            try:
                rows = self.resolve_cart(session.get("cart", []))
                if not rows:
                    raise ValueError("Cart is empty.")
                sale_id = self.db.create_sale(session["username"], payment_method, rows)
                session["cart"] = []
                return self.redirect("/receipt?id=%s" % sale_id, session=session)
            except Exception as exc:
                return Response(self.render_page("Cart", self.cart_view(session, "Checkout failed: %s" % exc)), status="400 Bad Request")

        if path == "/sales":
            return Response(self.render_page("Sales", self.sales_view()))

        if path == "/receipt":
            query = parse_qs(environ.get("QUERY_STRING", ""))
            try:
                sale_id = int(first_value(query, "id") or "0")
            except ValueError:
                return Response(self.render_page("Receipt", self.message_box("Receipt not found.")), status="404 Not Found")
            return Response(self.render_page("Receipt", self.receipt_view(sale_id)))

        return Response(self.render_page("Not Found", self.message_box("Page not found.")), status="404 Not Found")

    def load_session(self, environ):
        raw_cookie = environ.get("HTTP_COOKIE", "")
        jar = cookies.SimpleCookie()
        if raw_cookie:
            jar.load(raw_cookie)
        if "pos_session" not in jar:
            return {}
        value = jar["pos_session"].value
        data = unsign_data(value, self.secret_key)
        if not isinstance(data, dict):
            return {}
        data.setdefault("cart", [])
        return data

    def session_cookie(self, session):
        jar = cookies.SimpleCookie()
        jar["pos_session"] = sign_data(session, self.secret_key) if session else ""
        jar["pos_session"]["path"] = "/"
        jar["pos_session"]["httponly"] = True
        if not session:
            jar["pos_session"]["expires"] = "Thu, 01 Jan 1970 00:00:00 GMT"
        return ("Set-Cookie", jar.output(header="").strip())

    def redirect(self, location, session):
        headers = [("Location", location), self.session_cookie(session)]
        return Response("", status="302 Found", headers=headers)

    def require_csrf(self, environ, session):
        data = read_post(environ)
        token = first_value(data, "csrf_token")
        if not token or token != session.get("csrf"):
            return Response(self.render_page("Forbidden", self.message_box("Invalid CSRF token.")), status="403 Forbidden")
        return None

    def dashboard_view(self, session):
        products = self.db.list_products()
        sales = self.db.list_sales()[:5]
        low_stock = [row for row in products if row["stock_quantity"] <= 5]
        cart_count = sum(item["quantity"] for item in session.get("cart", []))
        return """
        <div class="grid">
          <section class="card">
            <h2>Shift Summary</h2>
            <p>Signed in as <strong>{user}</strong> ({role})</p>
            <p>Products in catalog: <strong>{product_total}</strong></p>
            <p>Items in active cart: <strong>{cart_count}</strong></p>
          </section>
          <section class="card">
            <h2>Recent Sales</h2>
            {sales}
          </section>
          <section class="card">
            <h2>Low Stock</h2>
            {low_stock}
          </section>
        </div>
        """.format(
            user=html_escape(session.get("username")),
            role=html_escape(session.get("role")),
            product_total=len(products),
            cart_count=cart_count,
            sales=self.sales_table(sales, compact=True),
            low_stock=self.low_stock_list(low_stock),
        )

    def products_view(self, session, message):
        products = self.db.list_products()
        product_rows = []
        for row in products:
            product_rows.append(
                """
                <tr>
                  <td>{name}</td>
                  <td>{sku}</td>
                  <td>{price}</td>
                  <td>{stock}</td>
                  <td>
                    <form method="post" action="/cart/add" class="inline-form">
                      <input type="hidden" name="csrf_token" value="{csrf}">
                      <input type="hidden" name="product_id" value="{product_id}">
                      <input type="number" min="1" name="quantity" value="1">
                      <button type="submit">Add to Cart</button>
                    </form>
                  </td>
                </tr>
                """.format(
                    name=html_escape(row["name"]),
                    sku=html_escape(row["sku"]),
                    price=money(row["price_cents"]),
                    stock=row["stock_quantity"],
                    csrf=html_escape(session["csrf"]),
                    product_id=row["id"],
                )
            )
        admin_tools = ""
        if session.get("role") == "admin":
            admin_tools = """
            <div class="grid two">
              <section class="card">
                <h2>Add Product</h2>
                <form method="post" action="/products/add">
                  <input type="hidden" name="csrf_token" value="{csrf}">
                  <label>Name <input name="name" required></label>
                  <label>SKU <input name="sku" required></label>
                  <label>Price <input name="price" required placeholder="12.99"></label>
                  <label>Opening Stock <input type="number" name="stock" min="0" value="0" required></label>
                  <button type="submit">Create Product</button>
                </form>
              </section>
              <section class="card">
                <h2>Adjust Stock</h2>
                <form method="post" action="/products/stock">
                  <input type="hidden" name="csrf_token" value="{csrf}">
                  <label>Product
                    <select name="product_id">{options}</select>
                  </label>
                  <label>Change Amount <input type="number" name="change_amount" value="1"></label>
                  <button type="submit">Apply Adjustment</button>
                </form>
              </section>
            </div>
            """.format(
                csrf=html_escape(session["csrf"]),
                options="".join(
                    [
                        '<option value="{0}">{1} ({2})</option>'.format(
                            row["id"], html_escape(row["name"]), html_escape(row["sku"])
                        )
                        for row in products
                    ]
                ),
            )
        notice = '<p class="notice">{0}</p>'.format(html_escape(message)) if message else ""
        return """
        {notice}
        <section class="card">
          <h2>Catalog</h2>
          <table>
            <thead><tr><th>Name</th><th>SKU</th><th>Price</th><th>Stock</th><th>Sale</th></tr></thead>
            <tbody>{rows}</tbody>
          </table>
        </section>
        {admin_tools}
        """.format(notice=notice, rows="".join(product_rows), admin_tools=admin_tools)

    def resolve_cart(self, cart):
        resolved = []
        for item in normalize_cart(cart):
            product = self.db.get_product(item["product_id"])
            if not product:
                continue
            resolved.append(
                {
                    "product_id": product["id"],
                    "name": product["name"],
                    "sku": product["sku"],
                    "price_cents": product["price_cents"],
                    "quantity": item["quantity"],
                    "stock_quantity": product["stock_quantity"],
                }
            )
        return resolved

    def cart_view(self, session, message):
        rows = self.resolve_cart(session.get("cart", []))
        if rows:
            table_rows = []
            total = 0
            for row in rows:
                line_total = row["price_cents"] * row["quantity"]
                total += line_total
                table_rows.append(
                    """
                    <tr>
                      <td>{name}</td>
                      <td>{sku}</td>
                      <td>{qty}</td>
                      <td>{price}</td>
                      <td>{line_total}</td>
                      <td>
                        <form method="post" action="/cart/remove" class="inline-form">
                          <input type="hidden" name="csrf_token" value="{csrf}">
                          <input type="hidden" name="product_id" value="{product_id}">
                          <button type="submit">Remove</button>
                        </form>
                      </td>
                    </tr>
                    """.format(
                        name=html_escape(row["name"]),
                        sku=html_escape(row["sku"]),
                        qty=row["quantity"],
                        price=money(row["price_cents"]),
                        line_total=money(line_total),
                        csrf=html_escape(session["csrf"]),
                        product_id=row["product_id"],
                    )
                )
            body = """
            <section class="card">
              <h2>Active Sale</h2>
              <table>
                <thead><tr><th>Item</th><th>SKU</th><th>Qty</th><th>Unit</th><th>Total</th><th></th></tr></thead>
                <tbody>{rows}</tbody>
              </table>
              <p class="sale-total">Total: <strong>{total}</strong></p>
              <form method="post" action="/checkout">
                <input type="hidden" name="csrf_token" value="{csrf}">
                <label>Payment Method
                  <select name="payment_method">
                    <option value="cash">Cash</option>
                    <option value="card">Card</option>
                    <option value="mobile">Mobile</option>
                  </select>
                </label>
                <button type="submit">Complete Sale</button>
              </form>
            </section>
            """.format(rows="".join(table_rows), total=money(total), csrf=html_escape(session["csrf"]))
        else:
            body = '<section class="card"><h2>Active Sale</h2><p>No items in the cart yet.</p></section>'
        notice = '<p class="notice">{0}</p>'.format(html_escape(message)) if message else ""
        return notice + body

    def sales_view(self):
        rows = self.db.list_sales()
        return '<section class="card"><h2>Sales History</h2>{0}</section>'.format(self.sales_table(rows, compact=False))

    def receipt_view(self, sale_id):
        sale, items = self.db.get_sale(sale_id)
        if not sale:
            return self.message_box("Receipt not found.")
        item_rows = []
        for row in items:
            item_rows.append(
                "<tr><td>{name}</td><td>{sku}</td><td>{qty}</td><td>{price}</td><td>{total}</td></tr>".format(
                    name=html_escape(row["product_name"]),
                    sku=html_escape(row["sku"]),
                    qty=row["quantity"],
                    price=money(row["price_cents"]),
                    total=money(row["line_total_cents"]),
                )
            )
        return """
        <section class="card receipt">
          <h2>Receipt #{sale_id}</h2>
          <p>Cashier: <strong>{cashier}</strong></p>
          <p>Payment: <strong>{payment}</strong></p>
          <p>Time: <strong>{created_at}</strong></p>
          <table>
            <thead><tr><th>Item</th><th>SKU</th><th>Qty</th><th>Unit</th><th>Total</th></tr></thead>
            <tbody>{items}</tbody>
          </table>
          <p class="sale-total">Grand Total: <strong>{total}</strong></p>
        </section>
        """.format(
            sale_id=sale["id"],
            cashier=html_escape(sale["cashier"]),
            payment=html_escape(sale["payment_method"]),
            created_at=html_escape(sale["created_at"]),
            items="".join(item_rows),
            total=money(sale["total_cents"]),
        )

    def sales_table(self, rows, compact):
        if not rows:
            return "<p>No sales recorded yet.</p>"
        table_rows = []
        for row in rows:
            action = '<a href="/receipt?id={0}">Receipt</a>'.format(row["id"])
            table_rows.append(
                "<tr><td>{id}</td><td>{cashier}</td><td>{payment}</td><td>{total}</td><td>{created}</td><td>{action}</td></tr>".format(
                    id=row["id"],
                    cashier=html_escape(row["cashier"]),
                    payment=html_escape(row["payment_method"]),
                    total=money(row["total_cents"]),
                    created=html_escape(row["created_at"]),
                    action=action,
                )
            )
        extra_class = " compact" if compact else ""
        return '<table class="{0}"><thead><tr><th>ID</th><th>Cashier</th><th>Payment</th><th>Total</th><th>Created</th><th></th></tr></thead><tbody>{1}</tbody></table>'.format(extra_class, "".join(table_rows))

    def low_stock_list(self, rows):
        if not rows:
            return "<p>All stocked above alert level.</p>"
        parts = ["<ul>"]
        for row in rows:
            parts.append(
                "<li>{0} ({1}) has {2} units left.</li>".format(
                    html_escape(row["name"]), html_escape(row["sku"]), row["stock_quantity"]
                )
            )
        parts.append("</ul>")
        return "".join(parts)

    def message_box(self, message):
        return '<section class="card"><p>{0}</p></section>'.format(html_escape(message))

    def login_form(self, message=""):
        notice = '<p class="notice">{0}</p>'.format(html_escape(message)) if message else ""
        return """
        <section class="card login-card">
          <h2>POS Sign In</h2>
          {notice}
          <form method="post" action="/login">
            <label>Username <input name="username" required></label>
            <label>Password <input type="password" name="password" required></label>
            <button type="submit">Sign In</button>
          </form>
          <p class="hint">Seeded users: admin / admin123, cashier / cashier123</p>
        </section>
        """.format(notice=notice)

    def render_page(self, title, body):
        nav = """
        <nav>
          <a href="/dashboard">Dashboard</a>
          <a href="/products">Products</a>
          <a href="/cart">Cart</a>
          <a href="/sales">Sales</a>
          <a href="/logout">Logout</a>
        </nav>
        """
        if title == "Login":
            nav = ""
        return """
        <!DOCTYPE html>
        <html>
        <head>
          <meta charset="utf-8">
          <meta name="viewport" content="width=device-width, initial-scale=1">
          <title>{title}</title>
          <style>
            body {{ font-family: "Trebuchet MS", sans-serif; margin: 0; background: linear-gradient(135deg, #f3efe6, #d8e6eb); color: #1c2a2f; }}
            header {{ background: #103b44; color: #fff8ea; padding: 1.25rem 1.5rem; }}
            nav a {{ color: #fff8ea; margin-right: 1rem; text-decoration: none; font-weight: bold; }}
            main {{ padding: 1.5rem; max-width: 1100px; margin: 0 auto; }}
            .card {{ background: rgba(255,255,255,0.94); border-radius: 14px; padding: 1rem 1.2rem; margin-bottom: 1rem; box-shadow: 0 10px 30px rgba(16, 59, 68, 0.12); }}
            .grid {{ display: grid; gap: 1rem; grid-template-columns: repeat(auto-fit, minmax(260px, 1fr)); }}
            .grid.two {{ grid-template-columns: repeat(auto-fit, minmax(320px, 1fr)); }}
            label {{ display: block; font-weight: bold; margin-bottom: 0.7rem; }}
            input, select, button {{ font: inherit; padding: 0.55rem; margin-top: 0.25rem; border-radius: 8px; border: 1px solid #95abb1; }}
            button {{ background: #ce5b31; color: white; border: none; cursor: pointer; }}
            button:hover {{ background: #a74825; }}
            table {{ width: 100%; border-collapse: collapse; }}
            th, td {{ text-align: left; padding: 0.65rem; border-bottom: 1px solid #dce6e8; }}
            .inline-form {{ display: flex; gap: 0.5rem; align-items: center; }}
            .notice {{ background: #fff0c6; border-left: 5px solid #ce5b31; padding: 0.8rem; border-radius: 8px; }}
            .sale-total {{ font-size: 1.15rem; }}
            .login-card {{ max-width: 420px; margin: 4rem auto; }}
            .hint {{ color: #566b71; font-size: 0.95rem; }}
            .receipt table {{ margin-top: 1rem; }}
            @media (max-width: 720px) {{ .inline-form {{ flex-direction: column; align-items: stretch; }} }}
          </style>
        </head>
        <body>
          <header>
            <h1>Northwind Counter POS</h1>
            {nav}
          </header>
          <main>{body}</main>
        </body>
        </html>
        """.format(title=html_escape(title), nav=nav, body=body)


def first_value(mapping, key):
    value = mapping.get(key, [""])
    if isinstance(value, list):
        return value[0]
    return value


def parse_price(value):
    cleaned = value.strip().replace("$", "")
    return int(round(float(cleaned) * 100))


def merge_cart_item(cart, product_id, quantity):
    if quantity <= 0:
        raise ValueError("Quantity must be positive.")
    for item in cart:
        if item["product_id"] == product_id:
            item["quantity"] += quantity
            return
    cart.append({"product_id": product_id, "quantity": quantity})


def normalize_cart(cart):
    normalized = []
    for item in cart:
        quantity = int(item.get("quantity", 0))
        product_id = int(item.get("product_id"))
        if quantity > 0:
            normalized.append({"product_id": product_id, "quantity": quantity})
    return normalized


def create_app(db_path=None, secret_key=None):
    if db_path is None:
        db_path = os.environ.get("POS_DB_PATH") or os.path.join(os.path.dirname(os.path.dirname(__file__)), "data", "pos_data.json")
    if secret_key is None:
        secret_key = os.environ.get("POS_SECRET_KEY") or "development-pos-secret-change-me"
    return POSApplication(db_path, secret_key)


def serve(host="127.0.0.1", port=8080):
    app = create_app()
    server = make_server(host, port, app)
    print("Serving POS system on http://{0}:{1}".format(host, port))
    server.serve_forever()
