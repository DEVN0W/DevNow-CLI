import io
import os
import secrets
from decimal import Decimal, InvalidOperation, ROUND_HALF_UP
from http import cookies
from urllib.parse import parse_qs
from wsgiref.simple_server import make_server

from .db import Database
from .views import dashboard, inventory_screen, page, parse_form, redirect, reports_screen, sale_screen


DEFAULT_DB = os.path.join(os.path.dirname(os.path.dirname(__file__)), "data", "pos.db")
DEFAULT_TAX_RATE = 0.07


def to_cents(amount_text):
    try:
        value = Decimal(amount_text)
    except InvalidOperation:
        raise ValueError("Invalid monetary value.")
    return int((value * 100).quantize(Decimal("1"), rounding=ROUND_HALF_UP))


def html_response(start_response, body, status="200 OK", headers=None):
    body_bytes = body.encode("utf-8")
    response_headers = [("Content-Type", "text/html; charset=utf-8"), ("Content-Length", str(len(body_bytes)))]
    if headers:
        response_headers.extend(headers)
    start_response(status, response_headers)
    return [body_bytes]


def text_response(start_response, body, status="200 OK", headers=None, content_type="text/plain; charset=utf-8"):
    body_bytes = body.encode("utf-8")
    response_headers = [("Content-Type", content_type), ("Content-Length", str(len(body_bytes)))]
    if headers:
        response_headers.extend(headers)
    start_response(status, response_headers)
    return [body_bytes]


class POSApplication(object):
    def __init__(self, db_path=None, tax_rate=DEFAULT_TAX_RATE):
        self.db = Database(db_path or DEFAULT_DB)
        self.db.seed_defaults()
        self.tax_rate = tax_rate
        self.sessions = {}

    def __call__(self, environ, start_response):
        try:
            return self.dispatch(environ, start_response)
        except ValueError as error:
            return self.render_notice(start_response, str(error), status="400 Bad Request")
        except Exception as error:
            return self.render_notice(start_response, "Unexpected error: %s" % error, status="500 Internal Server Error")

    def dispatch(self, environ, start_response):
        session_id, headers = self.get_or_create_session(environ)
        session = self.sessions[session_id]
        method = environ["REQUEST_METHOD"].upper()
        path = environ.get("PATH_INFO", "/")
        notice = self.request_notice(environ)

        if path == "/static/style.css":
            return self.static_css(start_response, headers)

        if method == "GET" and path == "/":
            body = dashboard(self.db.daily_summary(), self.db.list_products(), notice=notice)
            return html_response(start_response, body, headers=headers)

        if method == "GET" and path == "/sale":
            body = sale_screen(
                self.db.list_products(),
                session["cart"],
                self.cart_totals(session["cart"]),
                session["csrf_token"],
                notice=notice,
            )
            return html_response(start_response, body, headers=headers)

        if method == "GET" and path == "/inventory":
            body = inventory_screen(self.db.list_products(), session["csrf_token"], notice=notice)
            return html_response(start_response, body, headers=headers)

        if method == "GET" and path == "/reports":
            body = reports_screen(self.db.sales_history(), notice=notice)
            return html_response(start_response, body, headers=headers)

        if method == "POST" and path == "/inventory/create":
            form = parse_form(environ)
            self.require_csrf(session, form)
            self.db.create_product(
                name=form["name"].strip(),
                sku=form["sku"].strip().upper(),
                price_cents=to_cents(form["price"]),
                stock_qty=int(form["stock_qty"]),
            )
            _, status, redirect_headers = redirect("/inventory?notice=Product+created")
            return html_response(start_response, "", status=status, headers=headers + redirect_headers)

        if method == "POST" and path == "/inventory/adjust":
            form = parse_form(environ)
            self.require_csrf(session, form)
            self.db.update_stock(int(form["product_id"]), int(form["quantity_delta"]))
            _, status, redirect_headers = redirect("/inventory?notice=Inventory+updated")
            return html_response(start_response, "", status=status, headers=headers + redirect_headers)

        if method == "POST" and path == "/cart/add":
            form = parse_form(environ)
            self.require_csrf(session, form)
            self.add_to_cart(session, int(form["product_id"]), int(form["quantity"]))
            _, status, redirect_headers = redirect("/sale?notice=Item+added")
            return html_response(start_response, "", status=status, headers=headers + redirect_headers)

        if method == "POST" and path == "/cart/clear":
            form = parse_form(environ)
            self.require_csrf(session, form)
            session["cart"] = []
            _, status, redirect_headers = redirect("/sale?notice=Cart+cleared")
            return html_response(start_response, "", status=status, headers=headers + redirect_headers)

        if method == "POST" and path == "/checkout":
            form = parse_form(environ)
            self.require_csrf(session, form)
            sale = self.db.record_sale(session["cart"], to_cents(form["cash_received"]), self.tax_rate)
            session["cart"] = []
            location = "/reports?notice=Sale+%23{sale_id}+completed".format(sale_id=sale["sale_id"])
            _, status, redirect_headers = redirect(location)
            return html_response(start_response, "", status=status, headers=headers + redirect_headers)

        return self.render_notice(start_response, "Page not found.", status="404 Not Found", headers=headers)

    def render_notice(self, start_response, notice, status="200 OK", headers=None):
        body = page("Notice", "<section class='panel'><p>%s</p></section>" % notice, notice=notice)
        return html_response(start_response, body, status=status, headers=headers or [])

    def request_notice(self, environ):
        values = parse_qs(environ.get("QUERY_STRING", ""))
        return values.get("notice", [""])[0]

    def get_or_create_session(self, environ):
        cookie_header = environ.get("HTTP_COOKIE", "")
        jar = cookies.SimpleCookie()
        jar.load(cookie_header)
        session_cookie = jar.get("pos_session")
        session_id = session_cookie.value if session_cookie else None
        headers = []
        if not session_id or session_id not in self.sessions:
            session_id = secrets.token_hex(16)
            self.sessions[session_id] = {"cart": [], "csrf_token": secrets.token_hex(16)}
            headers.append(("Set-Cookie", "pos_session=%s; HttpOnly; Path=/; SameSite=Lax" % session_id))
        return session_id, headers

    def require_csrf(self, session, form):
        if form.get("csrf_token") != session["csrf_token"]:
            raise ValueError("Invalid CSRF token.")

    def add_to_cart(self, session, product_id, quantity):
        if quantity <= 0:
            raise ValueError("Quantity must be positive.")
        product = self.db.get_product(product_id)
        if not product:
            raise ValueError("Product not found.")
        if quantity > product["stock_qty"]:
            raise ValueError("Quantity exceeds current stock.")
        for item in session["cart"]:
            if item["product_id"] == product_id:
                new_quantity = item["quantity"] + quantity
                if new_quantity > product["stock_qty"]:
                    raise ValueError("Quantity exceeds current stock.")
                item["quantity"] = new_quantity
                item["line_total_cents"] = new_quantity * item["unit_price_cents"]
                return
        session["cart"].append(
            {
                "product_id": product["id"],
                "product_name": product["name"],
                "sku": product["sku"],
                "quantity": quantity,
                "unit_price_cents": product["price_cents"],
                "line_total_cents": product["price_cents"] * quantity,
            }
        )

    def cart_totals(self, cart):
        subtotal_cents = sum(item["line_total_cents"] for item in cart)
        tax_cents = int(round(subtotal_cents * self.tax_rate))
        return {
            "subtotal_cents": subtotal_cents,
            "tax_cents": tax_cents,
            "total_cents": subtotal_cents + tax_cents,
        }

    def static_css(self, start_response, headers):
        css_path = os.path.join(os.path.dirname(os.path.dirname(__file__)), "static", "style.css")
        with open(css_path, "r") as handle:
            content = handle.read()
        return text_response(start_response, content, headers=headers, content_type="text/css; charset=utf-8")


def create_app(db_path=None, tax_rate=DEFAULT_TAX_RATE):
    return POSApplication(db_path=db_path, tax_rate=tax_rate)


def run_dev_server(app, host="127.0.0.1", port=8000):
    server = make_server(host, port, app)
    print("Serving POS system on http://%s:%s" % (host, port))
    server.serve_forever()
