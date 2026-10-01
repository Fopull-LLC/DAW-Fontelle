import sys; sys.path.insert(0, sys.argv[1])
from rec import Rec
r = Rec((0, 0, 1138, 640), fps=20)
r.start(700, 450)
r.wait(0.3)
r.click(226, 377, 0.9)                          # the gear
r.wait(0.7)
r.click(216, 267, 0.7)                          # Extensions
r.wait(0.6)
r.click(205, 372, 0.6)                          # Project: this song's routing
r.wait(1.0)
r.click(1094, 59, 0.9)                          # close
r.wait(0.5)
r.key('F1')                                     # the guide
r.wait(0.8)
r.click(204, 302, 0.8)                          # Mixing
r.wait(2.1)
r.key('Escape')
r.wait(0.4)
print(r.save(sys.argv[2]), 'frames')
