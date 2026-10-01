import sys; sys.path.insert(0, sys.argv[1])
from rec import Rec
r = Rec((264, 388, 540, 304), fps=20)
r.start(700, 600)
r.wait(0.2)
r.click(458, 516, 0.8)                          # a note (cell 450-480)
r.wait(0.15)
r.click(578, 516, 0.6)                          # another (570-600)
r.wait(0.15)
r.drag(585, 516, 645, 500, secs=0.7, approach=0.3)   # move it: 630-660, a key up
r.wait(0.15)
r.drag(657, 500, 753, 500, secs=0.8, approach=0.4)   # its right edge: longer
r.wait(0.2)
r.click(465, 516, 0.7, button=3)                # right-click deletes
r.wait(0.4)
r.key('e', 'E')                                 # select
r.wait(0.15)
r.drag(352, 445, 790, 620, secs=1.0, approach=0.7, mod='Control_L', label='Ctrl')
r.wait(0.8)
r.key('p', 'P')
r.move(700, 600, 0.4)
print(r.save(sys.argv[2]), 'frames')
