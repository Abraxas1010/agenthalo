#!/usr/bin/env python3
"""Capture rotating proof explorer frames and assemble into GIF.

Uses Playwright to screenshot the HTML page and Pillow to build the GIF.
"""

import sys, time, threading, http.server, functools
from pathlib import Path
from playwright.sync_api import sync_playwright
from PIL import Image

DOCS = Path(__file__).resolve().parent.parent / "docs"
HTML = DOCS / "index.html"
ASSETS = Path(__file__).resolve().parent.parent / "assets"
OUT = ASSETS / "proof-explorer.gif"
FRAMES = 60
WIDTH = 960
HEIGHT = 540
DURATION_MS = 50  # per frame
PORT = 18923


def start_server():
    handler = functools.partial(http.server.SimpleHTTPRequestHandler, directory=str(DOCS))
    srv = http.server.HTTPServer(("127.0.0.1", PORT), handler)
    t = threading.Thread(target=srv.serve_forever, daemon=True)
    t.start()
    return srv


def main():
    if not HTML.exists():
        print(f"ERROR: {HTML} not found", file=sys.stderr)
        sys.exit(1)

    srv = start_server()
    print(f"Serving docs/ on http://127.0.0.1:{PORT}")

    frames = []

    with sync_playwright() as p:
        browser = p.chromium.launch(headless=True)
        page = browser.new_page(viewport={"width": WIDTH, "height": HEIGHT})
        page.goto(f"http://127.0.0.1:{PORT}/index.html")

        # Wait for Three.js scene to load (loading div hidden after init)
        page.wait_for_function("() => document.getElementById('loading')?.style.display === 'none'", timeout=30000)
        # Extra settle time for Three.js rendering + bloom to stabilize
        time.sleep(3)

        print(f"Capturing {FRAMES} frames at {WIDTH}x{HEIGHT}...")
        for i in range(FRAMES):
            buf = page.screenshot(type="png")
            img = Image.open(__import__("io").BytesIO(buf))
            # Resize to keep GIF small
            img = img.resize((WIDTH // 2, HEIGHT // 2), Image.LANCZOS)
            frames.append(img)

            # Wait for next animation frame (~50ms rotation)
            time.sleep(DURATION_MS / 1000)

            if (i + 1) % 10 == 0:
                print(f"  Frame {i + 1}/{FRAMES}")

        browser.close()

    # Assemble GIF
    print(f"Assembling GIF ({len(frames)} frames)...")
    frames[0].save(
        OUT,
        save_all=True,
        append_images=frames[1:],
        duration=DURATION_MS,
        loop=0,
        optimize=True,
    )

    size_kb = OUT.stat().st_size / 1024
    print(f"Saved: {OUT} ({size_kb:.0f} KB)")


if __name__ == "__main__":
    main()
