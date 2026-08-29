import unittest

import server


class ServerCompatibilityTestCase(unittest.TestCase):
    def test_health_alias_endpoint(self):
        status, payload = server.api_payload_for_path("/health")
        self.assertEqual(status, 200)
        self.assertEqual(payload["status"], "ok")

    def test_handler_factory(self):
        handler = server.build_handler()
        self.assertIs(handler, server.app.GameRequestHandler)


if __name__ == "__main__":
    unittest.main()
