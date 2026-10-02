# The guide's clips

The tour card and the guide page show a short looping clip of each action a
page describes (`docs/ux-routing-and-learning-plan.md` step 7). They are
recorded from the real binary on the tour's own song, so **when the studio's
look changes, the clips go stale** — re-record the ones that show it.

```sh
Xwayland :99 -geometry 1400x900 &
tools/guide-clips/record.sh build          # the recording binary
tools/guide-clips/record.sh mixer roll     # some clips
tools/guide-clips/record.sh all            # or every one, settings last
cargo test -p fontelle-ui --test guide_media
```

The web manual (hub cards 0346, 0358) takes the clips as
`guide-<name>.png` and the text as `guide.md`, which is generated, never
edited: `cargo run -q --example guide_markdown -p fontelle-ui > guide.md`.

Then look at the frames (`$CLIPS_WORK/<name>/NNNN.png`, default
`/tmp/fontelle-clips`) before believing a clip, and delete that folder.

- `clip_<name>.py` is one clip: a region of the window, and what the pointer
  and keys do in it. Coordinates are for the 1280×720 window on the tour song.
- `rec.py` drives the pointer with XTEST, grabs the region every tick, and
  draws a clean pointer, a ring on each click and a chip for each key over
  the frames — the X server's own cursor is left out.
- The encoder (`crates/fontelle-ui/examples/guide_clip_encode.rs`) writes an
  animated PNG whose frames hold only what changed, as up to three
  rectangles, all but the last with no delay.

Two things that cost time:

- **With vsync on, a grab of the nested server is a frame behind** until
  something else presents, so a drag's result never showed. `build` makes the
  recording binary with vsync off and puts the source back.
- Adding an instrument or an effect **opens its window**; the clip scripts
  close it, as a user would.
