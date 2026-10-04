//! The transport's flags the callback reads beside its state.

/// The main thread sometimes needs the graph to run for a moment while the
/// transport is stopped and nothing sounds: a plugin handed a state takes it
/// on its next block, and a processor asked back is given up at the top of
/// one. `summon` is that — the same wakefulness an open editor asks for,
/// counted, so two askers do not cancel each other.
#[test]
fn a_summoned_transport_keeps_the_graph_awake_until_everyone_is_done() {
    let transport = fontelle_engine::Transport::new();
    assert!(!transport.is_attended());
    transport.summon();
    transport.summon();
    assert!(transport.is_attended());
    transport.dismiss();
    assert!(transport.is_attended(), "one asker is still waiting");
    transport.dismiss();
    assert!(!transport.is_attended());
    // And an editor's own flag is untouched by it.
    transport.set_attended(true);
    transport.summon();
    transport.dismiss();
    assert!(transport.is_attended());
}

/// A render plays the studio's own plugins, so the live graph stands still
/// while it does: `hold` is that, counted like `summon`, and nothing to do
/// with the transport's state — the song's position is where it was left.
#[test]
fn a_held_transport_is_held_until_everyone_lets_go() {
    let transport = fontelle_engine::Transport::new();
    assert!(!transport.is_held());
    transport.hold();
    transport.hold();
    transport.release();
    assert!(transport.is_held(), "one holder is still rendering");
    transport.release();
    assert!(!transport.is_held());
    transport.release();
    assert!(!transport.is_held(), "a release too many is not a hold");
}
