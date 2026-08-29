import json
import threading
import time
import unittest
from http.client import HTTPConnection

import server


class ServerTests(unittest.TestCase):
    def setUp(self):
        self.httpd = server.ThreadedHTTPServer(("127.0.0.1", 0), server.build_handler())
        self.port = self.httpd.server_address[1]
        self.thread = threading.Thread(target=self.httpd.serve_forever)
        self.thread.daemon = True
        self.thread.start()
        time.sleep(0.05)

    def tearDown(self):
        self.httpd.shutdown()
        self.httpd.server_close()
        self.thread.join(timeout=1)

    def request(self, path):
        connection = HTTPConnection("127.0.0.1", self.port, timeout=2)
        connection.request("GET", path)
        response = connection.getresponse()
        body = response.read()
        connection.close()
        return response, body

    def test_health_endpoint(self):
        response, body = self.request("/health")
        payload = json.loads(body.decode("utf-8"))
        self.assertEqual(response.status, 200)
        self.assertEqual(payload["status"], "ok")

    def test_index_served(self):
        response, body = self.request("/")
        text = body.decode("utf-8")
        self.assertEqual(response.status, 200)
        self.assertIn("Maze Forge Hybrid", text)
        self.assertIn("canvas", text)


if __name__ == "__main__":
    unittest.main()
