import re
import tempfile
import unittest
from pathlib import Path

from pos.db import initialize_database
from pos.services import POSService
from pos.web import POSApplication, make_test_request


class POSServiceTests(unittest.TestCase):
    def setUp(self):
        self.temp_dir = tempfile.TemporaryDirectory()
        self.database_path = Path(self.temp_dir.name) / "test-pos.db"
        initialize_database(self.database_path)
        self.service = POSService(self.database_path)

    def tearDown(self):
        self.temp_dir.cleanup()

    def test_seed_products_are_available(self):
        products = self.service.list_products()
        self.assertGreaterEqual(len(products), 4)

    def test_create_sale_reduces_stock_and_records_sale(self):
        products = self.service.list_products()
        product = products[0]

        sale_id = self.service.create_sale({str(product["id"]): 2})

        self.assertEqual(sale_id, 1)
        updated = self.service.get_product(product["id"])
        self.assertEqual(updated["stock"], product["stock"] - 2)
        sales = self.service.list_sales()
        self.assertEqual(len(sales), 1)
        self.assertEqual(sales[0]["items"][0]["quantity"], 2)

    def test_create_product_validates_duplicate_sku(self):
        with self.assertRaisesRegex(ValueError, "SKU already exists"):
            self.service.create_product("COF-001", "Alt Coffee", "3.00", "2")


class POSWebTests(unittest.TestCase):
    def setUp(self):
        self.temp_dir = tempfile.TemporaryDirectory()
        self.database_path = Path(self.temp_dir.name) / "test-pos.db"
        self.app = POSApplication(database_path=self.database_path)
        first_page = make_test_request(self.app, "/products")
        self.cookie = extract_cookie(first_page["headers"])
        self.csrf = extract_csrf_token(first_page["body"])

    def tearDown(self):
        self.temp_dir.cleanup()

    def test_get_products_page(self):
        response = make_test_request(self.app, "/products", cookie=self.cookie)
        self.assertTrue(response["status"].startswith("200"))
        self.assertIn("Inventory", response["body"])

    def test_post_without_csrf_is_rejected(self):
        response = make_test_request(
            self.app,
            "/cart/add",
            method="POST",
            form={"product_id": "1", "quantity": "1"},
            cookie=self.cookie,
        )
        self.assertTrue(response["status"].startswith("403"))

    def test_add_to_cart_and_checkout(self):
        add_response = make_test_request(
            self.app,
            "/cart/add",
            method="POST",
            form={"csrf_token": self.csrf, "product_id": "1", "quantity": "2"},
            cookie=self.cookie,
        )
        self.assertTrue(add_response["status"].startswith("303"))

        checkout_response = make_test_request(
            self.app,
            "/checkout",
            method="POST",
            form={"csrf_token": self.csrf},
            cookie=self.cookie,
        )
        self.assertTrue(checkout_response["status"].startswith("303"))

        sales_page = make_test_request(self.app, "/sales", cookie=self.cookie)
        self.assertIn("Sale #1 completed.", sales_page["body"])
        self.assertIn("#1", sales_page["body"])


def extract_cookie(headers):
    for key, value in headers:
        if key.lower() == "set-cookie":
            return value
    raise AssertionError("No session cookie found in headers.")


def extract_csrf_token(body):
    match = re.search(r'name="csrf_token" value="([^"]+)"', body)
    if not match:
        raise AssertionError("No CSRF token found in page body.")
    return match.group(1)


if __name__ == "__main__":
    unittest.main()
