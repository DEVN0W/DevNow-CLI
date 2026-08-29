import io
import os
import shutil
import tempfile
import unittest

from pos_web.app import create_app


class POSAppTests(unittest.TestCase):
    def setUp(self):
        self.temp_dir = tempfile.mkdtemp(prefix="pos-app-test-")
        self.db_path = os.path.join(self.temp_dir, "pos.db")
        self.app = create_app(db_path=self.db_path, tax_rate=0.05)

    def tearDown(self):
        shutil.rmtree(self.temp_dir)

    def make_request(self, method, path, data="", cookie=""):
        encoded = data.encode("utf-8")
        environ = {
            "REQUEST_METHOD": method,
            "PATH_INFO": path,
            "QUERY_STRING": "",
            "CONTENT_LENGTH": str(len(encoded)),
            "CONTENT_TYPE": "application/x-www-form-urlencoded",
            "wsgi.input": io.BytesIO(encoded),
            "HTTP_COOKIE": cookie,
        }
        captured = {}

        def start_response(status, headers):
            captured["status"] = status
            captured["headers"] = headers

        body = b"".join(self.app(environ, start_response)).decode("utf-8")
        captured["body"] = body
        return captured

    def extract_cookie(self, response):
        for name, value in response["headers"]:
            if name == "Set-Cookie":
                return value.split(";", 1)[0]
        return ""

    def extract_csrf(self, cookie):
        session_id = cookie.split("=", 1)[1]
        return self.app.sessions[session_id]["csrf_token"]

    def test_dashboard_loads(self):
        response = self.make_request("GET", "/")
        self.assertEqual(response["status"], "200 OK")
        self.assertIn("CounterSale POS", response["body"])

    def test_create_product_and_adjust_stock(self):
        home = self.make_request("GET", "/inventory")
        cookie = self.extract_cookie(home)
        csrf = self.extract_csrf(cookie)
        create_response = self.make_request(
            "POST",
            "/inventory/create",
            "csrf_token=%s&name=Muffin&sku=MUF1&price=2.50&stock_qty=8" % csrf,
            cookie=cookie,
        )
        self.assertEqual(create_response["status"], "302 Found")
        inventory_page = self.make_request("GET", "/inventory", cookie=cookie)
        self.assertIn("Muffin", inventory_page["body"])

    def test_checkout_reduces_stock_and_records_sale(self):
        sale_page = self.make_request("GET", "/sale")
        cookie = self.extract_cookie(sale_page)
        csrf = self.extract_csrf(cookie)
        product_id = self.app.db.list_products()[0]["id"]
        add_response = self.make_request(
            "POST",
            "/cart/add",
            "csrf_token=%s&product_id=%s&quantity=2" % (csrf, product_id),
            cookie=cookie,
        )
        self.assertEqual(add_response["status"], "302 Found")
        checkout_response = self.make_request(
            "POST",
            "/checkout",
            "csrf_token=%s&cash_received=30.00" % csrf,
            cookie=cookie,
        )
        self.assertEqual(checkout_response["status"], "302 Found")
        sales = self.app.db.sales_history(limit=5)
        self.assertEqual(len(sales), 1)
        self.assertEqual(sales[0]["items"][0]["quantity"], 2)
        remaining_stock = self.app.db.get_product(product_id)["stock_qty"]
        self.assertEqual(remaining_stock, 16)

    def test_invalid_csrf_rejected(self):
        sale_page = self.make_request("GET", "/sale")
        cookie = self.extract_cookie(sale_page)
        response = self.make_request(
            "POST",
            "/cart/add",
            "csrf_token=bad&product_id=1&quantity=1",
            cookie=cookie,
        )
        self.assertEqual(response["status"], "400 Bad Request")


if __name__ == "__main__":
    unittest.main()
