import os
import re
import shutil
import tempfile
import unittest
from io import BytesIO
from urllib.parse import urlencode

from pos.web import create_app


class PosWebTests(unittest.TestCase):
    def setUp(self):
        self.temp_dir = tempfile.mkdtemp(prefix="pos-web-tests-")
        self.db_path = os.path.join(self.temp_dir, "test.db")
        self.app = create_app(db_path=self.db_path, secret_key="test-secret")
        self.cookie = ""

    def tearDown(self):
        shutil.rmtree(self.temp_dir)

    def request(self, path, method="GET", data=None, cookie=""):
        body = b""
        if data is not None:
            body = urlencode(data).encode("utf-8")
        environ = {
            "REQUEST_METHOD": method,
            "PATH_INFO": path.split("?", 1)[0],
            "QUERY_STRING": path.split("?", 1)[1] if "?" in path else "",
            "CONTENT_LENGTH": str(len(body)),
            "CONTENT_TYPE": "application/x-www-form-urlencoded",
            "wsgi.input": BytesIO(body),
            "HTTP_COOKIE": cookie,
        }
        captured = {}

        def start_response(status, headers):
            captured["status"] = status
            captured["headers"] = headers

        payload = b"".join(self.app(environ, start_response))
        return captured["status"], dict(captured["headers"]), payload.decode("utf-8")

    def csrf_from_html(self, html):
        match = re.search(r'name="csrf_token" value="([^"]+)"', html)
        self.assertIsNotNone(match)
        return match.group(1)

    def test_login_page_renders(self):
        status, headers, body = self.request("/")
        self.assertEqual("200 OK", status)
        self.assertIn("POS Sign In", body)
        self.assertNotIn("Set-Cookie", headers)

    def test_invalid_login_rejected(self):
        status, headers, body = self.request("/login", method="POST", data={"username": "admin", "password": "wrong"})
        self.assertEqual("401 Unauthorized", status)
        self.assertIn("Invalid credentials", body)

    def test_admin_can_create_product_and_checkout_sale(self):
        status, headers, _body = self.request("/login", method="POST", data={"username": "admin", "password": "admin123"})
        self.assertEqual("302 Found", status)
        self.cookie = headers["Set-Cookie"].split(";", 1)[0]

        status, _headers, body = self.request("/products", cookie=self.cookie)
        csrf = self.csrf_from_html(body)
        status, headers, _body = self.request(
            "/products/add",
            method="POST",
            data={"csrf_token": csrf, "name": "Orange Juice", "sku": "OJ-400", "price": "3.50", "stock": "12"},
            cookie=self.cookie,
        )
        self.assertEqual("302 Found", status)
        self.cookie = headers["Set-Cookie"].split(";", 1)[0]

        status, _headers, body = self.request("/products", cookie=self.cookie)
        self.assertIn("Orange Juice", body)
        csrf = self.csrf_from_html(body)
        product_id = re.search(r'name="product_id" value="(\d+)"', body).group(1)

        status, headers, _body = self.request(
            "/cart/add",
            method="POST",
            data={"csrf_token": csrf, "product_id": product_id, "quantity": "2"},
            cookie=self.cookie,
        )
        self.assertEqual("302 Found", status)
        self.cookie = headers["Set-Cookie"].split(";", 1)[0]

        status, _headers, body = self.request("/cart", cookie=self.cookie)
        self.assertIn("Active Sale", body)
        csrf = self.csrf_from_html(body)
        status, headers, _body = self.request(
            "/checkout",
            method="POST",
            data={"csrf_token": csrf, "payment_method": "card"},
            cookie=self.cookie,
        )
        self.assertEqual("302 Found", status)
        self.assertIn("/receipt?id=", headers["Location"])

        status, _headers, body = self.request(headers["Location"], cookie=self.cookie)
        self.assertEqual("200 OK", status)
        self.assertIn("Receipt #", body)
        self.assertIn("Grand Total", body)

    def test_invalid_receipt_id_returns_not_found(self):
        status, _headers, body = self.request("/receipt?id=bad-value")
        self.assertEqual("302 Found", status)

        status, headers, _body = self.request("/login", method="POST", data={"username": "cashier", "password": "cashier123"})
        self.cookie = headers["Set-Cookie"].split(";", 1)[0]
        status, _headers, body = self.request("/receipt?id=bad-value", cookie=self.cookie)
        self.assertEqual("404 Not Found", status)
        self.assertIn("Receipt not found", body)

    def test_empty_cart_checkout_is_rejected(self):
        status, headers, _body = self.request("/login", method="POST", data={"username": "cashier", "password": "cashier123"})
        self.cookie = headers["Set-Cookie"].split(";", 1)[0]
        status, _headers, body = self.request("/cart", cookie=self.cookie)
        csrf = self.csrf_from_html(body)
        status, _headers, body = self.request(
            "/checkout",
            method="POST",
            data={"csrf_token": csrf, "payment_method": "cash"},
            cookie=self.cookie,
        )
        self.assertEqual("400 Bad Request", status)
        self.assertIn("Cart is empty", body)

    def test_cashier_cannot_create_products(self):
        status, headers, _body = self.request("/login", method="POST", data={"username": "cashier", "password": "cashier123"})
        self.cookie = headers["Set-Cookie"].split(";", 1)[0]
        status, _headers, body = self.request("/products", cookie=self.cookie)
        csrf = self.csrf_from_html(body)
        status, _headers, body = self.request(
            "/products/add",
            method="POST",
            data={"csrf_token": csrf, "name": "Blocked Item", "sku": "BL-100", "price": "1.00", "stock": "1"},
            cookie=self.cookie,
        )
        self.assertEqual("403 Forbidden", status)
        self.assertIn("Admin role required", body)

    def test_checkout_rejects_insufficient_stock(self):
        status, headers, _body = self.request("/login", method="POST", data={"username": "admin", "password": "admin123"})
        self.cookie = headers["Set-Cookie"].split(";", 1)[0]
        status, _headers, body = self.request("/products", cookie=self.cookie)
        csrf = self.csrf_from_html(body)
        status, headers, _body = self.request(
            "/cart/add",
            method="POST",
            data={"csrf_token": csrf, "product_id": "1", "quantity": "999"},
            cookie=self.cookie,
        )
        self.cookie = headers["Set-Cookie"].split(";", 1)[0]
        status, _headers, body = self.request("/cart", cookie=self.cookie)
        csrf = self.csrf_from_html(body)
        status, _headers, body = self.request(
            "/checkout",
            method="POST",
            data={"csrf_token": csrf, "payment_method": "cash"},
            cookie=self.cookie,
        )
        self.assertEqual("400 Bad Request", status)
        self.assertIn("Insufficient stock", body)


if __name__ == "__main__":
    unittest.main()
