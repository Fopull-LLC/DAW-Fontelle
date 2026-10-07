//! Vocaloid 3 and 4's `.vsqx`: notes, lyrics, locked phonemes, the
//! pre-measure, tempo, and the controller curves as their nearest lanes.

mod common;

use common::{assert_near, lane};
use fontelle_sing::Lane;
use fontelle_vocal_files::vsqx;

const V4: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="no"?>
<vsq4 xmlns="http://www.yamaha.co.jp/vocaloid/schema/vsq4/" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">
  <vender><![CDATA[Yamaha corporation]]></vender>
  <version><![CDATA[4.0.0.3]]></version>
  <masterTrack>
    <seqName><![CDATA[Little Song]]></seqName>
    <comment><![CDATA[]]></comment>
    <resolution>480</resolution>
    <preMeasure>1</preMeasure>
    <timeSig><m>0</m><nu>4</nu><de>4</de></timeSig>
    <tempo><t>0</t><v>12000</v></tempo>
    <tempo><t>3840</t><v>15000</v></tempo>
  </masterTrack>
  <vsTrack>
    <tNo>0</tNo>
    <name><![CDATA[Lead]]></name>
    <vsPart>
      <t>1920</t>
      <playTime>3840</playTime>
      <name><![CDATA[NewPart]]></name>
      <singer><t>0</t><bs>0</bs><pc>0</pc></singer>
      <cc><t>0</t><v id="D">64</v></cc>
      <cc><t>480</t><v id="D">127</v></cc>
      <cc><t>0</t><v id="S">2</v></cc>
      <cc><t>600</t><v id="P">4096</v></cc>
      <cc><t>700</t><v id="P">0</v></cc>
      <cc><t>0</t><v id="B">127</v></cc>
      <cc><t>0</t><v id="G">0</v></cc>
      <note>
        <t>0</t><dur>480</dur><n>60</n><v>64</v>
        <y><![CDATA[a]]></y><p><![CDATA[a]]></p>
        <nStyle><v id="accent">50</v><v id="bendDep">8</v><v id="bendLen">0</v><v id="decay">50</v><v id="fallPort">0</v><v id="opening">127</v><v id="risePort">0</v><v id="vibLen">0</v><v id="vibType">0</v></nStyle>
      </note>
      <note>
        <t>480</t><dur>960</dur><n>62</n><v>127</v>
        <y><![CDATA[ka]]></y><p lock="1"><![CDATA[k a]]></p>
        <nStyle><v id="accent">50</v><v id="vibLen">50</v><v id="vibType">1</v>
          <seq id="vibDep"><cc><p>0</p><v>64</v></cc></seq>
          <seq id="vibRate"><cc><p>0</p><v>64</v></cc></seq>
        </nStyle>
      </note>
    </vsPart>
  </vsTrack>
</vsq4>
"#;

const V3: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<vsq3 xmlns="http://www.yamaha.co.jp/vocaloid/schema/vsq3/">
  <masterTrack>
    <seqName><![CDATA[Three]]></seqName>
    <resolution>480</resolution>
    <preMeasure>2</preMeasure>
    <timeSig><posMes>0</posMes><nume>3</nume><denomi>4</denomi></timeSig>
    <tempo><posTick>0</posTick><bpm>9000</bpm></tempo>
  </masterTrack>
  <vsTrack>
    <vsTrackNo>0</vsTrackNo>
    <trackName><![CDATA[Voice]]></trackName>
    <musicalPart>
      <posTick>2880</posTick>
      <playTime>1920</playTime>
      <partName><![CDATA[P]]></partName>
      <mCtrl><posTick>0</posTick><attr id="DYN">0</attr></mCtrl>
      <note>
        <posTick>0</posTick><durTick>240</durTick><noteNum>67</noteNum><velocity>64</velocity>
        <lyric><![CDATA[la]]></lyric><phnms><![CDATA[l a]]></phnms>
      </note>
    </musicalPart>
  </vsTrack>
</vsq3>
"#;

#[test]
fn notes_and_tempo_lose_the_pre_measure() {
    let song = vsqx::read(V4, "file").unwrap();
    assert_eq!(song.name, "Little Song");
    let tempo: Vec<(i64, f64)> = song.tempo.iter().map(|s| (s.start_tick, s.bpm)).collect();
    // One bar of 4/4 before the song, at 480 a quarter.
    assert_eq!(tempo, vec![(0, 120.0), (3840, 150.0)]);
    let part = &song.parts[0];
    assert_eq!(part.name, "Lead");
    let notes: Vec<(i64, i64, u8, &str)> = part
        .notes
        .iter()
        .map(|n| (n.start, n.length, n.key, n.lyric.as_str()))
        .collect();
    assert_eq!(notes, vec![(0, 960, 60, "a"), (960, 1920, 62, "ka")]);
    // Vocaloid's velocity is consonant speed too: 64 is the voice's own.
    assert_eq!(part.notes[0].velocity, 100);
    assert_eq!(part.notes[1].velocity, 127);
}

#[test]
fn locked_phonemes_and_vibrato_are_the_notes_voice() {
    let song = vsqx::read(V4, "file").unwrap();
    let part = &song.parts[0];
    assert!(
        part.notes[0]
            .voice
            .as_ref()
            .is_none_or(|v| v.phonemes.is_none()),
        "an unlocked reading is Vocaloid's, not the writer's"
    );
    let voice = part.notes[1].voice.as_ref().expect("voice");
    let symbols: Vec<String> = voice
        .phonemes
        .clone()
        .unwrap()
        .into_iter()
        .map(|p| p.symbol)
        .collect();
    assert_eq!(symbols, vec!["k", "a"]);
    let v = voice.vibrato.expect("vibrato");
    assert_near("half the note", v.delay, 0.5, 1e-3);
    assert!(v.depth > 10.0 && v.rate > 3.0, "{v:?}");
}

#[test]
fn controllers_are_held_values_on_their_lanes() {
    let song = vsqx::read(V4, "file").unwrap();
    let part = &song.parts[0];
    assert_near("DYN 64", lane(part, Lane::Dynamics, 100), 0.0, 0.01);
    assert_near("DYN 127", lane(part, Lane::Dynamics, 1000), 6.0, 0.01);
    // PIT 4096 of 8192 at a range (PBS) of 2 semitones is one semitone.
    assert_near("PIT", lane(part, Lane::PitchBend, 1300), 100.0, 0.5);
    assert_near("PIT before", lane(part, Lane::PitchBend, 1190), 0.0, 0.5);
    assert_near("PIT after", lane(part, Lane::PitchBend, 1500), 0.0, 0.5);
    assert_near("BRE", lane(part, Lane::Breathiness, 500), 1.0, 0.01);
    // GEN under 64 is younger/brighter.
    assert_near("GEN", lane(part, Lane::Gender, 500), 600.0, 1.0);
}

#[test]
fn vocaloid_3_reads_by_its_own_names() {
    let song = vsqx::read(V3, "file").unwrap();
    assert_eq!(song.beats_per_bar, 3);
    assert_eq!(song.tempo[0].bpm, 90.0);
    let part = &song.parts[0];
    assert_eq!(part.name, "Voice");
    // Two bars of 3/4 before the song.
    assert_eq!(part.notes[0].start, 0);
    assert_eq!(part.notes[0].length, 480);
    assert_eq!(part.notes[0].lyric, "la");
    assert_near("DYN 0", lane(part, Lane::Dynamics, 100), -24.0, 0.01);
}

#[test]
fn not_a_vsqx_says_so() {
    assert!(vsqx::read("<html/>", "x").is_err());
    assert!(vsqx::read("not xml at all <", "x").is_err());
}
