//! From lyrics to the aliases a bank has.

use fontelle_sing::Phrase;

use crate::bank::Voicebank;
use crate::{dict, kana};

/// How a bank's aliases are named, and so how lyrics become them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Phonemizer {
    /// Japanese, one sample per mora: "か".
    JaCv,
    /// Japanese, vowel-to-mora: "a か", "- か".
    JaVcv,
    /// Japanese, mora plus vowel-to-consonant: "か", "a k".
    JaCvvc,
    /// English diphones over ARPAbet: "- hh", "hh ah", "ah l", "ow -".
    EnArpasing,
    /// English VCCV: "-ka", "ka", "a k", "ak".
    EnVccv,
    /// English CVVC in X-SAMPA: "- ka", "ka", "{ k", "eI t-", "oU -".
    EnCvvc,
    /// Chinese on pinyin aliases: "ni", "- ni", "i h", "ao R".
    ZhCvvc,
    /// Korean on hangul aliases: "가", "- 가", "a k", "a R".
    KoCvvc,
}

impl Phonemizer {
    /// In setting order: new ones are appended, so a saved setting keeps
    /// its meaning.
    pub const ALL: [Phonemizer; 8] = [
        Phonemizer::JaCv,
        Phonemizer::JaVcv,
        Phonemizer::JaCvvc,
        Phonemizer::EnArpasing,
        Phonemizer::EnVccv,
        Phonemizer::EnCvvc,
        Phonemizer::ZhCvvc,
        Phonemizer::KoCvvc,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Phonemizer::JaCv => "Japanese CV",
            Phonemizer::JaVcv => "Japanese VCV",
            Phonemizer::JaCvvc => "Japanese CVVC",
            Phonemizer::EnArpasing => "English ARPAsing",
            Phonemizer::EnVccv => "English VCCV",
            Phonemizer::EnCvvc => "English CVVC (X-SAMPA)",
            Phonemizer::ZhCvvc => "Chinese CV/CVVC (pinyin)",
            Phonemizer::KoCvvc => "Korean CV/CVVC (hangul)",
        }
    }

    /// Its value in the `phonemizer` setting (0 is automatic).
    pub fn setting_value(self) -> f32 {
        (Phonemizer::ALL.iter().position(|&p| p == self).unwrap_or(0) + 1) as f32
    }

    /// The setting's value back; `None` for automatic.
    pub fn from_setting(value: f32) -> Option<Phonemizer> {
        let i = value.round();
        if i < 1.0 {
            return None;
        }
        Phonemizer::ALL.get(i as usize - 1).copied()
    }

    pub fn language(self) -> &'static str {
        match self {
            Phonemizer::JaCv | Phonemizer::JaVcv | Phonemizer::JaCvvc => "ja",
            Phonemizer::EnArpasing | Phonemizer::EnVccv | Phonemizer::EnCvvc => "en",
            Phonemizer::ZhCvvc => "zh",
            Phonemizer::KoCvvc => "ko",
        }
    }
}

/// Which phonemizer a bank's aliases call for: `character.yaml`'s
/// `default_phonemizer` when it names one, else the shape of its aliases.
pub fn detect(bank: &Voicebank) -> Phonemizer {
    if let Some(hint) = &bank.default_phonemizer {
        let h = hint.to_ascii_lowercase();
        let named = if h.contains("arpasing") || h.contains("arpa") {
            Some(Phonemizer::EnArpasing)
        } else if h.contains("vccv") && (h.contains("english") || h.contains(".en")) {
            Some(Phonemizer::EnVccv)
        } else if h.contains("cvvc") && h.contains("japanese") {
            Some(Phonemizer::JaCvvc)
        } else if h.contains("vcv") && h.contains("japanese") {
            Some(Phonemizer::JaVcv)
        } else if h.contains("japanese") && h.contains("cv") {
            Some(Phonemizer::JaCv)
        } else if h.contains("chinese") || h.contains(".zh") {
            Some(Phonemizer::ZhCvvc)
        } else if h.contains("korean") || h.contains(".ko") {
            Some(Phonemizer::KoCvvc)
        } else {
            None
        };
        if let Some(p) = named {
            return p;
        }
    }
    let (mut arpa, mut vcv, mut cvvc, mut vccv) = (0usize, 0usize, 0usize, 0usize);
    let mut xsampa = 0usize;
    let mut kana_aliases = 0usize;
    let (mut hangul_aliases, mut pinyin_aliases) = (0usize, 0usize);
    let arpa_token = |t: &str| {
        let t = t.trim_end_matches(|c: char| c.is_ascii_digit());
        t == "-" || dict::is_vowel(t) || dict::CONSONANTS.contains(&t)
    };
    let has_kana = |s: &str| s.chars().any(|c| matches!(c as u32, 0x3041..=0x30FF));
    for alias in bank.base_aliases() {
        if has_kana(&alias) {
            kana_aliases += 1;
        }
        if fontelle_lang::hangul::has_hangul(&alias) {
            hangul_aliases += 1;
        }
        // Syllables only pinyin spells: no Japanese romaji or English has
        // them as an alias.
        let last = alias.rsplit(' ').next().unwrap_or("");
        if fontelle_lang::pinyin::is_syllable(last)
            && [
                "zh", "ch", "sh", "x", "q", "ng", "ui", "iu", "ie", "ve", "ia", "uo", "v",
            ]
            .iter()
            .any(|m| last.contains(m))
        {
            pinyin_aliases += 1;
        }
        if alias.split([' ', '-']).any(|t| {
            ["{", "@", "eI", "aI", "aU", "oU", "OI", "3", "tS", "dZ", "V"]
                .iter()
                .any(|x| t == *x || (t.len() > x.len() && t.ends_with(x) && t.is_ascii()))
        }) {
            xsampa += 1;
        }
        match alias.split_once(' ') {
            Some((a, b)) if !b.contains(' ') => {
                // A vowel says ARPAbet: "b d" and "- b" are X-SAMPA too.
                if arpa_token(a)
                    && arpa_token(b)
                    && [a, b]
                        .iter()
                        .any(|t| dict::is_vowel(t.trim_end_matches(|c: char| c.is_ascii_digit())))
                {
                    arpa += 1;
                }
                // "- か" and "* か" are in single-sound banks too; only a
                // vowel before the mora says VCV.
                let v_like = ["a", "i", "u", "e", "o", "n"].contains(&a);
                if v_like && has_kana(b) {
                    vcv += 1;
                } else if v_like
                    && !b.is_empty()
                    && b.chars().all(|c| c.is_ascii_lowercase())
                    && !["a", "i", "u", "e", "o"].contains(&b)
                {
                    cvvc += 1;
                }
            }
            None if alias.is_ascii() && alias.len() >= 2 => {
                let starts = alias.starts_with('-')
                    && alias[1..]
                        .starts_with(|c: char| c.is_ascii_alphanumeric() || c == '@' || c == '&');
                let ends = alias.ends_with('-') && !alias.starts_with('-');
                if starts || ends {
                    vccv += 1;
                }
            }
            _ => {}
        }
    }
    if hangul_aliases >= 30 && hangul_aliases > kana_aliases {
        Phonemizer::KoCvvc
    } else if pinyin_aliases >= 20 && kana_aliases == 0 && pinyin_aliases * 2 > arpa {
        Phonemizer::ZhCvvc
    } else if arpa >= 30 && arpa > vcv + cvvc && kana_aliases * 4 < arpa {
        Phonemizer::EnArpasing
    } else if xsampa >= 40
        && kana_aliases == 0
        && xsampa > vccv
        && !has_any(bank, &["-ka", "-ba", "-re"])
    {
        Phonemizer::EnCvvc
    } else if vccv >= 8 && kana_aliases == 0 && vccv * 2 > arpa {
        Phonemizer::EnVccv
    } else if vcv >= 30 && vcv >= cvvc {
        Phonemizer::JaVcv
    } else if cvvc >= 15 {
        Phonemizer::JaCvvc
    } else {
        Phonemizer::JaCv
    }
}
fn has_any(bank: &Voicebank, aliases: &[&str]) -> bool {
    aliases.iter().any(|a| bank.has(a))
}

/// One sample to sing, before it is placed in time.
#[derive(Debug, Clone, PartialEq)]
pub struct PlannedUnit {
    /// The alias as the bank names it, without subbank or colour prefixes
    /// and suffixes (those are applied per note when it is found).
    pub alias: String,
    /// The phrase note it belongs to (its pitch picks the subbank).
    pub note: usize,
    /// Phrase seconds its preutterance lands on.
    pub anchor: f64,
    /// A hand-placed start (phrase seconds), which placement keeps.
    pub start: Option<f64>,
    /// An ending (`a R`, `ow -`): sung at its own length, not stretched.
    pub tail: bool,
}

/// What a phrase is sung as.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Plan {
    pub units: Vec<PlannedUnit>,
    /// Per note: the reading was guessed.
    pub guessed: Vec<bool>,
}

/// Notes further apart than this have a rest between them.
pub const GAP: f64 = 0.02;
/// How long a consonant gets in English diphone banks, at velocity 1.
pub const CONSONANT: f64 = 0.06;

/// Turns a phrase's lyrics into units of `bank` with `kind`.
pub fn plan(bank: &Voicebank, phrase: &Phrase, kind: Phonemizer) -> Plan {
    let ctx = Ctx::new(bank, phrase);
    let mut ctx = ctx;
    if matches!(kind, Phonemizer::ZhCvvc | Phonemizer::KoCvvc) {
        ctx.scripts = scripts(&ctx, kind);
    }
    let english = kind.language() == "en";
    let events = events(&ctx, english, kind);
    let mut plan = Plan {
        units: Vec::new(),
        guessed: vec![false; phrase.notes.len()],
    };
    for e in &events {
        if e.guessed {
            plan.guessed[e.note] = true;
        }
    }
    match kind {
        Phonemizer::JaCv | Phonemizer::JaVcv | Phonemizer::JaCvvc => {
            japanese(&ctx, &events, kind, &mut plan)
        }
        Phonemizer::EnArpasing | Phonemizer::EnVccv | Phonemizer::EnCvvc => {
            english_units(&ctx, &events, kind, &mut plan)
        }
        Phonemizer::ZhCvvc | Phonemizer::KoCvvc => syllabic(&ctx, &events, &mut plan),
    }
    plan.units.sort_by(|a, b| a.anchor.total_cmp(&b.anchor));
    plan
}

struct Ctx<'a> {
    bank: &'a Voicebank,
    notes: &'a [fontelle_sing::PhraseNote],
    tones: Vec<u8>,
    colours: Vec<String>,
    /// Chinese or Korean: each note's syllables read with its run of
    /// notes (words across notes, a coda carried over), when it is written
    /// in that script.
    scripts: Vec<Option<Vec<Content>>>,
}

impl<'a> Ctx<'a> {
    fn new(bank: &'a Voicebank, phrase: &'a Phrase) -> Self {
        Ctx {
            bank,
            notes: &phrase.notes,
            tones: phrase
                .notes
                .iter()
                .map(|n| n.pitch.round().clamp(0.0, 127.0) as u8)
                .collect(),
            colours: phrase
                .notes
                .iter()
                .map(|n| {
                    n.options
                        .get(crate::NOTE_OPTION_COLOUR)
                        .cloned()
                        .unwrap_or_default()
                })
                .collect(),
            scripts: Vec::new(),
        }
    }

    fn has(&self, alias: &str, note: usize) -> bool {
        self.bank
            .find(alias, self.tones[note], &self.colours[note])
            .is_some()
    }

    /// The first candidate the bank has.
    fn first(&self, candidates: &[String], note: usize) -> Option<String> {
        candidates.iter().find(|c| self.has(c, note)).cloned()
    }

    /// The preutterance of `alias` at `note`, seconds.
    fn pre(&self, alias: &str, note: usize) -> f64 {
        self.bank
            .find(alias, self.tones[note], &self.colours[note])
            .map(|e| e.preutterance / 1000.0)
            .unwrap_or(0.0)
    }

    fn velocity(&self, note: usize) -> f64 {
        f64::from(self.notes[note].velocity).clamp(0.1, 10.0)
    }

    /// Whether a rest separates notes `a` and `b` (`a < b`).
    fn rest_between(&self, a: usize, b: usize) -> bool {
        (a + 1..=b).any(|i| self.notes[i].start - self.notes[i - 1].end() > GAP)
    }
}

/// What one sung syllable is.
#[derive(Debug, Clone)]
enum Content {
    /// Japanese: a mora in hiragana (or the lyric as written when it is
    /// not kana).
    Mora(String),
    /// English: one syllable's phonemes.
    Syllable(Vec<String>),
    /// Aliases written in brackets, used as given.
    Aliases(Vec<String>),
    /// Hand-placed phonemes: (alias, start).
    Hand(Vec<(String, Option<f64>)>),
    /// Chinese: a pinyin syllable.
    Zh(String),
    /// Korean: a syllable's sounds.
    Ko(fontelle_lang::hangul::Syllable),
}

#[derive(Debug, Clone)]
struct Event {
    note: usize,
    /// The last note it is held through (slurs).
    last: usize,
    time: f64,
    content: Content,
    guessed: bool,
}

fn default_syllable(english: bool) -> &'static str {
    if english { "la" } else { "ら" }
}

/// The lyric rules every singer shares: slurs, hyphen joins, brackets,
/// several syllables on one note.
fn events(ctx: &Ctx, english: bool, kind: Phonemizer) -> Vec<Event> {
    let notes = ctx.notes;
    let mut out: Vec<Event> = Vec::new();
    let mut i = 0;
    while i < notes.len() {
        let n = &notes[i];
        if let Some(hand) = &n.phonemes {
            out.push(Event {
                note: i,
                last: i,
                time: n.start,
                content: Content::Hand(
                    hand.iter()
                        .map(|p| (p.symbol.clone(), p.offset.map(|o| n.start + o)))
                        .collect(),
                ),
                guessed: false,
            });
            i += 1;
            continue;
        }
        let lyric = n.lyric.trim();
        if lyric == fontelle_sing::LYRIC_SLUR {
            // A slur touching the note before only holds it.
            if i > 0 && !ctx.rest_between(i - 1, i) {
                if let Some(last) = out.last_mut() {
                    last.last = i;
                }
                i += 1;
                continue;
            }
            // After a rest it sings the last vowel again.
            let content = match out.last().map(|e| &e.content) {
                Some(Content::Syllable(p)) => Content::Syllable(vec![
                    p.iter()
                        .find(|x| dict::is_vowel(x))
                        .cloned()
                        .unwrap_or_else(|| "ah".into()),
                ]),
                Some(Content::Mora(m)) => Content::Mora(vowel_kana(m).unwrap_or("あ").to_string()),
                Some(Content::Zh(s)) => Content::Zh(zh_vowel(s).to_string()),
                Some(Content::Ko(s)) => Content::Ko(s.vowel_only()),
                _ if kind == Phonemizer::ZhCvvc => Content::Zh("a".into()),
                _ if kind == Phonemizer::KoCvvc => Content::Ko(ko_default()),
                _ => single(default_syllable(english), english).0,
            };
            out.push(Event {
                note: i,
                last: i,
                time: n.start,
                content,
                guessed: false,
            });
            i += 1;
            continue;
        }
        // The notes this lyric covers: hyphen-joined pieces (English) and
        // the slurs after them.
        let mut pieces = vec![(i, lyric.to_string())];
        let mut j = i + 1;
        if english && !lyric.starts_with('[') {
            while pieces
                .last()
                .is_some_and(|p| p.1.ends_with('-') && p.1.len() > 1)
                && j < notes.len()
                && notes[j].phonemes.is_none()
                && notes[j].lyric.trim() != fontelle_sing::LYRIC_SLUR
            {
                pieces.push((j, notes[j].lyric.trim().to_string()));
                j += 1;
            }
        }
        let mut slots: Vec<usize> = pieces.iter().map(|p| p.0).collect();
        while j < notes.len()
            && notes[j].lyric.trim() == fontelle_sing::LYRIC_SLUR
            && notes[j].phonemes.is_none()
            && !ctx.rest_between(j - 1, j)
        {
            slots.push(j);
            j += 1;
        }
        let (syllables, guessed) = if english {
            let word: String = pieces
                .iter()
                .map(|p| p.1.trim_end_matches('-').to_string())
                .collect();
            english_syllables(&word)
        } else {
            let text = lyric.trim_end_matches('-');
            match kind {
                Phonemizer::ZhCvvc | Phonemizer::KoCvvc => cjk_syllables(ctx, text, i, kind),
                _ => japanese_morae(ctx, text, i),
            }
        };
        spread(ctx, &syllables, &slots, guessed, &mut out);
        i = j;
    }
    out
}

/// Syllables over slots: one each in order; spare slots hold the last;
/// spare syllables share the last slot.
fn spread(ctx: &Ctx, syllables: &[Content], slots: &[usize], guessed: bool, out: &mut Vec<Event>) {
    if syllables.is_empty() || slots.is_empty() {
        return;
    }
    let s = slots.len();
    for (k, syl) in syllables.iter().enumerate() {
        let (slot, sub, subs) = if k + 1 < s || syllables.len() <= s {
            (k.min(s - 1), 0, 1)
        } else {
            (s - 1, k - (s - 1), syllables.len() - (s - 1))
        };
        let note = slots[slot];
        let n = &ctx.notes[note];
        let last = if k + 1 == syllables.len() {
            *slots.last().unwrap_or(&note)
        } else {
            note
        };
        out.push(Event {
            note,
            last,
            time: n.start + n.duration * sub as f64 / subs as f64,
            content: syl.clone(),
            guessed,
        });
    }
}

fn single(text: &str, english: bool) -> (Content, bool) {
    if english {
        let (mut s, g) = english_syllables(text);
        (s.pop().unwrap_or(Content::Syllable(vec!["ah".into()])), g)
    } else {
        (Content::Mora(text.to_string()), false)
    }
}

fn bracketed(text: &str) -> Option<&str> {
    text.strip_prefix('[')?.strip_suffix(']').map(str::trim)
}

fn english_syllables(word: &str) -> (Vec<Content>, bool) {
    if let Some(inner) = bracketed(word) {
        let phones: Vec<String> = inner.split_whitespace().map(str::to_string).collect();
        return (
            dict::syllabify(&phones)
                .into_iter()
                .map(Content::Syllable)
                .collect(),
            false,
        );
    }
    let word = if word.is_empty() {
        default_syllable(true)
    } else {
        word
    };
    let (phones, guessed) = match dict::cmudict().lookup(word) {
        Some(p) => (p, false),
        None => (dict::guess(word), true),
    };
    (
        dict::syllabify(&phones)
            .into_iter()
            .map(Content::Syllable)
            .collect(),
        guessed,
    )
}

fn japanese_morae(ctx: &Ctx, text: &str, note: usize) -> (Vec<Content>, bool) {
    if let Some(inner) = bracketed(text) {
        return (
            vec![Content::Aliases(split_aliases(ctx, inner, note))],
            false,
        );
    }
    let text = if text.is_empty() {
        default_syllable(false)
    } else {
        text
    };
    // A lyric the bank has as it is ("breath", "R") is sung as it is.
    if ctx.has(text, note) && kana::kana_to_romaji(&kana::to_hiragana(text)).is_none() {
        return (vec![Content::Aliases(vec![text.to_string()])], false);
    }
    let h = kana::to_hiragana(text);
    let morae = kana::morae(&h);
    let known = morae.iter().all(|m| kana::kana_to_romaji(m).is_some());
    if !known {
        if ctx.has(text, note) {
            return (vec![Content::Aliases(vec![text.to_string()])], false);
        }
        return (vec![Content::Mora("あ".into())], true);
    }
    (morae.into_iter().map(Content::Mora).collect(), false)
}

/// Bracketed aliases may contain spaces ("a か"): the longest run of
/// words the bank has is one alias.
fn split_aliases(ctx: &Ctx, inner: &str, note: usize) -> Vec<String> {
    let words: Vec<&str> = inner.split_whitespace().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < words.len() {
        let mut taken = 1;
        for len in (1..=words.len() - i).rev() {
            if ctx.has(&words[i..i + len].join(" "), note) {
                taken = len;
                break;
            }
        }
        out.push(words[i..i + taken].join(" "));
        i += taken;
    }
    out
}

/// The vowel kana a mora ends on (あ for か, ん for ん).
fn vowel_kana(mora: &str) -> Option<&'static str> {
    Some(match kana::vowel_of(mora)? {
        "a" => "あ",
        "i" => "い",
        "u" => "う",
        "e" => "え",
        "o" => "お",
        _ => "ん",
    })
}

/// The vowel (romaji) a unit or alias ends on.
fn vowel_of_alias(alias: &str) -> Option<&'static str> {
    let last = alias.split_whitespace().last()?;
    let morae = kana::morae(&kana::to_hiragana(last));
    let m = morae.last()?;
    kana::vowel_of(m)
}

/// When the event after `k` starts, or where it stops before a rest.
fn event_end(ctx: &Ctx, events: &[Event], k: usize) -> (f64, bool) {
    let e = &events[k];
    match events.get(k + 1) {
        Some(n) if n.note == e.last || !ctx.rest_between(e.last, n.note) => (n.time, false),
        _ => (ctx.notes[e.last].end(), true),
    }
}

fn after_rest(ctx: &Ctx, events: &[Event], k: usize) -> bool {
    k == 0 || {
        let p = &events[k - 1];
        p.last != events[k].note && ctx.rest_between(p.last, events[k].note)
    }
}

fn push(plan: &mut Plan, alias: String, note: usize, anchor: f64, tail: bool) {
    plan.units.push(PlannedUnit {
        alias,
        note,
        anchor,
        start: None,
        tail,
    });
}

fn hand_units(ctx: &Ctx, e: &Event, list: &[(String, Option<f64>)], end: f64, plan: &mut Plan) {
    let m = list.len().max(1);
    let span = (end - e.time).max(0.0);
    for (k, (symbol, start)) in list.iter().enumerate() {
        let alias = if ctx.has(symbol, e.note) {
            symbol.clone()
        } else {
            kana::to_hiragana(symbol)
        };
        let anchor = match start {
            Some(s) => s + ctx.pre(&alias, e.note),
            None => e.time + span * k as f64 / m as f64,
        };
        plan.units.push(PlannedUnit {
            alias,
            note: e.note,
            anchor,
            start: *start,
            tail: false,
        });
    }
}

fn japanese(ctx: &Ctx, events: &[Event], kind: Phonemizer, plan: &mut Plan) {
    let mut prev_vowel: Option<&'static str> = None;
    for k in 0..events.len() {
        let e = &events[k];
        let (end, rest_after) = event_end(ctx, events, k);
        let start_of_run = after_rest(ctx, events, k);
        if start_of_run {
            prev_vowel = None;
        }
        match &e.content {
            Content::Hand(list) => {
                hand_units(ctx, e, list, end, plan);
                prev_vowel = list.last().and_then(|l| vowel_of_alias(&l.0));
            }
            Content::Aliases(list) => {
                let m = list.len();
                for (j, a) in list.iter().enumerate() {
                    push(
                        plan,
                        a.clone(),
                        e.note,
                        e.time + (end - e.time) * j as f64 / m as f64,
                        false,
                    );
                }
                prev_vowel = list.last().and_then(|a| vowel_of_alias(a));
            }
            Content::Syllable(_) | Content::Zh(_) | Content::Ko(_) => {}
            Content::Mora(m) => {
                let romaji = kana::kana_to_romaji(m);
                let mut cands: Vec<String> = Vec::new();
                let is_long = m == "ー";
                if is_long {
                    // A long-vowel mark holds the vowel before it.
                    if let Some(v) = prev_vowel {
                        let vk = vowel_kana(v).unwrap_or("あ");
                        cands.push(format!("{v} {vk}"));
                        cands.push(vk.to_string());
                    }
                }
                match (kind, prev_vowel) {
                    (Phonemizer::JaVcv, Some(v)) => {
                        cands.push(format!("{v} {m}"));
                        cands.push(format!("* {m}"));
                    }
                    (Phonemizer::JaCvvc, Some(v))
                        if romaji
                            .as_deref()
                            .is_some_and(|r| kana::split_romaji(r).0.is_empty()) =>
                    {
                        cands.push(format!("{v} {m}"));
                    }
                    (_, None) => {
                        cands.push(format!("- {m}"));
                        if let Some(r) = &romaji {
                            cands.push(format!("- {r}"));
                        }
                    }
                    _ => {}
                }
                cands.push(m.clone());
                if let Some(r) = &romaji {
                    cands.push(r.clone());
                }
                if let Some(alias) = ctx.first(&cands, e.note) {
                    push(plan, alias, e.note, e.time, false);
                }
                if !is_long {
                    prev_vowel = kana::vowel_of(m).or(prev_vowel);
                }
            }
        }
        let Some(v) = prev_vowel else { continue };
        if rest_after {
            if kind != Phonemizer::JaCv {
                let cands = [format!("{v} R"), format!("{v} -"), format!("{v}R")];
                if let Some(alias) = ctx.first(&cands, e.last) {
                    push(plan, alias, e.last, end, true);
                }
            }
            continue;
        }
        // CVVC: the vowel into the next mora's consonant.
        if kind == Phonemizer::JaCvvc
            && let Some(next) = events.get(k + 1)
            && let Content::Mora(nm) = &next.content
            && let Some(nr) = kana::kana_to_romaji(nm)
        {
            let (c, _) = kana::split_romaji(&nr);
            if c.is_empty() {
                continue;
            }
            let mut cands = vec![format!("{v} {c}")];
            if c.chars().count() > 1 {
                cands.push(format!("{v} {}", &c[..1]));
            }
            let Some(vc) = ctx.first(&cands, e.last) else {
                continue;
            };
            let next_cv = ctx
                .first(&[nm.clone(), nr.clone()], next.note)
                .unwrap_or_else(|| nm.clone());
            let lead = (ctx.pre(&next_cv, next.note) / ctx.velocity(next.note))
                .min((next.time - e.time) / 2.0);
            let anchor = next.time - lead;
            let note = (e.note..=e.last)
                .rev()
                .find(|&i| ctx.notes[i].start <= anchor)
                .unwrap_or(e.note);
            push(plan, vc, note, anchor, false);
        }
    }
}

/// Each note's syllables when it is written in Chinese characters or
/// hangul, read with the notes sung next to it (a rest, a slur or a hand-
/// placed note breaks the run).
fn scripts(ctx: &Ctx, kind: Phonemizer) -> Vec<Option<Vec<Content>>> {
    let mut texts: Vec<&str> = Vec::new();
    let mut at = Vec::new();
    for (i, n) in ctx.notes.iter().enumerate() {
        if i > 0 && ctx.rest_between(i - 1, i) {
            texts.push("");
        }
        at.push(texts.len());
        texts.push(if n.phonemes.is_some() {
            ""
        } else {
            n.lyric.trim().trim_end_matches('-')
        });
    }
    let read: Vec<Option<Vec<Content>>> = match kind {
        Phonemizer::ZhCvvc => fontelle_lang::pinyin::read_run(&texts)
            .into_iter()
            .map(|r| r.map(|v| v.into_iter().map(Content::Zh).collect()))
            .collect(),
        _ => fontelle_lang::hangul::read_run(&texts)
            .into_iter()
            .map(|r| r.map(|v| v.into_iter().map(Content::Ko).collect()))
            .collect(),
    };
    at.into_iter().map(|k| read[k].clone()).collect()
}

/// 라: what an empty Korean lyric sings.
fn ko_default() -> fontelle_lang::hangul::Syllable {
    fontelle_lang::hangul::Syllable {
        text: '라',
        onset: "r",
        vowel: "a",
        coda: "",
    }
}

/// A Chinese or Korean lyric's syllables.
fn cjk_syllables(ctx: &Ctx, text: &str, note: usize, kind: Phonemizer) -> (Vec<Content>, bool) {
    if let Some(inner) = bracketed(text) {
        return (
            vec![Content::Aliases(split_aliases(ctx, inner, note))],
            false,
        );
    }
    let default = || match kind {
        Phonemizer::ZhCvvc => Content::Zh("la".into()),
        _ => Content::Ko(ko_default()),
    };
    if text.is_empty() {
        return (vec![default()], false);
    }
    if let Some(Some(read)) = ctx.scripts.get(note) {
        return (read.clone(), false);
    }
    let lower = text.to_lowercase();
    if kind == Phonemizer::ZhCvvc && fontelle_lang::pinyin::is_syllable(&lower) {
        return (vec![Content::Zh(lower)], false);
    }
    if ctx.has(text, note) {
        return (vec![Content::Aliases(vec![text.to_string()])], false);
    }
    (vec![default()], true)
}

/// The final a Chinese syllable ends on, sung again after a rest.
fn zh_vowel(syllable: &str) -> &str {
    let (_, f) = fontelle_lang::pinyin::split(syllable);
    if fontelle_lang::pinyin::is_syllable(f) {
        return f;
    }
    match f.chars().last() {
        Some('i') => "yi",
        Some('u') => "wu",
        Some('v') => "yu",
        Some('o') => "o",
        Some('e') => "e",
        _ => "a",
    }
}

/// How a Chinese or Korean syllable is asked for: (the onset the vowel
/// before it glides into, the CV candidates, whole-syllable candidates
/// that already hold its coda, its vowel, its coda).
struct SyllableNames {
    onset: String,
    cv: Vec<String>,
    vowel: String,
    coda: String,
}

fn syllable_names(content: &Content, start: bool) -> Option<SyllableNames> {
    match content {
        Content::Zh(s) => {
            let (i, f) = fontelle_lang::pinyin::split(s);
            let mut cv = Vec::new();
            if start {
                cv.push(format!("- {s}"));
                cv.push(format!("-{s}"));
            }
            cv.push(s.clone());
            Some(SyllableNames {
                onset: i.to_string(),
                cv,
                vowel: f.to_string(),
                coda: String::new(),
            })
        }
        Content::Ko(s) => {
            let open = s.open();
            let mut cv = Vec::new();
            // A bank recorded with whole syllables (한, 글) sings the
            // block whole, when the sung onset is the written one.
            let onset_as_written = fontelle_lang::hangul::decompose(s.text).map(|d| d.0)
                == fontelle_lang::hangul::decompose(open).map(|d| d.0);
            let whole = !s.coda.is_empty() && onset_as_written;
            if whole {
                if start {
                    cv.push(format!("- {}", s.text));
                }
                cv.push(s.text.to_string());
            }
            if start {
                cv.push(format!("- {open}"));
            }
            cv.push(open.to_string());
            cv.push(format!("{}{}", s.onset, s.vowel));
            Some(SyllableNames {
                onset: s.onset.to_string(),
                cv,
                vowel: s.vowel.to_string(),
                coda: s.coda.to_string(),
            })
        }
        _ => None,
    }
}

/// Chinese and Korean: a CV unit per syllable; a coda before the note
/// ends; vowel-to-consonant into the next syllable and an ending before a
/// rest, when the bank has them (a CV-only bank simply has none).
fn syllabic(ctx: &Ctx, events: &[Event], plan: &mut Plan) {
    // The open vowel the syllable before ended on, in this run.
    let mut open_vowel: Option<String> = None;
    for k in 0..events.len() {
        let e = &events[k];
        let (end, rest_after) = event_end(ctx, events, k);
        let start = after_rest(ctx, events, k);
        let before = if start { None } else { open_vowel.take() };
        // The vowel this event ends on, when something may follow it.
        let prev_vowel: Option<String> = match &e.content {
            Content::Hand(list) => {
                hand_units(ctx, e, list, end, plan);
                None
            }
            Content::Aliases(list) => {
                let m = list.len();
                for (j, a) in list.iter().enumerate() {
                    push(
                        plan,
                        a.clone(),
                        e.note,
                        e.time + (end - e.time) * j as f64 / m as f64,
                        false,
                    );
                }
                None
            }
            content => {
                let Some(names) = syllable_names(content, start) else {
                    continue;
                };
                // No initial: from the vowel before ("ao a"), as the bank
                // has it, else the syllable's start ("- ai"). Hitsuboku
                // Kumi has no bare "a": 你好啊 sang nothing on 啊.
                let mut cands = Vec::new();
                if names.onset.is_empty()
                    && let Some(pv) = &before
                {
                    cands.extend(names.cv.iter().map(|c| format!("{pv} {c}")));
                }
                cands.extend(names.cv.iter().cloned());
                if names.onset.is_empty() && !start {
                    cands.extend(names.cv.iter().map(|c| format!("- {c}")));
                }
                let Some(cv) = ctx.first(&cands, e.note) else {
                    continue;
                };
                // A whole syllable (its coda in it) needs no coda unit.
                let whole = matches!(content, Content::Ko(s)
                    if !s.coda.is_empty() && cv.contains(s.text));
                push(plan, cv, e.note, e.time, false);
                if whole {
                    None
                } else if !names.coda.is_empty() {
                    let cands = [
                        format!("{} {}", names.vowel, names.coda),
                        format!("{}{}", names.vowel, names.coda),
                    ];
                    if let Some(coda) = ctx.first(&cands, e.last) {
                        let lead = (ctx.pre(&coda, e.last) / ctx.velocity(e.last))
                            .min((end - e.time) / 3.0);
                        let anchor = end - lead;
                        let note = (e.note..=e.last)
                            .rev()
                            .find(|&i| ctx.notes[i].start <= anchor)
                            .unwrap_or(e.note);
                        push(plan, coda, note, anchor, false);
                    }
                    None
                } else {
                    Some(names.vowel)
                }
            }
        };
        open_vowel = prev_vowel.clone();
        let Some(v) = prev_vowel else {
            continue;
        };
        if rest_after {
            let cands = [format!("{v} R"), format!("{v} -"), format!("{v}R")];
            if let Some(alias) = ctx.first(&cands, e.last) {
                push(plan, alias, e.last, end, true);
            }
            continue;
        }
        // The vowel into the next syllable's onset.
        let Some(next) = events.get(k + 1) else {
            continue;
        };
        let Some(next_names) = syllable_names(&next.content, false) else {
            continue;
        };
        if next_names.onset.is_empty() {
            continue;
        }
        let mut cands = vec![format!("{v} {}", next_names.onset)];
        if let Some(last) = v.chars().last()
            && v.chars().count() > 1
        {
            cands.push(format!("{last} {}", next_names.onset));
        }
        let Some(vc) = ctx.first(&cands, e.last) else {
            continue;
        };
        let next_cv = ctx
            .first(&next_names.cv, next.note)
            .unwrap_or_else(|| next_names.cv[0].clone());
        let lead = (ctx.pre(&next_cv, next.note) / ctx.velocity(next.note))
            .min((next.time - e.time) / 2.0);
        let anchor = next.time - lead;
        let note = (e.note..=e.last)
            .rev()
            .find(|&i| ctx.notes[i].start <= anchor)
            .unwrap_or(e.note);
        push(plan, vc, note, anchor, false);
    }
}

/// VCCV's names for ARPAbet phonemes.
fn vccv_symbol(p: &str) -> &str {
    match p {
        "aa" => "a",
        "ae" => "@",
        "ah" | "ax" => "u",
        "ao" => "9",
        "aw" => "8",
        "ay" => "I",
        "eh" => "e",
        "er" => "3",
        "ey" => "A",
        "ih" => "i",
        "iy" => "E",
        "ow" => "O",
        "oy" => "Q",
        "uh" => "6",
        "uw" => "o",
        "hh" => "h",
        "jh" => "j",
        "dx" => "dd",
        other => other,
    }
}

/// Alternatives for a vowel a bank may lack.
fn vowel_fallbacks(p: &str) -> &'static [&'static str] {
    match p {
        "aa" => &["ah", "ae"],
        "ae" => &["ah", "aa"],
        "ah" => &["aa", "ae"],
        "ax" => &["ah", "uh"],
        "ao" => &["ow"],
        "ow" => &["ao"],
        "eh" => &["ae"],
        "ih" => &["iy"],
        "iy" => &["ih"],
        "uh" => &["uw"],
        "uw" => &["uh"],
        "aw" => &["ao"],
        _ => &[],
    }
}

/// A phoneme placed in time, and the note it belongs to.
struct Timed {
    phone: String,
    anchor: f64,
    note: usize,
    /// A consonant that begins its syllable (not the coda of the one
    /// before): only those may be sung as one joined cluster ("st", "bju").
    onset: bool,
}

fn english_units(ctx: &Ctx, events: &[Event], kind: Phonemizer, plan: &mut Plan) {
    // Runs of syllables between rests, as timed phonemes.
    let mut run: Vec<Timed> = Vec::new();
    let mut run_last_note = 0;
    let mut pending: Vec<(String, usize)> = Vec::new();
    for k in 0..events.len() {
        let e = &events[k];
        let (end, rest_after) = event_end(ctx, events, k);
        let phones: Vec<String> = match &e.content {
            Content::Syllable(p) => p.clone(),
            Content::Mora(m) => vec![m.clone()],
            Content::Zh(m) => vec![m.clone()],
            Content::Ko(m) => vec![m.roman()],
            Content::Aliases(list) => {
                flush(ctx, &mut run, run_last_note, kind, plan);
                let m = list.len();
                for (j, a) in list.iter().enumerate() {
                    push(
                        plan,
                        a.clone(),
                        e.note,
                        e.time + (end - e.time) * j as f64 / m as f64,
                        false,
                    );
                }
                continue;
            }
            Content::Hand(list) => {
                flush(ctx, &mut run, run_last_note, kind, plan);
                hand_units(ctx, e, list, end, plan);
                continue;
            }
        };
        let vowel_at = phones
            .iter()
            .position(|p| dict::is_vowel(p))
            .unwrap_or(phones.len().saturating_sub(1));
        let len = CONSONANT / ctx.velocity(e.note);
        // Consonants before this vowel: the last syllable's coda and this
        // one's onset, before the beat; no more than half the syllable
        // before them.
        let mut cluster: Vec<(String, usize, bool)> = std::mem::take(&mut pending)
            .into_iter()
            .map(|(p, n)| (p, n, false))
            .collect();
        for p in &phones[..vowel_at] {
            // "want to": a consonant ending one word and starting the next
            // is said once.
            if cluster.last().is_some_and(|l| &l.0 == p && !l.2) {
                cluster.pop();
            }
            cluster.push((p.clone(), e.note, true));
        }
        let room = match run.iter().rev().find(|t| dict::is_vowel(&t.phone)) {
            Some(v) => (e.time - v.anchor) / 2.0,
            None => f64::INFINITY,
        };
        let c = cluster.len() as f64;
        let step = if c > 0.0 { len.min(room / c) } else { len };
        for (j, (p, note, onset)) in cluster.iter().enumerate() {
            run.push(Timed {
                phone: p.clone(),
                anchor: e.time - step * (c - j as f64),
                note: *note,
                onset: *onset,
            });
        }
        if let Some(v) = phones.get(vowel_at) {
            run.push(Timed {
                phone: v.clone(),
                anchor: e.time,
                note: e.note,
                onset: false,
            });
        }
        let coda: Vec<(String, usize)> = phones
            .iter()
            .skip(vowel_at + 1)
            .map(|p| (p.clone(), e.last))
            .collect();
        run_last_note = e.last;
        if rest_after {
            let m = coda.len() as f64;
            let step = if m > 0.0 {
                len.min((end - e.time) / 2.0 / m)
            } else {
                len
            };
            for (j, (p, note)) in coda.iter().enumerate() {
                run.push(Timed {
                    phone: p.clone(),
                    anchor: end - step * (m - j as f64),
                    note: *note,
                    onset: false,
                });
            }
            run.push(Timed {
                phone: "-".into(),
                anchor: end,
                note: e.last,
                onset: false,
            });
            flush(ctx, &mut run, run_last_note, kind, plan);
        } else {
            pending = coda;
        }
    }
    flush(ctx, &mut run, run_last_note, kind, plan);
}

/// One run's phonemes as diphone (ARPAsing) or VCCV units.
fn flush(ctx: &Ctx, run: &mut Vec<Timed>, last_note: usize, kind: Phonemizer, plan: &mut Plan) {
    let _ = last_note;
    if run.is_empty() {
        return;
    }
    let ended = run.last().is_some_and(|t| t.phone == "-");
    let seq = std::mem::take(run);
    match kind {
        Phonemizer::EnVccv => vccv(ctx, &seq, ended, plan),
        Phonemizer::EnCvvc => xsampa_cvvc(ctx, &seq, ended, plan),
        _ => arpasing(ctx, &seq, ended, plan),
    }
}

fn arpasing(ctx: &Ctx, seq: &[Timed], ended: bool, plan: &mut Plan) {
    for k in 0..seq.len() {
        let cur = &seq[k];
        let prev = if k == 0 {
            "-"
        } else {
            seq[k - 1].phone.as_str()
        };
        // The phoneme before as the bank may name it: a schwa is "ah" in
        // banks that have no "ax".
        let prevs: Vec<&str> = std::iter::once(prev)
            .chain(vowel_fallbacks(prev).iter().copied())
            .collect();
        if cur.phone == "-" {
            let cands: Vec<String> = prevs
                .iter()
                .flat_map(|q| [format!("{q} -"), format!("{q}-")])
                .collect();
            if k > 0
                && ended
                && let Some(a) = ctx.first(&cands, cur.note)
            {
                push(plan, a, cur.note, cur.anchor, true);
            }
            continue;
        }
        let p = cur.phone.as_str();
        let mut cands = Vec::new();
        for q in &prevs {
            cands.push(format!("{q} {p}"));
            for f in vowel_fallbacks(p) {
                cands.push(format!("{q} {f}"));
            }
        }
        cands.push(format!("- {p}"));
        cands.push(p.to_string());
        for f in vowel_fallbacks(p) {
            cands.push(format!("- {f}"));
        }
        if let Some(a) = ctx.first(&cands, cur.note) {
            push(plan, a, cur.note, cur.anchor, false);
        }
    }
}

fn vccv(ctx: &Ctx, seq: &[Timed], ended: bool, plan: &mut Plan) {
    let sym: Vec<&str> = seq.iter().map(|t| vccv_symbol(&t.phone)).collect();
    let vowel = |i: usize| dict::is_vowel(&seq[i].phone);
    let n = seq.len();
    let mut k = 0;
    while k < n {
        let cur = &seq[k];
        if sym[k] == "-" {
            k += 1;
            continue;
        }
        let at_start = k == 0;
        let next_is_end = ended && k + 2 == n;
        // The start: "-rE" covers silence, consonant and vowel.
        if at_start && !vowel(k) && k + 1 < n && vowel(k + 1) {
            let v = &seq[k + 1];
            let cands = [
                format!("-{}{}", sym[k], sym[k + 1]),
                format!("- {}{}", sym[k], sym[k + 1]),
            ];
            if let Some(a) = ctx.first(&cands, v.note) {
                push(plan, a, v.note, v.anchor, false);
                k += 2;
                continue;
            }
        }
        if at_start {
            let cands = [
                format!("-{}", sym[k]),
                format!("- {}", sym[k]),
                sym[k].to_string(),
            ];
            if let Some(a) = ctx.first(&cands, cur.note) {
                push(plan, a, cur.note, cur.anchor, false);
            }
            k += 1;
            continue;
        }
        let (a, b) = (sym[k - 1], sym[k]);
        // The end: "Ed-" covers vowel, consonant and silence.
        if next_is_end
            && !vowel(k)
            && vowel(k - 1)
            && let Some(al) = ctx.first(&[format!("{a}{b}-")], cur.note)
        {
            push(plan, al, cur.note, cur.anchor, true);
            k += 2;
            continue;
        }
        let cands: Vec<String> = match (vowel(k - 1), vowel(k)) {
            (true, false) => vec![format!("{a} {b}"), format!("{a}{b}")],
            (false, true) => vec![format!("{a}{b}"), b.to_string()],
            (false, false) => vec![format!("{a}{b}"), format!("{a} {b}"), format!("-{b}")],
            (true, true) => vec![format!("{a}{b}"), format!("{a} {b}"), b.to_string()],
        };
        if let Some(al) = ctx.first(&cands, cur.note) {
            push(plan, al, cur.note, cur.anchor, false);
        }
        k += 1;
    }
    if ended && n >= 2 {
        let last = &seq[n - 1];
        let before = sym[n - 2];
        // A tail not already covered by "{V}{C}-".
        if !plan
            .units
            .last()
            .is_some_and(|u| u.tail && u.alias.ends_with('-'))
        {
            let cands = [format!("{before}-"), format!("{before} -")];
            if let Some(al) = ctx.first(&cands, last.note) {
                push(plan, al, last.note, last.anchor, true);
            }
        }
    }
}

/// X-SAMPA names for ARPAbet phonemes, with alternatives a bank may use.
fn xsampa(p: &str) -> &'static [&'static str] {
    match p {
        "aa" => &["A", "a"],
        "ae" => &["{", "a"],
        "ah" => &["V", "@", "A"],
        "ax" => &["@", "V", "A"],
        "ao" => &["O", "A"],
        "aw" => &["aU"],
        "ay" => &["aI"],
        "eh" => &["E", "e"],
        "er" => &["3", "@r"],
        "ey" => &["eI", "e"],
        "ih" => &["I", "i"],
        "iy" => &["i"],
        "ow" => &["oU", "o"],
        "oy" => &["OI"],
        "uh" => &["U", "u"],
        "uw" => &["u"],
        "ch" => &["tS"],
        "jh" => &["dZ"],
        "sh" => &["S"],
        "zh" => &["Z"],
        "th" => &["T"],
        "dh" => &["D"],
        "ng" => &["N"],
        "y" => &["j"],
        "hh" => &["h"],
        "b" => &["b"],
        "d" => &["d"],
        "f" => &["f"],
        "g" => &["g"],
        "k" => &["k"],
        "l" => &["l"],
        "m" => &["m"],
        "n" => &["n"],
        "p" => &["p"],
        "r" => &["r"],
        "s" => &["s"],
        "t" => &["t"],
        "v" => &["v"],
        "w" => &["w"],
        "z" => &["z"],
        _ => &[],
    }
}

/// Every spelling of a pattern over each phoneme's X-SAMPA alternatives,
/// the first alternatives first.
fn spellings(parts: &[&str], pattern: impl Fn(&[&str]) -> String) -> Vec<String> {
    let alts: Vec<Vec<&str>> = parts
        .iter()
        .map(|p| {
            let x = xsampa(p);
            if x.is_empty() { vec![*p] } else { x.to_vec() }
        })
        .collect();
    let mut out = Vec::new();
    let mut idx = vec![0usize; alts.len()];
    loop {
        let pick: Vec<&str> = idx.iter().zip(&alts).map(|(&i, a)| a[i]).collect();
        out.push(pattern(&pick));
        let mut k = alts.len();
        loop {
            if k == 0 {
                return out;
            }
            k -= 1;
            idx[k] += 1;
            if idx[k] < alts[k].len() {
                break;
            }
            idx[k] = 0;
        }
    }
}

/// Consonants a lone sample is sung for when nothing joins them to the
/// next: its release ("t" between "I t" and "s@"). A nasal, liquid or glide
/// is already heard in the unit before it.
fn released(p: &str) -> bool {
    !matches!(p, "m" | "n" | "ng" | "l" | "r" | "w" | "y" | "hh")
}

/// English CVVC (X-SAMPA), as Kasane Teto's English bank and those made
/// like it are cut:
///
/// - a CV on each vowel, the longest onset the bank joins to it ("bju",
///   "- kA"), or VCV where it has one ("@ hA");
/// - a VC where a vowel meets the next consonant ("A n");
/// - the consonants between joined as the bank allows: a whole onset
///   ("st", "- str", "tw"), a pair ("r g"), else a released consonant alone
///   ("t");
/// - endings over the whole final cluster ("l d-", "s kt-", "A p-", "oU -").
///
/// Ty: "the english wording sounds off" — "world" ended on `3 l` without
/// its d, "hand" on `{ n`, and no cluster sample was ever used.
fn xsampa_cvvc(ctx: &Ctx, seq: &[Timed], ended: bool, plan: &mut Plan) {
    let n = seq.len();
    let ph = |i: usize| seq[i].phone.as_str();
    let vowel = |i: usize| dict::is_vowel(ph(i));
    let onset = |c: &[usize]| c.iter().all(|&i| seq[i].onset);
    let unit = |plan: &mut Plan, cands: Vec<String>, i: usize, tail: bool| -> bool {
        match ctx.first(&cands, seq[i].note) {
            Some(a) => {
                push(plan, a, seq[i].note, seq[i].anchor, tail);
                true
            }
            None => false,
        }
    };
    // Phones `idx` written together, between `pre` and `post`.
    let joined = |idx: &[usize], pre: &str, post: &str| -> Vec<String> {
        let parts: Vec<&str> = idx.iter().map(|&i| ph(i)).collect();
        spellings(&parts, |s| format!("{pre}{}{post}", s.concat()))
    };
    let alone = |i: usize| spellings(&[ph(i)], |s| s[0].to_string());
    let mut prev: Option<usize> = None;
    let mut k = 0;
    while k < n {
        let mut j = k;
        while j < n && !vowel(j) && ph(j) != "-" {
            j += 1;
        }
        let c: Vec<usize> = (k..j).collect();
        let len = c.len();
        if j >= n || ph(j) == "-" {
            // The end of the run, with the coda before it.
            let dash = ended && j < n;
            match prev {
                Some(p) if len == 0 => {
                    if dash {
                        unit(
                            plan,
                            spellings(&[ph(p)], |s| format!("{} -", s[0])),
                            j,
                            true,
                        );
                    }
                }
                Some(p) => {
                    let pc = [p, c[0]];
                    if len == 1 && dash && unit(plan, joined_vc(&pc, ph, "-"), c[0], true) {
                        k = j + 1;
                        prev = None;
                        continue;
                    }
                    unit(plan, joined_vc(&pc, ph, ""), c[0], false);
                    let last = len - 1;
                    let mut covered = false;
                    let mut i = 0;
                    while i < last {
                        let mut moved = false;
                        for kk in (i + 1..=last).rev() {
                            let d = if kk == last && dash { "-" } else { "" };
                            let parts: Vec<&str> = c[i..=kk].iter().map(|&x| ph(x)).collect();
                            let mut cands =
                                spellings(&parts, |s| format!("{} {}{d}", s[0], s[1..].concat()));
                            if !d.is_empty() {
                                // "l d" then "d -" where there is no "l d-".
                                cands.extend(spellings(&parts, |s| {
                                    format!("{} {}", s[0], s[1..].concat())
                                }));
                            }
                            if let Some(a) = ctx.first(&cands, seq[c[i + 1]].note) {
                                covered = a.ends_with('-');
                                push(plan, a, seq[c[i + 1]].note, seq[c[i + 1]].anchor, covered);
                                i = kk;
                                moved = true;
                                break;
                            }
                        }
                        if !moved {
                            if released(ph(c[i])) {
                                unit(plan, alone(c[i]), c[i + 1], false);
                            }
                            i += 1;
                        }
                    }
                    if dash && !covered {
                        let l = c[last];
                        let mut cands = spellings(&[ph(l)], |s| format!("{} -", s[0]));
                        cands.extend(spellings(&[ph(l)], |s| format!("{}-", s[0])));
                        if released(ph(l)) {
                            cands.extend(alone(l));
                        }
                        unit(plan, cands, l, true);
                    }
                }
                None => {
                    for &i in &c {
                        unit(plan, alone(i), i, false);
                    }
                }
            }
            prev = None;
            k = j + 1;
            continue;
        }
        let v = j;
        // VCV, where the bank has it ("@ hA").
        if let Some(p) = prev
            && len == 1
        {
            let cands = spellings(&[ph(p), ph(c[0]), ph(v)], |s| {
                format!("{} {}{}", s[0], s[1], s[2])
            });
            if unit(plan, cands, v, false) {
                prev = Some(v);
                k = v + 1;
                continue;
            }
        }
        // The CV with the longest onset the bank has; a coda consonant only
        // as the single consonant before the vowel ("world I": "daI").
        let mut cv: Option<(String, usize)> = None;
        for m in 0..=len {
            if m + 1 < len && !onset(&c[m..]) {
                continue;
            }
            let idx: Vec<usize> = c[m..].iter().copied().chain([v]).collect();
            let mut cands = Vec::new();
            if prev.is_none() && m == 0 {
                cands.extend(joined(&idx, "- ", ""));
            }
            if let Some(p) = prev
                && len == 0
            {
                cands.extend(spellings(&[ph(p), ph(v)], |s| format!("{} {}", s[0], s[1])));
            }
            cands.extend(joined(&idx, "", ""));
            if let Some(a) = ctx.first(&cands, seq[v].note) {
                cv = Some((a, m));
                break;
            }
        }
        // Without a CV (or with the vowel alone) the last consonant is sung
        // on its own before the vowel.
        let m = match &cv {
            Some((_, m)) if *m < len => *m,
            _ => len.saturating_sub(1),
        };
        // What sounds the first consonant: the VC, or a start.
        let mut i = 0;
        if len > 0 {
            match prev {
                Some(p) => {
                    unit(plan, joined_vc(&[p, c[0]], ph, ""), c[0], false);
                }
                // A start before the CV: "- str" before "rE", or "- b"
                // before "bA" in banks whose CVs have no start of their own.
                None if m > 0 || cv.as_ref().is_none_or(|c| !c.0.starts_with("- ")) => {
                    for jj in (0..=m.min(len - 1)).rev() {
                        let mut cands = joined(&c[..=jj], "- ", "");
                        if jj == 0 && m > 0 {
                            cands.extend(alone(c[0]));
                        }
                        if unit(plan, cands, c[jj], false) {
                            i = jj;
                            break;
                        }
                    }
                }
                None => {}
            }
        }
        // The consonants between it and the CV's.
        while i < m {
            let mut moved = false;
            for kk in (i + 1..=m).rev() {
                if onset(&c[i..=kk]) && unit(plan, joined(&c[i..=kk], "", ""), c[kk], false) {
                    i = kk;
                    moved = true;
                    break;
                }
            }
            if moved {
                continue;
            }
            let pair = spellings(&[ph(c[i]), ph(c[i + 1])], |s| format!("{} {}", s[0], s[1]));
            if !unit(plan, pair, c[i + 1], false) && released(ph(c[i])) {
                unit(plan, alone(c[i]), c[i + 1], false);
            }
            i += 1;
        }
        match cv {
            Some((a, _)) => push(plan, a, seq[v].note, seq[v].anchor, false),
            None => {
                let mut cands = alone(v);
                cands.extend(spellings(&[ph(v)], |s| format!("- {}", s[0])));
                unit(plan, cands, v, false);
            }
        }
        prev = Some(v);
        k = v + 1;
    }
}

/// "V C" (or "V C-" with `post` "-") over the X-SAMPA spellings of a vowel
/// and a consonant.
fn joined_vc<'a>(pc: &[usize; 2], ph: impl Fn(usize) -> &'a str, post: &str) -> Vec<String> {
    spellings(&[ph(pc[0]), ph(pc[1])], |s| {
        format!("{} {}{post}", s[0], s[1])
    })
}
