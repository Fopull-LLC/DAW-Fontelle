//! MusicXML: a sung part's notes and lyrics — syllables joined by hyphens,
//! ties made one note, slurred notes carrying the syllable on — rests,
//! tempo, the time signature, the first voice only, and `.mxl`.

mod common;

use common::assert_near;
use fontelle_vocal_files::{Format, musicxml};

const SCORE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE score-partwise PUBLIC "-//Recordare//DTD MusicXML 3.1 Partwise//EN" "http://www.musicxml.org/dtds/partwise.dtd">
<score-partwise version="3.1">
  <work><work-title>Little Tune</work-title></work>
  <part-list>
    <score-part id="P1"><part-name>Soprano</part-name></score-part>
    <score-part id="P2"><part-name>Piano</part-name></score-part>
  </part-list>
  <part id="P1">
    <measure number="1">
      <attributes>
        <divisions>2</divisions>
        <time><beats>4</beats><beat-type>4</beat-type></time>
      </attributes>
      <direction placement="above"><sound tempo="90"/></direction>
      <note>
        <pitch><step>C</step><octave>4</octave></pitch>
        <duration>2</duration><voice>1</voice><type>quarter</type>
        <lyric number="1"><syllabic>begin</syllabic><text>Hap</text></lyric>
      </note>
      <note>
        <pitch><step>D</step><octave>4</octave></pitch>
        <duration>2</duration><voice>1</voice><type>quarter</type>
        <notations><slur type="start" number="1"/></notations>
        <lyric number="1"><syllabic>end</syllabic><text>py</text></lyric>
      </note>
      <note>
        <pitch><step>E</step><octave>4</octave></pitch>
        <duration>2</duration><voice>1</voice><type>quarter</type>
        <notations><slur type="stop" number="1"/></notations>
      </note>
      <note>
        <rest/>
        <duration>2</duration><voice>1</voice><type>quarter</type>
      </note>
      <backup><duration>8</duration></backup>
      <note>
        <pitch><step>C</step><octave>3</octave></pitch>
        <duration>8</duration><voice>2</voice><type>whole</type>
        <lyric number="1"><text>low</text></lyric>
      </note>
    </measure>
    <measure number="2">
      <note>
        <pitch><step>F</step><octave>4</octave></pitch>
        <duration>8</duration><voice>1</voice><type>whole</type>
        <tie type="start"/>
        <notations><tied type="start"/></notations>
        <lyric number="1"><syllabic>single</syllabic><text>day</text></lyric>
      </note>
    </measure>
    <measure number="3">
      <note>
        <pitch><step>F</step><octave>4</octave></pitch>
        <duration>2</duration><voice>1</voice><type>quarter</type>
        <tie type="stop"/>
        <notations><tied type="stop"/></notations>
      </note>
      <note>
        <pitch><step>G</step><octave>4</octave></pitch>
        <duration>2</duration><voice>1</voice><type>quarter</type>
        <lyric number="1"><text>to</text></lyric>
      </note>
      <note>
        <chord/>
        <pitch><step>B</step><octave>4</octave></pitch>
        <duration>2</duration><voice>1</voice><type>quarter</type>
      </note>
      <note>
        <pitch><step>A</step><alter>-1</alter><octave>4</octave></pitch>
        <duration>4</duration><voice>1</voice><type>half</type>
        <lyric number="1"><syllabic>single</syllabic><text>you</text><extend/></lyric>
      </note>
    </measure>
  </part>
  <part id="P2">
    <measure number="1">
      <attributes><divisions>1</divisions></attributes>
      <note><pitch><step>C</step><octave>3</octave></pitch><duration>4</duration></note>
    </measure>
  </part>
</score-partwise>
"#;

#[test]
fn the_sung_part_and_its_first_voice() {
    let song = musicxml::read(SCORE, "file").unwrap();
    assert_eq!(song.name, "Little Tune");
    assert_eq!(song.parts.len(), 1, "the piano has no words");
    let part = &song.parts[0];
    assert_eq!(part.name, "Soprano");
    let notes: Vec<(i64, i64, u8, &str)> = part
        .notes
        .iter()
        .map(|n| (n.start, n.length, n.key, n.lyric.as_str()))
        .collect();
    assert_eq!(
        notes,
        vec![
            // A word's syllables keep their hyphen, as typed lyrics do.
            (0, 960, 60, "Hap-"),
            (960, 960, 62, "py"),
            // Under the slur with no word of its own: the syllable goes on.
            (1920, 960, 64, "-"),
            // The rest is a gap; the tie is one note.
            (3840, 4800, 65, "day"),
            // Of the chord, the note written first.
            (8640, 960, 67, "to"),
            (9600, 1920, 68, "you"),
        ]
    );
}

#[test]
fn tempo_and_time_signature() {
    let song = musicxml::read(SCORE, "file").unwrap();
    assert_near("tempo", song.tempo[0].bpm as f32, 90.0, 1e-3);
    assert_eq!(song.beats_per_bar, 4);
}

#[test]
fn six_eight_is_three_quarter_beats() {
    let score = SCORE.replace(
        "<time><beats>4</beats><beat-type>4</beat-type></time>",
        "<time><beats>6</beats><beat-type>8</beat-type></time>",
    );
    let song = musicxml::read(&score, "file").unwrap();
    assert_eq!(song.beats_per_bar, 3);
    assert_eq!(song.beat_unit, 8);
}

#[test]
fn a_score_with_no_words_brings_its_first_part() {
    let score = r#"<score-partwise><part-list><score-part id="A"><part-name>Flute</part-name></score-part></part-list>
      <part id="A"><measure number="1"><attributes><divisions>1</divisions></attributes>
      <note><pitch><step>A</step><octave>4</octave></pitch><duration>1</duration></note>
      </measure></part></score-partwise>"#;
    let song = musicxml::read(score, "x").unwrap();
    assert_eq!(song.parts.len(), 1);
    assert_eq!(song.parts[0].notes[0].key, 69);
    assert_eq!(song.parts[0].notes[0].lyric, "");
}

#[test]
fn metronome_marks_count_too() {
    let score = r#"<score-partwise><part-list><score-part id="A"><part-name>V</part-name></score-part></part-list>
      <part id="A"><measure number="1"><attributes><divisions>1</divisions></attributes>
      <direction><direction-type><metronome><beat-unit>half</beat-unit><per-minute>40</per-minute></metronome></direction-type></direction>
      <note><pitch><step>A</step><octave>4</octave></pitch><duration>1</duration><lyric><text>la</text></lyric></note>
      </measure></part></score-partwise>"#;
    let song = musicxml::read(score, "x").unwrap();
    // Forty halves a minute is eighty quarters.
    assert_near("tempo", song.tempo[0].bpm as f32, 80.0, 1e-3);
}

/// A zip of `files`, deflated, as a notation program writes an `.mxl`.
fn zip(files: &[(&str, &str)]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut central = Vec::new();
    for (name, data) in files {
        let packed = miniz_oxide::deflate::compress_to_vec(data.as_bytes(), 6);
        let offset = out.len() as u32;
        let header = |sig: u32, central: bool| {
            let mut h = Vec::new();
            h.extend_from_slice(&sig.to_le_bytes());
            if central {
                h.extend_from_slice(&20u16.to_le_bytes());
            }
            h.extend_from_slice(&20u16.to_le_bytes());
            h.extend_from_slice(&0u16.to_le_bytes());
            h.extend_from_slice(&8u16.to_le_bytes());
            h.extend_from_slice(&0u32.to_le_bytes());
            h.extend_from_slice(&0u32.to_le_bytes());
            h.extend_from_slice(&(packed.len() as u32).to_le_bytes());
            h.extend_from_slice(&(data.len() as u32).to_le_bytes());
            h.extend_from_slice(&(name.len() as u16).to_le_bytes());
            h.extend_from_slice(&0u16.to_le_bytes());
            if central {
                h.extend_from_slice(&0u16.to_le_bytes());
                h.extend_from_slice(&0u16.to_le_bytes());
                h.extend_from_slice(&0u16.to_le_bytes());
                h.extend_from_slice(&0u32.to_le_bytes());
                h.extend_from_slice(&offset.to_le_bytes());
            }
            h.extend_from_slice(name.as_bytes());
            h
        };
        out.extend(header(0x0403_4b50, false));
        out.extend(&packed);
        central.extend(header(0x0201_4b50, true));
    }
    let at = out.len() as u32;
    out.extend(&central);
    out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    out.extend_from_slice(&[0, 0, 0, 0]);
    out.extend_from_slice(&(files.len() as u16).to_le_bytes());
    out.extend_from_slice(&(files.len() as u16).to_le_bytes());
    out.extend_from_slice(&(central.len() as u32).to_le_bytes());
    out.extend_from_slice(&at.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}

#[test]
fn a_compressed_score_reads_through_its_container() {
    let container = r#"<?xml version="1.0"?><container><rootfiles><rootfile full-path="music/song.xml"/></rootfiles></container>"#;
    let bytes = zip(&[
        ("mimetype", "application/vnd.recordare.musicxml"),
        ("META-INF/container.xml", container),
        ("music/song.xml", SCORE),
    ]);
    let song = musicxml::read_mxl(&bytes, "x").unwrap();
    assert_eq!(song.parts[0].notes.len(), 6);
    // And by the generic entry point, which sees the zip.
    let again = fontelle_vocal_files::read(Format::MusicXml, &bytes, "x").unwrap();
    assert_eq!(again.parts[0].notes.len(), 6);
}

#[test]
fn an_xml_file_is_only_musicxml_when_it_says_so() {
    assert!(musicxml::looks_like_musicxml(SCORE));
    assert!(!musicxml::looks_like_musicxml("<svg></svg>"));
}
