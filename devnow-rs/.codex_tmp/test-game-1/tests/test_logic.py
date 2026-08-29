from __future__ import absolute_import, division, print_function

import os
import sys
import unittest


ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
if ROOT not in sys.path:
    sys.path.insert(0, ROOT)

from game import GameModel, STATE_GAME_OVER, STATE_PLAYING, STATE_TITLE


class GameModelTests(unittest.TestCase):
    def test_title_state_starts_idle(self):
        model = GameModel(seed=7)
        self.assertEqual(model.state, STATE_TITLE)
        self.assertEqual(model.score, 0)
        model.close()

    def test_start_command_enters_playing_state(self):
        model = GameModel(seed=7)
        model.handle_command("start")
        self.assertEqual(model.state, STATE_PLAYING)
        self.assertEqual(model.player_lane, 2)
        model.close()

    def test_lane_movement_is_clamped(self):
        model = GameModel(seed=7)
        model.handle_command("start")
        for _ in range(10):
            model.handle_command("left")
        self.assertEqual(model.player_lane, 0)
        for _ in range(10):
            model.handle_command("right")
        self.assertEqual(model.player_lane, model.lane_count - 1)
        model.close()

    def test_scripted_round_reaches_game_over_and_restarts(self):
        model = GameModel(scripted=True)
        model.handle_command("start")
        model.handle_command("left")
        model.handle_command("right")
        for _ in range(30):
            model.step()
            if model.state == STATE_GAME_OVER:
                break
        self.assertEqual(model.state, STATE_GAME_OVER)
        first_score = model.score
        model.handle_command("restart")
        self.assertEqual(model.state, STATE_PLAYING)
        self.assertEqual(model.score, 0)
        self.assertGreater(first_score, 0)
        model.close()


if __name__ == "__main__":
    unittest.main()
