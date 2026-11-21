#!/usr/bin/env python3
"""Build a screen gallery for EVE from Mac app screenshots. Standard library only.

    python3 -I eval/usability/mac/gallery.py <shots dir> <out dir> [--ax <ax dir>]

Writes ``<out>/index.html`` (each screenshot as an ``<img>`` whose ``alt`` is the screen's
accessibility text, then that text as a list) and ``<out>/screens.md`` (the accessibility text
only). EVE perceives these two ways:

- ``eve read <out>/screens.md --genre interface --persona accessibility-user``: what a keyboard
  or VoiceOver user would get from the labels alone. Works offline today.
- ``eve_multimodal_scan`` / ``eve run file://<out>/index.html``: icons, text-in-images and
  unlabeled images, which needs EVE's Playwright surface (``npx playwright install chromium``).

``<ax dir>/<screen>.txt`` is the accessibility dump ``drive-mac.sh`` writes for a screenshot
``<screen>.png``; a screen without one is marked "no accessibility text captured".
"""

import argparse
import html
import os
import shutil
import sys


def build(shots: str, out: str, ax: str = "") -> int:
    os.makedirs(out, exist_ok=True)
    names = sorted(n for n in os.listdir(shots) if n.lower().endswith(".png"))
    parts = ["<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><title>Heron screens</title></head><body>",
             "<h1>Heron for Mac, captured screens</h1>"]
    md = ["# Heron for Mac, captured screens", ""]
    for name in names:
        screen = name[:-4]
        text = ""
        if ax:
            path = os.path.join(ax, screen + ".txt")
            if os.path.isfile(path):
                with open(path, "r", encoding="utf-8", errors="replace") as handle:
                    text = handle.read().strip()
        lines = [line.strip() for line in text.splitlines() if line.strip()] or ["(no accessibility text captured)"]
        shutil.copyfile(os.path.join(shots, name), os.path.join(out, name))
        parts.append("<section><h2>%s</h2>" % html.escape(screen))
        parts.append("<img src=\"%s\" alt=\"%s\" width=\"880\">" % (html.escape(name), html.escape("; ".join(lines))))
        parts.append("<ul>%s</ul></section>" % "".join("<li>%s</li>" % html.escape(line) for line in lines))
        md += ["## %s" % screen, ""] + ["- %s" % line for line in lines] + [""]
    parts.append("</body></html>")
    with open(os.path.join(out, "index.html"), "w", encoding="utf-8") as handle:
        handle.write("\n".join(parts) + "\n")
    with open(os.path.join(out, "screens.md"), "w", encoding="utf-8") as handle:
        handle.write("\n".join(md))
    return len(names)


def main(argv) -> int:
    parser = argparse.ArgumentParser(description="Build an EVE screen gallery from screenshots.")
    parser.add_argument("shots")
    parser.add_argument("out")
    parser.add_argument("--ax", default="", help="folder of <screen>.txt accessibility dumps")
    args = parser.parse_args(argv)
    if not os.path.isdir(args.shots):
        print("gallery: %s is not a folder" % args.shots, file=sys.stderr)
        return 2
    count = build(args.shots, args.out, args.ax)
    print("gallery: %d screen(s) in %s" % (count, args.out))
    return 0 if count else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
