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
        if self.path.startswith("/search") or self.path == "/catalogue":
            self.search_get()
        elif self.path == "/commands/set_debug":
            self.reply(200, {"synopsis": "set_debug <switch> <on>", "about": "Render switches"})
        elif self.path == "/state/missing":
            self.reply(404, {"error": "Unknown snapshot path"})
        elif self.path == "/state/lists":
            self.reply(200, [{"id": "demo", "name": "Demo", "tasks": 3,
                              "systems": [{"page": 0, "star": "g1.2.3"}]}])
        elif self.path == "/state":
            # Flying on the first read after a command, landed on the next.
            self.server.flights = getattr(self.server, "flights", 0) + 1
            self.reply(200, {"sampled_unix_ms": 2 ** 53, "trail": ["Milky Way", "Demo"],
                             "presentation_mode": "simulation", "location_planet": None,
                             "camera": {"flying": self.server.flights == 1}})
        else:
            self.reply(200, ["split"])

    def do_POST(self):
        body = self.rfile.read(int(self.headers.get("Content-Length", 0))).decode()
        self.server.requests.append(("POST", self.path, body, self.headers.get("X-Night-Sky-Client")))
        if self.path.startswith("/search"):
            self.reply(202, {"id": 5, "status": "running"} if self.path == "/search"
                       else {"id": 5, "status": "cancelling"})
        elif body.startswith("search "):
            self.server.shown = True
            self.reply(202, {"status": "accepted"})
        elif body == "jump missing":
            self.reply(409, {"error": "Unknown list `missing`"})
        elif self.path == "/screenshot" and getattr(self.server, "png", None):
            self.reply(202, {"status": "capture_scheduled", "png": self.server.png})
        else:
            self.reply(202, {"status": "accepted" if body.startswith("jump") else "queued"})

    def search_get(self):
        entry = {"id": 5, "asker": "api", "query": "kind=planet within_ly=50", "origin_pc": [1.0, 2.0, 3.0],
                 "error": None, "result_count": 1,
                 "progress": {"state": "running", "complete_to_ly": 3.0, "elapsed_ms": 12.0, "stars": 0}}
        if self.path == "/catalogue":
            self.reply(200, {"kinds": [{"id": "planet", "label": "Planet", "family": "planetary"}],
                             "families": [{"id": "planetary", "label": "Planetary bodies"}],
                             "properties": [{"id": "radius_km", "label": "Planet radius", "unit": "km",
                                             "values": {"type": "range"}, "kinds": ["planet"]}],
                             "relations": [{"id": "orbits", "label": "Orbits", "subjects": ["planet"],
                                            "targets": ["star"]}],
                             "syntax": ["kind=<kind>"]})
        elif self.path == "/search":
            game = dict(entry, id=6, asker="game")
            self.reply(200, [game] if getattr(self.server, "shown", False) else [entry])
        elif self.path in ("/search/5", "/search/6"):
            # Running on the first read, done on the next; #7 is refused.
            self.server.polls = getattr(self.server, "polls", 0) + 1
            if self.server.polls > 1:
                entry["progress"]["state"] = "capped"
                entry["results"] = [{"distance_ly": 4.2, "kind": "planet", "name": "TS 1-2-3 b",
                                     "class": "Sub-Neptune", "system": "TS 1-2-3"}]
            self.reply(200, dict(entry, id=int(self.path[-1])))
        elif self.path == "/search/7":
            self.reply(200, dict(entry, id=7, error="no system `g1.0.9` in this galaxy"))
        else:
            self.reply(404, {"error": "Unknown or expired search"})

    def reply(self, status, value):
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.end_headers()
        self.wfile.write(json.dumps(value).encode())

    def log_message(self, *args):
        pass


class CliTests(unittest.TestCase):
    def test_ship_orders_are_organised_and_translated_to_api_json(self):
        parser = NS["parser"]()
        cases = [
            (["warp", "engage"], {"action": "warp", "command": {"command": "engage"}}),
            (["warp", "disengage"], {"action": "warp", "command": {"command": "disengage"}}),
            (["warp", "match", "sun/0"], {"action": "warp", "command": {"command": "match", "body": "sun/0", "orbit": False}}),
            (["warp", "match", "sun/0", "--orbit"], {"action": "warp", "command": {"command": "match", "body": "sun/0", "orbit": True}}),
            (["warp", "up"], {"action": "warp", "command": {"command": "up"}}),
            (["warp", "down"], {"action": "warp", "command": {"command": "down"}}),
            (["warp", "set", "1000000"], {"action": "warp", "command": {"command": "set", "speed_ms": 1000000.0}}),
            (["warp", "turn-rate", "90"], {"action": "warp", "command": {"command": "turn_rate", "degrees_per_second": 90.0}}),
            (["warp", "gravity", "suspended"], {"action": "warp", "command": {"command": "gravity", "policy": "suspended"}}),
            (["throttle", "-0.5"], {"action": "throttle", "value": -0.5}),
            (["translate", "--up", "0.2"], {"action": "translate", "right": 0.0, "up": 0.2, "forward": 0.0}),
            (["rotate", "--roll", "-0.1"], {"action": "rotate", "pitch": 0.0, "yaw": 0.0, "roll": -0.1}),
            (["point", "prograde"], {"action": "point", "target": {"target": "prograde"}, "up": None}),
            (["point", "radial-in"], {"action": "point", "target": {"target": "radial_in"}, "up": None}),
            (["point", "vector", "--up", "0,1,0", "--", "-1,0,0"],
             {"action": "point", "target": {"target": "vector", "value": [-1.0, 0.0, 0.0]}, "up": [0.0, 1.0, 0.0]}),
            (["stop"], {"action": "stop"}), (["release"], {"action": "release"}), (["cut"], {"action": "cut"}),
        ]
        for words, expected in cases:
            with self.subTest(words=words):
                command = NS["ship_command"](parser.parse_args(["ship", *words]))
                self.assertEqual(command, {"type": "steer_craft", "id": None, "order": expected})
        for words in (["point", "body"], ["point", "prograde", "extra"], ["point", "vector", "nan,0,0"]):
            with self.assertRaises(RuntimeError):
                NS["ship_command"](parser.parse_args(["ship", *words]))

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

    def test_wait_polls_until_the_flight_lands(self):
        self.server.flights = 0
        result = self.run_cli("cmd", "--wait", "jump demo")
        self.assertEqual(result.returncode, 0, result.stderr)
        settled = json.loads(result.stdout)["settled"]
        self.assertEqual(settled["trail"], ["Milky Way", "Demo"])
        self.assertFalse(settled["camera"]["flying"])
        self.assertEqual(self.server.flights, 2)

    def test_refused_commands_explain_why(self):
        result = self.run_cli("cmd", "jump missing")
        self.assertEqual(result.returncode, 1)
        self.assertIn("HTTP 409: Unknown list `missing`", result.stderr)

    def test_lists_and_screenshot_paths(self):
        result = self.run_cli("lists")
        self.assertEqual(json.loads(result.stdout),
                         [{"id": "demo", "name": "Demo", "tasks": 3, "systems": ["g1.2.3"]}])
        import tempfile
        with tempfile.TemporaryDirectory() as directory:
            self.server.png = str(Path(directory) / "shot.png")
            Path(self.server.png).write_bytes(b"png")
            try:
                result = self.run_cli("screenshot")
            finally:
                self.server.png = None
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(json.loads(result.stdout)["status"], "saved")

    def test_errors_are_nonzero_and_readable(self):
        result = self.run_cli("state", "missing")
        self.assertEqual(result.returncode, 1)
        self.assertIn("HTTP 404: Unknown snapshot path", result.stderr)
        self.assertEqual(result.stdout, "")
        result = self.run_cli("keys", "commands")
        self.assertEqual(result.returncode, 2)
        self.assertIn("must start with state or perf", result.stderr)

    def test_search_posts_json_and_waits_for_results(self):
        self.server.polls = 0
        result = self.run_cli("search", "--home", "kind=planet", "within_ly=50")
        self.assertEqual(result.returncode, 0, result.stderr)
        post = [r for r in self.server.requests if r[0] == "POST"][-1]
        self.assertEqual(post[:2], ("POST", "/search"))
        self.assertEqual(json.loads(post[2]), {"query": "kind=planet within_ly=50", "origin": "home"})
        self.assertEqual(post[3], "cli")
        self.assertIn("#5 capped · 1 results", result.stdout)
        self.assertIn("searched out to 3.0 ly", result.stdout)
        self.assertIn("TS 1-2-3 b", result.stdout)
        self.server.polls = 0
        result = self.run_cli("--json", "search", "--origin", "1,2,3", "--work-cap", "5", "kind=planet")
        self.assertEqual(json.loads(result.stdout)["results"][0]["class"], "Sub-Neptune")
        post = [r for r in self.server.requests if r[0] == "POST"][-1]
        self.assertEqual(json.loads(post[2]), {"query": "kind=planet", "origin": [1.0, 2.0, 3.0],
                                               "work_cap": 5000000})
        result = self.run_cli("search", "--origin", "1,2", "kind=planet")
        self.assertEqual(result.returncode, 1)
        self.assertIn("--origin is x,y,z", result.stderr)

    def test_search_show_sends_the_command_then_follows_the_game_request(self):
        self.server.polls, self.server.shown = 0, False
        try:
            result = self.run_cli("search", "--show", "kind=planet", "within_ly=50")
        finally:
            self.server.shown = False
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn(("POST", "/command", "search kind=planet within_ly=50", "cli"), self.server.requests)
        self.assertIn("#6 capped", result.stdout)

    def test_refused_searches_explain_why(self):
        globals_ = NS["main"].__globals__
        with mock.patch.dict(globals_, {"request": mock.Mock(side_effect=[
                {"id": 7}, {"id": 7, "error": "no system `g1.0.9` in this galaxy",
                            "progress": {"state": "done"}}])}):
            with contextlib.redirect_stderr(io.StringIO()) as errors:
                self.assertEqual(NS["main"](["search", "--home", "TS", "1-0-9"]), 1)
        self.assertIn("refused: no system", errors.getvalue())

    def test_catalogue_and_searches_are_readable(self):
        result = self.run_cli("catalogue")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("radius_km", result.stdout)
        self.assertIn("min..max km", result.stdout)
        self.assertIn("orbits.", result.stdout)
        result = self.run_cli("searches")
        self.assertIn("#5", result.stdout)
        self.assertIn("kind=planet within_ly=50", result.stdout)
        result = self.run_cli("--json", "catalogue")
        self.assertEqual(json.loads(result.stdout)["kinds"][0]["id"], "planet")

    def test_ship_actions_send_app_commands_and_read_craft(self):
        for args, expected in [
            (("ship", "spawn", "--body", "g1.2.3/2", "--altitude", "400", "--speed", "7.5"),
             {"type": "spawn_ship", "altitude_km": 400.0, "body": "g1.2.3/2", "speed_kms": 7.5}),
            (("ship", "spawn", "--altitude", "400", "--velocity", "0,7.5,0"),
             {"type": "spawn_ship", "altitude_km": 400.0, "velocity_kms": [0.0, 7.5, 0.0]}),
            (("ship", "thrust", "0,0,1", "--id", "craft-1"),
             {"type": "set_thrust", "thrust": [0.0, 0.0, 1.0], "id": "craft-1"}),
            (("ship", "follow", "craft-1"), {"type": "fly_to_craft", "id": "craft-1"}),
            (("ship", "despawn"), {"type": "despawn_craft", "id": None}),
        ]:
            result = self.run_cli(*args)
            self.assertEqual(result.returncode, 0, result.stderr)
            post = [r for r in self.server.requests if r[0] == "POST"][-1]
            self.assertEqual(post[1], "/command")
            self.assertEqual(json.loads(post[2]), expected)
        result = self.run_cli("ship", "state")
        self.assertEqual(self.server.requests[-1][1], "/state/craft")
        for args, message in [
            (("ship", "thrust", "0,1"), "thrust is x,y,z"),
            (("ship", "spawn", "--altitude", "1", "--speed", "1", "--velocity", "1,0,0"), "not both"),
        ]:
            result = self.run_cli(*args)
            self.assertEqual(result.returncode, 1)
            self.assertIn(message, result.stderr)

    def test_watch_stops_on_interrupt(self):
        globals_ = NS["main"].__globals__
        with mock.patch.dict(globals_, {"request": mock.Mock(return_value=1)}):
            with mock.patch.object(globals_["time"], "sleep", side_effect=KeyboardInterrupt):
                with contextlib.redirect_stdout(io.StringIO()) as output:
                    self.assertEqual(NS["main"](["watch", "perf", "latest"]), 0)
                    self.assertEqual(output.getvalue().strip(), "1")


if __name__ == "__main__":
    unittest.main()
