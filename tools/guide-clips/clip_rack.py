import sys; sys.path.insert(0, sys.argv[1])
from rec import Rec
r = Rec((0, 90, 720, 405), fps=20)
r.start(420, 420)
r.wait(0.3)
r.click(131, 306, 0.8)                          # + Add instrument
r.wait(0.6)
r.click(171, 364, 0.6)                          # Flopsynth
r.wait(0.45)
r.close_others()
r.wait(0.8)
r.click(216, 139, 0.7)                          # solo Bass
r.wait(0.7)
r.click(216, 139, 0.2, after=0.2)
r.click(237, 161, 0.5)                          # mute Keys
r.wait(0.7)
r.click(237, 161, 0.2, after=0.2)
r.click(60, 161, 0.6)                           # choose Keys
r.wait(0.8)
r.move(420, 420, 0.5)
print(r.save(sys.argv[2]), 'frames')
