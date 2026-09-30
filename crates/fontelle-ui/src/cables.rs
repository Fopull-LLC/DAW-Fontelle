//! The mixer's patch cables, hung like real ones.
//!
//! > *"right now theres no visual indicator at a glance for mixer track
//! > routing ... there should be wires connecting tracks showing where theyre
//! > routing to and the wires should have nice physicsy looking animations so
//! > as i reorder mixer tracks or add or change wirings they will kind of
//! > snap together and jiggle around like real wires"*
//!
//! Each cable is a rope of [`SEGMENTS`] pieces, integrated by Verlet with its
//! lengths held by relaxation — the textbook rope, because a rope is exactly
//! what is being drawn. The two ends are *pinned*: the plug in the source's
//! output jack, and the plug in the target's input. A pinned end goes where
//! its jack goes at once (the jack is part of the strip), and the rope, which
//! has mass, catches up — that lag is the jiggle.
//!
//! Pure, like `motion.rs`: the window hands in a `dt`, asks where the points
//! are, and asks [`Cables::is_moving`] when it decides whether it may sleep
//! (§16.3). A cable that has come to rest stops being integrated at all
//! rather than shivering at a tenth of a pixel forever.

use std::hash::Hash;

/// A point in window pixels.
pub type Pt = [f32; 2];

/// How many pieces a cable is made of. Enough for a smooth curve through the
/// points and a swing that reads as a rope; few enough that a hundred cables
/// cost nothing.
pub const SEGMENTS: usize = 14;

/// How long a new plug takes to travel from where it starts to its jack.
///
/// Long enough to be *seen* going somewhere — the point is that a routing
/// just made shows where it went — and short enough not to be waited for.
pub const PLUG_SECONDS: f32 = 0.3;

/// How long a cable plugged in at one end only is: it hangs, rather than
/// reaching anywhere.
pub const LOOSE_LENGTH: f32 = 40.0;

/// How long a removed cable takes to fade once it has been let go.
pub const FADE_SECONDS: f32 = 0.35;

/// How big the knob on a send's cable is, and so how near a press must land.
pub const KNOB_RADIUS: f32 = 7.0;

/// The substep. Fixed, so a slow frame is more steps rather than one big one
/// — a rope integrated over a quarter of a second in one step explodes.
const STEP: f32 = 1.0 / 240.0;

/// The most time one call is allowed to simulate. A window that stalled for a
/// second resumes where it was rather than spending the next frame catching
/// up on a second of physics nobody saw.
const MAX_DT: f32 = 0.05;

/// Pixels per second squared. Heavier than the world's: the bay is forty
/// pixels deep, and a cable that fell at 9.8 m/s² scaled to a screen would
/// take a visible age to hang.
const GRAVITY: f32 = 2200.0;

/// Velocity kept per substep. At 240 steps a second this loses most of a
/// swing in about a second — a cable, not a spring.
const DAMPING: f32 = 0.986;

/// How many times a step pulls every piece back to its length.
const ITERATIONS: usize = 20;

/// A point slower than this, in pixels a second, is at rest.
const REST_SPEED: f32 = 6.0;

/// And a cable is asleep once every point has been at rest this long — a
/// swing through its turning point is momentarily slow, not stopped.
const QUIET_SECONDS: f32 = 0.25;

/// How quickly a cable's length reaches what it wants, as a time constant.
const LENGTH_SECONDS: f32 = 0.08;

/// How far down a plug dips on its way to a jack, at most. A plug carried
/// across the bay swings low rather than sliding along a ruler.
const PLUG_DIP: f32 = 14.0;

/// What a cable is: a track's output, or one of its sends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CableRole {
    Output,
    /// The send's place in the track's list.
    Send(usize),
}

/// Which cable this is, by the **track's** identity rather than its place in
/// the mixer — so a strip that moves takes its cables with it, and the rope
/// is seen to follow, rather than every cable after it quietly becoming a
/// different one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CableKey<T> {
    pub track: T,
    pub role: CableRole,
}

/// Where a cable should be: what the mixer's routing says, in pixels.
#[derive(Debug, Clone, PartialEq)]
pub struct CableSpec<T> {
    pub key: CableKey<T>,
    /// The source's output jack.
    pub from: Pt,
    /// The target's input jack, and which track that is. `None` is a cable
    /// plugged in at the source only — an output switched off.
    ///
    /// The track is how a *reroute* is told from a strip moving: the same
    /// target somewhere new keeps its plug in, a different target unplugs it.
    pub to: Option<(Pt, T)>,
    pub color: [u8; 4],
    /// Drawn bright: it belongs to the selected track, or arrives at it.
    pub lit: bool,
    /// A send's level, and so its knob. `None` for an output, whose level is
    /// the fader.
    pub level_db: Option<f32>,
}

/// One cable as it is now, for drawing.
#[derive(Debug, Clone, PartialEq)]
pub struct CableLine<T> {
    pub key: CableKey<T>,
    /// From the source's plug to the far end, [`SEGMENTS`]` + 1` of them.
    pub points: Vec<Pt>,
    pub color: [u8; 4],
    pub lit: bool,
    /// 1.0, and falling to nothing once the cable has been removed.
    pub alpha: f32,
    pub level_db: Option<f32>,
    /// Where the send's knob is — on the wire, at its middle, so it moves
    /// with it.
    pub knob: Option<Pt>,
    /// Whether the far end is in a jack. A loose end is drawn as a bare plug.
    pub plugged: bool,
}

/// What the far end is doing.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Far {
    /// In its jack.
    Plugged,
    /// On its way to it: from where, and how far through.
    Plugging { from: Pt, t: f32 },
    /// Hanging free.
    Loose,
}

#[derive(Debug, Clone)]
struct Cable<T> {
    spec: CableSpec<T>,
    pos: Vec<Pt>,
    prev: Vec<Pt>,
    /// One piece's length, now.
    piece: f32,
    /// And what it is on its way to. A cable's length is eased, never
    /// jumped: pulled out of a jack, a cable that became forty pixels long in
    /// one step was a rope yanked straight by an invisible hand.
    want: f32,
    far: Far,
    /// Seconds since it was removed, while it falls and fades.
    dying: Option<f32>,
}

/// Every cable in the bay.
#[derive(Debug, Clone)]
pub struct Cables<T> {
    cables: Vec<Cable<T>>,
    floor: f32,
    /// Simulated time not yet spent, below one substep.
    carry: f32,
    /// How long every point has been at rest.
    quiet: f32,
}

impl<T: Copy + Eq + Hash> Default for Cables<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Copy + Eq + Hash> Cables<T> {
    pub fn new() -> Self {
        Self {
            cables: Vec::new(),
            floor: f32::INFINITY,
            carry: 0.0,
            quiet: QUIET_SECONDS,
        }
    }

    /// Brings the bay to what the routing now is. Called when the mixer's
    /// lists or its layout change, not every frame — though calling it with
    /// the same cables again is harmless and wakes nothing.
    pub fn sync(&mut self, specs: Vec<CableSpec<T>>, floor: f32) {
        let mut woke = floor != self.floor && self.floor.is_finite();
        self.floor = floor;
        // **A whole new set is a new mixer**, not a routing somebody just
        // made: the window lays the mixer out empty before it has read the
        // project, and opening another replaces every track at once. It
        // arrives already hung, and what it replaces is simply gone. A cable
        // plugs itself in only when it joins cables that are staying.
        let fresh = !specs.is_empty()
            && !self
                .cables
                .iter()
                .any(|c| c.dying.is_none() && specs.iter().any(|s| s.key == c.spec.key));
        if fresh {
            self.cables.clear();
        }
        let mut old: Vec<Option<Cable<T>>> = std::mem::take(&mut self.cables)
            .into_iter()
            .map(Some)
            .collect();
        let mut out = Vec::with_capacity(specs.len());

        for spec in specs {
            let found = old.iter().position(|c| {
                c.as_ref()
                    .is_some_and(|c| c.dying.is_none() && c.spec.key == spec.key)
            });
            let cable = match found.and_then(|i| old[i].take()) {
                Some(mut cable) => {
                    woke |= cable.retarget(&spec, floor);
                    cable.spec = spec;
                    cable
                }
                None => {
                    woke = true;
                    Cable::new(spec, floor, !fresh)
                }
            };
            out.push(cable);
        }
        // What is left was removed: it lets go of its far end and falls.
        for mut cable in old.into_iter().flatten() {
            if cable.dying.is_none() {
                cable.dying = Some(0.0);
                cable.far = Far::Loose;
                woke = true;
            }
            out.push(cable);
        }
        self.cables = out;
        if woke {
            self.quiet = 0.0;
        }
    }

    /// Moves everything on by `dt` seconds.
    pub fn step(&mut self, dt: f32) {
        if !self.is_moving() {
            return;
        }
        let dt = dt.clamp(0.0, MAX_DT);
        self.carry += dt;
        let mut fastest: f32 = 0.0;
        while self.carry >= STEP {
            self.carry -= STEP;
            fastest = 0.0;
            for cable in &mut self.cables {
                fastest = fastest.max(cable.substep(self.floor));
            }
        }
        for cable in &mut self.cables {
            if let Some(age) = &mut cable.dying {
                *age += dt;
            }
        }
        self.cables
            .retain(|c| c.dying.is_none_or(|age| age < FADE_SECONDS));
        let travelling = self.cables.iter().any(|c| {
            c.dying.is_some()
                || matches!(c.far, Far::Plugging { .. })
                || (c.piece - c.want).abs() > 0.01
        });
        if travelling || fastest / STEP > REST_SPEED {
            self.quiet = 0.0;
        } else {
            self.quiet += dt;
        }
        if !self.is_moving() {
            // Asleep: whatever velocity is left is below what anyone can see,
            // and must not come back as a twitch when something wakes it.
            for cable in &mut self.cables {
                cable.prev.clone_from(&cable.pos);
            }
        }
    }

    /// Whether anything will look different next frame. The window holds an
    /// animator on the tree while this is true (§16.3).
    pub fn is_moving(&self) -> bool {
        self.quiet < QUIET_SECONDS
    }

    /// Every cable as it is now, in the order the routes were given, with the
    /// ones falling away last.
    pub fn lines(&self) -> Vec<CableLine<T>> {
        self.cables
            .iter()
            .map(|c| CableLine {
                key: c.spec.key,
                points: c.pos.clone(),
                color: c.spec.color,
                lit: c.spec.lit,
                alpha: c
                    .dying
                    .map_or(1.0, |age| (1.0 - age / FADE_SECONDS).clamp(0.0, 1.0)),
                level_db: c.spec.level_db,
                knob: c.knob(self.floor),
                plugged: matches!(c.far, Far::Plugged),
            })
            .collect()
    }

    /// The send whose knob is under `(x, y)`, the topmost first — the last
    /// drawn.
    pub fn knob_at(&self, x: f32, y: f32) -> Option<CableKey<T>> {
        self.cables.iter().rev().find_map(|c| {
            let knob = c.knob(self.floor)?;
            ((knob[0] - x).hypot(knob[1] - y) <= KNOB_RADIUS + 2.0).then_some(c.spec.key)
        })
    }
}

impl<T: Copy + Eq> Cable<T> {
    /// A cable arriving: hanging in place already, or — `plug_in` — bunched
    /// at its source with its plug on the way to the jack.
    fn new(spec: CableSpec<T>, floor: f32, plug_in: bool) -> Self {
        let from = spec.from;
        let (pos, far) = match (spec.to, plug_in) {
            (Some((to, _)), false) => {
                let sag = sag_for(from, to, floor);
                let pos: Vec<Pt> = (0..=SEGMENTS)
                    .map(|i| {
                        let t = i as f32 / SEGMENTS as f32;
                        let x = from[0] + (to[0] - from[0]) * t;
                        let y = from[1] + (to[1] - from[1]) * t + 4.0 * sag * t * (1.0 - t);
                        [x, y.min(floor)]
                    })
                    .collect();
                (pos, Far::Plugged)
            }
            // A small loop hanging from the source, plug and all: a rope
            // that starts as one point has no shape to swing from, and
            // unfolded as a scribble.
            (Some(_), true) => {
                let depth = (LOOSE_LENGTH / 2.0).min((floor - from[1]).max(0.0));
                let pos = (0..=SEGMENTS)
                    .map(|i| {
                        let arc = std::f32::consts::PI * i as f32 / SEGMENTS as f32;
                        [from[0] + 4.0 * arc.sin(), from[1] + depth * arc.sin()]
                    })
                    .collect();
                (pos, Far::Plugging { from, t: 0.0 })
            }
            (None, _) => {
                let piece = LOOSE_LENGTH / SEGMENTS as f32;
                let pos = (0..=SEGMENTS)
                    .map(|i| [from[0], (from[1] + piece * i as f32).min(floor)])
                    .collect();
                (pos, Far::Loose)
            }
        };
        // What it starts as is what it was spawned as; what it wants is what
        // its jacks ask for. The same for a cable hung in place.
        let piece = pos
            .windows(2)
            .map(|w| (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1]))
            .sum::<f32>()
            / SEGMENTS as f32;
        let want = measure(spec.from, spec.to, far, floor) / SEGMENTS as f32;
        Self {
            prev: pos.clone(),
            pos,
            piece: if plug_in { piece } else { want },
            want,
            far,
            dying: None,
            spec,
        }
    }

    /// Points the cable at `spec`'s jacks. Returns whether anything moved.
    fn retarget(&mut self, spec: &CableSpec<T>, floor: f32) -> bool {
        let last = SEGMENTS;
        let mut moved = spec.from != self.spec.from;
        self.pos[0] = spec.from;
        self.prev[0] = spec.from;
        let same_socket = match (self.spec.to, spec.to) {
            (Some((_, a)), Some((_, b))) => a == b,
            (None, None) => true,
            _ => false,
        };
        match spec.to {
            Some((to, _)) if same_socket => {
                // The same jack, perhaps somewhere new: a plug that is in
                // stays in and goes with it; one on its way keeps going.
                moved |= self.spec.to.is_some_and(|(was, _)| was != to);
                if matches!(self.far, Far::Plugged) {
                    self.pos[last] = to;
                    self.prev[last] = to;
                }
            }
            Some(_) => {
                // Somewhere else: out of the old jack, off to the new one.
                self.far = Far::Plugging {
                    from: self.pos[last],
                    t: 0.0,
                };
                moved = true;
            }
            None => {
                moved |= !matches!(self.far, Far::Loose);
                self.far = Far::Loose;
            }
        }
        let want = measure(spec.from, spec.to, self.far, floor) / SEGMENTS as f32;
        moved |= (want - self.want).abs() > 1e-3;
        self.want = want;
        moved
    }

    /// One substep. Returns how far the fastest free point moved.
    fn substep(&mut self, floor: f32) -> f32 {
        let last = SEGMENTS;
        let free_end = matches!(self.far, Far::Loose);

        // The far plug's place, if something other than the rope decides it.
        let pinned_far = match (&mut self.far, self.spec.to) {
            (Far::Plugged, Some((to, _))) => Some(to),
            (Far::Plugging { from, t }, Some((to, _))) => {
                *t += STEP / PLUG_SECONDS;
                if *t >= 1.0 {
                    self.far = Far::Plugged;
                    Some(to)
                } else {
                    let s = *t * *t * (3.0 - 2.0 * *t);
                    let dip = PLUG_DIP.min((floor - to[1].max(from[1])).max(0.0))
                        * (std::f32::consts::PI * s).sin();
                    Some([
                        from[0] + (to[0] - from[0]) * s,
                        from[1] + (to[1] - from[1]) * s + dip,
                    ])
                }
            }
            _ => None,
        };

        // The length, eased towards what it wants — and while a plug is
        // travelling, never shorter than what reaches it, or the cable goes
        // ruler-straight behind a plug being carried away from its source.
        let mut want = self.want;
        if let (Far::Plugging { .. }, Some(plug)) = (self.far, pinned_far) {
            let from = self.spec.from;
            let reach = (plug[0] - from[0]).hypot(plug[1] - from[1]) * 1.1 + 12.0;
            want = want.max(reach / SEGMENTS as f32);
        }
        self.piece += (want - self.piece) * (1.0 - (-STEP / LENGTH_SECONDS).exp());

        // Integrate everything that is free to move.
        let end = if free_end { last } else { last - 1 };
        for i in 1..=end {
            let p = self.pos[i];
            let q = self.prev[i];
            let v = [(p[0] - q[0]) * DAMPING, (p[1] - q[1]) * DAMPING];
            self.prev[i] = p;
            self.pos[i] = [p[0] + v[0], p[1] + v[1] + GRAVITY * STEP * STEP];
        }
        self.pos[0] = self.spec.from;
        if let Some(far) = pinned_far {
            self.pos[last] = far;
            self.prev[last] = far;
        }

        // Hold the pieces to their length. The plugs do not give; the rope
        // does.
        let fixed = |i: usize| i == 0 || (i == last && !free_end);
        for _ in 0..ITERATIONS {
            for i in 0..last {
                let (a, b) = (self.pos[i], self.pos[i + 1]);
                let d = [b[0] - a[0], b[1] - a[1]];
                let len = d[0].hypot(d[1]);
                if len < 1e-6 {
                    continue;
                }
                let error = (len - self.piece) / len;
                let (wa, wb) = match (fixed(i), fixed(i + 1)) {
                    (true, true) => continue,
                    (true, false) => (0.0, 1.0),
                    (false, true) => (1.0, 0.0),
                    (false, false) => (0.5, 0.5),
                };
                self.pos[i][0] += d[0] * error * wa;
                self.pos[i][1] += d[1] * error * wa;
                self.pos[i + 1][0] -= d[0] * error * wb;
                self.pos[i + 1][1] -= d[1] * error * wb;
            }
        }

        // The floor of the bay, with a little friction: a cable lying on it
        // stays where it lies rather than sliding about.
        let mut fastest: f32 = 0.0;
        for i in 1..=end {
            if self.pos[i][1] >= floor {
                self.pos[i][1] = floor;
                self.prev[i][1] = self.prev[i][1].min(floor);
                self.prev[i][0] = self.pos[i][0] - (self.pos[i][0] - self.prev[i][0]) * 0.5;
            }
            let (p, q) = (self.pos[i], self.prev[i]);
            fastest = fastest.max((p[0] - q[0]).hypot(p[1] - q[1]));
        }
        fastest
    }

    /// Over the middle of the wire — and never lower than a whole knob
    /// above the floor: a long send lies on the floor of the bay, and a knob
    /// drawn there was cut in half by its bottom edge.
    fn knob(&self, floor: f32) -> Option<Pt> {
        (self.spec.level_db.is_some() && self.dying.is_none()).then(|| {
            let [x, y] = self.pos[SEGMENTS / 2];
            [x, y.min(floor - KNOB_RADIUS)]
        })
    }
}

/// How long a cable is: enough to hang [`sag_for`] below its plugs, or
/// [`LOOSE_LENGTH`] with nothing at the far end.
fn measure<T>(from: Pt, to: Option<(Pt, T)>, far: Far, floor: f32) -> f32 {
    match to {
        Some((to, _)) if far != Far::Loose => {
            let d = (to[0] - from[0]).hypot(to[1] - from[1]);
            let sag = sag_for(from, to, floor);
            // A parabola's arc length, near enough for a cable.
            if d < 1.0 {
                2.0 * sag
            } else {
                d + 8.0 * sag * sag / (3.0 * d)
            }
        }
        _ => LOOSE_LENGTH,
    }
}

/// How far below its plugs a cable between `a` and `b` hangs.
///
/// Deeper the further it reaches, as a real one does — which is also what
/// keeps two cables from different distances from lying on each other — and
/// never through the floor.
fn sag_for(a: Pt, b: Pt, floor: f32) -> f32 {
    let d = (b[0] - a[0]).hypot(b[1] - a[1]);
    let room = (floor - a[1].max(b[1])).max(0.0);
    (0.12 * d + 8.0).min(room * 0.85)
}
