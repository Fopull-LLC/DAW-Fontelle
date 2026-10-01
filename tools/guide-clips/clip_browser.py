import sys; sys.path.insert(0, sys.argv[1])
from rec import Rec
r = Rec((0, 100, 960, 540), fps=20)
r.start(560, 520)
r.wait(0.3)
r.click(37, 268, 0.8)                           # the sounds tab
r.wait(0.5)
r.click(70, 320, 0.6)                           # Flopsynth's presets
r.wait(0.6)
r.click(47, 546, 0.6)                           # a click plays it
r.wait(0.7)
r.dclick(40, 568, 0.5)                          # a double-click loads it
r.wait(1.0)
r.drag(56, 612, 70, 161, secs=1.1, approach=0.7)   # or drag one onto a channel
r.wait(1.0)
r.move(560, 520, 0.6)
print(r.save(sys.argv[2]), 'frames')
