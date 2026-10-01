import sys; sys.path.insert(0, sys.argv[1])
from rec import Rec
r = Rec((560, 312, 720, 405), fps=20)
r.start(900, 560)
r.wait(0.3)
r.key('2')                                      # the mixer
r.wait(0.8)
r.click(618, 404, 0.7)                          # select the Keys strip
r.wait(0.7)
r.drag(624, 476, 624, 506, secs=0.6, approach=0.5)  # its fader
r.wait(0.3)
r.click(1146, 500, 0.8)                         # + Add effect
r.wait(0.5)
r.click(1134, 344, 0.6)                         # Chorus
r.wait(0.25)
r.close_others()
r.wait(0.8)
r.click(713, 524, 0.8)                          # + makes a new track
r.wait(1.0)
r.move(900, 560, 0.5)
print(r.save(sys.argv[2]), 'frames')
