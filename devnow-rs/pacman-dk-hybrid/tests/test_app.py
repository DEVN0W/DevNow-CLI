import unittest

import app


class AppLogicTestCase(unittest.TestCase):
    def test_health_endpoint(self):
        status, payload = app.api_payload_for_path("/api/health")
        self.assertEqual(status, 200)
        self.assertEqual(payload["status"], "ok")
        self.assertIn("game", payload)

    def test_game_config_endpoint(self):
        status, payload = app.api_payload_for_path("/api/game-config")
        self.assertEqual(status, 200)
        self.assertEqual(payload["title"], "Maze Climber")
        self.assertTrue(payload["loop"])

    def test_index_serves_game_shell(self):
        body = app.load_static_text("index.html")
        self.assertIn("Maze Climber", body)
        self.assertIn("gameCanvas", body)


if __name__ == "__main__":
    unittest.main()
