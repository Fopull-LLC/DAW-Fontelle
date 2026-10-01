#!/usr/bin/env python3
"""Draws Ember's pictures (the built-in theme's backdrops).

Procedural so they can be redrawn: python3 make.py, beside this file. Needs
ImageMagick's `magick`. A fixed seed, so the same pictures every time.
"""
import os
import random
import subprocess

HERE = os.path.dirname(os.path.abspath(__file__))
rng = random.Random(1007)
EMBER = ["#ffb347", "#ff7a1a", "#ffd27a", "#ff9a3c", "#ffe2a8"]


def magick(*args):
    subprocess.run(["magick", *args], check=True, cwd=HERE)


def sparks(w, h, n, rise_from=None, size=(1.0, 3.6), spread=1.0):
    """Draw commands for n embers; `rise_from` (x, y) gathers them above a
    point, thinning as they rise, like sparks off a fire."""
    out = []
    for _ in range(n):
        if rise_from:
            fx, fy = rise_from
            up = rng.random() ** 0.7
            y = fy - up * fy * 0.95
            x = fx + rng.gauss(0, w * 0.12 * spread) * (0.4 + up)
        else:
            x, y = rng.random() * w, rng.random() * h
        r = rng.uniform(*size)
        col = rng.choice(EMBER)
        a = rng.uniform(0.35, 1.0)
        out += ["-fill", col + "%02x" % int(a * 255), "-draw",
                f"circle {x:.1f},{y:.1f} {x + r:.1f},{y:.1f}"]
    return out


def glow(w, h, colour, cx, cy, r):
    """A soft round glow centred at (cx, cy) on a w×h layer, laid over what
    is there: a circle blurred to nothing, drawn small and scaled up so a
    wide glow costs nothing to blur. No edge anywhere."""
    k = 8
    return ["(", "-size", f"{w // k}x{h // k}", "xc:none", "-fill", colour,
            "-draw", f"circle {cx / k},{cy / k} {(cx + r * 0.55) / k},{cy / k}",
            "-blur", f"0x{r / k * 0.45}", "-resize", f"{w}x{h}!", ")",
            "-compose", "over", "-composite"]


def glowing(name, w, h, base, draws, glow=6, extra=()):
    """`base` canvas, embers drawn, then two soft glows of them laid over:
    a tight one that makes each spark burn and a wide one that lights the
    air around it."""
    magick(*base, *draws,
           "(", "+clone", "-blur", f"0x{glow}", ")",
           "-compose", "screen", "-composite",
           "(", "+clone", "-blur", f"0x{glow * 3}", ")",
           "-compose", "screen", "-composite", *extra, name)


# The window: a dark room lit from below by the fire, smoke, embers drifting.
magick("-size", "1600x900", "xc:#0c0705",
       *glow(1600, 900, "#6a2208", 800, 1000, 900),
       "(", "-seed", "7", "-size", "800x450", "plasma:fractal", "-colorspace",
       "gray", "-blur", "0x10", "-resize", "1600x900!", "+level-colors",
       "#000000,#3a1a0c", ")", "-gravity", "center", "-compose", "screen",
       "-composite", "smoke.png")
glowing("window.jpg", 1600, 900, ["smoke.png"],
        sparks(1600, 900, 320, rise_from=(800, 920), spread=2.2), glow=4,
        extra=("-quality", "86"))
os.remove(os.path.join(HERE, "smoke.png"))

# The transport bar: a line of heat along its foot.
glowing("transport.png", 1600, 48,
        ["-size", "1600x48", "gradient:#ff7a1a00-#ff7a1a55"],
        sparks(1600, 48, 70, size=(0.5, 1.4)), glow=3)

# The two side panels: a glow in the bottom corner, sparks rising from it.
glowing("channels.png", 600, 500,
        ["-size", "600x500", "xc:none",
         *glow(600, 500, "#ff6a1a70", 0, 520, 420)],
        sparks(600, 500, 90, rise_from=(60, 520), spread=0.9), glow=4)
glowing("browser.png", 500, 900,
        ["-size", "500x900", "xc:none",
         *glow(500, 900, "#ff7a1a60", 250, 930, 520)],
        sparks(500, 900, 120, rise_from=(250, 940), spread=1.0), glow=4)

# The arrangement: a column of sparks climbing the right-hand side.
glowing("arrangement.png", 1800, 450,
        ["-size", "1800x450", "xc:none",
         *glow(1800, 450, "#ff6a1a60", 1600, 480, 520)],
        sparks(1800, 450, 220, rise_from=(1600, 470), spread=1.3), glow=5)

# The roll: only heat — no sparks where the notes are read.
magick("-size", "1800x400", "gradient:#ff7a1a00-#ff7a1a26", "roll.png")

# The mixer: a bed of coals along the bottom, under the strips.
coals = []
for _ in range(110):
    x, y = rng.random() * 1000, 323 + rng.random() * 20
    r = rng.uniform(4, 12)
    col = rng.choice(["#ff5a0a", "#ff7a1a", "#c43a08", "#ffb347"])
    coals += ["-fill", col + "%02x" % rng.randint(90, 200), "-draw",
              f"circle {x:.1f},{y:.1f} {x + r:.1f},{y:.1f}"]
glowing("mixer.png", 1000, 333,
        ["-size", "1000x333", "gradient:#ff6a1a00-#ff6a1a40", *coals,
         "-blur", "0x1.2"],
        sparks(1000, 333, 130, rise_from=(660, 333), spread=2.4,
               size=(0.8, 2.6)), glow=5)

# Shipped inside the binary, so kept small: soft light loses nothing at a
# lower resolution, and the window scales each picture to its section.
for name, keep in [("arrangement.png", 55), ("browser.png", 55),
                   ("channels.png", 60), ("mixer.png", 75),
                   ("transport.png", 60)]:
    magick(name, "-resize", f"{keep}%", "-define", "png:compression-level=9",
           name)
