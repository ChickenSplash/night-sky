#!/usr/bin/env python3
"""GTK/WebKit companion UI, not a browser wrapper for the simulation."""
import json
import os
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
import sys
import threading

import gi

gi.require_version("Gtk", "3.0")
gi.require_version("WebKit2", "4.1")
from gi.repository import GLib, Gtk, WebKit2


def main():
    manager = WebKit2.UserContentManager()
    manager.register_script_message_handler("native")

    def message(_manager, result):
        text = result.get_js_value().to_string()
        if text.startswith('smoke:'):
            report = text.removeprefix('smoke:')
            path = root / 'data/logs/native-panel-smoke.json'
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(report + '\n')
            print('Native panel smoke: ' + report, file=sys.stderr, flush=True)
        elif text.startswith('error:'):
            print('Native panel JavaScript ' + text, file=sys.stderr, flush=True)
        else:
            print(text, flush=True)

    manager.connect("script-message-received::native", message)
    manager.add_script(WebKit2.UserScript.new("""
        window.addEventListener('error', event =>
            window.webkit.messageHandlers.native.postMessage('error:' + event.message));
        window.addEventListener('unhandledrejection', event =>
            window.webkit.messageHandlers.native.postMessage('error:' + String(event.reason)));
        """, WebKit2.UserContentInjectedFrames.TOP_FRAME,
        WebKit2.UserScriptInjectionTime.START, None, None))
    if os.environ.get('SPACE_NATIVE_PANEL_SMOKE') == '1':
        manager.add_script(WebKit2.UserScript.new('window.nativeSmoke = true;',
            WebKit2.UserContentInjectedFrames.TOP_FRAME,
            WebKit2.UserScriptInjectionTime.START, None, None))
    view = WebKit2.WebView.new_with_user_content_manager(manager)
    window = Gtk.Window(title="Night Sky HTML UI prototype")
    window.set_wmclass("night-sky-html-panel", "night-sky-html-panel")
    window.set_default_size(960, 900)
    window.add(view)
    window.connect("destroy", lambda *_: Gtk.main_quit())
    root = Path(__file__).resolve().parent.parent

    class Assets(SimpleHTTPRequestHandler):
        def __init__(self, *args, **kwargs):
            super().__init__(*args, directory=str(root / 'web'), **kwargs)

        def do_GET(self):
            if self.path not in ('/native-panel.html', '/ui/panels.js',
                                 '/ui/panels.css', '/ui/bridge.js',
                                 '/ui/native-panel-smoke.js'):
                self.send_error(404)
                return
            super().do_GET()

        def do_HEAD(self):
            self.send_error(405)

        def log_message(self, *_args):
            pass

    # Modules need a normal origin. Serve only these public UI assets, never
    # simulation state or commands; those travel solely over inherited stdio.
    server = ThreadingHTTPServer(('127.0.0.1', 0), Assets)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    uri = f'http://127.0.0.1:{server.server_port}/native-panel.html'

    def policy(_view, decision, kind):
        if kind in (WebKit2.PolicyDecisionType.NAVIGATION_ACTION,
                    WebKit2.PolicyDecisionType.NEW_WINDOW_ACTION):
            if decision.get_navigation_action().get_request().get_uri() != uri:
                decision.ignore()
                return True
        return False

    # Never expose the native command channel to navigated external pages.
    view.connect('decide-policy', policy)
    view.load_uri(uri)

    def deliver(packet):
        # JSON encoding, never interpolate state into executable JS directly.
        script = "window.dispatchEvent(new CustomEvent(" + json.dumps(packet["name"]) + ", {detail:" + json.dumps(packet["detail"]) + "}));"
        view.evaluate_javascript(script, -1, None, None, None, None, None)
        return GLib.SOURCE_REMOVE

    def receive():
        for line in sys.stdin:
            GLib.idle_add(deliver, json.loads(line))
        GLib.idle_add(Gtk.main_quit)

    threading.Thread(target=receive, daemon=True).start()
    window.show_all()
    Gtk.main()
    server.shutdown()
    server.server_close()


if __name__ == "__main__":
    main()
