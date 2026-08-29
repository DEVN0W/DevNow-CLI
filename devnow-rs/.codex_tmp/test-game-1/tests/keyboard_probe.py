from __future__ import absolute_import, division, print_function

import errno
import fcntl
import os
import pty
import signal
import struct
import subprocess
import sys
import tempfile
import termios
import time


ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


def set_window_size(fd, rows, cols):
    packed = struct.pack("HHHH", rows, cols, 0, 0)
    fcntl.ioctl(fd, termios.TIOCSWINSZ, packed)


def wait_for_trace(trace_path, token, timeout):
    deadline = time.time() + timeout
    while time.time() < deadline:
        if os.path.exists(trace_path):
            with open(trace_path, "r") as handle:
                data = handle.read()
            if token in data:
                return data
        time.sleep(0.1)
    if os.path.exists(trace_path):
        with open(trace_path, "r") as handle:
            return handle.read()
    return ""


def send_key(master_fd, value, delay):
    os.write(master_fd, value)
    time.sleep(delay)


def main():
    trace_fd, trace_path = tempfile.mkstemp(prefix="neon-run-trace-", suffix=".log")
    os.close(trace_fd)

    master_fd, slave_fd = pty.openpty()
    set_window_size(slave_fd, 32, 100)

    env = os.environ.copy()
    env["TERM"] = "xterm-256color"
    env["METEOR_RUN_TRACE_FILE"] = trace_path
    env["METEOR_RUN_SCRIPTED"] = "1"
    env["METEOR_RUN_FRAME_DELAY"] = "0.03"

    def setup_child_terminal():
        os.setsid()
        fcntl.ioctl(slave_fd, termios.TIOCSCTTY, 0)

    proc = subprocess.Popen(
        [sys.executable, "-B", "app.py"],
        cwd=ROOT,
        stdin=slave_fd,
        stdout=slave_fd,
        stderr=slave_fd,
        env=env,
        preexec_fn=setup_child_terminal,
    )
    os.close(slave_fd)

    try:
        time.sleep(0.5)
        send_key(master_fd, b" ", 0.4)
        send_key(master_fd, b"a", 0.3)
        send_key(master_fd, b"d", 0.3)

        trace_data = wait_for_trace(trace_path, "state:game_over", timeout=5.0)
        if "state:game_over" not in trace_data:
            raise AssertionError("game never reached game-over state; trace=%r" % trace_data)

        send_key(master_fd, b"r", 0.4)
        send_key(master_fd, b"q", 0.2)

        exit_code = proc.wait(timeout=5.0)
        if exit_code != 0:
            raise AssertionError("game exited with status %s" % exit_code)

        with open(trace_path, "r") as handle:
            lines = [line.strip() for line in handle.readlines() if line.strip()]

        required = ["command:start", "command:left", "command:right", "state:game_over", "command:restart"]
        for item in required:
            if item not in lines:
                raise AssertionError("missing trace event %r in %r" % (item, lines))

        if lines.count("state:playing") < 2:
            raise AssertionError("restart did not re-enter playing state; trace=%r" % lines)

        print("keyboard probe passed")
        return 0
    finally:
        try:
            os.close(master_fd)
        except OSError:
            pass
        if proc.poll() is None:
            try:
                os.killpg(proc.pid, signal.SIGTERM)
            except OSError as exc:
                if exc.errno != errno.ESRCH:
                    raise
            try:
                proc.wait(timeout=2.0)
            except subprocess.TimeoutExpired:
                os.killpg(proc.pid, signal.SIGKILL)
                proc.wait()
        if os.path.exists(trace_path):
            os.remove(trace_path)


if __name__ == "__main__":
    sys.exit(main())
