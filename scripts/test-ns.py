#!/usr/bin/env python3
"""CLI transport tests using an ephemeral local server, never the running app."""
import contextlib
import http.server
import io
import json
from pathlib import Path
import runpy
import subprocess
import threading
import unittest
from unittest import mock

SCRIPT = Path(__file__).with_name("ns")
NS = runpy.run_path(str(SCRIPT))


class Handler(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        self.server.requests.append(("GET", self.path, None, None))
        if self.path == "/commands/set_debug":
            self.reply(200, {"synopsis": "set_debug <switch> <on>", "about": "Render switches"})
        elif self.path == "/state/missing":
            self.reply(404, {"error": "Unknown snapshot path"})
        else:
            self.reply(200, ["split"])

    def do_POST(self):
        body = self.rfile.read(int(self.headers.get("Content-Length", 0))).decode()
        self.server.requests.append(("POST", self.path, body, self.headers.get("X-Night-Sky-Client")))
        self.reply(202, {"status": "queued"})

    def reply(self, status, value):
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.end_headers()
        self.wfile.write(json.dumps(value).encode())

    def log_message(self, *args):
        pass


class CliTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        cls.server.requests = []
        cls.thread = threading.Thread(target=cls.server.serve_forever, daemon=True)
        cls.thread.start()

    @classmethod
    def tearDownClass(cls):
        cls.server.shutdown()
        cls.server.server_close()
        cls.thread.join()

    def run_cli(self, *args):
        return subprocess.run([str(SCRIPT), "--port", str(self.server.server_port), *args],
                              capture_output=True, text=True, timeout=5)

    def test_help_is_offline(self):
        with mock.patch.dict(NS["main"].__globals__, {"request": mock.Mock(side_effect=AssertionError("network"))}):
            with contextlib.redirect_stdout(io.StringIO()) as output:
                self.assertEqual(NS["main"](["help"]), 0)
                self.assertIn("commands", output.getvalue())

    def test_reads_keys_and_history(self):
        for args, expected in [
            (("keys", "perf/latest"), "/perf/latest?keys=1"),
            (("state", "camera"), "/state/camera"),
            (("perf", "history", "--seconds", "2"), "/perf/history?seconds=2.0"),
            (("perf", "--history"), "/perf?history=1"),
            (("state", "space key"), "/state/space%20key"),
            (("inspect",), "/state/inspector"),
            (("inspect", "facts"), "/state/inspector/facts"),
            (("inspect", "actions/0/commands"), "/state/inspector/actions/0/commands"),
        ]:
            result = self.run_cli(*args)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(json.loads(result.stdout), ["split"])
            self.assertEqual(self.server.requests[-1][1], expected)

    def test_command_discovery_text_and_json(self):
        result = self.run_cli("commands", "set_debug")
        self.assertIn("set_debug <switch> <on>", result.stdout)
        result = self.run_cli("--json", "commands", "set_debug")
        self.assertEqual(json.loads(result.stdout)["about"], "Render switches")

    def test_mutations_use_header_and_preserve_command_body(self):
        text = 'inspect "a star/1"'
        result = self.run_cli("cmd", text)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.server.requests[-1], ("POST", "/command", text, "cli"))
        result = self.run_cli("screenshot")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.server.requests[-1], ("POST", "/screenshot", "", "cli"))

    def test_errors_are_nonzero_and_readable(self):
        result = self.run_cli("state", "missing")
        self.assertEqual(result.returncode, 1)
        self.assertIn("HTTP 404: Unknown snapshot path", result.stderr)
        self.assertEqual(result.stdout, "")
        result = self.run_cli("keys", "commands")
        self.assertEqual(result.returncode, 2)
        self.assertIn("must start with state or perf", result.stderr)

    def test_watch_stops_on_interrupt(self):
        globals_ = NS["main"].__globals__
        with mock.patch.dict(globals_, {"request": mock.Mock(return_value=1)}):
            with mock.patch.object(globals_["time"], "sleep", side_effect=KeyboardInterrupt):
                with contextlib.redirect_stdout(io.StringIO()) as output:
                    self.assertEqual(NS["main"](["watch", "perf", "latest"]), 0)
                    self.assertEqual(output.getvalue().strip(), "1")


if __name__ == "__main__":
    unittest.main()
