//! Every plugin this session has open (TDD §8.4).
//!
//! `fontelle-host` knows how to load one; this knows **which ones the document
//! is asking for**, keeps them alive across graph rebuilds, and gives the
//! graph the two shared handles a [`fontelle_engine::PluginNode`] is made
//! from.
//!
//! It lives in `fontelle-app` for the reason [`crate::instrument`] does: it
//! needs the document and the host at once, and neither may depend on the
//! other (§4.1).
//!
//! # The lifetime problem this exists to solve
//!
//! A CLAP plugin may be activated once, and a `PluginInstance` dropped while
//! its audio processor is still out is **leaked** rather than freed on the
//! wrong thread — `clack` makes that choice deliberately and it is the right
//! one. Meanwhile the graph is rebuilt whole on every structural edit, and the
//! new graph is built while the old one is still playing.
//!
//! So a plugin cannot belong to a graph. It belongs here, for as long as the
//! document has a slot for it, and the graph gets a share of two things: the
//! [`ProcessorBay`] its processor travels between graphs in, and the
//! [`ParamValues`] a knob writes on. When the document stops asking for it,
//! the plugin is **retired** rather than dropped, and swept once its processor
//! has come home.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use fontelle_host::{
    Bridges, GuiSize, HostedParam, HostedPlugin, ParamValues, PluginHost, PluginScan, PluginWindow,
    ProcessorBay,
};
use fontelle_model::Project;
use fontelle_types::{ChannelId, MixerTrackId, PluginKey, PluginState};

/// How long a snapshot waits for a playing graph to hand a processor back —
/// see [`PluginRack::snapshot`]. A graph that is being processed answers in
/// a block or two, which is milliseconds; this is for one that is not.
const STATE_RECALL_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(250);

/// Where in the document a plugin sits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PluginSlot {
    /// One insert of one mixer track.
    Insert { track: MixerTrackId, slot: usize },
    /// One channel's instrument.
    Channel(ChannelId),
}

/// What the graph needs to render one plugin: where its processor waits, and
/// where its knobs are written.
#[derive(Clone)]
pub struct PluginWiring {
    pub bay: Arc<ProcessorBay>,
    pub values: Arc<ParamValues>,
    /// The insert's bypass switch, live.
    ///
    /// Shared rather than baked into the node because a bypass is flicked
    /// while listening: `Session::toggle_insert_bypass` writes the document
    /// **and** the running graph, and rebuilding to flick a switch would
    /// reload every soundfont in the project. A plugin instrument's is never
    /// set.
    pub bypassed: Arc<std::sync::atomic::AtomicBool>,
    /// What the plugin says it delays by, in samples — see
    /// [`fontelle_host::HostedPlugin::latency_samples`]. Handed over here
    /// because delay compensation is decided when the graph is built, and
    /// this is the moment the document's slot and the open plugin are both
    /// in front of the same code.
    pub latency: u32,
}

/// One plugin, open.
struct Live {
    plugin: HostedPlugin,
    bay: Arc<ProcessorBay>,
    key: PluginKey,
    bypassed: Arc<std::sync::atomic::AtomicBool>,
    /// What the plugin says each of its values reads as — "2.00x", "-6.0 dB".
    ///
    /// Cached because asking is a main-thread call into the plugin and the
    /// panel is redrawn on every frame that touches it; and because the panel
    /// is built from `&self`, where asking would need `&mut`. Refreshed when a
    /// value changes, which is the only time the answer can.
    displays: HashMap<u32, String>,
    /// The plugin's **own** editor, when one is open — see
    /// [`fontelle_host::gui`].
    ///
    /// It lives here rather than beside the studio's other windows for the
    /// same reason the plugin does: the document decides when a plugin stops
    /// existing, and an editor that outlived its plugin would be a window
    /// drawing into freed memory. `retire` closes it first, which makes that
    /// unexpressible.
    editor: Option<PluginWindow>,
}

impl Live {
    fn refresh_display(&mut self, id: u32) {
        let Some(value) = self.plugin.values().get(id) else {
            return;
        };
        if let Some(text) = self.plugin.display(id, value) {
            self.displays.insert(id, text);
        }
    }

    /// Writes the document's opinion of every parameter onto the plugin.
    ///
    /// **This is INVARIANT 9 reaching a plugin.** An undo changes the
    /// document and nothing else — the plugin is still set the way the
    /// gesture left it — so something has to carry the document's answer back,
    /// and the rebuild that follows every history move is where.
    ///
    /// A parameter the document does not mention goes to the plugin's own
    /// **default**, which is what makes undoing the first touch of a knob put
    /// it back where it started rather than where it happened to be.
    ///
    /// Values that already agree are left alone: a plugin is entitled to treat
    /// an incoming parameter event as a gesture, and a rebuild caused by
    /// something else must not look like two hundred knobs being turned.
    fn apply_params(&mut self, state: &PluginState) {
        let wanted: Vec<(u32, f64)> = self
            .plugin
            .params()
            .iter()
            .map(|param| (param.id, state.param(param.id).unwrap_or(param.default)))
            .collect();
        for (id, value) in wanted {
            if self.plugin.values().get(id) == Some(value) {
                continue;
            }
            self.plugin.set_param(id, value);
            self.refresh_display(id);
        }
    }

    fn refresh_displays(&mut self) {
        let ids: Vec<u32> = self.plugin.params().iter().map(|param| param.id).collect();
        for id in ids {
            self.refresh_display(id);
        }
    }

    /// Drives the plugin's editor for one frame, and answers whether it is
    /// still open.
    ///
    /// **Everything a CLAP editor needs from its host, once per frame.** It
    /// has no thread of its own: it repaints when the timer it registered
    /// fires and it sees a click when the host says its connection is
    /// readable, and both of those are this call. See
    /// [`HostedPlugin::tick_editor`].
    fn tick_editor(&mut self) -> bool {
        let Some(window) = &mut self.editor else {
            return false;
        };
        // Once a second under `FONTELLE_ATOM_TRACE`: whether the processor
        // is out in a graph or sitting in the bay, beside what the editor
        // and the plugin have said to each other. See `fontelle_host::atom_trace`.
        if fontelle_host::atom_trace() {
            static TICKS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let n = TICKS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            if n.is_multiple_of(60) {
                let (to_plugin, to_editor) = self.plugin.editor_traffic();
                eprintln!(
                    "[rack] tick {n}: parked={} active={} to_plugin={to_plugin} to_editor={to_editor}",
                    self.bay.is_parked(),
                    self.plugin.is_active()
                );
            }
        }
        self.plugin.tick_editor();
        let polled = window.poll();
        if let Some(size) = polled.resized {
            self.plugin.resize_editor(size);
        }
        let asked = self.plugin.take_editor_requests();
        if let Some(size) = asked.resize {
            window.resize(size);
            self.plugin.resize_editor(size);
        }
        if polled.closed || asked.closed {
            self.close_editor();
            return false;
        }
        true
    }

    /// Shuts the editor, in the order the specification asks for: the plugin
    /// lets go of the window, and only then is the window destroyed.
    fn close_editor(&mut self) {
        self.plugin.close_editor();
        self.editor = None;
    }

    /// Takes the processor back and stops the plugin, so it can be dropped
    /// without leaking. `false` if the processor is still out in a graph.
    fn retire(&mut self) -> bool {
        // A window belonging to a plugin that is going away goes first —
        // see the field.
        self.close_editor();
        match self.bay.reclaim() {
            Some(processor) => {
                self.plugin.deactivate(processor);
                true
            }
            // A plugin that was never activated has nothing to take back.
            None => !self.plugin.is_active(),
        }
    }
}

/// The session's plugins.
pub struct PluginRack {
    /// The bridges found in Fontelle's own folder at startup — see
    /// `fontelle_host::bridge`. What makes a VST3 hostable on *this* machine
    /// without a line of SDK in this tree.
    bridges: Arc<Bridges>,
    host: PluginHost,
    scan: PluginScan,
    /// Folders to look in beyond the ones the format nominates.
    extra: Vec<PathBuf>,
    /// Whether the folders the formats nominate are searched at all.
    ///
    /// `true` everywhere but a test — see
    /// [`search_standard_folders`](PluginRack::search_standard_folders).
    standard: bool,
    /// Whether the folders have been walked yet.
    ///
    /// The studio walks them **while it opens** (`Session::scan_plugins`), so
    /// in a window this is true before any menu asks. It is still a *once*
    /// rather than an always: a rig that never touches a plugin never pays for
    /// a scan, and the things that need the list — opening a project that has
    /// plugins in it, or dropping the browser — still ask rather than assume.
    scanned: bool,
    live: HashMap<PluginSlot, Live>,
    /// Plugins whose slot has gone, waiting for their processor to come back
    /// from a graph that has not been freed yet.
    retired: Vec<Live>,
    /// What went wrong with the last thing asked of it, for the status line.
    message: Option<String>,
}

impl Default for PluginRack {
    /// Everything empty, and the **standard folders searched** — which is
    /// the one field a `derive` would get wrong, since `false` there is a
    /// studio that finds no plugins at all.
    fn default() -> Self {
        Self {
            bridges: Arc::default(),
            host: PluginHost::default(),
            scan: PluginScan::default(),
            extra: Vec::new(),
            standard: true,
            scanned: false,
            live: HashMap::new(),
            retired: Vec::new(),
            message: None,
        }
    }
}

impl PluginRack {
    pub fn new() -> Self {
        let mut rack = Self::default();
        rack.load_bridges(fontelle_host::bridge_search_paths());
        rack
    }

    fn load_bridges(&mut self, folders: Vec<PathBuf>) {
        let bridges = Bridges::load(&folders);
        // A bridge that would not load is the one somebody just installed,
        // and silence about it is an afternoon lost. The first failure is
        // reported the way a missing plugin is; the rest wait their turn.
        if let Some(failure) = bridges.failures.first() {
            self.message = Some(format!(
                "bridge {} did not load: {}",
                failure.path.display(),
                failure.why
            ));
        }
        self.bridges = Arc::new(bridges);
        self.host = PluginHost::with_bridges(Arc::clone(&self.bridges));
        self.scanned = false;
    }

    /// Loads bridges from `folders` instead of Fontelle's own, and starts
    /// the host over with them. Only while nothing is open: a plugin opened
    /// through a bridge that is then dropped is a plugin with no bridge.
    pub fn set_bridge_folders(&mut self, folders: Vec<PathBuf>) {
        assert!(
            self.live.is_empty(),
            "bridges can only be changed while no plugin is open"
        );
        self.load_bridges(folders);
    }

    /// Whether any plugin is open — the moment an extension may not be
    /// installed or removed, because a bridge dropped under an open plugin
    /// is a plugin with no bridge (`docs/vst-plan.md` §4.2).
    pub fn has_open_plugins(&self) -> bool {
        !self.live.is_empty()
    }

    /// Loads bridges again from Fontelle's own folders — after an extension
    /// was installed or removed. Only legal while nothing is open, which
    /// [`set_bridge_folders`](Self::set_bridge_folders) asserts and the
    /// caller checks first.
    pub fn reload_bridges(&mut self) {
        self.load_bridges(fontelle_host::bridge_search_paths());
    }

    /// What each loaded bridge calls itself.
    pub fn bridges(&self) -> Vec<String> {
        self.bridges.names()
    }

    /// Which folders are searched, the standard ones first — every hosted
    /// format's, then every bridged one's, then the user's own.
    pub fn folders(&self) -> Vec<PathBuf> {
        let mut folders = if self.standard {
            fontelle_host::search_paths_with(&self.bridges)
        } else {
            Vec::new()
        };
        folders.extend(self.extra.iter().cloned());
        folders
    }

    /// Whether to walk the folders the formats nominate at all.
    ///
    /// On for the studio, which has to find what an installer put where it
    /// was told to. **Off for a test**, whose whole claim is usually "the
    /// rack lists what is in the folders it was given" — and which is
    /// otherwise counting whatever this machine happens to have installed.
    /// That went from nothing to three hundred and seventy the day real
    /// plugins were put on this one, and took four tests with it; it is the
    /// same argument `fontelle-testplug` exists for, arriving a second time
    /// from the other end.
    ///
    /// Takes effect on the next [`rescan`](Self::rescan).
    pub fn search_standard_folders(&mut self, search: bool) {
        self.standard = search;
        self.scanned = false;
    }

    /// Adds a folder of the user's own. Takes effect on the next
    /// [`rescan`](Self::rescan).
    pub fn add_folder(&mut self, folder: PathBuf) {
        if folder.is_absolute() && !self.extra.contains(&folder) {
            self.extra.push(folder);
        }
    }

    pub fn set_folders(&mut self, folders: Vec<PathBuf>) {
        self.extra = folders.into_iter().filter(|f| f.is_absolute()).collect();
    }

    pub fn extra_folders(&self) -> &[PathBuf] {
        &self.extra
    }

    /// Walks the folders again.
    ///
    /// Slow — it opens every bundle it finds, which is running somebody else's
    /// code once per file — so it happens where a wait is expected: while the
    /// studio is opening, when somebody presses *Rescan plugins*, or when the
    /// first project that names a plugin needs the list. Never on the way down
    /// of a menu. See [`scan_once`](Self::scan_once).
    pub fn rescan(&mut self) {
        self.scan = PluginScan::of_with(&self.folders(), &self.bridges);
        self.scanned = true;
    }

    /// Walks the folders if nothing has yet.
    pub fn scan_once(&mut self) {
        if !self.scanned {
            self.rescan();
        }
    }

    pub fn scan(&self) -> &PluginScan {
        &self.scan
    }

    pub fn take_message(&mut self) -> Option<String> {
        self.message.take()
    }

    /// A fresh [`PluginState`] naming one of the scanned plugins, by its place
    /// in a list the window was given.
    pub fn state_for(&self, plugin: &fontelle_host::PluginInfo) -> PluginState {
        PluginState::new(plugin.key.clone(), plugin.name.clone())
    }

    /// The plugin open in `slot`, if there is one.
    pub fn plugin(&self, slot: PluginSlot) -> Option<&HostedPlugin> {
        self.live.get(&slot).map(|live| &live.plugin)
    }

    pub fn plugin_mut(&mut self, slot: PluginSlot) -> Option<&mut HostedPlugin> {
        self.live.get_mut(&slot).map(|live| &mut live.plugin)
    }

    /// What the panel draws for `slot`.
    pub fn params(&self, slot: PluginSlot) -> &[HostedParam] {
        self.live
            .get(&slot)
            .map(|live| live.plugin.params())
            .unwrap_or(&[])
    }

    /// Moves one of a plugin's knobs, now, without a graph rebuild.
    ///
    /// The same shape a fader takes: the document is still written by a
    /// command, and this is how the sound moves while the mouse is down.
    pub fn set_param(&mut self, slot: PluginSlot, id: u32, value: f64) -> bool {
        let Some(live) = self.live.get_mut(&slot) else {
            return false;
        };
        if !live.plugin.set_param(id, value) {
            return false;
        }
        live.refresh_display(id);
        true
    }

    /// Switches a plugin insert out of its chain, now, without a rebuild.
    pub fn set_bypassed(&self, slot: PluginSlot, bypassed: bool) {
        if let Some(live) = self.live.get(&slot) {
            live.bypassed
                .store(bypassed, std::sync::atomic::Ordering::Relaxed);
        }
    }

    /// The plugin's own read-out for what a parameter is set to.
    pub fn display(&self, slot: PluginSlot, id: u32) -> Option<&str> {
        self.live.get(&slot)?.displays.get(&id).map(String::as_str)
    }

    /// What a parameter is set to, in the plugin's own units.
    pub fn value(&self, slot: PluginSlot, id: u32) -> Option<f64> {
        self.live.get(&slot)?.plugin.values().get(id)
    }

    /// What the document should store about `slot` right now — parameters and
    /// the plugin's own blob.
    ///
    /// **An LV2 plugin's blob is out with its processor.** Its state lives on
    /// the instance, the instance rides in the processor, and LV2 forbids
    /// reading it while `run` executes — so when the plugin is running this
    /// asks the bay for the processor back ([`ProcessorBay::recall`]), reads
    /// the state with it in hand, and parks it again. A graph that is playing
    /// answers within a block or two; the plugin is silent for those and for
    /// however long the read takes, which for Ctrl+S is a few milliseconds
    /// against a sampler whose file was gone the next time the project
    /// opened. An audio thread that never answers — stopped, or a graph
    /// nothing is processing — is given up on after
    /// [`STATE_RECALL_TIMEOUT`], and the snapshot then carries what the
    /// plugin was last *given*, with a message saying so.
    pub fn snapshot(&mut self, slot: PluginSlot) -> Option<PluginState> {
        let live = self.live.get_mut(&slot)?;
        if !live.plugin.state_needs_processor() {
            return Some(live.plugin.snapshot());
        }
        match live.bay.recall(STATE_RECALL_TIMEOUT) {
            Some(mut processor) => {
                let state = live.plugin.snapshot_with(&mut processor);
                live.bay.park(processor);
                Some(state)
            }
            None => {
                let name = live.plugin.name().to_string();
                let state = live.plugin.snapshot();
                self.message = Some(format!(
                    "{name}'s own state could not be read: the audio thread did not answer"
                ));
                Some(state)
            }
        }
    }

    /// Opens whatever the document asks for, closes whatever it no longer
    /// does, and hands back what the graph needs.
    ///
    /// Called before every rebuild. A plugin already open at the right key is
    /// **left alone** — that is the whole point of this type — so a rebuild
    /// caused by something else does not reload a sampler's gigabyte.
    pub fn realise(
        &mut self,
        project: &Project,
        sample_rate: f64,
        max_block: u32,
    ) -> HashMap<PluginSlot, PluginWiring> {
        let wanted = wanted_slots(project);
        // A project that names plugins is the first thing that needs to know
        // what is installed. One that names none never asks.
        if !wanted.is_empty() {
            self.scan_once();
        }
        self.sweep(&wanted.keys().copied().collect());

        let bypassed = bypassed_slots(project);
        let mut wiring = HashMap::with_capacity(wanted.len());
        for (slot, state) in wanted {
            let Some(live) = self.ensure(slot, &state, sample_rate, max_block) else {
                continue;
            };
            live.bypassed.store(
                bypassed.contains(&slot),
                std::sync::atomic::Ordering::Relaxed,
            );
            wiring.insert(
                slot,
                PluginWiring {
                    bay: Arc::clone(&live.bay),
                    values: Arc::clone(live.plugin.values()),
                    bypassed: Arc::clone(&live.bypassed),
                    latency: live.plugin.latency_samples(),
                },
            );
        }
        wiring
    }

    /// Opens `slot`'s plugin if it is not already the right one.
    fn ensure(
        &mut self,
        slot: PluginSlot,
        state: &PluginState,
        sample_rate: f64,
        max_block: u32,
    ) -> Option<&Live> {
        let matches = self
            .live
            .get(&slot)
            .is_some_and(|live| live.key == state.key);
        if !matches {
            if let Some(mut old) = self.live.remove(&slot)
                && !old.retire()
            {
                self.retired.push(old);
            }
            let Some(found) = self.scan.find(&state.key) else {
                // §17.4's rule for a missing file, applied to a missing
                // plugin: the project opens, the slot is silent, and somebody
                // is told which plugin it wanted rather than left to guess.
                self.message = Some(format!("{} is not installed", state.name));
                return None;
            };
            let path = found.path.clone();
            let mut plugin = match self.host.open(&path, &state.key) {
                Ok(plugin) => plugin,
                Err(e) => {
                    self.message = Some(e.to_string());
                    return None;
                }
            };
            plugin.restore(state);
            let bay = Arc::new(ProcessorBay::new());
            match plugin.activate(sample_rate, max_block) {
                Ok(processor) => bay.park(processor),
                Err(e) => {
                    self.message = Some(e.to_string());
                    return None;
                }
            }
            let mut live = Live {
                plugin,
                bay,
                key: state.key.clone(),
                bypassed: Arc::new(std::sync::atomic::AtomicBool::new(false)),
                displays: HashMap::new(),
                editor: None,
            };
            live.refresh_displays();
            self.live.insert(slot, live);
        }
        if let Some(live) = self.live.get_mut(&slot) {
            // Whatever the document says, every time — see `apply_params`.
            live.apply_params(state);
        }
        self.live.get(&slot)
    }

    /// Closes every plugin, for a run that is over.
    ///
    /// What a headless bounce calls on its way out: its graph has been
    /// dropped on this thread, so every processor is home and every plugin
    /// can be deactivated and freed now rather than leaked at exit. One
    /// whose processor is still out — a graph that has not been dropped —
    /// waits in the retired list the way it would after a rebuild.
    pub fn close_all(&mut self) {
        self.sweep(&HashSet::new());
    }

    /// Retires plugins the document has stopped asking for, and frees the ones
    /// whose processor has come home.
    fn sweep(&mut self, wanted: &HashSet<PluginSlot>) {
        let gone: Vec<PluginSlot> = self
            .live
            .keys()
            .copied()
            .filter(|slot| !wanted.contains(slot))
            .collect();
        for slot in gone {
            if let Some(mut live) = self.live.remove(&slot)
                && !live.retire()
            {
                self.retired.push(live);
            }
        }
        // And the ones already waiting. A processor that is still out belongs
        // to a graph the RT thread has not handed back yet; it will be here
        // next time.
        self.retired.retain_mut(|live| !live.retire());
    }

    /// How many plugins are open, and how many are waiting to be freed. For
    /// tests, and for anything that wants to say so.
    pub fn counts(&self) -> (usize, usize) {
        (self.live.len(), self.retired.len())
    }

    // ------------------------------------------ the plugins' own editors ---

    /// Whether the plugin in `slot` has an editor of its own to show.
    ///
    /// `false` when there is no plugin there, when it offers no GUI, and when
    /// it offers one in a shape this host cannot provide — which is every LV2
    /// and bridged plugin in this build. Those keep the generated panel, which
    /// is what it is for.
    pub fn has_editor(&mut self, slot: PluginSlot) -> bool {
        self.live
            .get_mut(&slot)
            .is_some_and(|live| live.plugin.has_editor())
    }

    /// Whether that slot's editor is open right now.
    pub fn editor_is_open(&self, slot: PluginSlot) -> bool {
        self.live
            .get(&slot)
            .is_some_and(|live| live.editor.is_some())
    }

    /// Opens the plugin's own editor, or raises the one already open.
    ///
    /// `Ok(false)` when the plugin has no editor to show — not an error, and
    /// the caller falls back to Fontelle's panel.
    pub fn open_editor(&mut self, slot: PluginSlot) -> Result<bool, String> {
        let Some(live) = self.live.get_mut(&slot) else {
            return Ok(false);
        };
        if let Some(window) = &mut live.editor {
            // Already open is *raise it*, the same answer the studio's own
            // editor windows give — see `WindowApp::raise_editor`.
            window.raise();
            return Ok(true);
        }
        if !live.plugin.has_editor() {
            return Ok(false);
        }
        let title = live.plugin.name().to_string();
        let window = PluginWindow::open(&title, GuiSize::FALLBACK).map_err(|e| e.to_string())?;
        // The plugin's own idea of how big it should be, asked for while it is
        // being created and applied to the frame afterwards: a window opened
        // at the wrong size and corrected is a window that jumps.
        // The monitor's scale on Windows, where a plugin is told; one on X11,
        // where it reads the desktop's own setting.
        let scale = window.scale();
        match live.plugin.open_editor(&window, scale) {
            Ok(wanted) => {
                let mut window = window;
                window.resize(wanted);
                live.plugin.resize_editor(wanted);
                live.editor = Some(window);
                Ok(true)
            }
            Err(e) => Err(e.to_string()),
        }
    }

    /// Shuts one plugin's editor.
    pub fn close_editor(&mut self, slot: PluginSlot) {
        if let Some(live) = self.live.get_mut(&slot) {
            live.close_editor();
        }
    }

    /// Drives every open editor for one frame, and answers whether any is
    /// still open — which is what tells the window to keep waking up.
    pub fn tick_editors(&mut self) -> bool {
        let mut any = false;
        for live in self.live.values_mut() {
            any |= live.tick_editor();
        }
        any
    }
}

/// How one of a plugin instrument's knobs is addressed inside its channel.
///
/// `patch/plugin/param/<id>`, which is deliberately the same shape a built-in
/// patch's addresses have: `ParamTarget::ChannelPatch` takes anything after
/// `patch/`, and the node reads the plugin's own id off the tail after
/// `/param/` exactly as an insert's does. One rule for both, which is what
/// §8.2 means by not inventing a second scheme.
pub fn param_address(id: u32) -> String {
    format!("patch/plugin/param/{id}")
}

/// The plugin's own parameter id, from the tail of an address.
///
/// One rule for both places a plugin's parameter is named — an insert's
/// `mixer:<t>/insert[0]/param/7` and an instrument's
/// `channel:<c>/patch/plugin/param/7` — and the same rule
/// `fontelle_engine::PluginNode` reads. A bare `7` parses too, which is what a
/// panel hands back when it has already been resolved to a slot.
pub fn param_id(address: &str) -> Option<u32> {
    match address.rsplit_once("/param/") {
        Some((_, id)) => id.parse().ok(),
        None => address.parse().ok(),
    }
}

/// Every plugin slot the document has switched out of its chain.
fn bypassed_slots(project: &Project) -> HashSet<PluginSlot> {
    let mut off = HashSet::new();
    for (track, strip) in project.mixer.tracks.iter() {
        for (slot, insert) in strip.inserts.iter().enumerate() {
            if insert.is_plugin() && insert.bypassed {
                off.insert(PluginSlot::Insert { track, slot });
            }
        }
    }
    off
}

/// Every slot in `project` that holds a plugin.
pub fn plugin_slots(project: &Project) -> Vec<PluginSlot> {
    let mut slots: Vec<PluginSlot> = wanted_slots(project).into_keys().collect();
    // A stable order, so anything that walks them twice walks them the same
    // way — a `HashMap`'s is not.
    slots.sort_by_key(|slot| match slot {
        PluginSlot::Channel(_) => (0usize, 0usize),
        PluginSlot::Insert { slot, .. } => (1, *slot),
    });
    slots
}

/// Every plugin the document is asking for, and what it should be set to.
fn wanted_slots(project: &Project) -> HashMap<PluginSlot, PluginState> {
    let mut wanted = HashMap::new();
    for (id, channel) in project.channels.iter() {
        if channel.instrument == Some(fontelle_types::InstrumentKind::Plugin)
            && let Some(state) = &channel.plugin
        {
            wanted.insert(PluginSlot::Channel(id), state.clone());
        }
    }
    for (track, strip) in project.mixer.tracks.iter() {
        for (slot, insert) in strip.inserts.iter().enumerate() {
            if let Some(state) = &insert.plugin {
                wanted.insert(PluginSlot::Insert { track, slot }, state.clone());
            }
        }
    }
    wanted
}
