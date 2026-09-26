#!/usr/bin/env python3
"""Serve a built site locally the way the static host does: `/c/magnesium`
from `c/magnesium.html`, `/` from `index.html`, 404 from `404.html`.

For previews and the Lighthouse check only; production uses the CDN.
Usage: tools/serve-site.py out/current/site [port]
"""
import http.server
import os
import sys


def main() -> None:
    root = os.path.abspath(sys.argv[1] if len(sys.argv) > 1 else "out/current/site")
    port = int(sys.argv[2]) if len(sys.argv) > 2 else 8080

    class Handler(http.server.SimpleHTTPRequestHandler):
        def __init__(self, *args, **kwargs):
            super().__init__(*args, directory=root, **kwargs)

        def send_head(self):
            path = self.path.split("?", 1)[0].split("#", 1)[0]
            if path == "/":
                path = "/index.html"
            elif not os.path.splitext(path)[1]:
                path += ".html"
            full = os.path.normpath(os.path.join(root, path.lstrip("/")))
            if not full.startswith(root) or not os.path.isfile(full):
                self.path = "/404.html"
                self.send_response(404)
                self.send_header("Content-Type", "text/html; charset=utf-8")
                body = open(os.path.join(root, "404.html"), "rb").read()
                self.send_header("Content-Length", str(len(body)))
                self.end_headers()
                return __import__("io").BytesIO(body)
            self.path = path
            return super().send_head()

    server = http.server.ThreadingHTTPServer(("127.0.0.1", port), Handler)
    print(f"listening on http://127.0.0.1:{port}", flush=True)
    server.serve_forever()


if __name__ == "__main__":
    main()
