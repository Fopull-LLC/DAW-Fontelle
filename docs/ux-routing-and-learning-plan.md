# Routing you can see, two ways to route, and learning Fontelle

Planned 2026-09-30 from a Reddit thread and an interview with Ty. **Nothing
here is built.** The decisions below are Ty's; where something is still
open it says so, and it is his to settle before it is built.

## Where it came from

> *"Real quick, how do mixer tracks work on fontelle. Is each track
> connected to a lane or do you have to route each track to the master?
> Usually tracks are connected to the corresponding vertical lane so I'm
> just confused. If there's a manual I would also love to know where to
> find it."*
>
> *"I've never used FL before, I'm much more use to daw's like waveform,
> reaper and logic so I'll be upfront in saying that it feels quite clunky
> ... it might be worth adding in a feature that highlights items that are
> linked to a mixer track so people can better see what is what."*

Three things are wrong at once. **Where sound goes is invisible**: a
channel's mixer track is a two-digit number on its rack row, and an audio
clip's is behind a double-click. **The model is FL's**, and nothing says
so, so a Reaper or Logic user looks for lane = track and finds nothing.
**There is nowhere to read about it**: `?` opens the shortcuts page, and
there is no manual. The patch cables under the mixer (v0.19 work, same day)
are the first step on the first problem.

## Decided

### 1. Two routing modes, per project

- **Rack-style** (today's, FL's): a channel's route chip decides where its
  sound goes; lanes are free-form places to put clips.
- **Lane-style** (Reaper, Logic, Waveform): **each lane owns a mixer track**
  and everything placed on a lane — any instrument's clip, any audio, any
  recording — plays through that lane's track. The rack's route chip is not
  shown in this mode; there is nothing for it to decide.
- **Choosing**: the first time somebody makes a new song without having
  chosen, they are asked which they prefer, with a one-line picture of
  each. That answer becomes the default for every new project. **Each
  project can change it** in its project settings.
- **The rule Ty set above all**: the split must be *extremely* clear, and
  the two must never interfere — whichever mode a project is in, it simply
  behaves the way that mode's user expects. No mixed state, no "why did
  this go there".

- **One instrument, one lane** (Ty, 2026-09-30, over duplicating
  instruments per lane — *"i definitely do NOT want to have duplicate
  copies of instruments"*): in lane-style, the first time an instrument's
  clip is drawn onto a lane, that instrument **claims the lane** and plays
  through its track. Drawing the same instrument onto a second lane offers
  to **move** it there or **duplicate the channel** (a new rack channel,
  same instrument and settings, claiming the new lane). An instrument is
  one running copy with one output, as in rack-style.

### 2. Selecting a lane, and where new material lands

- **A click on a lane header selects it.** It no longer mutes. Mute, solo,
  rename, render and the rest move to the lane's right-click menu (which
  already holds add, rename, mute and delete).
- Small **mute and solo icons beside the lane name** may stay, so the
  common two do not always need a right-click — whether they fit is a
  visual call to make on the real header, and Ty left it to the build.
- **Where new material goes follows the mode.** Lane-style: into the
  lane it was recorded or placed on, through that lane's track. Rack-style:
  into the **selected lane** (see open question A).

### 3. Linked things light up

All three of the options pitched:

- **Select a mixer strip → its sources glow**: the rack channels and the
  arrangement clips feeding it are outlined in its colour.
- **Hover a route → its strip glows**: hovering a channel's route chip or
  an audio clip lights the strip it goes to, and its patch cable.
- **"Fed by" on each strip**: what feeds it, readable without clicking.

### 4. The route chip is the track's colour

Colour only, the name on hover. **Every mixer track gets a colour**: each
new one takes the next from a fixed, well-separated palette, and the user
can change it. (Today new tracks are all the same grey, which would make a
colour chip useless.)

### 5. One way to learn Fontelle

Ty chose all four and asked that they be **one clean experience, not
intrusive and not hard to use**:

- **An interactive tutorial**, offered on the start menu on first launch
  (a "Learn Fontelle" card; never starting by itself) and always reachable
  from a Learn button there. It opens a **demo project** and walks through
  it with an overlay: animated, polished, and in **sections that can be
  browsed between** to look back at something forgotten. It covers basic
  edits and the main features, and it **sets up what a user needs** as it
  goes: their preferences (the routing mode among them), and things like
  enabling VST 2 support (the `vst2` extension).
- **In-app guide pages** behind `?`, beside the shortcuts: the same
  sections as the tutorial in reading form.
- **"Coming from…" pages** (Reaper, Logic, Ableton, FL): how lanes, the
  rack and the mixer map onto what they know. The first of these is the
  answer this user needed.
- **A web manual** on the Fontelle product page. That page is the website
  agent's (hub task `0234`); the manual would be a coordination card to W,
  not something built in this repo.

The way these fit together, proposed: **one source of guide content**
(sections of text, pictures and tutorial steps), drawn as the overlay in
the tutorial, as pages behind `?`, and exported for the web manual — so the
three never disagree and a feature is documented once.

### 6. The settings page is redesigned — as a full page

Ty, 2026-09-30: **a full settings page**, opened by the gear, over the
studio like the shortcuts page — a section list on the left (MIDI input,
Folders, Plugins, Updates, and Project for the routing mode) and roomy,
clearly styled controls on the right; Escape closes it. Not the sidebar.


Ty's report on it: *"formatted weird like everything looks like a button
even when things are just labels, some things just have no or little
feedback"*. Labels have to look like labels and controls like controls,
every control has to show that it was used, and the project settings (the
routing mode's new home) belong in the same design.

## Settled in the second interview

- **A. Rack-style's "selected track" is the selected lane**: new clips,
  recordings and imports land on the lane selected. Sound still goes where
  each channel's route says.
- **B. Changing a project's mode keeps the mixer tracks and resets the
  routes** to the new mode's default — the lanes' tracks, or the master.
  Simple and predictable; the user re-routes by hand. No conversion that
  can be lossy, so nothing to warn about beyond saying it will happen.
- **C. In lane-style the rack stays as it is**: instruments live there and
  are drawn onto lanes; only where the sound goes differs. One place to
  learn for both modes.
- **F. The next release carries all of it**, tutorial and guide included.

## Settled in the third answer

- **D. The demo project is extremely simple**: only as much song as it takes
  to teach the necessary things well — a few lanes, an instrument or two,
  one audio clip, a mixer track with an effect and a send. Small enough to
  ship in the download.
- **E. The web manual card waits for the in-app guide.** The web guide is
  built from it — its screenshots and its text repurposed — so the card to
  W is filed once the guide exists, with those to hand.

## Proposed order

All in the next release (F); built in this order, each useful alone:

1. **Colours and highlighting** (§3, §4) — small, in rack-style's terms,
   and answers the Reddit suggestion directly.
2. **Lane selection** (§2) — the click change and the lane menu.
3. **The settings redesign** (§6), with the project-settings page the mode
   will need.
4. **Lane-style** (§1) — the largest: a project setting, the first-song
   question, the compiler's routing, recording and placement by mode, and
   conversion (open question B).
5. **Guide content and `?` pages**, then the **tutorial overlay** and the
   demo project, then the **web manual** card.
6. **New screenshots**, once everything above is in (Ty, 2026-09-30: *"we
   should also get updated screenshots for fontelle in general"* — and
   only after this release, since it changes most of what they show). The
   website's gallery is still the six shots from v0.1.0 (Sept 11) in the
   hub's `tasks/fontelle/assets/`. A **showcase song** is made for them in
   a fresh project in a scratch folder — drums, bass, a Flopsynth lead, an
   audio clip, mixer tracks with sends so the cables show. Ty's own
   projects may be shot too where one is more detailed than the showcase
   (he said so, 2026-09-30) — always from a **copy** of the project, never
   the original, since opening one writes backups and recent-project
   entries. Same six views at 1280×720 (the synth window at its own
   size), clean names, handed over on task `0234`'s thread for W; the
   deploy stays Ty's. The in-app guide's screenshots come from the same
   session, and the web manual is built from those.
7. **Animations in the guide** (Ty, 2026-09-30, after v0.19.0 shipped:
   *"the windowed tutorials [should] show related gifs of the mentioned
   actions being performed so the animations will make it more interactive
   and easy to follow"*, and *"you should produce these animations
   yourself and they should be accurate, helpful, and visually polished"*;
   next release). One short looping clip per page that describes an
   action — the transport, the rack, the browser, clips, lanes, the piano
   roll, the mixer, exporting, settings and help — shown in the tour card
   and on the guide page alike, the same file in both. The choice pages
   (routing, VST 2), the welcome and the *Coming from…* pages have none:
   the first two are already something to do, the others are reading.
   - **Recorded, not drawn**: each is the real binary on the tour's own
     song, driven by a script, so what it shows is what the user will see.
     The X server's cursor is left out and a clean pointer drawn over the
     frames from the script's own record of where it was, with a ring on
     each click and a key chip for each shortcut.
   - **Animated PNG, not GIF**: the same thing to the reader, without GIF's
     256 colours (the theme's gradients band). The `png` crate already in
     the tree decodes it; each frame stores only what changed, so a clip is
     small.
   - **The tour card goes landscape** when its page has a clip: the clip on
     the left, the words on the right. Stacked, the card would be too tall
     to sit beside the arrangement or the editor without covering them; a
     window too narrow for landscape stacks it.
   - The web manual (step 5's last item) waits for these, and reuses them.
