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

### 6. The settings page is redesigned

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
