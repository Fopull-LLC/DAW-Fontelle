# The setup driver: c,x,y click, r,x,y right-click, m,x,y move, dn/up press and
# release, k,Key (or Control_L+e), w,x,y,n[,Mod] wheel, s,secs wait, g,name grab.
import os, sys, time, subprocess
from Xlib import display, X
from Xlib.ext import xtest
d = display.Display(":99")
def mv(x, y): xtest.fake_input(d, X.MotionNotify, x=x, y=y); d.sync()
S = os.environ.get("CLIPS_WORK", ".")
for a in sys.argv[1:]:
    k, *v = a.split(',')
    if k == 's': time.sleep(float(v[0])); continue
    if k == 'g':  # grab: g,name  (twice: grabs lag a frame)
        for _ in range(2):
            subprocess.run(["ffmpeg","-loglevel","error","-f","x11grab","-video_size","1400x900","-i",":99.0+0,0","-frames:v","1","-y",f"{S}/{v[0]}.png"])
        continue
    if k == 'burst':  # burst,name,n,interval
        for i in range(int(v[1])):
            subprocess.run(["ffmpeg","-loglevel","error","-f","x11grab","-video_size","1400x900","-i",":99.0+0,0","-frames:v","1","-y",f"{S}/{v[0]}{i}.png"])
            time.sleep(float(v[2]))
        continue
    if k == 'k':  # k,Escape
        from Xlib import XK
        root = d.screen().root
        for w in root.query_tree().children:
            try:
                if w.get_wm_name() and 'ontelle' in w.get_wm_name():
                    d.set_input_focus(w, X.RevertToParent, X.CurrentTime)
            except Exception: pass
        codes = [d.keysym_to_keycode(XK.string_to_keysym(n)) for n in v[0].split('+')]
        for code in codes: xtest.fake_input(d, X.KeyPress, code); d.sync(); time.sleep(0.05)
        for code in reversed(codes): xtest.fake_input(d, X.KeyRelease, code); d.sync(); time.sleep(0.05)
        time.sleep(0.3)
        continue
    if k == 'w':  # w,x,y,n[,mod]  n>0 up, n<0 down
        from Xlib import XK
        mv(int(v[0]), int(v[1])); time.sleep(0.1)
        mod = d.keysym_to_keycode(XK.string_to_keysym(v[3])) if len(v) > 3 else None
        if mod: xtest.fake_input(d, X.KeyPress, mod); d.sync()
        n = int(v[2]); b = 4 if n > 0 else 5
        for _ in range(abs(n)):
            xtest.fake_input(d, X.ButtonPress, b); d.sync(); xtest.fake_input(d, X.ButtonRelease, b); d.sync(); time.sleep(0.08)
        if mod: xtest.fake_input(d, X.KeyRelease, mod); d.sync()
        time.sleep(0.2); continue
    x, y = int(v[0]), int(v[1])
    mv(x, y)
    if k == 'c' or k == 'r':
        b = 1 if k == 'c' else 3
        time.sleep(0.05); xtest.fake_input(d, X.ButtonPress, b); d.sync()
        time.sleep(0.05); xtest.fake_input(d, X.ButtonRelease, b); d.sync()
    if k == 'dn': xtest.fake_input(d, X.ButtonPress, 1); d.sync()
    if k == 'up': xtest.fake_input(d, X.ButtonRelease, 1); d.sync()
    time.sleep(0.15)
