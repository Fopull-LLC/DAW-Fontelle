import sys; sys.path.insert(0, sys.argv[1])
from rec import Rec
r = Rec((264, 50, 540, 304), fps=20)
r.start(640, 330)
r.wait(0.3)
r.dclick(505, 281, 0.8)                         # draw a clip
r.wait(0.4)
r.drag(535, 281, 535, 315, secs=0.7)            # move it down a lane
r.wait(0.3)
r.drag(580, 315, 676, 315, secs=0.8)            # its right edge: longer
r.wait(0.4)
r.key('Control_L+d', 'Ctrl + D')                # duplicate
r.wait(1.0)
r.key('Delete')                                 # and remove the copy
r.wait(0.8)
r.move(640, 330, 0.6)
print(r.save(sys.argv[2]), 'frames')
