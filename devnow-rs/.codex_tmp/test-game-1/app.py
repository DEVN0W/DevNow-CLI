from __future__ import absolute_import, division, print_function

import os
import select
import shutil
import sys
import termios
import time
import tty

from game import GameModel, STATE_GAME_OVER, STATE_TITLE


TITLE_ART = [
    " _   _                         ____              ",
    "| \\ | | ___  ___  _ __       |  _ \\ _   _ _ __  ",
    "|  \\| |/ _ \\/ _ \\| '_ \\ _____| |_) | | | | '_ \\ ",
    "| |\\  |  __/ (_) | | | |_____|  _ <| |_| | | | |",
    "|_| \\_|\\___|\\___/|_| |_|     |_| \\_\\\\__,_|_| |_|",
]

ROAD_SHADE = "."
LANE_MARK = ":"
PLAYER_SPRITE = "<^>"
OBSTACLE_SPRITES = {
    1: "[#]",
    2: "[X]",
}

RESET = "\033[0m"
HIDE_CURSOR = "\033[?25l"
SHOW_CURSOR = "\033[?25h"
ALT_SCREEN = "\033[?1049h"
EXIT_ALT_SCREEN = "\033[?1049l"
CLEAR_SCREEN = "\033[2J"
HOME = "\033[H"

STYLES = {
    "accent": "\033[95m\033[1m",
    "chrome": "\033[96m\033[1m",
    "hud": "\033[93m\033[1m",
    "dim": "\033[37m",
    "lane": "\033[94m",
    "player": "\033[92m\033[1m",
    "danger": "\033[91m\033[1m",
}


def center_text(width, text):
    if len(text) >= width:
        return text[:width]
    padding = (width - len(text)) // 2
    return (" " * padding) + text


def clamp(value, low, high):
    return max(low, min(high, value))


class TerminalController(object):
    def __init__(self, input_stream, output_stream):
        self.input_stream = input_stream
        self.output_stream = output_stream
        self.fd = input_stream.fileno()
        self.original_mode = None

    def __enter__(self):
        self.original_mode = termios.tcgetattr(self.fd)
        tty.setraw(self.fd)
        self.output_stream.write(ALT_SCREEN + HIDE_CURSOR + CLEAR_SCREEN + HOME)
        self.output_stream.flush()
        return self

    def __exit__(self, exc_type, exc, tb):
        if self.original_mode is not None:
            termios.tcsetattr(self.fd, termios.TCSADRAIN, self.original_mode)
        self.output_stream.write(RESET + SHOW_CURSOR + EXIT_ALT_SCREEN)
        self.output_stream.flush()


class NeonRunApp(object):
    def __init__(self):
        self.screen = sys.stdout
        self.frame = 0
        self.running = True
        self.input_buffer = b""
        self.frame_delay = float(os.environ.get("METEOR_RUN_FRAME_DELAY", "0.08"))
        self.model = GameModel(
            seed=os.environ.get("METEOR_RUN_SEED"),
            scripted=os.environ.get("METEOR_RUN_SCRIPTED") == "1",
            trace_file=os.environ.get("METEOR_RUN_TRACE_FILE"),
        )

    def run(self):
        if not sys.stdin.isatty() or not sys.stdout.isatty():
            sys.stderr.write("Neon Run needs a terminal TTY.\n")
            return 1

        with TerminalController(sys.stdin, sys.stdout):
            try:
                while self.running:
                    self.frame += 1
                    self.process_input()
                    self.model.step()
                    self.render()
                    time.sleep(self.frame_delay)
            finally:
                self.model.close()
        return 0

    def process_input(self):
        while True:
            ready, _, _ = select.select([sys.stdin.fileno()], [], [], 0)
            if not ready:
                break
            chunk = os.read(sys.stdin.fileno(), 32)
            if not chunk:
                self.running = False
                return
            self.model._log("bytes:%r" % (chunk,))
            self.input_buffer += chunk

        while self.input_buffer:
            if self.input_buffer.startswith(b"\x1b[D"):
                self.model.handle_command("left")
                self.input_buffer = self.input_buffer[3:]
                continue

            if self.input_buffer.startswith(b"\x1b[C"):
                self.model.handle_command("right")
                self.input_buffer = self.input_buffer[3:]
                continue

            byte = self.input_buffer[:1]
            self.input_buffer = self.input_buffer[1:]

            if byte in (b"q", b"Q"):
                self.running = False
                return
            if byte in (b" ", b"\r", b"\n"):
                self.model.handle_command("start")
                continue
            if byte in (b"r", b"R"):
                self.model.handle_command("restart")
                continue
            if byte in (b"a", b"A", b"h"):
                self.model.handle_command("left")
                continue
            if byte in (b"d", b"D", b"l"):
                self.model.handle_command("right")
                continue

    def render(self):
        width, height = shutil.get_terminal_size((100, 32))
        lines = []
        if width < 84 or height < 28:
            lines.append("")
            lines.append(center_text(width, "Resize terminal to at least 84x28"))
            lines.append(center_text(width, "Current terminal: %sx%s" % (width, height)))
            lines.append("")
            lines.append(center_text(width, "Neon Run uses live keyboard input and ANSI graphics."))
        else:
            lines.extend(self.render_scene(width))

        output = HOME + "\n".join(lines) + "\033[J"
        self.screen.write(output)
        self.screen.flush()

    def render_scene(self, width):
        lines = []
        lines.append("")
        lines.append(center_text(width, STYLES["chrome"] + "Neon Run" + RESET))
        score_text = "Score %04d   Best %04d" % (self.model.score, self.model.best_score)
        controls = "Controls: A/D or arrows move, Space starts, R restarts, Q quits"
        lines.append("")
        lines.append(center_text(width, STYLES["hud"] + score_text + RESET))
        lines.append(center_text(width, STYLES["dim"] + controls + RESET))
        lines.append("")

        field_lines = self.build_field_lines()
        left_padding = " " * max(0, (width - 47) // 2)
        for field_line in field_lines:
            lines.append(left_padding + field_line)

        lines.append("")
        lines.extend(self.build_status_lines(width))
        return lines

    def build_field_lines(self):
        outer_width = 47
        road_width = outer_width - 4
        lane_width = road_width // self.model.lane_count
        field_lines = []
        field_lines.append(STYLES["chrome"] + "+" + ("-" * (outer_width - 2)) + "+" + RESET)

        scroll = self.frame % 4
        obstacle_map = {}
        for obstacle in self.model.visible_obstacles():
            obstacle_map[(obstacle["row"], obstacle["lane"])] = OBSTACLE_SPRITES.get(obstacle["style"], "[#]")

        for row in range(self.model.field_height):
            chars = [ROAD_SHADE] * road_width
            styles = ["lane"] * road_width

            for lane in range(1, self.model.lane_count):
                divider = lane * lane_width
                if divider < road_width:
                    chars[divider] = LANE_MARK if (row + scroll) % 4 < 2 else " "
                    styles[divider] = "dim"

            for lane in range(self.model.lane_count):
                sprite = obstacle_map.get((row, lane))
                if not sprite:
                    continue
                start = lane * lane_width + max(0, (lane_width - len(sprite)) // 2)
                for offset, ch in enumerate(sprite):
                    index = clamp(start + offset, 0, road_width - 1)
                    chars[index] = ch
                    styles[index] = "danger"

            if row == self.model.player_row:
                start = self.model.player_lane * lane_width + max(0, (lane_width - len(PLAYER_SPRITE)) // 2)
                for offset, ch in enumerate(PLAYER_SPRITE):
                    index = clamp(start + offset, 0, road_width - 1)
                    chars[index] = ch
                    styles[index] = "player"

            field_lines.append(STYLES["chrome"] + "|" + RESET + self.paint(chars, styles) + STYLES["chrome"] + "|" + RESET)

        footer = "Thread the neon lanes. Survive as long as you can."
        footer_text = footer[: outer_width - 4]
        padding = max(0, (outer_width - 2 - len(footer_text)) // 2)
        footer_line = "+" + ("-" * padding) + footer_text + ("-" * max(0, outer_width - 2 - padding - len(footer_text))) + "+"
        field_lines.append(STYLES["accent"] + footer_line[:outer_width] + RESET)
        return field_lines

    def paint(self, chars, styles):
        result = []
        active_style = None
        for ch, style_name in zip(chars, styles):
            style = STYLES[style_name]
            if style != active_style:
                result.append(style)
                active_style = style
            result.append(ch)
        result.append(RESET)
        return "".join(result)

    def build_status_lines(self, width):
        if self.model.state == STATE_TITLE:
            lines = []
            for line in TITLE_ART:
                lines.append(center_text(width, STYLES["accent"] + line + RESET))
            lines.append("")
            lines.append(center_text(width, STYLES["dim"] + "A polished Python terminal arcade game with real-time keyboard control." + RESET))
            lines.append(center_text(width, STYLES["hud"] + "Press SPACE to launch" + RESET))
            return lines

        if self.model.state == STATE_GAME_OVER:
            return [
                center_text(width, STYLES["danger"] + "Game Over" + RESET),
                center_text(width, STYLES["hud"] + "Final score %04d" % self.model.score + RESET),
                center_text(width, STYLES["dim"] + "Press R or SPACE to restart, or Q to quit." + RESET),
            ]

        return [
            center_text(width, STYLES["dim"] + "Stay in motion. The track gets busier the longer you survive." + RESET)
        ]


def main():
    app = NeonRunApp()
    return app.run()


if __name__ == "__main__":
    sys.exit(main())
