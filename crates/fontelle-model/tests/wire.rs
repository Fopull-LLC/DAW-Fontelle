//! The document as a stream of edits (docs/collab-plan.md §5, §12.1).
//!
//! Two machines can only work on one song if they agree about what every id
//! in it names. These tests hold the pieces that agreement is built from: ids
//! that survive a trip to disk, an edit that can cross a wire and land as the
//! same edit, an outbox that sees what the history saw, and a hash that says
//! two documents are the same.

use std::path::PathBuf;

use fontelle_model::{
    AddAudioClip, AddAutomationPoint, AddChannel, AddClip, AddInsert, AddLane, AddMarker,
    AddMixerTrack, AddNotes, AddPluginInsert, AddPrefab, AddPrefabInstance, AddSend, ApplyPreset,
    ApplyTrackChain, Arena, AutomationData, AutomationPoint, Clip, ClipSource, Command, Compound,
    CopyChannelAb, CurveShape, DetachPrefab, DuplicateChannel, DuplicateClip, Edit, EditLapse,
    EditNotepad, FlagTarget, History, ImportPart, ImportParts, MakePrefabFromClip,
    MoveAutomationPoints, MoveClip, MoveInsert, MoveLane, MoveNotes, NoteData, NoteProperty,
    NudgeNoteProperty, NumberTarget, PROJECT_FILE, PresetTarget, Project, RemoveAutomationPoints,
    RemoveChannel, RemoveClip, RemoveInsert, RemoveLane, RemoveMarker, RemoveMixerTrack,
    RemoveNotes, RemovePrefab, RemoveSend, RenameChannel, RenameLane, RenameMixerTrack,
    RenamePrefab, RenameProject, ResizeClip, ResizeNotes, RestoreInsert, SetAudioClip,
    SetChannelKind, SetChannelPatch, SetChannelPlugin, SetChannelRoute, SetClipLoop, SetEqBand,
    SetFlag, SetInsertBypassed, SetInsertKey, SetInsertMix, SetInsertNotes, SetInsertParam,
    SetLoopRange, SetNoteLengths, SetNoteProperty, SetNotePropertyEach, SetNoteSlide,
    SetNoteVelocity, SetNumber, SetPluginParam, SetPointCurve, SetPresetRef, SetSendLevel,
    SetSendPreFader, SetTrackInput, SetTrackOutput, SliceNotes, SplitClip, SwitchChannelAb,
    TrimClipStart, load_project, peek_meta, save_project,
};
use fontelle_types::{
    AssetKind, AssetRef, AudioClipData, BandChannel, BandType, ChannelId, ClipId, DeviceKind,
    DisgustingBeatEdit, EffectConfig, EffectKind, EqBand, InstrumentKind, LaneId, MixerTrackId,
    NoteId, NotepadEdit, PPQN, ParamAddress, PatchData, PersistentId, PluginKey, PluginState,
    PointId, PrefabId, Preset, PresetOrigin, PresetPayload, PresetRef, TrackChain, TrackInsert,
};

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "fontelle-wire-{name}-{}.fontelle",
        std::process::id()
    ));
    std::fs::remove_dir_all(&path).ok();
    path
}

/// The document as it would be saved. Two projects are the same project when
/// these match.
fn snapshot(project: &Project) -> serde_json::Value {
    serde_json::to_value(project).expect("a project must serialise")
}

fn a_note(start: i64, key: u8) -> fontelle_model::Note {
    fontelle_model::Note {
        start,
        length: PPQN,
        key,
        velocity: 100,
        pan: 0,
        fine_pitch: 0,
        release: 0,
        mod_x: 0,
        mod_y: 0,
        slide: false,
        channel: None,
    }
}

fn a_patch(marker: &str) -> PatchData {
    PatchData {
        format_version: 1,
        body: serde_json::json!({ "marker": marker }),
    }
}

fn an_asset(name: &str) -> AssetRef {
    AssetRef {
        id: fontelle_types::AssetId::default(),
        path: name.into(),
        content_hash: 0x5eed,
        size: 1024,
        kind: AssetKind::Sample,
    }
}

fn a_band(freq_hz: f32, gain_db: f32) -> EqBand {
    EqBand {
        band_type: BandType::Bell,
        freq_hz,
        gain_db,
        q: 1.0,
        enabled: true,
        solo: false,
        channel: BandChannel::Stereo,
    }
}

fn applied<C: Command>(mut command: C, project: &mut Project) -> C {
    command
        .apply(project)
        .expect("the fixture's own edits apply");
    command
}

// The insert chain on the part's strip, by position.
const GATE: usize = 0;
const EQ: usize = 1;
const PAD: usize = 2;
const LAPSE: usize = 3;
const TUNE: usize = 4;
const PLUGIN: usize = 5;

/// A song with one of everything a command can reach: two strips and the
/// master, a chain of six inserts (one of them somebody else's plugin), a
/// send, two channels (one hosting a plugin), two rows, a clip of notes, an
/// automation clip, an audio clip, a prefab with a place, and a marker.
struct Studio {
    project: Project,
    master: MixerTrackId,
    part: MixerTrackId,
    bus: MixerTrackId,
    /// A strip the part does not feed, so it can key the part's gate.
    side: MixerTrackId,
    keys: ChannelId,
    drums: ChannelId,
    lanes: [LaneId; 2],
    notes_clip: ClipId,
    notes: Vec<NoteId>,
    auto_clip: ClipId,
    points: Vec<PointId>,
    audio_clip: ClipId,
    prefab: PrefabId,
    place: ClipId,
    marker: fontelle_types::MarkerId,
}

fn a_studio() -> Studio {
    let mut project = Project::new("wire");
    let master = project.mixer.master.unwrap();
    let part = applied(AddMixerTrack::new("Part"), &mut project)
        .track()
        .unwrap();
    let bus = applied(AddMixerTrack::new("Bus"), &mut project)
        .track()
        .unwrap();
    let side = applied(AddMixerTrack::new("Side"), &mut project)
        .track()
        .unwrap();
    for kind in [
        EffectKind::Gate,
        EffectKind::Eq,
        EffectKind::Notepad,
        EffectKind::DisgustingBeat,
        EffectKind::Tune,
    ] {
        applied(AddInsert::new(part, kind), &mut project);
    }
    applied(
        AddPluginInsert::new(
            part,
            PluginState::new(PluginKey::clap("com.u-he.diva"), "Diva"),
        ),
        &mut project,
    );
    applied(AddSend::new(part, bus), &mut project);

    let keys = applied(
        AddChannel::new("Keys", Some(a_patch("keys")))
            .of_kind(InstrumentKind::Flopsynth)
            .routed_to(Some(part)),
        &mut project,
    )
    .channel()
    .unwrap();
    let drums = applied(
        AddChannel::new("Drums", None).routed_to(Some(part)),
        &mut project,
    )
    .channel()
    .unwrap();
    applied(
        SetChannelPlugin::new(
            drums,
            Some(PluginState::new(PluginKey::clap("org.surge"), "Surge XT")),
        ),
        &mut project,
    );

    let lanes = [
        applied(AddLane::new("One"), &mut project).id().unwrap(),
        applied(AddLane::new("Two"), &mut project).id().unwrap(),
    ];
    let notes_clip = applied(
        AddClip::new(Clip {
            lane: lanes[0],
            start: 0,
            length: PPQN * 4,
            source: ClipSource::Notes(NoteData {
                channel: keys,
                notes: Arena::default(),
            }),
            prefab_link: None,
            color: None,
            muted: false,
            loop_length: None,
        }),
        &mut project,
    )
    .id()
    .unwrap();
    let notes = applied(
        AddNotes::new(
            notes_clip,
            vec![a_note(0, 60), a_note(PPQN, 64), a_note(PPQN * 2, 67)],
        ),
        &mut project,
    )
    .ids()
    .to_vec();

    let mut curve = Arena::default();
    for (tick, value) in [(0, 0.25), (PPQN * 4, 0.75)] {
        curve.insert(AutomationPoint {
            tick,
            value,
            curve: CurveShape::Linear,
            tension: 0.0,
        });
    }
    let auto_clip = applied(
        AddClip::new(Clip {
            lane: lanes[1],
            start: 0,
            length: PPQN * 4,
            source: ClipSource::Automation(AutomationData {
                target: ParamAddress::new("track:gain"),
                points: curve,
            }),
            prefab_link: None,
            color: None,
            muted: false,
            loop_length: None,
        }),
        &mut project,
    )
    .id()
    .unwrap();
    let points = match &project.clips[auto_clip].source {
        ClipSource::Automation(data) => data.points.keys().collect(),
        _ => unreachable!(),
    };

    let audio_clip = applied(
        AddAudioClip::new(
            "take.wav",
            AudioClipData::whole(an_asset("/home/alice/take.wav"), 48_000, 48_000),
            PPQN * 4,
            PPQN * 2,
        )
        .on_lane(lanes[1]),
        &mut project,
    )
    .clip()
    .unwrap();

    let prefab = applied(
        AddPrefab::new(
            "Riff",
            ClipSource::Notes(NoteData {
                channel: keys,
                notes: [a_note(0, 48)].into_iter().collect(),
            }),
        ),
        &mut project,
    )
    .prefab()
    .unwrap();
    let place = applied(
        AddPrefabInstance::new(prefab, lanes[0], PPQN * 8, PPQN * 4),
        &mut project,
    )
    .clip()
    .unwrap();
    let marker = applied(AddMarker::new("Chorus", PPQN * 16), &mut project)
        .id()
        .unwrap();

    project.view_state.zoom = 3.0;
    project.view_state.scroll = 7.0;
    Studio {
        project,
        master,
        part,
        bus,
        side,
        keys,
        drums,
        lanes,
        notes_clip,
        notes,
        auto_clip,
        points,
        audio_clip,
        prefab,
        place,
        marker,
    }
}

fn a_chain() -> TrackChain {
    TrackChain {
        gain_db: -6.0,
        pan: -0.5,
        phase_invert: true,
        inserts: vec![TrackInsert {
            config: EffectConfig::new(EffectKind::Compressor),
            bypassed: false,
            preset: Some(PresetRef::new("Vocal Glue", "Vocal", PresetOrigin::User)),
            disgusting_beat: None,
        }],
    }
}

type Make = fn(&Studio) -> Box<dyn Command>;

/// One of every command the window can reach, each built against the studio.
///
/// Every public command is here, and every `Restore*` is reached by
/// inverting one of these — the tests below run the inverse too.
fn every_command() -> Vec<(&'static str, Make)> {
    vec![
        // --- channels
        ("AddChannel", |s| {
            Box::new(
                AddChannel::new("Bass", Some(a_patch("bass")))
                    .of_kind(InstrumentKind::Osc3)
                    .with_pan(-0.25)
                    .routed_to(Some(s.bus)),
            )
        }),
        ("RemoveChannel", |s| Box::new(RemoveChannel::new(s.drums))),
        ("DuplicateChannel", |s| {
            Box::new(DuplicateChannel::new(s.keys))
        }),
        ("SetChannelRoute", |s| {
            Box::new(SetChannelRoute::new(s.keys, Some(s.bus)))
        }),
        ("SetChannelKind", |s| {
            Box::new(SetChannelKind::new(s.keys, InstrumentKind::Osc3))
        }),
        ("SetChannelPatch", |s| {
            Box::new(SetChannelPatch::new(s.keys, Some(a_patch("other"))))
        }),
        ("RenameChannel", |s| {
            Box::new(RenameChannel::new(s.keys, "Piano"))
        }),
        ("SetChannelPlugin", |s| {
            Box::new(SetChannelPlugin::new(
                s.keys,
                Some(PluginState::new(PluginKey::clap("com.u-he.diva"), "Diva")),
            ))
        }),
        ("SetPluginParam channel", |s| {
            Box::new(SetPluginParam::channel(s.drums, 3, 0.5))
        }),
        ("SwitchChannelAb", |s| {
            Box::new(SwitchChannelAb::new(s.keys))
        }),
        ("CopyChannelAb", |s| Box::new(CopyChannelAb::new(s.keys))),
        ("ApplyPreset channel", |s| {
            Box::new(ApplyPreset::new(
                PresetTarget::Channel(s.keys),
                Preset::new(
                    DeviceKind::Instrument(InstrumentKind::Flopsynth),
                    "Glass",
                    "Pad",
                    PresetPayload::Patch(a_patch("glass")),
                ),
            ))
        }),
        ("SetPresetRef channel", |s| {
            Box::new(SetPresetRef::new(
                PresetTarget::Channel(s.keys),
                Some(PresetRef::new("Glass", "Pad", PresetOrigin::Factory)),
            ))
        }),
        // --- rows
        ("AddLane", |_| Box::new(AddLane::new("Three"))),
        ("AddLane at", |_| Box::new(AddLane::at("Zero", 0))),
        ("MoveLane", |_| Box::new(MoveLane::down(0))),
        ("RemoveLane", |s| Box::new(RemoveLane::new(s.lanes[1]))),
        ("RenameLane", |s| {
            Box::new(RenameLane::new(s.lanes[0], "Verse"))
        }),
        // --- the mixer
        ("AddMixerTrack", |_| Box::new(AddMixerTrack::new("Aux"))),
        ("RemoveMixerTrack", |s| {
            Box::new(RemoveMixerTrack::new(s.bus))
        }),
        ("RenameMixerTrack", |s| {
            Box::new(RenameMixerTrack::new(s.part, "Lead"))
        }),
        ("SetTrackOutput", |s| {
            Box::new(SetTrackOutput::new(s.part, Some(s.bus)))
        }),
        ("AddSend", |s| Box::new(AddSend::new(s.bus, s.master))),
        ("RemoveSend", |s| Box::new(RemoveSend::new(s.part, 0))),
        ("SetSendLevel", |s| {
            Box::new(SetSendLevel::new(s.part, 0, -3.0))
        }),
        ("SetSendPreFader", |s| {
            Box::new(SetSendPreFader::new(s.part, 0, true))
        }),
        ("SetTrackInput", |s| {
            Box::new(SetTrackInput::new(s.part, Some("mic".into())))
        }),
        ("SetNumber gain", |s| {
            Box::new(SetNumber::new(NumberTarget::TrackGainDb(s.part), -3.0))
        }),
        ("SetNumber tempo", |_| {
            Box::new(SetNumber::new(NumberTarget::Tempo, 140.0))
        }),
        ("SetFlag", |s| {
            Box::new(SetFlag::new(FlagTarget::TrackMute(s.part), true))
        }),
        ("ApplyTrackChain", |s| {
            Box::new(ApplyTrackChain::new(s.part, a_chain()))
        }),
        // --- inserts
        ("AddInsert", |s| {
            Box::new(AddInsert::new(s.part, EffectKind::Reverb))
        }),
        ("RemoveInsert", |s| {
            Box::new(RemoveInsert::new(s.part, GATE))
        }),
        ("RestoreInsert", |s| {
            Box::new(RestoreInsert::new(
                s.bus,
                0,
                fontelle_model::EffectSlot::new(EffectKind::Delay),
            ))
        }),
        ("MoveInsert", |s| {
            Box::new(MoveInsert::new(s.part, GATE, TUNE))
        }),
        ("SetInsertBypassed", |s| {
            Box::new(SetInsertBypassed::new(s.part, GATE, true))
        }),
        ("SetInsertParam", |s| {
            let param = EffectConfig::new(EffectKind::Gate).specs()[0].id;
            Box::new(SetInsertParam::new(s.part, GATE, param, 0.3))
        }),
        ("SetInsertNotes", |s| {
            Box::new(SetInsertNotes::new(s.part, TUNE, Some(s.keys)))
        }),
        ("SetInsertKey", |s| {
            Box::new(SetInsertKey::new(s.part, GATE, Some(s.side)))
        }),
        ("SetInsertMix", |s| {
            Box::new(SetInsertMix::new(s.part, GATE, 0.5))
        }),
        ("EditNotepad", |s| {
            Box::new(EditNotepad::new(
                s.part,
                PAD,
                NotepadEdit::Write {
                    page: 0,
                    text: "verse one".into(),
                },
            ))
        }),
        ("EditLapse", |s| {
            Box::new(EditLapse::new(
                s.part,
                LAPSE,
                DisgustingBeatEdit::RenameScene {
                    scene: 0,
                    name: "Drop".into(),
                },
            ))
        }),
        ("SetEqBand", |s| {
            Box::new(SetEqBand::new(s.part, EQ, 0, a_band(800.0, 5.0)))
        }),
        ("AddPluginInsert", |s| {
            Box::new(AddPluginInsert::new(
                s.bus,
                PluginState::new(PluginKey::clap("org.lsp"), "LSP"),
            ))
        }),
        ("SetPluginParam insert", |s| {
            Box::new(SetPluginParam::insert(s.part, PLUGIN, 7, 0.25))
        }),
        ("ApplyPreset insert", |s| {
            let config = EffectConfig::new(EffectKind::Gate);
            Box::new(ApplyPreset::new(
                PresetTarget::Insert {
                    track: s.part,
                    index: GATE,
                },
                Preset::new(
                    DeviceKind::Effect(config.kind()),
                    "Tight",
                    "Drums",
                    PresetPayload::Effect(config),
                ),
            ))
        }),
        ("SetPresetRef insert", |s| {
            Box::new(SetPresetRef::new(
                PresetTarget::Insert {
                    track: s.part,
                    index: GATE,
                },
                Some(PresetRef::new("Tight", "Drums", PresetOrigin::User)),
            ))
        }),
        // --- notes
        ("AddNotes", |s| {
            Box::new(AddNotes::new(s.notes_clip, vec![a_note(PPQN * 3, 72)]))
        }),
        ("RemoveNotes", |s| {
            Box::new(RemoveNotes::new(s.notes_clip, vec![s.notes[1]]))
        }),
        ("MoveNotes", |s| {
            Box::new(MoveNotes::new(s.notes_clip, s.notes.clone(), PPQN / 2, 2))
        }),
        ("ResizeNotes", |s| {
            Box::new(ResizeNotes::new(s.notes_clip, s.notes.clone(), -PPQN / 4))
        }),
        ("SetNoteVelocity", |s| {
            Box::new(SetNoteVelocity::new(s.notes_clip, s.notes.clone(), 64))
        }),
        ("SetNoteSlide", |s| {
            Box::new(SetNoteSlide::new(s.notes_clip, vec![s.notes[0]], true))
        }),
        ("SliceNotes", |s| {
            Box::new(SliceNotes::new(s.notes_clip, vec![(s.notes[0], PPQN / 2)]))
        }),
        ("SetNoteProperty", |s| {
            Box::new(SetNoteProperty::new(
                s.notes_clip,
                s.notes.clone(),
                NoteProperty::Pan,
                10,
            ))
        }),
        ("NudgeNoteProperty", |s| {
            Box::new(NudgeNoteProperty::new(
                s.notes_clip,
                s.notes.clone(),
                NoteProperty::Velocity,
                -5,
            ))
        }),
        ("SetNotePropertyEach", |s| {
            Box::new(SetNotePropertyEach::new(
                s.notes_clip,
                s.notes.clone(),
                NoteProperty::FinePitch,
                vec![1, 2, 3],
            ))
        }),
        ("SetNoteLengths", |s| {
            Box::new(SetNoteLengths::new(
                s.notes_clip,
                s.notes.clone(),
                vec![PPQN / 2, PPQN / 3, PPQN / 4],
            ))
        }),
        ("ImportParts", |_| {
            Box::new(ImportParts::new(
                "song.mid",
                vec![ImportPart {
                    name: "Strings".into(),
                    notes: vec![a_note(0, 55)],
                    pan: 0.1,
                    volume_db: -2.0,
                    color: [1, 2, 3, 255],
                }],
            ))
        }),
        // --- clips
        ("AddClip", |s| {
            Box::new(AddClip::new(Clip {
                lane: s.lanes[1],
                start: PPQN * 8,
                length: PPQN * 4,
                source: ClipSource::Notes(NoteData {
                    channel: s.drums,
                    notes: Arena::default(),
                }),
                prefab_link: None,
                color: None,
                muted: false,
                loop_length: None,
            }))
        }),
        ("AddClip on a new row", |s| {
            Box::new(AddClip::on_new_row(
                Clip {
                    lane: s.lanes[0],
                    start: 0,
                    length: PPQN * 4,
                    source: ClipSource::Automation(AutomationData {
                        target: ParamAddress::new("track:pan"),
                        points: Arena::default(),
                    }),
                    prefab_link: None,
                    color: None,
                    muted: false,
                    loop_length: None,
                },
                "Part \u{2014} pan",
                [0xb4, 0xa2, 0xe8, 0xff],
            ))
        }),
        ("RemoveClip", |s| Box::new(RemoveClip::new(s.audio_clip))),
        ("MoveClip", |s| {
            Box::new(MoveClip::new(s.notes_clip, PPQN, Some(s.lanes[1])))
        }),
        ("SetClipLoop", |s| {
            Box::new(SetClipLoop::new(s.notes_clip, Some(PPQN * 2)))
        }),
        ("ResizeClip", |s| {
            Box::new(ResizeClip::new(s.notes_clip, PPQN))
        }),
        ("TrimClipStart", |s| {
            Box::new(TrimClipStart::new(s.audio_clip, PPQN / 2))
        }),
        ("DuplicateClip", |s| {
            Box::new(DuplicateClip::new(s.notes_clip, PPQN * 16))
        }),
        ("SplitClip", |s| {
            Box::new(SplitClip::new(s.notes_clip, PPQN * 2))
        }),
        ("AddAudioClip", |s| {
            Box::new(
                AddAudioClip::new(
                    "hit.wav",
                    AudioClipData::whole(an_asset("/home/alice/hit.wav"), 4_800, 48_000),
                    0,
                    PPQN,
                )
                .at_row(1)
                .on_lane(s.lanes[0]),
            )
        }),
        ("SetAudioClip", |s| {
            Box::new(SetAudioClip::new(
                s.audio_clip,
                AudioClipData::whole(an_asset("/home/alice/other.wav"), 96_000, 48_000),
            ))
        }),
        ("SetLoopRange", |_| {
            Box::new(SetLoopRange::new(Some((0, PPQN * 4))))
        }),
        // --- automation
        ("AddAutomationPoint", |s| {
            Box::new(AddAutomationPoint::new(
                s.auto_clip,
                AutomationPoint {
                    tick: PPQN * 2,
                    value: 0.9,
                    curve: CurveShape::SCurve,
                    tension: 0.0,
                },
            ))
        }),
        ("MoveAutomationPoints", |s| {
            Box::new(MoveAutomationPoints::new(
                s.auto_clip,
                vec![s.points[0]],
                PPQN,
                0.1,
            ))
        }),
        ("RemoveAutomationPoints", |s| {
            Box::new(RemoveAutomationPoints::new(s.auto_clip, vec![s.points[1]]))
        }),
        ("SetPointCurve", |s| {
            Box::new(SetPointCurve::new(
                s.auto_clip,
                vec![s.points[0]],
                CurveShape::Exponential,
            ))
        }),
        // --- prefabs
        ("AddPrefab", |s| {
            Box::new(AddPrefab::new(
                "Hook",
                ClipSource::Notes(NoteData {
                    channel: s.drums,
                    notes: [a_note(0, 36)].into_iter().collect(),
                }),
            ))
        }),
        ("AddPrefabInstance", |s| {
            Box::new(AddPrefabInstance::new(
                s.prefab,
                s.lanes[1],
                PPQN * 12,
                PPQN * 4,
            ))
        }),
        ("RenamePrefab", |s| {
            Box::new(RenamePrefab::new(s.prefab, "Lick"))
        }),
        ("DetachPrefab", |s| Box::new(DetachPrefab::new(s.place))),
        ("RemovePrefab", |s| Box::new(RemovePrefab::new(s.prefab))),
        ("MakePrefabFromClip", |s| {
            Box::new(MakePrefabFromClip::new(s.notes_clip, "Made"))
        }),
        // --- the song
        ("RenameProject", |_| Box::new(RenameProject::new("Song v2"))),
        ("AddMarker", |_| {
            Box::new(AddMarker::new("Bridge", PPQN * 32))
        }),
        ("RemoveMarker", |s| Box::new(RemoveMarker::new(s.marker))),
        ("Compound", |s| {
            Box::new(Compound::new(
                "two at once",
                vec![
                    Box::new(RenameLane::new(s.lanes[1], "Hats")),
                    Box::new(AddNotes::new(s.notes_clip, vec![a_note(PPQN * 3, 59)])),
                ],
            ))
        }),
    ]
}

/// A snapshot with each row's `order` replaced by its place in the stack.
///
/// Adding a row in the middle renumbers the stack densely and taking it away
/// again does not, so an undone insert leaves the rows in the same order under
/// different numbers — the same arrangement, and on both machines alike.
fn by_rank(mut json: serde_json::Value) -> serde_json::Value {
    let lanes = json["lanes"].as_array_mut().unwrap();
    let mut orders: Vec<u64> = lanes
        .iter()
        .map(|pair| pair[1]["order"].as_u64().unwrap())
        .collect();
    orders.sort_unstable();
    for pair in lanes.iter_mut() {
        let order = pair[1]["order"].as_u64().unwrap();
        pair[1]["order"] = orders.iter().position(|o| *o == order).unwrap().into();
    }
    json
}

/// `command`, applied, over a wire and back: the bytes are what another
/// machine would be handed.
fn across_the_wire(command: &dyn Command) -> Box<dyn Command> {
    let bytes = command.to_edit().to_bytes();
    let edit = Edit::from_bytes(&bytes).expect("an edit reads back from its own bytes");
    assert_eq!(
        edit.to_bytes(),
        bytes,
        "{}: an edit is the same bytes after a round trip",
        command.label()
    );
    edit.into_command()
}

/// Applies `command` on `here`, sends it, and applies what arrives on a copy
/// of `here` from before: the two must be the same document.
fn lands_the_same(
    name: &str,
    mut command: Box<dyn Command>,
    here: &mut Project,
) -> Box<dyn Command> {
    let mut there = here.clone();
    command
        .apply(here)
        .unwrap_or_else(|e| panic!("{name} must apply to the studio: {e}"));
    across_the_wire(command.as_ref())
        .apply(&mut there)
        .unwrap_or_else(|e| panic!("{name} must apply on the other machine: {e}"));
    assert_eq!(
        snapshot(here),
        snapshot(&there),
        "{name}: the edit that crossed the wire made a different document"
    );
    command
}

/// F2. Every command crosses a wire and lands as the same edit — ids and all,
/// which is what makes it redo's path on the other machine rather than a
/// second, different edit. And so does its inverse, which is how the
/// `Restore*` commands are reached.
#[test]
fn every_command_has_a_wire_form() {
    for (name, make) in every_command() {
        let studio = a_studio();
        let mut here = studio.project.clone();
        let command = lands_the_same(name, make(&studio), &mut here);
        lands_the_same(&format!("{name}, inverted"), command.invert(), &mut here);
        assert_eq!(
            by_rank(snapshot(&here)),
            by_rank(snapshot(&studio.project)),
            "{name}: the inverse that crossed the wire did not put the studio back"
        );
    }
}

/// F2. The wire names an edit by its variant, so a name, once shipped, is
/// forever: these literals are edits as this build writes them, and every
/// later build has to read them as the same edit. A rename fails here.
#[test]
fn wire_variant_order_is_pinned() {
    let pinned = [
        (r#"{"MoveLane":{"from":2,"delta":-1}}"#, "MoveLane"),
        (
            r#"{"SetLoopRange":{"range":[0,960],"previous":null}}"#,
            "SetLoopRange",
        ),
        (
            r#"{"RenameProject":{"name":"Song","previous":null}}"#,
            "RenameProject",
        ),
        (
            r#"{"Compound":{"label":"both","parts":[{"MoveLane":{"from":0,"delta":1}}],"applied":false}}"#,
            "Compound",
        ),
    ];
    for (json, tag) in pinned {
        let edit = Edit::from_bytes(json.as_bytes())
            .unwrap_or_else(|e| panic!("{tag} no longer reads as it was written: {e}"));
        assert_eq!(edit.tag(), tag);
    }
    // And every name is one edit's: two variants sharing a tag would read
    // one as the other.
    let mut tags: Vec<&str> = Edit::TAGS.to_vec();
    let all = tags.len();
    tags.sort_unstable();
    tags.dedup();
    assert_eq!(tags.len(), all, "two edits share a name on the wire");
}

/// F11. Zoom and scroll live in the document, and they are each person's
/// own: an edit that moved them would scroll somebody else's screen.
#[test]
fn no_command_touches_view_state() {
    for (name, make) in every_command() {
        let studio = a_studio();
        let mut project = studio.project.clone();
        let mut command = make(&studio);
        command.apply(&mut project).unwrap();
        let mut inverse = command.invert();
        inverse.apply(&mut project).unwrap();
        assert_eq!(
            (project.view_state.zoom, project.view_state.scroll),
            (3.0, 7.0),
            "{name} moved the view"
        );
    }
}

// ------------------------------------------------------------- the history

fn moved_by(project: &Project, clip: ClipId, note: NoteId) -> i64 {
    match &project.clips[clip].source {
        ClipSource::Notes(data) => data.notes[note].start,
        _ => unreachable!(),
    }
}

/// F4. A drag is four hundred commands folded into one history entry, and it
/// crosses the wire once — as the folded entry, when the gesture breaks.
#[test]
fn the_outbox_sees_a_drag_once() {
    let studio = a_studio();
    let mut project = studio.project.clone();
    let before = project.clone();
    let mut history = History::new();
    history.open_outbox();
    let note = studio.notes[0];
    for _ in 0..400 {
        history
            .apply(
                Box::new(MoveNotes::new(studio.notes_clip, vec![note], 1, 0)),
                &mut project,
            )
            .unwrap();
    }
    assert!(
        history.take_outbox().is_empty(),
        "a drag still in the hand has not happened yet as far as anyone else knows"
    );
    history.break_gesture();
    let sent = history.take_outbox();
    assert_eq!(sent.len(), 1, "one drag, one edit");
    assert!(history.take_outbox().is_empty(), "taken is gone");

    let mut there = before;
    sent.into_iter()
        .next()
        .unwrap()
        .into_command()
        .apply(&mut there)
        .unwrap();
    assert_eq!(moved_by(&there, studio.notes_clip, note), 400);
    assert_eq!(snapshot(&there), snapshot(&project));
}

/// F4 and §1.9. Nothing in the studio behaves differently when no session is
/// open: a history nobody opened an outbox on keeps no copies.
#[test]
fn the_outbox_is_shut_until_a_session_opens_it() {
    let studio = a_studio();
    let mut project = studio.project.clone();
    let mut history = History::new();
    history
        .apply(
            Box::new(RenameLane::new(studio.lanes[0], "Verse")),
            &mut project,
        )
        .unwrap();
    history.break_gesture();
    assert!(history.take_outbox().is_empty());
}

/// §5.7. An undo is an edit like any other and goes out as one — and an
/// undo of a gesture nobody has heard about yet sends the gesture first, so
/// the other side never inverts a thing it was never given.
#[test]
fn an_undo_goes_to_the_outbox_after_what_it_undoes() {
    let studio = a_studio();
    let mut project = studio.project.clone();
    let before = project.clone();
    let mut history = History::new();
    history.open_outbox();
    history
        .apply(
            Box::new(RenameLane::new(studio.lanes[0], "Verse")),
            &mut project,
        )
        .unwrap();
    history.undo(&mut project).unwrap().unwrap();
    let sent = history.take_outbox();
    assert_eq!(sent.len(), 2, "the rename, then its undo");

    let mut there = before.clone();
    for edit in sent {
        edit.into_command().apply(&mut there).unwrap();
    }
    assert_eq!(snapshot(&there), snapshot(&before));

    history.redo(&mut project).unwrap().unwrap();
    let redone = history.take_outbox();
    assert_eq!(redone.len(), 1, "a redo goes out too");
}

/// F16's first half, in the model: a foreign edit changes the document and
/// is not yours to undo.
#[test]
fn a_foreign_edit_does_not_enter_the_undo_stack() {
    let studio = a_studio();
    let mut project = studio.project.clone();
    let mut history = History::new();
    history.open_outbox();

    let mut elsewhere = project.clone();
    let mut theirs = RenameLane::new(studio.lanes[0], "Theirs");
    theirs.apply(&mut elsewhere).unwrap();

    history
        .apply_foreign(theirs.to_edit(), &mut project)
        .unwrap();
    assert_eq!(project.lanes[studio.lanes[0]].name, "Theirs");
    assert_eq!(
        history.depth(),
        0,
        "somebody else's edit is not on my stack"
    );
    assert!(history.undo(&mut project).is_none());
    assert!(
        history.take_outbox().is_empty(),
        "an edit that came in is not sent back out"
    );
}

/// F16's second half: with mine and theirs interleaved, undo takes back mine
/// and leaves theirs.
#[test]
fn undo_after_a_foreign_edit_undoes_only_mine() {
    let studio = a_studio();
    let mut project = studio.project.clone();
    let mut history = History::new();

    history
        .apply(
            Box::new(AddNotes::new(studio.notes_clip, vec![a_note(PPQN * 3, 71)])),
            &mut project,
        )
        .unwrap();
    history.break_gesture();

    // The host had my note when it made its own; that is the order it sends.
    let mut host = project.clone();
    let mut theirs = AddNotes::new(studio.notes_clip, vec![a_note(PPQN * 3, 52)]);
    theirs.apply(&mut host).unwrap();
    history
        .apply_foreign(theirs.to_edit(), &mut project)
        .unwrap();

    history.undo(&mut project).unwrap().unwrap();
    let keys: Vec<u8> = match &project.clips[studio.notes_clip].source {
        ClipSource::Notes(data) => data.notes.values().map(|n| n.key).collect(),
        _ => unreachable!(),
    };
    assert!(keys.contains(&52), "their note stays");
    assert!(!keys.contains(&71), "mine is undone");
}

// --------------------------------------------------------------- the hash

/// F11 and §5.6. Two documents are the same song when everything but the
/// view, the bundle's own label and where each machine keeps its files is
/// the same.
#[test]
fn sync_hash_ignores_view_state_and_paths() {
    let studio = a_studio();
    let here = studio.project.clone();

    let mut there = here.clone();
    there.view_state.zoom = 0.5;
    there.view_state.scroll = 99.0;
    // The joiner's copy may be "Song (2)" in its own Shared folder.
    there.meta.name = "wire (2)".into();
    there.meta.saved_revision = 12;
    if let ClipSource::Audio(data) = &mut there.clips[studio.audio_clip].source {
        data.asset.path = "C:\\Users\\bob\\Music\\take.wav".into();
    }
    assert_eq!(
        here.sync_hash(),
        there.sync_hash(),
        "the view, the label and the paths are each machine's own"
    );

    let mut edited = here.clone();
    MoveNotes::new(studio.notes_clip, vec![studio.notes[0]], 1, 0)
        .apply(&mut edited)
        .unwrap();
    assert_ne!(
        here.sync_hash(),
        edited.sync_hash(),
        "a moved note is a different song"
    );

    let mut other_file = here.clone();
    if let ClipSource::Audio(data) = &mut other_file.clips[studio.audio_clip].source {
        data.asset.content_hash = 0xbeef;
    }
    assert_ne!(
        here.sync_hash(),
        other_file.sync_hash(),
        "a different recording is a different song"
    );
}

// ------------------------------------------------------ the project's id

/// A format-0 bundle as a build before 2026-09-25 wrote it: no id, markers a
/// plain list, and no ids on inserts or sends.
fn a_format_0_bundle(name: &str, created: &str) -> PathBuf {
    let studio = a_studio();
    let mut json = snapshot(&studio.project);
    json["meta"] = serde_json::json!({
        "name": "Old Song",
        "created": created,
        "app_version": "0.15.0",
        "format_version": 0,
    });
    json["markers"] = serde_json::json!([{ "name": "Chorus", "tick": 7680 }]);
    for (_, track) in json["mixer"]["tracks"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .map(|pair| {
            let pair = pair.as_array_mut().unwrap();
            (pair[0].clone(), &mut pair[1])
        })
    {
        for slot in track["inserts"].as_array_mut().unwrap() {
            slot.as_object_mut().unwrap().remove("id");
        }
        for send in track["sends"].as_array_mut().unwrap() {
            send.as_object_mut().unwrap().remove("id");
        }
    }
    let bundle = scratch(name);
    std::fs::create_dir_all(&bundle).unwrap();
    std::fs::write(
        bundle.join(PROJECT_FILE),
        serde_json::to_string_pretty(&json).unwrap(),
    )
    .unwrap();
    bundle
}

/// F1. A project written before ids existed is given one derived from its
/// birth — its `created` and its `name` — so two copies of it on two machines
/// agree about who they are, and two loads of the same bytes agree about
/// every id in it.
#[test]
fn a_format_0_project_gets_an_id_derived_from_its_birth() {
    let bundle = a_format_0_bundle("v0", "2026-08-01T10:00:00Z");
    let first = load_project(&bundle).expect("a format-0 project opens");
    let second = load_project(&bundle).expect("and opens again");
    assert_eq!(first.meta.id, second.meta.id);
    assert_eq!(
        snapshot(&first),
        snapshot(&second),
        "every id a migration gives out is the same on every load"
    );
    assert_eq!(
        first.meta.format_version,
        fontelle_model::PROJECT_FORMAT_VERSION
    );
    assert_eq!(first.markers.len(), 1, "the marker came through");
    assert_eq!(
        peek_meta(&bundle).expect("the head reads alone").id,
        first.meta.id,
        "a peek and a load agree"
    );

    let other = a_format_0_bundle("v0-other", "2026-08-02T10:00:00Z");
    assert_ne!(
        load_project(&other).unwrap().meta.id,
        first.meta.id,
        "a different birth is a different song"
    );
    std::fs::remove_dir_all(&bundle).ok();
    std::fs::remove_dir_all(&other).ok();

    assert_ne!(
        Project::new("a").meta.id,
        Project::new("a").meta.id,
        "from format 1 on an id is minted, never derived"
    );
}

/// F1. The id and the save stamps survive a trip to disk, and `peek_meta`
/// reads them without the body.
#[test]
fn the_id_and_the_save_stamps_are_kept() {
    let mut project = a_studio().project;
    project.meta.stamp_save("alice");
    project.meta.stamp_save("alice");
    assert_eq!(project.meta.saved_revision, 2);
    assert_eq!(project.meta.saved_by, "alice");
    assert!(!project.meta.saved_at.is_empty());

    let bundle = scratch("stamps");
    save_project(&project, &bundle).unwrap();
    let head = peek_meta(&bundle).unwrap();
    assert_eq!(head.id, project.meta.id);
    assert_eq!(head.saved_revision, 2);
    assert_eq!(load_project(&bundle).unwrap().meta.id, project.meta.id);
    std::fs::remove_dir_all(&bundle).ok();
}

/// §15 decision 2: a Save As is a new song that remembers where it came from.
#[test]
fn a_fork_is_a_new_song_that_remembers_its_parent() {
    let mut project = a_studio().project;
    let parent = project.meta.id;
    project.meta.fork();
    assert_ne!(project.meta.id, parent);
    assert_eq!(project.meta.forked_from, Some(parent));
    assert_eq!(project.meta.shared_revision, None);
}

// ------------------------------------------------------------- slot ids

/// F8. A command names an insert by its place in the chain, and remembers
/// which insert was there when it was made. If another edit has moved a
/// different insert into that place, the command is refused rather than
/// applied to the neighbour.
#[test]
fn a_slot_command_refuses_when_the_slot_moved() {
    let studio = a_studio();
    let mut here = studio.project.clone();
    let mut there = studio.project.clone();

    let mut mine = SetInsertBypassed::new(studio.part, EQ, true);
    mine.apply(&mut here).unwrap();

    // Meanwhile, on the other machine, the gate moved below the EQ.
    MoveInsert::new(studio.part, GATE, EQ)
        .apply(&mut there)
        .unwrap();
    let untouched = snapshot(&there);

    let result = across_the_wire(&mine).apply(&mut there);
    assert!(result.is_err(), "the EQ is not at that place any more");
    assert_eq!(
        snapshot(&there),
        untouched,
        "and nothing else was bypassed in its stead"
    );

    // A send is the same.
    let mut level = SetSendLevel::new(studio.part, 0, -12.0);
    let mut here = studio.project.clone();
    level.apply(&mut here).unwrap();
    let mut there = studio.project.clone();
    RemoveSend::new(studio.part, 0).apply(&mut there).unwrap();
    AddSend::new(studio.part, studio.master)
        .apply(&mut there)
        .unwrap();
    assert!(
        across_the_wire(&level).apply(&mut there).is_err(),
        "the send at that place is a different send"
    );
}

/// F8. Every insert and every send has an id of its own, and a copy of a
/// chain is a copy with ids of its own.
#[test]
fn every_slot_has_an_id_of_its_own() {
    let studio = a_studio();
    let mut project = studio.project.clone();
    DuplicateChannel::new(studio.keys)
        .apply(&mut project)
        .unwrap();
    ApplyTrackChain::new(studio.bus, a_chain())
        .apply(&mut project)
        .unwrap();
    let mut seen = std::collections::HashSet::new();
    for track in project.mixer.tracks.values() {
        for slot in &track.inserts {
            assert!(seen.insert(slot.id), "two inserts share {:?}", slot.id);
        }
        for send in &track.sends {
            assert!(seen.insert(send.id), "two sends share {:?}", send.id);
        }
    }
    assert!(seen.len() >= 8);
    let _: PersistentId = *seen.iter().next().unwrap();
}

/// F10. `prefab.rs` said a `NoteId` "is minted fresh every session", and the
/// arena's serde keeps `(index, version)` pairs. A join is only possible if
/// the second is true: a note named by an edit on one machine has to be the
/// same note on the other, and the other got its copy through a file.
///
/// The ids are made awkward on purpose: a hole where a note was deleted, and
/// a slot whose version has moved past one because it was reused.
#[test]
fn note_ids_survive_save_and_load() {
    let studio = a_studio();
    let (mut project, clip) = (studio.project, studio.notes_clip);
    let note_ids = |project: &Project| -> Vec<(NoteId, u8)> {
        match &project.clips[clip].source {
            ClipSource::Notes(data) => data.notes.iter().map(|(id, n)| (id, n.key)).collect(),
            _ => panic!("a note clip"),
        }
    };
    let first = note_ids(&project);
    RemoveNotes::new(clip, vec![first[0].0, first[1].0])
        .apply(&mut project)
        .unwrap();
    // Reuses a freed slot, so its version is past the first.
    AddNotes::new(clip, vec![a_note(PPQN * 3, 67)])
        .apply(&mut project)
        .unwrap();
    let before = note_ids(&project);
    assert_eq!(before.len(), 2);

    let bundle = scratch("note-ids");
    save_project(&project, &bundle).expect("save");
    let mut back = load_project(&bundle).expect("load");
    std::fs::remove_dir_all(&bundle).ok();

    assert_eq!(
        note_ids(&back),
        before,
        "a note is the same note, under the same id, after a trip to disk"
    );
    // And an edit written against the ids before the save finds its notes
    // after it — which is what a joiner's copy has to do with the host's edits.
    RemoveNotes::new(clip, vec![before[1].0])
        .apply(&mut back)
        .expect("an id from before the save names a note after it");
    assert_eq!(note_ids(&back), vec![before[0]]);
}

/// F53. A prefab's overrides are a map keyed by (element, property) and a
/// set of elements. As a `HashMap` the map could not be written as JSON at
/// all once it held anything — a tuple is not an object key — so the first
/// override ever made would have failed the save; and a `HashSet` writes in
/// whatever order it iterates, which would give two copies of one song two
/// hashes. Both are empty in every project today; this holds the shape for
/// the day they are not.
#[test]
fn an_override_map_writes_one_way_whatever_order_it_was_filled_in() {
    use fontelle_model::{ElementId, OverrideMap, PropKey, PropValue};
    let ids: Vec<ElementId> = (0..4).map(|_| ElementId(PersistentId::new())).collect();
    let fill = |order: &[usize]| {
        let mut map = OverrideMap::default();
        for &i in order {
            map.props
                .insert((ids[i], PropKey::Velocity), PropValue::Int(i as i64));
            map.props
                .insert((ids[i], PropKey::Transpose), PropValue::Float(i as f64));
            map.removed.insert(ids[i]);
        }
        map
    };
    let one = serde_json::to_string(&fill(&[0, 1, 2, 3])).expect("an override map writes");
    let other = serde_json::to_string(&fill(&[3, 1, 0, 2])).unwrap();
    assert_eq!(one, other, "the same overrides are the same bytes");

    let back: OverrideMap = serde_json::from_str(&one).expect("and reads back");
    assert_eq!(serde_json::to_string(&back).unwrap(), one);

    // Every project written so far has an empty map as a JSON object.
    let old: OverrideMap =
        serde_json::from_str(r#"{"props":{},"added":[],"removed":[]}"#).expect("old files read");
    assert!(old.props.is_empty() && old.removed.is_empty());
}
