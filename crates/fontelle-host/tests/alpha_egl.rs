//! Refusing a plugin editor that would abort the studio on this machine's
//! EGL, and the setting that makes it draw.
//!
//! Reported (2026-10-05): Vital's editor, CLAP and VST 3, came up black and
//! aborted Fontelle — *"BGFX FATAL ... Failed to create surface"*. On NVIDIA's
//! EGL an 8-bit-alpha config has no depth-24 window to draw on; Mesa's EGL
//! has. See `fontelle_host::alpha_egl`.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use fontelle_host::alpha_egl::{
    AlphaEgl, EditorGate, NEEDS_ALPHA_EGL, NeedsAlphaEgl, editor_refusal, needs_alpha_egl,
    read_egl_answer,
};
use fontelle_types::{PluginFormat, PluginKey};

fn nvidia_fails() -> AlphaEgl {
    AlphaEgl::Fails {
        vendor: "NVIDIA".to_string(),
        why: "EGL_BAD_CONFIG".to_string(),
    }
}

// ------------------------------------------------------------ the table

#[test]
fn vital_is_known_by_every_key_it_has_and_by_its_name() {
    for key in [
        PluginKey::clap("audio.vital.synth"),
        PluginKey::new(PluginFormat::Vst3, "56535456697461766974616C00000000"),
    ] {
        assert!(
            needs_alpha_egl(NEEDS_ALPHA_EGL, &key, "Vital"),
            "{key} should be known"
        );
    }
    // A key nobody listed, under Vital's name: an LV2 or a rebuild.
    assert!(needs_alpha_egl(
        NEEDS_ALPHA_EGL,
        &PluginKey::new(PluginFormat::Lv2, "urn:example:vital"),
        "Vital"
    ));
}

#[test]
fn its_forks_and_other_synths_are_not() {
    // Vitalium and Vial draw with JUCE's OpenGL over GLX, which is fine.
    for (key, name) in [
        (PluginKey::clap("org.surge-synth-team.surge-xt"), "Surge XT"),
        (
            PluginKey::new(PluginFormat::Lv2, "urn:distrho:vitalium"),
            "Vitalium",
        ),
        (PluginKey::clap("org.vital.vial"), "Vial"),
    ] {
        assert!(
            !needs_alpha_egl(NEEDS_ALPHA_EGL, &key, name),
            "{name} should not be refused"
        );
    }
}

// ------------------------------------------------------------ the decision

#[test]
fn a_plugin_that_does_not_need_it_never_asks_the_machine() {
    let refusal = editor_refusal("Surge XT", false, false, || {
        panic!("the probe was run for a plugin that does not need it")
    });
    assert_eq!(refusal, None);
}

#[test]
fn where_the_surface_is_made_the_editor_opens() {
    let refusal = editor_refusal("Vital", true, false, || AlphaEgl::Works {
        vendor: "Mesa Project".to_string(),
    });
    assert_eq!(refusal, None);
}

#[test]
fn where_it_is_not_the_editor_is_refused_in_plain_words() {
    let refusal = editor_refusal("Vital", true, false, nvidia_fails);
    assert_eq!(
        refusal.as_deref(),
        Some(
            "Vital's window can't open with this graphics driver (NVIDIA on X11). \
             Its knobs are in Fontelle's panel. Turn on Settings \u{2192} Compatible \
             plugin graphics and restart to use its window."
        )
    );
}

#[test]
fn another_driver_is_named_by_its_vendor() {
    let refusal = editor_refusal("Vital", true, false, || AlphaEgl::Fails {
        vendor: "Some Vendor".to_string(),
        why: "EGL_BAD_MATCH".to_string(),
    })
    .expect("refused");
    assert!(refusal.contains("(Some Vendor on X11)"), "{refusal}");
}

#[test]
fn with_compatible_graphics_on_and_still_failing_it_says_so() {
    let refusal = editor_refusal("Vital", true, true, nvidia_fails).expect("still refused");
    assert!(
        refusal.contains("even with Compatible plugin graphics on"),
        "{refusal}"
    );
    assert!(refusal.contains("Fontelle's panel"), "{refusal}");
    assert!(!refusal.contains("Turn on"), "{refusal}");
}

#[test]
fn a_probe_that_crashed_is_a_refusal() {
    // A driver that dies making the surface would have taken the studio.
    let refusal = editor_refusal("Vital", true, false, || {
        AlphaEgl::Crashed("SIGABRT".to_string())
    });
    assert!(refusal.is_some());
}

#[test]
fn not_knowing_opens_the_editor_as_before() {
    let refusal = editor_refusal("Vital", true, false, || {
        AlphaEgl::Unknown("no X display".to_string())
    });
    assert_eq!(refusal, None);
}

// ------------------------------------------------------------ the gate

const FACE: NeedsAlphaEgl = NeedsAlphaEgl {
    name: "Fontelle Test Face",
    keys: &["clap:com.fopull.fontelle.testface"],
};

#[test]
fn the_gate_asks_the_machine_once_a_session() {
    let asked = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&asked);
    let gate = EditorGate::with(vec![FACE], false, move || {
        counter.fetch_add(1, Ordering::Relaxed);
        nvidia_fails()
    });
    let key = PluginKey::clap("com.fopull.fontelle.testface");
    assert!(gate.refusal(&key, "Fontelle Test Face").is_some());
    assert!(gate.refusal(&key, "Fontelle Test Face").is_some());
    assert!(
        gate.refusal(&PluginKey::clap("other"), "Other").is_none(),
        "only what is in its table"
    );
    assert_eq!(asked.load(Ordering::Relaxed), 1);
    assert_eq!(gate.probes(), 1);
}

#[test]
fn the_gate_that_is_off_refuses_nothing() {
    let gate = EditorGate::off();
    assert_eq!(
        gate.refusal(&PluginKey::clap("audio.vital.synth"), "Vital"),
        None
    );
    assert_eq!(gate.probes(), 0);
}

// ------------------------------------------------------------ the probe's answer

#[test]
fn the_probe_answer_is_read() {
    assert_eq!(
        read_egl_answer(
            "some driver chatter\n{\"surface\":true,\"vendor\":\"Mesa Project\"}\n",
            None
        ),
        AlphaEgl::Works {
            vendor: "Mesa Project".to_string()
        }
    );
    assert_eq!(
        read_egl_answer(
            "{\"surface\":false,\"vendor\":\"NVIDIA\",\"why\":\"EGL_BAD_CONFIG\"}",
            None
        ),
        nvidia_fails()
    );
    assert_eq!(
        read_egl_answer("{\"unknown\":\"no X display\"}", None),
        AlphaEgl::Unknown("no X display".to_string())
    );
}

#[test]
fn a_probe_that_died_without_an_answer_crashed() {
    assert_eq!(
        read_egl_answer("BGFX FATAL", Some("SIGABRT")),
        AlphaEgl::Crashed("SIGABRT".to_string())
    );
    // An answer before the crash (a driver's exit handler) is the answer.
    assert_eq!(
        read_egl_answer("{\"surface\":true,\"vendor\":\"NVIDIA\"}", Some("SIGSEGV")),
        AlphaEgl::Works {
            vendor: "NVIDIA".to_string()
        }
    );
}

#[test]
fn a_probe_that_said_nothing_readable_is_unknown() {
    assert!(matches!(read_egl_answer("", None), AlphaEgl::Unknown(_)));
    assert!(matches!(
        read_egl_answer("{not json", None),
        AlphaEgl::Unknown(_)
    ));
}

#[test]
fn not_a_probe_is_not_answered() {
    assert_eq!(
        fontelle_host::alpha_egl::egl_probe_main(&["--version".to_string()]),
        None
    );
}

/// The child itself, with no display to ask: it answers, and says why it
/// does not know.
#[cfg(target_os = "linux")]
#[test]
fn the_probe_child_with_no_display_answers_unknown() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_fontelle-scan-probe"))
        .arg(fontelle_host::alpha_egl::EGL_PROBE_FLAG)
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .output()
        .expect("the probe runs");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{:?}: {stdout}", output.status);
    assert!(
        matches!(read_egl_answer(&stdout, None), AlphaEgl::Unknown(_)),
        "{stdout}"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn running_the_probe_reads_its_answer() {
    // No display here either, so this is the whole round trip without
    // touching a GPU.
    let mut command = fontelle_host::alpha_egl::egl_probe_command(Path::new(env!(
        "CARGO_BIN_EXE_fontelle-scan-probe"
    )));
    command.env_remove("DISPLAY").env_remove("WAYLAND_DISPLAY");
    let answer =
        fontelle_host::alpha_egl::run_egl_probe(command, std::time::Duration::from_secs(10));
    assert!(matches!(answer, AlphaEgl::Unknown(_)), "{answer:?}");
}

#[test]
fn a_probe_that_cannot_be_started_is_unknown() {
    let command =
        fontelle_host::alpha_egl::egl_probe_command(&PathBuf::from("/nonexistent/fontelle"));
    let answer =
        fontelle_host::alpha_egl::run_egl_probe(command, std::time::Duration::from_secs(1));
    assert!(matches!(answer, AlphaEgl::Unknown(_)), "{answer:?}");
}

// ------------------------------------------------------------ the setting's variable

const MESA_SHARE: &str = "/usr/share/glvnd/egl_vendor.d/50_mesa.json";

#[test]
fn mesa_is_found_where_glvnd_looks() {
    let found = fontelle_host::gui::mesa_egl_vendor(|path| path == Path::new(MESA_SHARE));
    assert_eq!(found, Some(PathBuf::from(MESA_SHARE)));
    let etc = "/etc/glvnd/egl_vendor.d/50_mesa.json";
    let found = fontelle_host::gui::mesa_egl_vendor(|path| {
        path == Path::new(etc) || path == Path::new(MESA_SHARE)
    });
    assert_eq!(found, Some(PathBuf::from(etc)), "the system's own first");
    assert_eq!(fontelle_host::gui::mesa_egl_vendor(|_| false), None);
}

#[test]
fn the_variable_is_set_only_when_asked_for_and_free() {
    use fontelle_host::gui::egl_vendor_for;
    let mesa = |path: &Path| path == Path::new(MESA_SHARE);
    assert_eq!(
        egl_vendor_for(true, false, mesa),
        Some(PathBuf::from(MESA_SHARE))
    );
    assert_eq!(
        egl_vendor_for(false, false, mesa),
        None,
        "the setting is off"
    );
    assert_eq!(egl_vendor_for(true, true, mesa), None, "somebody set it");
    assert_eq!(egl_vendor_for(true, false, |_| false), None, "no Mesa");
}
