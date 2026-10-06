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
    Bridges, GuiSize, HostedParam, HostedPlugin, OwnPreset, ParamValues, PluginHost, PluginScan,
    PluginWindow, ProcessorBay,
};
use fontelle_model::Project;
use fontelle_types::{ChannelId, MixerTrackId, PluginKey, PluginState};

/// How long a snapshot waits for a playing graph to hand a processor back —
/// see [`PluginRack::snapshot`]. A graph that is being processed answers in
/// a block or two, which is milliseconds; this is for one that is not.
const STATE_RECALL_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(250);

/// Asks a plugin's processor home, waking a graph that is asleep for as long
/// as the wait lasts — see [`fontelle_engine::Transport::summon`].
///
/// `None` when nobody answers: no audio device at all, or one whose callback
/// has stopped.
fn recall_home(
    bay: &ProcessorBay,
    transport: Option<&Arc<fontelle_engine::Transport>>,
    rendering: bool,
) -> Option<fontelle_host::HostedProcessor> {
    // A render has it, and taking it back would leave a hole in the file.
    if rendering {
        return None;
    }
    if let Some(transport) = transport {
        transport.summon();
    }
    let processor = bay.recall(STATE_RECALL_TIMEOUT);
    if let Some(transport) = transport {
        transport.dismiss();
    }
    processor
}

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
    /// The plugin's own state as the **document** held it the last time
    /// this rack looked — so a rebuild can tell a document whose state is new
    /// (a preset was loaded, an undo put an old one back) from one whose copy
    /// is merely old. See [`PluginRack::ensure`].
    blob: Option<String>,
    /// The parameter values the **document** held the last time this rack
    /// looked — so a rebuild can tell a value the document changed (a knob
    /// on the studio's panel, an undo) from one that is merely old. See
    /// [`Live::apply_params`].
    seen: HashMap<u32, f64>,
    /// Presses on the editor's strip since the session last asked.
    header_presses: Vec<(i32, i32)>,
    /// Where the pointer is over the strip.
    header_hover: Option<(i32, i32)>,
    /// Spaces the editor's window heard that the plugin did not take — the
    /// studio's play and stop (`fontelle_host::GuiPoll::play_pause`).
    play_pause: u32,
}

/// The document's parameter list for a plugin, by id.
fn seen_params(state: &PluginState) -> HashMap<u32, f64> {
    state.params.iter().map(|p| (p.id, p.value)).collect()
}

impl Live {
    /// The document holds `state`, and the plugin already is it.
    fn keep(&mut self, state: &PluginState) {
        if state.blob.is_some() {
            self.blob = state.blob.clone();
        }
        self.seen = seen_params(state);
    }

    fn refresh_display(&mut self, id: u32) {
        let Some(value) = self.plugin.values().get(id) else {
            return;
        };
        if let Some(text) = self.plugin.display(id, value) {
            self.displays.insert(id, text);
        }
    }

    /// Carries what the document **changed its mind about** onto the plugin.
    ///
    /// **This is INVARIANT 9 reaching a plugin.** An undo changes the
    /// document and nothing else — the plugin is still set the way the
    /// gesture left it — so something has to carry the document's answer back,
    /// and the rebuild that follows every history move is where.
    ///
    /// > *"sometimes they'll just revert back to the init preset when working
    /// > on a saved project"*
    ///
    /// **Only what the document changed** since this rack last looked
    /// ([`Live::seen`]). This used to write the document's whole list every
    /// time, and the document's list is as old as the last save: a knob
    /// turned in the plugin's own window, or a preset picked in its own
    /// browser, was put back by the next rebuild for anything at all — an
    /// undo of a note, a channel added. The same rule as the blob
    /// ([`PluginRack::ensure`]): a document that says what it said before has
    /// nothing new to say, and what the plugin did since is newer.
    ///
    /// A parameter the document **stopped** mentioning goes to the plugin's
    /// own default, which is what makes undoing the first touch of a knob put
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
            .filter(|param| state.param(param.id) != self.seen.get(&param.id).copied())
            .map(|param| (param.id, state.param(param.id).unwrap_or(param.default)))
            .collect();
        for (id, value) in wanted {
            if self.plugin.values().get(id) == Some(value) {
                continue;
            }
            self.plugin.set_param(id, value);
            self.refresh_display(id);
        }
        self.seen = seen_params(state);
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
        self.header_presses
            .extend(polled.header_presses.iter().copied());
        self.play_pause += polled.play_pause;
        if let Some(hover) = polled.header_hover {
            self.header_hover = hover;
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
        self.header_presses.clear();
        self.header_hover = None;
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

/// One open plugin editor's strip, as the rack knows it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EditorHeader {
    pub slot: PluginSlot,
    /// The strip's size, in the window's own pixels.
    pub width: u32,
    pub height: u32,
    /// The window's pixels per logical one.
    pub scale: f32,
    /// Where the pointer is over the strip.
    pub hover: Option<(i32, i32)>,
}

/// One plugin's own library: still being listed, or listed.
enum Library {
    Listing(std::sync::mpsc::Receiver<Vec<OwnPreset>>),
    Ready(Vec<OwnPreset>),
}

/// The session's plugins.
pub struct PluginRack {
    /// A plugin not opened whatever the document says — see
    /// [`hold_back`](PluginRack::hold_back).
    held_back: Option<fontelle_types::PluginKey>,
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
    /// Each plugin's own library — see [`take_libraries`](Self::take_libraries).
    /// By plugin rather than by slot: two slots holding the same plugin have
    /// one library, listed once.
    libraries: HashMap<PluginKey, Library>,
    /// Each plugin's **programs** — the presets compiled into it, which only
    /// an instance can list (`HostedPlugin::programs`). Read on the main
    /// thread when the plugin is opened, and again whenever an open one says
    /// they may be others; shown after its library.
    ///
    /// By plugin, like the library: two instances of Dexed holding different
    /// cartridges show the names of whichever said so last.
    programs: HashMap<PluginKey, Vec<OwnPreset>>,
    /// Libraries listed and not yet handed to the preset bank.
    fresh: Vec<PluginKey>,
    /// Where a plugin's library is looked for beyond what it lists itself.
    preset_roots: fontelle_host::PresetRoots,
    /// How tall the strip across the top of a plugin's own editor is, in
    /// pixels — the studio's preset bar, drawn there. Zero: no strip, which
    /// is what a rack with no studio (a bounce, a test of the rack) has.
    editor_header: u32,
    /// Whether an editor opens on no screen — for a test of the studio's
    /// side of a plugin's window, on a machine with no display.
    headless_editors: bool,
    /// Plugins whose slot has gone, waiting for their processor to come back
    /// from a graph that has not been freed yet.
    retired: Vec<Live>,
    /// What went wrong with the last thing asked of it, for the status line.
    message: Option<String>,
    /// The transport the device's callback is driven by, when there is one —
    /// so a wait on the audio thread can [summon] a graph that is asleep.
    ///
    /// [summon]: fontelle_engine::Transport::summon
    transport: Option<Arc<fontelle_engine::Transport>>,
    /// What reads a bundle that has to be loaded to be read, in a child
    /// process — see [`fontelle_host::BundleProber`]. `None` reads in this
    /// process, which is a test's rack and nobody else's.
    prober: Option<Arc<fontelle_host::BundleProber>>,
    /// Whether a render is playing these plugins — see
    /// [`lend_for_render`](Self::lend_for_render). Nothing asks for a
    /// processor back while it is.
    rendering: bool,
    /// Asked before a plugin's own editor is opened: one that would abort
    /// the studio on this machine's EGL is refused, and its knobs are in the
    /// panel (`fontelle_host::alpha_egl`). Off for a rack with no studio.
    editor_gate: fontelle_host::EditorGate,
    /// The name of the plugin whose editor [`editor_gate`](Self::editor_gate)
    /// last refused, until the session asks — what decides whether to offer
    /// Compatible plugin graphics.
    gate_refused: Option<String>,
}

impl Default for PluginRack {
    /// Everything empty, and the **standard folders searched** — which is
    /// the one field a `derive` would get wrong, since `false` there is a
    /// studio that finds no plugins at all.
    fn default() -> Self {
        Self {
            held_back: None,
            bridges: Arc::default(),
            host: PluginHost::default(),
            scan: PluginScan::default(),
            extra: Vec::new(),
            standard: true,
            scanned: false,
            live: HashMap::new(),
            libraries: HashMap::new(),
            programs: HashMap::new(),
            fresh: Vec::new(),
            preset_roots: fontelle_host::PresetRoots::standard(),
            editor_header: 0,
            headless_editors: false,
            retired: Vec::new(),
            message: None,
            transport: None,
            prober: None,
            rendering: false,
            editor_gate: fontelle_host::EditorGate::off(),
            gate_refused: None,
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
        self.scan = match &self.prober {
            Some(prober) => {
                let scan = PluginScan::of_probed(&self.folders(), &self.bridges, prober);
                prober.save();
                scan
            }
            None => PluginScan::of_with(&self.folders(), &self.bridges),
        };
        self.scanned = true;
    }

    /// [`rescan`](Self::rescan), trying again every bundle that could not be
    /// read last time — what *Rescan plugins* is for. A bundle that was read
    /// and has not changed is not read again.
    pub fn rescan_fresh(&mut self) {
        if let Some(prober) = &self.prober {
            prober.forget_failures();
        }
        self.rescan();
    }

    /// Reads bundles that have to be loaded to be read in child processes —
    /// see the field. The studio's rack and a bounce's are given one.
    pub fn set_prober(&mut self, prober: Option<Arc<fontelle_host::BundleProber>>) {
        self.prober = prober;
        self.scanned = false;
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

    /// The bay a slot's processor is parked in, while the slot is open —
    /// what the graph's node shares with the rack.
    pub fn bay(&self, slot: PluginSlot) -> Option<Arc<ProcessorBay>> {
        self.live.get(&slot).map(|live| Arc::clone(&live.bay))
    }

    /// The plugins whose node silenced something they played that was not a
    /// number since this was last asked, with their names — see
    /// `fontelle_engine::PluginNode` and `Session::deal_with_silenced`.
    pub fn take_silenced(&mut self) -> Vec<(PluginSlot, String)> {
        self.live
            .iter()
            .filter(|(_, live)| live.bay.take_silenced())
            .map(|(slot, live)| (*slot, live.plugin.name().to_string()))
            .collect()
    }

    /// Lets go of a slot's plugin, so the next realise opens it again from
    /// the document's copy of its state.
    pub fn reopen(&mut self, slot: PluginSlot) {
        if let Some(mut old) = self.live.remove(&slot)
            && !old.retire()
        {
            self.retired.push(old);
        }
    }

    /// Whether a render has the plugins — see [`lend_for_render`](Self::lend_for_render).
    pub fn is_rendering(&self) -> bool {
        self.rendering
    }

    /// A plugin not to open, whatever the document says — the one a crash
    /// named. See `Session::with_held_back`.
    pub fn hold_back(&mut self, key: Option<fontelle_types::PluginKey>) {
        self.held_back = key;
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
        // The command beside this call wrote the same value to the
        // document, and no rebuild follows to show it to `apply_params` —
        // which then has to know, or the undo of this very turn would look
        // like a document that never said anything.
        live.seen.insert(id, value);
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

    /// Lends the studio's own plugins to a render: the graph a render builds
    /// with this wiring plays **these** instances.
    ///
    /// > *"most times it just renders with nothing"* — and a sweep of every
    /// > installed instrument, in which padthv1 died when a render made a
    /// > second instance of it beside the one that was playing.
    ///
    /// The live graph is held (`Transport::hold`) and its processors come
    /// home to their bays, where the render's nodes take them; nothing asks
    /// for one back until [`end_render`](Self::end_render). No plugin is
    /// opened twice — some do not survive it, a sampler would load its
    /// gigabytes twice, some are licensed per instance — and what renders is
    /// exactly what was playing.
    pub fn lend_for_render(
        &mut self,
        project: &Project,
        sample_rate: f64,
        max_block: u32,
    ) -> HashMap<PluginSlot, PluginWiring> {
        let wiring = self.realise(project, sample_rate, max_block);
        if let Some(transport) = &self.transport {
            transport.hold();
        }
        self.rendering = true;
        // Told before the render starts — see `HostedPlugin::set_offline`.
        for live in self.live.values_mut() {
            live.plugin.set_offline(true);
        }
        // The callback parks the live graph's processors on its next block;
        // with no callback running they are home already.
        let deadline = std::time::Instant::now() + STATE_RECALL_TIMEOUT;
        for live in self.live.values() {
            while !live.bay.is_parked() && std::time::Instant::now() < deadline {
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
        }
        wiring
    }

    /// The render is over and its graph gone: the live graph plays these
    /// plugins again.
    pub fn end_render(&mut self) {
        if self.rendering {
            self.rendering = false;
            for live in self.live.values_mut() {
                live.plugin.set_offline(false);
            }
            if let Some(transport) = &self.transport {
                transport.release();
            }
        }
    }

    /// Hands the rack the transport the callback is driven by. See the field.
    pub fn set_transport(&mut self, transport: Arc<fontelle_engine::Transport>) {
        self.transport = Some(transport);
    }

    /// Says the document now holds `state` for `slot`, **read off the
    /// plugin** — a save, a capture before a preset goes over it.
    ///
    /// Reading a plugin's state is not the document taking it, so
    /// [`snapshot`](Self::snapshot) changes nothing about what a rebuild
    /// compares against; whoever writes the snapshot into the document says
    /// so here. Without it the next rebuild takes the document's fresh copy
    /// for news and loads it back into the plugin, over whatever its own
    /// window did since it was read.
    pub fn kept(&mut self, slot: PluginSlot, state: &PluginState) {
        if let Some(live) = self.live.get_mut(&slot)
            && live.key == state.key
        {
            live.keep(state);
        }
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
            let state = live.plugin.snapshot();
            return Some(state);
        }
        match recall_home(&live.bay, self.transport.as_ref(), self.rendering) {
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
            // The plugin the last crash named (`Session::with_held_back`):
            // not run, and said why. The document keeps it, so a save keeps it
            // and the next opening tries it again.
            if self.held_back.as_ref() == Some(&state.key) {
                self.message = Some(format!(
                    "{} was not opened: it crashed Fontelle last time. Open the song again to try it",
                    state.name
                ));
                return None;
            }
            let Some(found) = self.scan.find(&state.key) else {
                // §17.4's rule for a missing file, applied to a missing
                // plugin: the project opens, the slot is silent, and somebody
                // is told which plugin it wanted rather than left to guess.
                self.message = Some(format!("{} is not installed", state.name));
                return None;
            };
            let path = found.path.clone();
            let found_info = found.clone();
            let mut plugin = match self.host.open(&path, &state.key) {
                Ok(plugin) => plugin,
                Err(e) => {
                    self.message = Some(e.to_string());
                    return None;
                }
            };
            // > *"sometimes they'll just revert back to the init preset"*
            //
            // A state the plugin will not take used to be dropped without a
            // word: it opened as a new one, and nothing said that what was
            // saved had not gone in. (LV2 is handed its state as it is
            // instantiated, below, and answers there.)
            if state.blob.is_some()
                && !plugin.restore_blob(state)
                && state.key.format != fontelle_types::PluginFormat::Lv2
            {
                self.message = Some(format!(
                    "{} would not take its saved state \u{2014} it has opened as a new one",
                    state.name
                ));
            }
            plugin.restore_params(state);
            let bay = Arc::new(ProcessorBay::new());
            // The presets compiled into it, read while its processor is in
            // hand — which an LV2 plugin's need — the first time it opens.
            let mut programs = Vec::new();
            match plugin.activate(sample_rate, max_block) {
                Ok(mut processor) => {
                    // What the restore left for the plugin's worker thread,
                    // done before anything plays: setBfree builds the organ
                    // its state describes there, and a project rendered the
                    // moment it opened lost its notes to the swap.
                    if state.blob.is_some() {
                        processor.finish_work();
                    }
                    if !self.libraries.contains_key(&state.key) {
                        programs = plugin.programs_with(&mut processor);
                    }
                    bay.park(processor)
                }
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
                blob: state.blob.clone(),
                seen: seen_params(state),
                header_presses: Vec::new(),
                header_hover: None,
                play_pause: 0,
            };
            live.refresh_displays();
            self.list_library(&found_info, &mut live, programs);
            self.live.insert(slot, live);
        } else if self.state_is_new(slot, state) {
            // > *"non native plugins are not integrated with the presets
            // > system"*
            //
            // **A new state for a plugin that is already open** — a preset
            // loaded, or an undo putting the one before it back. This used
            // to happen only on opening, so a preset changed the knobs below
            // and nothing else: the rest of the patch — most of a real
            // plugin, which is not parameters — stayed where it was.
            //
            // Only when the document's blob differs from the last one this
            // rack put in or read out. The document's copy is as old as the
            // last save, and whatever was done in the plugin's own editor
            // since is newer; a rebuild for something else must not undo it.
            //
            // LV2 lets a host restore a state only into an instance nothing
            // is running. An LV2 plugin used to be opened again with it for
            // that reason, closing its window; now its processor is taken
            // home like any other and it takes the state where it stands
            // (`HostedPlugin::restore_blob_with`). Opening it again is what
            // is left for one whose processor does not come home in time.
            let lv2 = state.key.format == fontelle_types::PluginFormat::Lv2;
            // **With its processor home, and run until it is in.** Surge XT
            // hands a state to its audio thread once it has processed, so
            // one put in where it stood was not in it when the call came
            // back — and a save, or the preset bar, read the state from
            // before. See `load_own_preset`, which is the same thing for a
            // preset of the plugin's own.
            let transport = self.transport.clone();
            let rendering = self.rendering;
            let mut refused = false;
            let mut reopen = false;
            if let Some(live) = self.live.get_mut(&slot) {
                match recall_home(&live.bay, transport.as_ref(), rendering) {
                    Some(mut processor) => {
                        let target = state
                            .blob
                            .as_ref()
                            .and_then(|blob| fontelle_types::decode_base64(blob));
                        let before = match &target {
                            Some(bytes) => live.plugin.settle_mark_for(&mut processor, bytes),
                            None => live.plugin.settle_mark(&mut processor),
                        };
                        // The state, the wait for it to be in, and only
                        // then what the document says beyond it: asked
                        // before it landed, the plugin would answer with
                        // the patch it is leaving.
                        if live.plugin.restore_blob_with(&mut processor, state) {
                            live.plugin.settle_with(&mut processor, &before);
                        } else {
                            refused = true;
                        }
                        live.plugin.restore_params(state);
                        live.bay.park(processor);
                    }
                    // An LV2 plugin takes a state only with nothing running
                    // it: opened again with it instead.
                    None if lv2 => reopen = true,
                    None => {
                        refused = !live.plugin.restore_blob(state);
                        live.plugin.restore_params(state);
                    }
                }
                live.refresh_displays();
            }
            if reopen {
                if let Some(mut old) = self.live.remove(&slot)
                    && !old.retire()
                {
                    self.retired.push(old);
                }
                return self.ensure(slot, state, sample_rate, max_block);
            }
            if refused {
                self.message = Some(format!(
                    "{} would not take that state \u{2014} it is as it was",
                    state.name
                ));
            }
        }
        if let Some(live) = self.live.get_mut(&slot) {
            // What the document holds now, new or not, is what it is
            // compared against next time.
            if state.blob.is_some() {
                live.blob = state.blob.clone();
            }
            // What the document changed its mind about — see `apply_params`.
            live.apply_params(state);
        }
        self.live.get(&slot)
    }

    /// Starts listing `live`'s own library, the first time this plugin is
    /// opened.
    ///
    /// **Off the main thread**, but for LV2: Surge XT describes each of its
    /// three thousand files through its own provider, most of a second the
    /// studio would otherwise stand still for, the first time Surge is put
    /// on a channel. The thread is handed the state of the instance this rack
    /// already has, and never makes one of its own — see
    /// [`fontelle_host::list_own_presets`]. An LV2 plugin's is listed here
    /// and now, by the host its instances come from: its presets' state is
    /// in that host's URIDs, and a library of Turtle is a millisecond.
    ///
    /// `programs` are the presets compiled into it, read off this instance
    /// as it was activated (`HostedPlugin::programs_with`) — a few dozen
    /// calls. An LV2 plugin's are kept only when its Turtle lists no preset
    /// of its own: a DPF plugin offers the same set both ways.
    fn list_library(
        &mut self,
        info: &fontelle_host::PluginInfo,
        live: &mut Live,
        programs: Vec<OwnPreset>,
    ) {
        if self.libraries.contains_key(&info.key) {
            return;
        }
        if !programs.is_empty() {
            self.programs.insert(info.key.clone(), programs);
        }
        if info.key.format == fontelle_types::PluginFormat::Lv2 {
            let listed = self.host.own_lv2_presets(info);
            if !listed.is_empty() {
                self.programs.remove(&info.key);
            }
            self.libraries
                .insert(info.key.clone(), Library::Ready(listed));
            self.fresh.push(info.key.clone());
            return;
        }
        let own = live.plugin.save_state();
        let roots = self.preset_roots.clone();
        let info = info.clone();
        let (sender, receiver) = std::sync::mpsc::channel();
        let key = info.key.clone();
        let spawned = std::thread::Builder::new()
            .name("plugin-presets".into())
            .spawn(move || {
                let _ = sender.send(fontelle_host::list_own_presets(
                    &info,
                    &roots,
                    own.as_deref(),
                ));
            });
        let library = match spawned {
            Ok(_) => Library::Listing(receiver),
            Err(_) => Library::Ready(Vec::new()),
        };
        self.libraries.insert(key, library);
    }

    /// Where plugins' libraries are looked for beyond what each lists itself
    /// — the standard folders unless told otherwise (a test's rig).
    pub fn set_preset_roots(&mut self, roots: fontelle_host::PresetRoots) {
        self.preset_roots = roots;
    }

    /// The libraries listed since this was last asked, as `(name, category)`
    /// rows for the preset bank. Does not wait.
    pub fn take_libraries(&mut self) -> Vec<(PluginKey, Vec<(String, String)>)> {
        // > *"Dexed loading a new cartridge changes its 32 names"*
        //
        // A plugin that said its programs may be others is asked again, and
        // its library handed over again only when they are.
        for live in self.live.values_mut() {
            if !live.plugin.take_programs_changed() {
                continue;
            }
            let programs = live.plugin.programs();
            let held = self.programs.get(&live.key).map_or(&[][..], Vec::as_slice);
            if programs != held {
                self.programs.insert(live.key.clone(), programs);
                if !self.fresh.contains(&live.key) {
                    self.fresh.push(live.key.clone());
                }
            }
        }
        for (key, library) in &mut self.libraries {
            if let Library::Listing(receiver) = library {
                match receiver.try_recv() {
                    Ok(listed) => {
                        *library = Library::Ready(listed);
                        self.fresh.push(key.clone());
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => {}
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        *library = Library::Ready(Vec::new());
                    }
                }
            }
        }
        std::mem::take(&mut self.fresh)
            .into_iter()
            .filter_map(|key| match self.libraries.get(&key) {
                Some(Library::Ready(listed)) => Some((
                    key.clone(),
                    listed
                        .iter()
                        .chain(self.programs.get(&key).into_iter().flatten())
                        .map(|preset| (preset.name.clone(), preset.category.clone()))
                        .collect(),
                )),
                _ => None,
            })
            .collect()
    }

    /// Waits for every library still being listed — for a headless run and a
    /// test, which have no next frame to pick one up in.
    pub fn wait_for_libraries(&mut self) {
        for (key, library) in &mut self.libraries {
            if let Library::Listing(receiver) = library {
                *library = Library::Ready(receiver.recv().unwrap_or_default());
                self.fresh.push(key.clone());
            }
        }
    }

    /// One preset of a plugin's own library, by what the bank calls it.
    pub fn own_preset(&self, key: &PluginKey, name: &str, category: &str) -> Option<OwnPreset> {
        let listed = match self.libraries.get(key)? {
            Library::Ready(listed) => listed.as_slice(),
            Library::Listing(_) => &[],
        };
        listed
            .iter()
            .chain(self.programs.get(key).into_iter().flatten())
            .find(|preset| preset.name == name && preset.category == category)
            .cloned()
    }

    /// Loads one of a plugin's own presets into the plugin at `slot`, and
    /// answers with the state it is in now — what the document is to hold.
    ///
    /// > *"sometimes they'll just revert back to the init preset"*
    ///
    /// **With its processor recalled out of the graph, whatever the format.**
    /// LV2 has to be (its state goes into the instance). The others were
    /// loaded where they stood and read straight back — and Surge XT queues
    /// a preset for its next block, so with the audio running what was read
    /// back was the patch *before* the preset. The document kept that, and
    /// the rebuild that follows a preset then wrote the old patch's values
    /// over the new one whenever the block had come round in between: the
    /// preset stuck or did not by where the block boundary fell.
    ///
    /// So the processor comes home, the preset goes in with nothing else
    /// running the plugin, and it is run here in silence until it has
    /// stopped changing ([`HostedPlugin::settle_with`]) before the state is
    /// read. A processor nobody
    /// hands back (a graph nothing is processing) leaves the old way, which
    /// is right for a plugin that is not running at all.
    pub fn load_own_preset(
        &mut self,
        slot: PluginSlot,
        preset: &OwnPreset,
    ) -> Result<PluginState, String> {
        let live = self.live.get_mut(&slot).ok_or("that plugin is not open")?;
        match recall_home(&live.bay, self.transport.as_ref(), self.rendering) {
            Some(mut processor) => {
                let before = live.plugin.settle_mark(&mut processor);
                let loaded = live.plugin.load_own_preset_with(&mut processor, preset);
                if loaded.is_ok() {
                    live.plugin.settle_with(&mut processor, &before);
                }
                let state = loaded
                    .is_ok()
                    .then(|| live.plugin.snapshot_with(&mut processor));
                live.bay.park(processor);
                loaded?;
                // What is handed back is what the document is to hold.
                if let Some(state) = &state {
                    live.keep(state);
                }
                live.refresh_displays();
                state.ok_or_else(|| "the plugin's state could not be read back".to_string())
            }
            None if live.plugin.own_preset_needs_processor() => {
                Err("the audio thread did not hand the plugin over".to_string())
            }
            None => {
                live.plugin.load_own_preset(preset)?;
                live.refresh_displays();
                let state = self
                    .snapshot(slot)
                    .ok_or_else(|| "the plugin's state could not be read back".to_string())?;
                self.kept(slot, &state);
                Ok(state)
            }
        }
    }

    /// Whether `state` carries a blob the document did not hold the last
    /// time this rack looked (or was [told](Self::kept)). A document with no
    /// blob never is: it has nothing of the plugin's own to say.
    fn state_is_new(&self, slot: PluginSlot, state: &PluginState) -> bool {
        match (&state.blob, self.live.get(&slot)) {
            (Some(blob), Some(live)) => live.blob.as_ref() != Some(blob),
            _ => false,
        }
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
        // Before the window and before the plugin is asked anything: Vital's
        // editor aborts the process from its own thread once it has a window
        // its driver cannot draw on, and nothing after that is ours.
        if let Some(refusal) = self
            .editor_gate
            .refusal(live.plugin.key(), live.plugin.name())
        {
            self.gate_refused = Some(live.plugin.name().to_string());
            return Err(refusal);
        }
        let title = live.plugin.name().to_string();
        let window = match self.headless_editors {
            true => PluginWindow::headless_with_header(
                GuiSize::FALLBACK.width,
                GuiSize::FALLBACK.height,
                self.editor_header,
            ),
            false => PluginWindow::open_with_header(&title, GuiSize::FALLBACK, self.editor_header)
                .map_err(|e| e.to_string())?,
        };
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

    /// Whether any plugin changed its own values since this was last asked —
    /// a knob turned in its window, a preset from its own browser. Those
    /// edits never pass through the document's history, and this is how
    /// they become unsaved changes.
    pub fn take_changes_heard(&mut self) -> bool {
        let mut any = false;
        for live in self.live.values_mut() {
            any |= live.plugin.take_changes_heard();
        }
        any
    }

    /// Answers every loaded plugin that asked to be called back on the main
    /// thread — CLAP's `request_callback`, which this studio recorded and
    /// never answered. Every plugin, not only those with an editor open:
    /// what a plugin defers to that call is not only drawing.
    pub fn service_main_thread(&mut self) {
        for live in self.live.values_mut() {
            live.plugin.service_main_thread();
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

    /// Gives every plugin editor opened from now a strip `height` pixels
    /// high across its top — see the field.
    pub fn set_editor_header(&mut self, height: u32) {
        self.editor_header = height;
    }

    /// The plugin whose editor the driver probe refused last, taken — see
    /// `gate_refused`.
    pub fn take_gate_refusal(&mut self) -> Option<String> {
        self.gate_refused.take()
    }

    /// What is asked before a plugin's own editor opens — see
    /// [`fontelle_host::EditorGate`]. The studio's probes its machine.
    pub fn set_editor_gate(&mut self, gate: fontelle_host::EditorGate) {
        self.editor_gate = gate;
    }

    /// Opens editors on no screen — see the field.
    pub fn set_headless_editors(&mut self, headless: bool) {
        self.headless_editors = headless;
    }

    /// Every open editor that has a strip.
    pub fn editor_headers(&self) -> Vec<EditorHeader> {
        let mut headers: Vec<_> = self
            .live
            .iter()
            .filter_map(|(slot, live)| {
                let window = live.editor.as_ref()?;
                (window.header_height() > 0).then(|| EditorHeader {
                    slot: *slot,
                    width: window.size().width,
                    height: window.header_height(),
                    scale: window.scale() as f32,
                    hover: live.header_hover,
                })
            })
            .collect();
        headers.sort_by_key(|header| format!("{:?}", header.slot));
        headers
    }

    /// Spaces any editor's window heard that its plugin did not take, since
    /// this was last asked.
    pub fn take_play_pause(&mut self) -> u32 {
        self.live
            .values_mut()
            .map(|live| std::mem::take(&mut live.play_pause))
            .sum()
    }

    /// The presses on editors' strips since this was last asked, in each
    /// window's own pixels.
    pub fn take_header_presses(&mut self) -> Vec<(PluginSlot, i32, i32)> {
        let mut presses = Vec::new();
        for (slot, live) in &mut self.live {
            for (x, y) in live.header_presses.drain(..) {
                presses.push((*slot, x, y));
            }
        }
        presses
    }

    /// Hands `slot`'s editor the pixels of its strip.
    pub fn set_header_pixels(&mut self, slot: PluginSlot, rgba: &[u8], width: u32, height: u32) {
        if let Some(window) = self
            .live
            .get_mut(&slot)
            .and_then(|live| live.editor.as_mut())
        {
            window.set_header(rgba, width, height);
        }
    }

    /// **A headless editor only**: a press on its strip — see
    /// `fontelle_host::PluginWindow::press_header`.
    pub fn press_header(&mut self, slot: PluginSlot, x: i32, y: i32) {
        if let Some(window) = self
            .live
            .get_mut(&slot)
            .and_then(|live| live.editor.as_mut())
        {
            window.press_header(x, y);
        }
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
