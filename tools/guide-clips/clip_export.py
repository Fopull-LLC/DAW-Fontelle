import sys; sys.path.insert(0, sys.argv[1])
from rec import Rec
r = Rec((330, 350, 600, 338), fps=20)
r.start(640, 520)
r.wait(0.4)
r.move(560, 420, 0.6)
r.wait(0.2)
r.key('Control_L+e', 'Ctrl + E')               # export
r.wait(0.8)
r.click(600, 456, 0.5)                          # Whole song, keep the tail
r.wait(3.2)
r.key('Control_L+s', 'Ctrl + S')                # save
r.wait(1.8)
print(r.save(sys.argv[2]), 'frames')
