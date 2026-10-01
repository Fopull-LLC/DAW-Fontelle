import sys; sys.path.insert(0, sys.argv[1])
from rec import Rec
r = Rec((264, 50, 540, 304), fps=20)
r.start(640, 330)
r.wait(0.2)
r.click(302, 281, 0.7)          # select Lane 5
r.wait(0.4)
r.click(378, 179, 0.6)          # solo Bass
r.wait(0.7)
r.click(378, 179, 0.2, after=0.2)   # and back
r.click(358, 213, 0.5)          # mute Keys
r.wait(0.7)
r.click(358, 213, 0.2, after=0.2)
r.click(302, 145, 0.7, button=3, after=0.3)     # the lane menu
r.move(340, 200, 0.6)
r.wait(0.8)
r.key('Escape')
r.wait(0.2)
r.move(640, 330, 0.5)
print(r.save(sys.argv[2]), 'frames')
