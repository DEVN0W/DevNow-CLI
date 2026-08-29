from __future__ import absolute_import, division, print_function

import os
import random


STATE_TITLE = "title"
STATE_PLAYING = "playing"
STATE_GAME_OVER = "game_over"


class GameModel(object):
    lane_count = 5
    field_height = 18
    player_row = field_height - 2

    def __init__(self, seed=None, scripted=False, trace_file=None):
        self.seed = self._normalize_seed(seed)
        self.scripted = scripted
        self.trace_file = trace_file
        self.best_score = 0
        self.round_number = 0
        self.trace_handle = None
        self._scripted_schedule = {
            2: [0],
            4: [4],
            8: [2],
            12: [1],
            16: [3],
        }
        self._last_spawn_lane = None
        self._rng = random.Random(self.seed)
        self.state = STATE_TITLE
        self.score = 0
        self.tick_count = 0
        self.player_lane = self.lane_count // 2
        self.obstacles = []
        self.spawn_cooldown = 0
        self._open_trace()
        self._log("state:%s" % self.state)

    def close(self):
        if self.trace_handle is not None:
            self.trace_handle.close()
            self.trace_handle = None

    def _normalize_seed(self, seed):
        if seed is None:
            return None
        if isinstance(seed, int):
            return seed
        try:
            return int(seed)
        except (TypeError, ValueError):
            return sum(ord(ch) for ch in str(seed))

    def _open_trace(self):
        if not self.trace_file:
            return
        directory = os.path.dirname(os.path.abspath(self.trace_file))
        if directory and not os.path.exists(directory):
            os.makedirs(directory)
        self.trace_handle = open(self.trace_file, "a")

    def _log(self, message):
        if self.trace_handle is None:
            return
        self.trace_handle.write(message + "\n")
        self.trace_handle.flush()

    def handle_command(self, command):
        if command == "start" and self.state in (STATE_TITLE, STATE_GAME_OVER):
            self._log("command:start")
            self.start_round()
            return

        if command == "restart" and self.state == STATE_GAME_OVER:
            self._log("command:restart")
            self.start_round()
            return

        if command == "restart" and self.state == STATE_TITLE:
            self._log("command:restart")
            self.start_round()
            return

        if command == "left" and self.state == STATE_PLAYING:
            next_lane = max(0, self.player_lane - 1)
            if next_lane != self.player_lane:
                self.player_lane = next_lane
                self._log("command:left")
                self._log("lane:%s" % self.player_lane)
            return

        if command == "right" and self.state == STATE_PLAYING:
            next_lane = min(self.lane_count - 1, self.player_lane + 1)
            if next_lane != self.player_lane:
                self.player_lane = next_lane
                self._log("command:right")
                self._log("lane:%s" % self.player_lane)
            return

    def start_round(self):
        self.round_number += 1
        round_seed = self.seed
        if round_seed is not None and not self.scripted:
            round_seed += self.round_number
        self._rng = random.Random(round_seed)
        self.state = STATE_PLAYING
        self.score = 0
        self.tick_count = 0
        self.player_lane = self.lane_count // 2
        self.obstacles = []
        self.spawn_cooldown = 3
        self._last_spawn_lane = None
        self._log("state:%s" % self.state)
        self._log("lane:%s" % self.player_lane)

    def step(self):
        if self.state != STATE_PLAYING:
            return

        self.tick_count += 1
        self.score += 5
        self._advance_obstacles()
        self._spawn_if_needed()
        self._check_collision()

    def _advance_obstacles(self):
        next_obstacles = []
        for obstacle in self.obstacles:
            obstacle["row"] += 1
            if obstacle["row"] <= self.field_height:
                next_obstacles.append(obstacle)
        self.obstacles = next_obstacles

    def _spawn_if_needed(self):
        if self.scripted:
            for lane in self._scripted_schedule.get(self.tick_count, []):
                self._spawn_obstacle(lane)
            return

        if self.spawn_cooldown > 0:
            self.spawn_cooldown -= 1
            return

        lane_options = list(range(self.lane_count))
        if self._last_spawn_lane in lane_options and len(lane_options) > 1:
            lane_options.remove(self._last_spawn_lane)
        lane = self._rng.choice(lane_options)
        self._spawn_obstacle(lane)
        self.spawn_cooldown = max(3, 9 - min(5, self.score // 80))

    def _spawn_obstacle(self, lane):
        self._last_spawn_lane = lane
        self.obstacles.append(
            {
                "lane": lane,
                "row": 0,
                "style": 1 + ((self.tick_count + lane) % 2),
            }
        )

    def _check_collision(self):
        for obstacle in self.obstacles:
            if obstacle["lane"] == self.player_lane and obstacle["row"] >= self.player_row:
                self.best_score = max(self.best_score, self.score)
                self.state = STATE_GAME_OVER
                self._log("state:%s" % self.state)
                return

    def visible_obstacles(self):
        return [item for item in self.obstacles if 0 <= item["row"] < self.field_height]
