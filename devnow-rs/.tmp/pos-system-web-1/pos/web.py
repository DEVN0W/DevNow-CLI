import secrets
from http import cookies
from io import BytesIO
from urllib.parse import parse_qs, urlencode
from wsgiref.simple_server import make_server

from pos.config import DATABASE_PATH, HOST, PORT
from pos.db import initialize_database
from pos.services import POSService, parse_non_negative_int
from pos.templates import cart_view, dashboard_view, products_view, render_page, sales_view


class POSApplication:
    def __init__(self, database_path=DATABASE_PATH):
        self.database_path = database_path
        initialize_database(self.database_path)
        self.service = POSService(self.database_path)
        self.sessions = {}

    def __call__(self, environ, start_response):
        method = environ.get("REQUEST_METHOD", "GET").upper()
        path = environ.get("PATH_INFO", "/")

        response_headers = [("Content-Type", "text/html; charset=utf-8")]
        session = self._get_session(environ, response_headers)

        try:
            if method == "GET" and path == "/":
                body = self._render_dashboard(session)
                return self._respond(start_response, "200 OK", response_headers, body)
            if method == "GET" and path == "/products":
                body = self._render_products(session)
                return self._respond(start_response, "200 OK", response_headers, body)
            if method == "GET" and path == "/cart":
                body = self._render_cart(session)
                return self._respond(start_response, "200 OK", response_headers, body)
            if method == "GET" and path == "/sales":
                body = self._render_sales(session)
                return self._respond(start_response, "200 OK", response_headers, body)
            if method == "POST" and path == "/products":
                form = self._parse_form(environ)
                self._require_csrf(session, form)
                self.service.create_product(
                    form.get("sku", ""),
                    form.get("name", ""),
                    form.get("price", ""),
                    form.get("stock", ""),
                )
                self._set_flash(session, "Product created.")
                return self._redirect(start_response, response_headers, "/products")
            if method == "POST" and path == "/cart/add":
                form = self._parse_form(environ)
                self._require_csrf(session, form)
                product_id = str(parse_non_negative_int(form.get("product_id", "0"), "Product"))
                quantity = parse_non_negative_int(form.get("quantity", "1"), "Quantity")
                if quantity == 0:
                    raise ValueError("Quantity must be greater than zero.")
                if not self.service.get_product(product_id):
                    raise ValueError("Product was not found.")
                self._cart(session)[product_id] = self._cart(session).get(product_id, 0) + quantity
                self._set_flash(session, "Item added to cart.")
                return self._redirect(start_response, response_headers, "/cart")
            if method == "POST" and path == "/cart/update":
                form = self._parse_form(environ)
                self._require_csrf(session, form)
                product_id = str(parse_non_negative_int(form.get("product_id", "0"), "Product"))
                quantity = parse_non_negative_int(form.get("quantity", "0"), "Quantity")
                if quantity == 0:
                    self._cart(session).pop(product_id, None)
                    self._set_flash(session, "Item removed from cart.")
                else:
                    self._cart(session)[product_id] = quantity
                    self._set_flash(session, "Cart updated.")
                return self._redirect(start_response, response_headers, "/cart")
            if method == "POST" and path == "/checkout":
                form = self._parse_form(environ)
                self._require_csrf(session, form)
                sale_id = self.service.create_sale(self._cart(session))
                session["cart"] = {}
                self._set_flash(session, "Sale #{0} completed.".format(sale_id))
                return self._redirect(start_response, response_headers, "/sales")
        except ValueError as exc:
            self._set_flash(session, str(exc))
            redirect_target = "/products" if path == "/products" else "/cart"
            return self._redirect(start_response, response_headers, redirect_target)
        except PermissionError:
            return self._respond(
                start_response,
                "403 Forbidden",
                response_headers,
                render_page("Forbidden", "<section class='card'><h2>Forbidden</h2><p>Invalid CSRF token.</p></section>"),
            )

        return self._respond(
            start_response,
            "404 Not Found",
            response_headers,
            render_page("Not Found", "<section class='card'><h2>Not Found</h2></section>"),
        )

    def _render_dashboard(self, session):
        summary = self.service.dashboard_summary()
        cart_snapshot = self.service.get_cart_items(self._cart(session))
        return render_page("Dashboard", dashboard_view(summary, cart_snapshot), self._pop_flash(session))

    def _render_products(self, session):
        products = self.service.list_products()
        return render_page(
            "Products",
            products_view(products, session["csrf_token"]),
            self._pop_flash(session),
        )

    def _render_cart(self, session):
        cart_snapshot = self.service.get_cart_items(self._cart(session))
        return render_page(
            "Cart",
            cart_view(cart_snapshot, session["csrf_token"]),
            self._pop_flash(session),
        )

    def _render_sales(self, session):
        return render_page("Sales", sales_view(self.service.list_sales()), self._pop_flash(session))

    def _get_session(self, environ, response_headers):
        raw_cookie = environ.get("HTTP_COOKIE", "")
        jar = cookies.SimpleCookie()
        jar.load(raw_cookie)
        session_id = jar["sid"].value if "sid" in jar else None
        if not session_id or session_id not in self.sessions:
            session_id = secrets.token_urlsafe(24)
            self.sessions[session_id] = {"cart": {}, "flash": None, "csrf_token": secrets.token_urlsafe(24)}
            cookie = cookies.SimpleCookie()
            cookie["sid"] = session_id
            cookie["sid"]["path"] = "/"
            cookie["sid"]["httponly"] = True
            cookie["sid"]["samesite"] = "Lax"
            response_headers.append(("Set-Cookie", cookie.output(header="").strip()))
        return self.sessions[session_id]

    def _parse_form(self, environ):
        body_size = int(environ.get("CONTENT_LENGTH") or 0)
        raw_body = environ["wsgi.input"].read(body_size).decode("utf-8")
        parsed = parse_qs(raw_body, keep_blank_values=True)
        return {key: values[0] for key, values in parsed.items()}

    def _require_csrf(self, session, form):
        if form.get("csrf_token") != session["csrf_token"]:
            raise PermissionError("Invalid CSRF token.")

    def _cart(self, session):
        session.setdefault("cart", {})
        return session["cart"]

    def _set_flash(self, session, message):
        session["flash"] = message

    def _pop_flash(self, session):
        message = session.get("flash")
        session["flash"] = None
        return message

    def _redirect(self, start_response, response_headers, location):
        headers = list(response_headers)
        headers.append(("Location", location))
        start_response("303 See Other", headers)
        return [b""]

    def _respond(self, start_response, status, headers, body):
        payload = body.encode("utf-8")
        response_headers = list(headers)
        response_headers.append(("Content-Length", str(len(payload))))
        start_response(status, response_headers)
        return [payload]


def make_test_request(app, path="/", method="GET", form=None, cookie=None):
    form = form or {}
    payload = urlencode(form).encode("utf-8")
    environ = {
        "REQUEST_METHOD": method,
        "PATH_INFO": path,
        "CONTENT_LENGTH": str(len(payload)),
        "CONTENT_TYPE": "application/x-www-form-urlencoded",
        "wsgi.input": BytesIO(payload),
        "HTTP_COOKIE": cookie or "",
    }
    response = {"status": None, "headers": []}

    def start_response(status, headers):
        response["status"] = status
        response["headers"] = headers

    body = b"".join(app(environ, start_response)).decode("utf-8")
    response["body"] = body
    return response


def run_server():
    app = POSApplication()
    print("Serving POS on http://{0}:{1}".format(HOST, PORT))
    with make_server(HOST, PORT, app) as server:
        server.serve_forever()
