# Records a guide clip from Fontelle on :99: drives the pointer and keys with
# XTEST, grabs the region every tick, then draws a clean cursor, click rings
# and key chips over the frames and writes NNNN.png + delays.txt.
import os, time, math
from Xlib import display, X, XK
from Xlib.ext import xtest
from PIL import Image, ImageDraw, ImageFont, ImageFilter

OUT_W, OUT_H = 540, 304
FONT = os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', '..', 'crates', 'fontelle-ui', 'fonts', 'OpenSans-Regular.ttf')
ACCENT = (94, 200, 222)


def ease(t):
    return 4 * t * t * t if t < 0.5 else 1 - (-2 * t + 2) ** 3 / 2


def cursor_sprite(scale=4):
    # A plain arrow, drawn big and shrunk for smooth edges.
    pts = [(0, 0), (0, 17), (4.2, 13.2), (7.2, 20), (10, 18.8), (7.1, 12.3), (12.6, 12.3)]
    pad = 4
    w, h = 18 + pad * 2, 26 + pad * 2
    big = Image.new('RGBA', (w * scale, h * scale), (0, 0, 0, 0))
    d = ImageDraw.Draw(big)
    p = [((x + pad) * scale, (y + pad) * scale) for x, y in pts]
    shadow = Image.new('RGBA', big.size, (0, 0, 0, 0))
    ImageDraw.Draw(shadow).polygon([(x + 1.5 * scale, y + 2 * scale) for x, y in p], fill=(0, 0, 0, 110))
    shadow = shadow.filter(ImageFilter.GaussianBlur(2 * scale))
    big = Image.alpha_composite(big, shadow)
    d = ImageDraw.Draw(big)
    d.polygon(p, fill=(255, 255, 255, 255), outline=(20, 24, 28, 255))
    d.line(p + [p[0]], fill=(20, 24, 28, 255), width=int(1.3 * scale), joint='curve')
    return big.resize((w, h), Image.LANCZOS), pad


class Rec:
    def __init__(self, region, fps=25):
        self.d = display.Display(':99')
        self.root = self.d.screen().root
        self.rx, self.ry, self.rw, self.rh = region
        self.dt = 1.0 / fps
        self.frames = []  # (image, t, cursor, pressed)
        self.clicks = []  # (t, x, y)
        self.chips = []   # (t0, t1, label)
        self.cur = (self.rx + self.rw // 2, self.ry + self.rh // 2)
        self.pressed = False
        self.t0 = None
        self.next = None

    def focus(self):
        for w in self.root.query_tree().children:
            try:
                if 'ontelle' in (w.get_wm_name() or ''):
                    self.d.set_input_focus(w, X.RevertToParent, X.CurrentTime)
            except Exception:
                pass
        self.d.sync()

    def close_others(self):
        # Every window but the studio's — an instrument's or an effect's,
        # which open on their own when one is added — closed as a window
        # manager would.
        from Xlib import protocol
        proto = self.d.intern_atom('WM_PROTOCOLS'); delete = self.d.intern_atom('WM_DELETE_WINDOW')
        for w in self.root.query_tree().children:
            try:
                name = w.get_wm_name() or ''
                if name and 'ontelle' not in name:
                    ev = protocol.event.ClientMessage(window=w, client_type=proto, data=(32, [delete, X.CurrentTime, 0, 0, 0]))
                    w.send_event(ev)
            except Exception:
                pass
        self.d.sync()
        self.focus()

    def _motion(self, x, y):
        self.cur = (int(round(x)), int(round(y)))
        xtest.fake_input(self.d, X.MotionNotify, x=self.cur[0], y=self.cur[1])
        self.d.sync()

    def start(self, x=None, y=None):
        self.focus()
        if x is not None:
            self._motion(x, y)
        time.sleep(0.3)
        self.t0 = time.time()
        self.next = self.t0
        self.tick()

    def tick(self):
        now = time.time()
        if now < self.next:
            time.sleep(self.next - now)
        self.next += self.dt
        raw = self.root.get_image(self.rx, self.ry, self.rw, self.rh, X.ZPixmap, 0xffffffff)
        im = Image.frombytes('RGB', (self.rw, self.rh), raw.data, 'raw', 'BGRX')
        self.frames.append((im, time.time() - self.t0, self.cur, self.pressed))

    def wait(self, secs):
        for _ in range(max(1, int(round(secs / self.dt)))):
            self.tick()

    def move(self, x, y, secs=0.6):
        x0, y0 = self.cur
        n = max(1, int(round(secs / self.dt)))
        # A slight arc reads as a hand, a straight line as a robot.
        dx, dy = x - x0, y - y0
        bend = 0.08 * math.hypot(dx, dy)
        nx, ny = (-dy, dx) if (dx or dy) else (0, 0)
        ln = math.hypot(nx, ny) or 1
        for i in range(1, n + 1):
            t = ease(i / n)
            arc = math.sin(math.pi * t) * bend
            self._motion(x0 + dx * t + nx / ln * arc, y0 + dy * t + ny / ln * arc)
            self.tick()

    def down(self, button=1):
        self.pressed = True
        self.clicks.append((time.time() - self.t0, *self.cur))
        xtest.fake_input(self.d, X.ButtonPress, button); self.d.sync()
        self.tick()

    def up(self, button=1):
        xtest.fake_input(self.d, X.ButtonRelease, button); self.d.sync()
        self.pressed = False
        self.tick()

    def click(self, x=None, y=None, secs=0.6, button=1, after=0.35):
        if x is not None:
            self.move(x, y, secs)
            self.wait(0.12)
        self.down(button)
        self.up(button)
        self.wait(after)

    def dclick(self, x=None, y=None, secs=0.6, after=0.4):
        if x is not None:
            self.move(x, y, secs)
            self.wait(0.12)
        self.clicks.append((time.time() - self.t0, *self.cur))
        for _ in range(2):
            xtest.fake_input(self.d, X.ButtonPress, 1); self.d.sync()
            time.sleep(0.03)
            xtest.fake_input(self.d, X.ButtonRelease, 1); self.d.sync()
            time.sleep(0.07)
        self.tick()
        self.wait(after)

    def hold(self, key, label=None):
        code = self.d.keysym_to_keycode(XK.string_to_keysym(key))
        xtest.fake_input(self.d, X.KeyPress, code); self.d.sync()
        if label:
            t = time.time() - self.t0
            self.chips.append([t, t + 600, label])
        return code

    def release(self, code):
        xtest.fake_input(self.d, X.KeyRelease, code); self.d.sync()
        for chip in self.chips:
            if chip[1] > time.time() - self.t0 + 100:
                chip[1] = time.time() - self.t0 + 0.3

    def drag(self, x0, y0, x1, y1, secs=0.9, approach=0.6, mod=None, label=None):
        self.move(x0, y0, approach)
        self.wait(0.12)
        code = self.hold(mod, label) if mod else None
        if code: self.wait(0.15)
        self.down()
        self.wait(0.1)
        self.move(x1, y1, secs)
        self.wait(0.1)
        self.up()
        if code: self.release(code)
        self.wait(0.3)

    def wheel(self, n, mod=None):
        code = self.d.keysym_to_keycode(XK.string_to_keysym(mod)) if mod else None
        if code: xtest.fake_input(self.d, X.KeyPress, code); self.d.sync()
        b = 4 if n > 0 else 5
        for _ in range(abs(n)):
            xtest.fake_input(self.d, X.ButtonPress, b); self.d.sync()
            xtest.fake_input(self.d, X.ButtonRelease, b); self.d.sync()
            self.tick()
        if code: xtest.fake_input(self.d, X.KeyRelease, code); self.d.sync()

    def key(self, combo, label=None, hold=1.3):
        self.focus()
        t = time.time() - self.t0
        self.chips.append((t, t + hold, label or combo.replace('Control_L', 'Ctrl').replace('Shift_L', 'Shift').replace('+', ' + ')))
        codes = [self.d.keysym_to_keycode(XK.string_to_keysym(k)) for k in combo.split('+')]
        for c in codes:
            xtest.fake_input(self.d, X.KeyPress, c); self.d.sync(); time.sleep(0.03)
        for c in reversed(codes):
            xtest.fake_input(self.d, X.KeyRelease, c); self.d.sync(); time.sleep(0.03)
        self.tick()

    def type(self, text):
        self.focus()
        for ch in text:
            name = {' ': 'space', '.': 'period'}.get(ch, ch)
            c = self.d.keysym_to_keycode(XK.string_to_keysym(name))
            xtest.fake_input(self.d, X.KeyPress, c); self.d.sync()
            xtest.fake_input(self.d, X.KeyRelease, c); self.d.sync()
            self.tick()

    # ------------------------------------------------------------ output
    def save(self, out, lead=0.7, tail=1.6):
        os.makedirs(out, exist_ok=True)
        for f in os.listdir(out):
            if f.endswith('.png') or f == 'delays.txt':
                os.remove(os.path.join(out, f))
        sx, sy = OUT_W / self.rw, OUT_H / self.rh
        arrow, pad = cursor_sprite()
        font = ImageFont.truetype(FONT, 15)
        delays = []
        n = len(self.frames)
        for i, (im, t, (cx, cy), pressed) in enumerate(self.frames):
            frame = im.resize((OUT_W, OUT_H), Image.LANCZOS) if (sx, sy) != (1, 1) else im.copy()
            frame = frame.convert('RGBA')
            px, py = (cx - self.rx) * sx, (cy - self.ry) * sy
            over = Image.new('RGBA', frame.size, (0, 0, 0, 0))
            od = ImageDraw.Draw(over)
            for (ct, x, y) in self.clicks:
                age = t - ct
                if 0 <= age < 0.45:
                    k = age / 0.45
                    r = 7 + 17 * ease(k)
                    a = int(230 * (1 - k))
                    qx, qy = (x - self.rx) * sx, (y - self.ry) * sy
                    od.ellipse([qx - r, qy - r, qx + r, qy + r], outline=ACCENT + (a,), width=3)
            if pressed:
                od.ellipse([px - 9, py - 9, px + 9, py + 9], fill=ACCENT + (90,))
            for (a0, a1, label) in self.chips:
                if a0 <= t < a1:
                    fade = min(1, (t - a0) / 0.12, (a1 - t) / 0.2)
                    tw = od.textlength(label, font=font)
                    bx, by = 12, OUT_H - 12 - 30
                    od.rounded_rectangle([bx, by, bx + tw + 24, by + 30], 7, fill=(14, 22, 28, int(225 * fade)), outline=ACCENT + (int(200 * fade),), width=1)
                    od.text((bx + 12, by + 5), label, font=font, fill=(236, 244, 248, int(255 * fade)))
            frame = Image.alpha_composite(frame, over)
            if 0 <= px < OUT_W and 0 <= py < OUT_H:
                frame.alpha_composite(arrow, (int(round(px)) - pad, int(round(py)) - pad)) if px - pad >= 0 and py - pad >= 0 else frame.paste(arrow, (int(round(px)) - pad, int(round(py)) - pad), arrow)
            frame.save(os.path.join(out, '%04d.png' % i))
            nxt = self.frames[i + 1][1] if i + 1 < n else t + self.dt
            ms = int(round((nxt - t) * 1000))
            if i == 0:
                ms += int(lead * 1000)
            if i == n - 1:
                ms += int(tail * 1000)
            delays.append(max(10, ms))
        with open(os.path.join(out, 'delays.txt'), 'w') as f:
            f.write(' '.join(map(str, delays)))
        return n
