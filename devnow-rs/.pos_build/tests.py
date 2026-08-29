import os
import shutil
import stat
import tempfile
import unittest
from urllib.parse import urlencode

from app import database
from app.pos_app import POSApplication, build_test_environ


class POSApplicationTests(unittest.TestCase):
    def setUp(self):
        self.temp_dir = tempfile.mkdtemp(prefix="pos-tests-")
        os.makedirs(os.path.join(self.temp_dir, "app", "static"))
        source_css = os.path.join(os.path.dirname(__file__), "app", "static", "app.css")
        target_css = os.path.join(self.temp_dir, "app", "static", "app.css")
        with open(source_css, "rb") as src:
            with open(target_css, "wb") as dst:
                dst.write(src.read())
        self.app = POSApplication(self.temp_dir)
        self.session_cookie = self.login()
        self.csrf_token = self.app.get_session_data(
            {"HTTP_COOKIE": self.session_cookie}
        )["csrf_token"]

    def tearDown(self):
        shutil.rmtree(self.temp_dir)

    def login(self):
        payload = urlencode({"pin": self.app.admin_pin})
        status, headers, _ = self.call_app(
            build_test_environ("/login", method="POST", body=payload),
            cookie="",
        )
        self.assertEqual(status, "302 Found")
        return headers["Set-Cookie"].split(";", 1)[0]

    def call_app(self, environ, cookie=None):
        captured = {}
        environ["HTTP_COOKIE"] = self.session_cookie if cookie is None else cookie

        def start_response(status, headers):
            captured["status"] = status
            captured["headers"] = headers

        body = b"".join(self.app(environ, start_response)).decode("utf-8")
        return captured["status"], dict(captured["headers"]), body

    def test_dashboard_loads(self):
        status, headers, body = self.call_app(build_test_environ("/"))
        self.assertEqual(status, "200 OK")
        self.assertIn("Corner Shop POS", body)
        self.assertEqual(headers["Content-Type"], "text/html; charset=utf-8")

    def test_product_creation(self):
        payload = urlencode(
            {
                "csrf_token": self.csrf_token,
                "sku": "SKU-5555",
                "name": "Sparkling Water",
                "price": "1.99",
                "stock_qty": "12",
            }
        )
        status, headers, body = self.call_app(
            build_test_environ("/products", method="POST", body=payload)
        )
        self.assertEqual(status, "302 Found")
        self.assertEqual(headers["Location"], "/products")
        self.assertEqual(body, "")
        names = [product["name"] for product in database.list_products(self.app.db_path)]
        self.assertIn("Sparkling Water", names)

    def test_inventory_adjustment(self):
        products = database.list_products(self.app.db_path)
        product_id = products[0]["id"]
        original_stock = products[0]["stock_qty"]
        payload = urlencode(
            {
                "csrf_token": self.csrf_token,
                "product_id": str(product_id),
                "change_qty": "3",
                "reason": "Restock",
            }
        )
        status, headers, _ = self.call_app(
            build_test_environ("/inventory", method="POST", body=payload)
        )
        self.assertEqual(status, "302 Found")
        updated = database.get_product(self.app.db_path, product_id)
        self.assertEqual(updated["stock_qty"], original_stock + 3)
        self.assertEqual(headers["Location"], "/inventory")

    def test_checkout_and_receipt(self):
        products = database.list_products(self.app.db_path)
        first = products[0]
        payload = urlencode(
            {
                "csrf_token": self.csrf_token,
                "qty_{0}".format(first["id"]): "2",
                "payment_method": "cash",
                "amount_tendered": "50.00",
            }
        )
        status, headers, _ = self.call_app(
            build_test_environ("/checkout", method="POST", body=payload)
        )
        self.assertEqual(status, "302 Found")
        self.assertTrue(headers["Location"].startswith("/receipt/"))

        receipt_status, _, receipt_body = self.call_app(
            build_test_environ(headers["Location"])
        )
        self.assertEqual(receipt_status, "200 OK")
        self.assertIn("Receipt", receipt_body)
        self.assertIn(first["name"], receipt_body)

    def test_reject_negative_opening_stock(self):
        payload = urlencode(
            {
                "csrf_token": self.csrf_token,
                "sku": "SKU-9000",
                "name": "Broken Item",
                "price": "3.50",
                "stock_qty": "-4",
            }
        )
        status, _, body = self.call_app(
            build_test_environ("/products", method="POST", body=payload)
        )
        self.assertEqual(status, "400 Bad Request")
        self.assertIn("Opening stock cannot be negative", body)
        names = [product["name"] for product in database.list_products(self.app.db_path)]
        self.assertNotIn("Broken Item", names)

    def test_reject_duplicate_sku(self):
        payload = urlencode(
            {
                "csrf_token": self.csrf_token,
                "sku": "SKU-1001",
                "name": "Duplicate Coffee",
                "price": "5.00",
                "stock_qty": "1",
            }
        )
        status, _, body = self.call_app(
            build_test_environ("/products", method="POST", body=payload)
        )
        self.assertEqual(status, "400 Bad Request")
        self.assertIn("SKU already exists", body)

    def test_reject_empty_cart_checkout(self):
        payload = urlencode(
            {
                "csrf_token": self.csrf_token,
                "payment_method": "cash",
                "amount_tendered": "0.00",
            }
        )
        status, _, body = self.call_app(
            build_test_environ("/checkout", method="POST", body=payload)
        )
        self.assertEqual(status, "400 Bad Request")
        self.assertIn("Cart cannot be empty", body)

    def test_reject_underpayment(self):
        first = database.list_products(self.app.db_path)[0]
        payload = urlencode(
            {
                "csrf_token": self.csrf_token,
                "qty_{0}".format(first["id"]): "1",
                "payment_method": "cash",
                "amount_tendered": "0.01",
            }
        )
        status, _, body = self.call_app(
            build_test_environ("/checkout", method="POST", body=payload)
        )
        self.assertEqual(status, "400 Bad Request")
        self.assertIn("Amount tendered is less than total", body)

    def test_requires_login_for_dashboard(self):
        status, headers, _ = self.call_app(build_test_environ("/"), cookie="")
        self.assertEqual(status, "302 Found")
        self.assertEqual(headers["Location"], "/login")

    def test_login_page_renders(self):
        status, _, body = self.call_app(build_test_environ("/login"), cookie="")
        self.assertEqual(status, "200 OK")
        self.assertIn("Staff Login", body)

    def test_reject_missing_csrf_token(self):
        payload = urlencode(
            {
                "sku": "SKU-6000",
                "name": "No CSRF Item",
                "price": "2.00",
                "stock_qty": "1",
            }
        )
        status, _, body = self.call_app(
            build_test_environ("/products", method="POST", body=payload)
        )
        self.assertEqual(status, "400 Bad Request")
        self.assertIn("Invalid CSRF token", body)

    def test_secret_files_use_owner_only_permissions(self):
        for filename in ("admin_pin.txt", "session_secret.txt"):
            path = os.path.join(self.temp_dir, "data", filename)
            mode = stat.S_IMODE(os.stat(path).st_mode)
            self.assertEqual(mode, 0o600)

    def test_login_rate_limit_triggers_after_repeated_failures(self):
        bad_payload = urlencode({"pin": "wrong-passcode"})
        for _ in range(5):
            status, _, body = self.call_app(
                build_test_environ("/login", method="POST", body=bad_payload),
                cookie="",
            )
            self.assertEqual(status, "400 Bad Request")
            self.assertIn("Invalid staff PIN", body)

        blocked_status, _, blocked_body = self.call_app(
            build_test_environ("/login", method="POST", body=bad_payload),
            cookie="",
        )
        self.assertEqual(blocked_status, "400 Bad Request")
        self.assertIn("Too many failed login attempts", blocked_body)


if __name__ == "__main__":
    unittest.main()
