# Shaping a slide: carry on from a note's end, add a point, drag it.
# Same roll as clip_slide.py.
import sys; sys.path.insert(0, sys.argv[1])
from rec import Rec

def x(step): return 336 + 30 * step
def y(key): return 446 - (key - 60) * 16
def edge(step): return 328 + 30 * step       # where a point sits

r = Rec((272, 288, 540, 304), fps=20)
r.start(700, 600)
r.wait(0.3)
# A note that slides up a fifth.
r.move(x(0), y(60), 0.6); r.wait(0.15)
r.down(); r.wait(0.4)
r.key('s', 'S'); r.wait(0.2)
r.move(x(4), y(67), 0.8); r.wait(0.2)
r.up(); r.wait(0.6)
# Grab its end and S: it holds on from there, then slides back down.
r.move(edge(4), y(67), 0.6); r.wait(0.15)
r.down(); r.wait(0.4)
r.key('s', 'S'); r.wait(0.2)
r.move(x(7), y(67), 0.6); r.wait(0.1)
r.key('s', 'S'); r.wait(0.2)
r.move(x(10), y(60), 0.7); r.wait(0.2)
r.up(); r.wait(0.6)
# Double-click the way down for a point, and drag it under C: a dip.
# Step 9 is a third of the way from G to C (62.3): the point lands on D.
r.dclick(edge(9) + 2, 409, 0.6)
r.wait(0.4)
r.drag(edge(9), y(62), edge(9), y(55), secs=0.7, approach=0.4)
r.wait(0.6)
r.move(700, 600, 0.5)
print(r.save(sys.argv[2]), 'frames')
