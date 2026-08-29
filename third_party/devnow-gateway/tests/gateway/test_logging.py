"""Tests for gateway stderr logging configuration.

Covers the ``--log-level`` / ``--verbose`` resolution added so managed DevNow
startup can request warning-only, colour-free output while standalone usage
keeps its existing INFO default.
"""

from __future__ import annotations

import io
import logging
import os

import pytest

from llm_rosetta.gateway import logging as gateway_logging
from llm_rosetta.gateway.logging import (
    ColoredFormatter,
    get_logger,
    log_original_request,
    setup_logging,
)


@pytest.fixture(autouse=True)
def _restore_handler():
    """Ensure each test starts/ends without leaking a handler onto the shared logger."""
    yield
    logger = get_logger()
    if gateway_logging._handler is not None:
        logger.removeHandler(gateway_logging._handler)
        gateway_logging._handler = None


def _attach_capturing_handler(level: str, **kwargs) -> io.StringIO:
    """Call setup_logging(level=level, **kwargs) and redirect its handler to a buffer."""
    logger = setup_logging(level=level, **kwargs)
    buffer = io.StringIO()
    # setup_logging always attaches a StreamHandler(sys.stderr); swap the
    # stream so we can assert on captured output without touching sys.stderr.
    gateway_logging._handler.stream = buffer
    return buffer


class TestLogLevelResolution:
    def test_warning_level_suppresses_info_request_summaries(self):
        buffer = _attach_capturing_handler("warning")

        log_original_request({"model": "claude-sonnet-4", "messages": [{"role": "user"}]})

        assert buffer.getvalue() == ""

    def test_info_level_allows_info_request_summaries(self):
        buffer = _attach_capturing_handler("info")

        log_original_request({"model": "claude-sonnet-4", "messages": [{"role": "user"}]})

        output = buffer.getvalue()
        assert "ORIGINAL REQUEST" in output
        assert "model=claude-sonnet-4" in output

    def test_verbose_forces_debug_regardless_of_level(self):
        # Simulates cli.py's precedence: --verbose wins over --log-level.
        buffer = _attach_capturing_handler("debug")

        get_logger().debug("debug-only diagnostic")

        assert "debug-only diagnostic" in buffer.getvalue()

    def test_unknown_level_falls_back_to_info(self):
        buffer = _attach_capturing_handler("not-a-real-level")

        get_logger().info("fallback info line")
        get_logger().debug("should not appear")

        output = buffer.getvalue()
        assert "fallback info line" in output
        assert "should not appear" not in output


class TestWarningsAndErrorsAlwaysReachHandler:
    def test_warning_records_reach_handler_at_warning_level(self):
        buffer = _attach_capturing_handler("warning")

        get_logger().warning("upstream is slow")
        get_logger().error("upstream failed")

        output = buffer.getvalue()
        assert "upstream is slow" in output
        assert "upstream failed" in output

    def test_info_records_do_not_reach_handler_at_warning_level(self):
        buffer = _attach_capturing_handler("warning")

        get_logger().info("routine info line")

        assert buffer.getvalue() == ""


class TestColorPolicy:
    def test_no_color_env_removes_ansi_sequences(self, monkeypatch):
        monkeypatch.setenv("NO_COLOR", "1")
        formatter = ColoredFormatter(use_colors=True)
        record = logging.LogRecord(
            name="devnow-gateway",
            level=logging.INFO,
            pathname=__file__,
            lineno=1,
            msg="hello world",
            args=(),
            exc_info=None,
        )
        formatted = formatter.format(record)
        assert "\033[" not in formatted
        assert "hello world" in formatted

    def test_colors_enabled_without_no_color_when_tty(self, monkeypatch):
        monkeypatch.delenv("NO_COLOR", raising=False)
        monkeypatch.setattr(gateway_logging, "_supports_color", lambda: True)
        formatter = ColoredFormatter(use_colors=True)
        record = logging.LogRecord(
            name="devnow-gateway",
            level=logging.INFO,
            pathname=__file__,
            lineno=1,
            msg="hello world",
            args=(),
            exc_info=None,
        )
        formatted = formatter.format(record)
        assert "\033[" in formatted


class TestBodyLoggingDefault:
    def test_body_logging_disabled_by_default(self):
        buffer = _attach_capturing_handler("debug")
        assert gateway_logging._log_bodies is False

        from llm_rosetta.gateway.logging import log_response

        log_response({"choices": []})

        # log_response is a no-op entirely when _log_bodies is False.
        assert buffer.getvalue() == ""

    def test_body_logging_enabled_when_requested(self):
        buffer = _attach_capturing_handler("debug", log_bodies=True)
        assert gateway_logging._log_bodies is True

        from llm_rosetta.gateway.logging import log_response

        log_response({"choices": []})

        assert "choices" in buffer.getvalue()
