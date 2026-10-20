# Feedback round, 2026-10-20

Ty, after testing 0.29.0 with other people. Ten reports. For each one: what is
wrong, the cause found in the code, and the fix. Ty said to release on stable
once all of it is closed.

| # | Report | Cause | Fix |
|---|---|---|---|
| 1 | *"every typing input right now isnt a box you can click inside, clicking at all actually stops the entering process"* | `WindowApp::press` ends a rename and a typed tempo on **any** press, the field included. The name prompt spends a press on its field and does nothing with it. The two search boxes reset their entry on every press, which puts the caret back at the end. | A press inside the field that has the keyboard puts the caret where it landed. A drag selects, a double-click selects the word, Shift+click extends the selection. A press anywhere else still ends the field, as before. The caret's byte index comes from the shaped widths of the field's prefixes, measured by the window (INVARIANT 2); `TextEntry::place_at_x` takes those widths and is pure. |
| 2 | *"add smoothing to collaboration syncing on cursor and time bar"* | Whereabouts arrive at 15 Hz (`PRESENCE_EVERY`) and are drawn where the last one put them, so they step. | `canvas::PresenceGlide`: each person's pointer and time marker glide toward where they were last heard, and snap when they change place (another lane, another clip, a jump). A marker that is playing runs on at its heard pace between messages. Frames are asked for only while something is still gliding. |
| 3 | *"when leaving a shared session, then making a new song, and pressing share, it pops up the screen saying that the previous person in the last session stopped sharing"* | `Session::start_sharing` resets `told_end` while the ended session is still in `self.collab`. On the next pump `note_end` reads that session's ending again and shows its card a second time. | `told_end` is reset only when the new session is put in place (`share` / `begin_join` already do this). |
| 4 | *"chords tool is constantly moving around when clicking through the settings"* | `chord_strip_layout` places the strip above the highest key any chip would add, and sizes it to the number of chips. Both change with every control. | The strip's reach is worked out once, when it opens: the widest it could be, and the highest and lowest key any setting could give. It stays put until it closes. |
| 5 | *"add quantize tool and make ctrl q shortcut"* | None existed. | A Quantize dialog in the roll's Tools menu (grid, strength, starts or starts and ends), and `Action::Quantize` (Ctrl+Q) runs it on the selection at its current settings, or on every note when nothing is selected. The grid follows the roll's snap unless a grid is chosen. The pure part is `fontelle_model::quantized`. |
| 6 | *"make arpeggiator let you preview changes"* | The dialog applied blind. | While a tool's dialog is open its result is ghosted in the roll in the accent and updates with every setting. The Arpeggiate (and Quantize) dialogs have a *Hear it* row that plays the result once and replays it when a setting changes while it sounds. |
| 7 | *"add subtle cross pattern showing when notes overlap in the piano roll"* | Not drawn. | `canvas::note_overlaps` finds where two notes on one key overlap. The roll cross-hatches that stretch faintly over the notes. |
| 8 | *"add f11 to toggle fullscreen"* | Not bound. | `Action::Fullscreen` (F11, global), borderless on the current monitor. |
| 9 | *"pause playback while moving around your time marker via clicking and dragging and if you were playing before starting the move just resume after"* | A ruler drag seeks while the song keeps playing. | A ruler drag (arrangement, roll, bar) that starts while the song plays (and is not recording) stops the transport. Letting go plays from where the marker was dropped. |
| 10 | *"hard to tell where time change markers are ... add a little marker indicator into the time bar at the top to show where its actually placed like a pin"* | The flag sits 24 px right of its bar line, clear of the bar's number, with nothing at the line itself. | `canvas::meter_pins`: a pin head at the top of the ruler on the bar line, a stem down the ruler, and a short leader to the flag. |

## Hub work alongside

- **0428, install ping**: the contract merged (caef4af). Built to the
  contract, with the notice on first launch, nothing sent on that launch,
  a Settings toggle and `FOPULL_NO_PING`. The contract's link
  (`fopull.com/privacy#install-ping`) now redirects to the live policy, so
  the notice uses it as written.
- **0429**: the store screenshots and trailer, due Mon 2026-10-27.

## Caveats

- Item 1 covers the fields that take typing: the name prompt, the tempo box,
  the browser search, the Flopsynth preset search and the four inline renames
  (rack, prefabs, lane headers, mixer strips). The Notepad and the theme
  editor's code card could already be clicked. The DisgustingBeat scene chips
  and the lyric field still end on a press.
- Item 2 smooths only what is drawn. What is sent is unchanged, so two
  studios on different versions still work together.
- Item 9 does not pause a take that is recording: stopping it would end
  the take.
