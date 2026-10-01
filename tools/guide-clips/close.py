# Closes every window on :99 whose name contains the argument (default: the
# studio), as a window manager would.
import sys
from Xlib import display, X, protocol
d = display.Display(':99')
proto = d.intern_atom('WM_PROTOCOLS'); delete = d.intern_atom('WM_DELETE_WINDOW')
for w in d.screen().root.query_tree().children:
    try:
        if (sys.argv[1] if len(sys.argv) > 1 else 'ontelle') in (w.get_wm_name() or ''):
            ev = protocol.event.ClientMessage(window=w, client_type=proto, data=(32, [delete, X.CurrentTime, 0, 0, 0]))
            w.send_event(ev); print('closed', w.get_wm_name())
    except Exception as e: print(e)
d.sync()
