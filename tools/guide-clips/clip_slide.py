# Sliding notes: a melody from one note, then a chord sliding apart.
# The roll is a blank one-bar clip pulled tall (record.sh's `slide` setup):
# a step is 30 px from x=328, a row 16 px with C4's centre at y=446. Points
# go just inside a cell's left edge, because a point snaps to the nearest
# grid line.
import sys; sys.path.insert(0, sys.argv[1])
from rec import Rec

def x(step): return 336 + 30 * step
def y(key): return 446 - (key - 60) * 16

r = Rec((272, 288, 540, 304), fps=20)
r.start(700, 600)
r.wait(0.3)
# One note, held down: S fixes its end, and the pointer leads on.
r.move(x(0), y(60), 0.6); r.wait(0.15)
r.down(); r.wait(0.4)
r.key('s', 'S'); r.wait(0.2)
r.move(x(4), y(64), 0.8); r.wait(0.1)      # slides up to E
r.key('s', 'S'); r.wait(0.2)
r.move(x(6), y(64), 0.5); r.wait(0.1)      # holds
r.key('s', 'S'); r.wait(0.2)
r.move(x(8), y(62), 0.6); r.wait(0.1)      # down to D
r.key('s', 'S'); r.wait(0.2)
r.move(x(10), y(67), 0.6); r.wait(0.2)     # up to G, and let go
r.up(); r.wait(0.7)
# A chord: each note slides somewhere of its own.
r.move(x(11), y(60), 0.5); r.wait(0.15)
r.down(); r.wait(0.4)
r.key('s', 'S'); r.wait(0.2)
r.move(x(15), y(57), 0.8); r.wait(0.2)
r.up(); r.wait(0.3)
r.move(x(11), y(64), 0.5); r.wait(0.15)
r.down(); r.wait(0.4)
r.key('s', 'S'); r.wait(0.2)
r.move(x(15), y(69), 0.8); r.wait(0.2)
r.up(); r.wait(0.4)
r.move(700, 600, 0.5)
print(r.save(sys.argv[2]), 'frames')
