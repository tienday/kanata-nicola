//! Runs the generated .kbd in the kanata simulation and compares it with the expected-behavior case file
//! (`testdata/kanata_prefix_cases.toml`), and checks what must hold for any input (random typing).
//!
//! To look at the judgment alone, the Unicode encoder (one output per character) is used; the romaji output is decoded
//! back into the same tokens.

mod common;

use std::collections::HashMap;
use std::sync::OnceLock;

use common::{pressed, pressed_with_shift, presses, random_input, rng, simulate_raw, stuck_keys};
use kanata_state_machine::oskbd::{KeyEvent, KeyValue};
use kanata_state_machine::str_to_oscode;
use nicola_kbdgen::decode::{canonical, Decoder};
use nicola_kbdgen::encoder::{romaji, typed_keys};
use nicola_kbdgen::{generate, kanata_key, Encoder, Options};
use nicola_layout::{keycodes, LayoutDef, NICOLA_J};
use nicola_testcases::{compare_within, load, parse_expect, KANATA_PREFIX_CASES};

/// The default options (romaji) with NICOLA on from the start.
fn nicola_on() -> Options {
    Options { start_enabled: true, ..Options::default() }
}

/// The name of an output key in the case files: `[OL]` / `[OR]` for the thumb keys, the evdev name without `KEY_`
/// otherwise.
fn output_name(kanata_debug: &str) -> String {
    match keycodes::name_of(evdev_code(kanata_debug)).unwrap() {
        "KEY_MUHENKAN" => "[OL]".into(),
        "KEY_HENKAN" => "[OR]".into(),
        name => name.trim_start_matches("KEY_").into(),
    }
}

/// Turns the simulation output into the tokens of the case files (with times).
fn tokens(events: &[String]) -> Vec<(String, u64)> {
    let mut t = 0;
    let mut out = Vec::new();
    for e in events {
        if let Some(ms) = e.strip_prefix("t:").and_then(|s| s.strip_suffix("ms")) {
            t += ms.parse::<u64>().unwrap();
        } else if let Some(c) = e.strip_prefix("outU:") {
            out.push((c.to_string(), t));
        } else if let Some(k) = e.strip_prefix("out:↓") {
            let n = output_name(k);
            out.push((if n.starts_with('[') { n } else { format!("pass:d:{n}") }, t));
        } else if let Some(k) = e.strip_prefix("out:↑") {
            let n = output_name(k);
            if !n.starts_with('[') {
                out.push((format!("pass:u:{n}"), t));
            }
        } else {
            panic!("unexpected sim output {e}");
        }
    }
    out
}

pub fn simulate(cfg: &str, input: &str) -> Vec<(String, u64)> {
    let raw = simulate_raw(cfg, input);
    let events: Vec<String> = raw.iter().map(|(e, _)| e.clone()).collect();
    let times: Vec<u64> = raw.iter().map(|(_, t)| *t).collect();
    // tokens() expects a list with t:Nms entries, so turn the times back into differences.
    let mut with_ticks = Vec::new();
    let mut prev = 0;
    for (e, t) in events.into_iter().zip(times) {
        if t > prev {
            with_ticks.push(format!("t:{}ms", t - prev));
            prev = t;
        }
        with_ticks.push(e);
    }
    tokens(&with_ticks)
}

/// kanata KeyCode display name -> evdev code, for every evdev key that has a kanata name (`kanata_key`).
pub fn evdev_code(kanata_debug: &str) -> u16 {
    static CODES: OnceLock<HashMap<String, u16>> = OnceLock::new();
    let codes = CODES.get_or_init(|| {
        (1..=255u16).filter_map(|code| Some((pressed_name(kanata_key(keycodes::name_of(code)?)?), code))).collect()
    });
    *codes.get(kanata_debug).unwrap_or_else(|| panic!("no evdev key with the kanata name {kanata_debug}"))
}

/// Decodes the output of a key-sending encoder into tokens.
pub fn simulate_decoded(cfg: &str, input: &str, decoder: &Decoder) -> Vec<String> {
    decode_output(&simulate_raw(cfg, input), decoder)
}

fn decode_output(raw: &[(String, u64)], decoder: &Decoder) -> Vec<String> {
    let events: Vec<(u16, i32)> = raw
        .iter()
        .filter_map(|(e, _)| {
            if let Some(k) = e.strip_prefix("out:↓") {
                Some((evdev_code(k), 1))
            } else {
                e.strip_prefix("out:↑").map(|k| (evdev_code(k), 0))
            }
        })
        .collect();
    decoder.decode(&events)
}

/// With random fast typing, the romaji output matches the judgment (the Unicode output).
/// The judgment is the same, so any difference means a character broke during output (regression test for OQ-11).
/// Returns the failures. No output key may be left pressed (checked here, whatever the failures).
fn random_typing_failures(cases: usize) -> Vec<String> {
    let layout = LayoutDef::parse(NICOLA_J).unwrap();
    let uni = generate(&layout, &Options { encoder: Encoder::Unicode, ..nicola_on() }).unwrap();
    let cfg = generate(&layout, &nicola_on()).unwrap();
    let decoder = Decoder::new(&layout);

    let keys = ["W", "E", "R", "T", "A", "S", "D", "K", "L", "J", "I", "O", "N", "M", "OL", "OR"];
    let mut rnd = rng(0x2545_F491_4F6C_DD1D);
    let mut failures = Vec::new();
    for i in 0..cases {
        let input = random_input(&mut rnd, &keys);
        let want: Vec<String> = simulate(&uni, &input).into_iter().map(|(s, _)| canonical(&s)).collect();
        let raw = simulate_raw(&cfg, &input);
        let stuck = stuck_keys(&raw);
        assert!(stuck.is_empty(), "case {i}: {input}\n  output keys left pressed: {stuck:?}");
        let got = decode_output(&raw, &decoder);
        if got != want {
            failures.push(format!("case {i}: {input}\n  want: {}\n  got:  {}", want.join(" "), got.join(" ")));
        }
    }
    failures
}

/// Default number of cases; raise it with `NICOLA_RANDOM_CASES`.
const DEFAULT_RANDOM_CASES: usize = 1000;

fn random_cases() -> usize {
    std::env::var("NICOLA_RANDOM_CASES").ok().and_then(|v| v.parse().ok()).unwrap_or(DEFAULT_RANDOM_CASES)
}

/// Romaji shares vowels among many characters, so when a roll overlaps the keys of the previous character,
/// a reordering remains (OQ-11), seen with more cases than the default. The default cases have none, as
/// docs/design.md §7.2 says (fixed seed, so deterministic); more cases may differ in up to 1.0%.
#[test]
fn random_typing_romaji() {
    let cases = random_cases();
    let f = random_typing_failures(cases);
    println!("romaji: {} of {cases} differ", f.len());
    let limit = if cases == DEFAULT_RANDOM_CASES { 0 } else { cases * 10 / 1000 };
    assert!(
        f.len() <= limit,
        "Romaji: {} of {cases} differ (limit {limit}):\n{}",
        f.len(),
        f[..f.len().min(5)].join("\n")
    );
}

/// The roll in docs/kanata-kbd-design.md §2.2 (its sequence diagram): the keys pressed at the times it gives send
/// the keys it gives, at the times it gives.
#[test]
fn docs_roll_example_gives_the_times() {
    let doc = include_str!("../../../docs/kanata-kbd-design.md");
    let diagram = doc
        .split("**ロール打鍵の例:**")
        .nth(1)
        .and_then(|s| s.split("```mermaid\n").nth(1))
        .and_then(|s| s.split("```").next())
        .expect("the diagram of the roll in docs/kanata-kbd-design.md");
    let mut input = vec![];
    let mut want = vec![];
    // "P->>Q: 0ms w を押す" (a physical key), "Q->>O: 6ms a を離す（up-か）" (output keys).
    for line in diagram.lines().map(str::trim) {
        let Some((from, rest)) = line.split_once(": ") else { continue };
        let Some((t, rest)) = rest.split_once("ms ") else { continue };
        let (keys, act) = rest.split_once(" を").unwrap_or_else(|| panic!("{line}"));
        let t: u64 = t.parse().unwrap();
        let arrow = if act.starts_with("押す") { '↓' } else { '↑' };
        match from {
            "P->>Q" => input.push(format!("d:{}@{t}", keys.to_uppercase())),
            "Q->>O" => want.extend(keys.split(' ').map(|k| (format!("out:{arrow}{}", k.to_uppercase()), t))),
            _ => {}
        }
    }
    let cfg = generate(&LayoutDef::parse(NICOLA_J).unwrap(), &nicola_on()).unwrap();
    assert_eq!(simulate_raw(&cfg, &input.join(" ")), want, "{input:?}");
}

/// A key treated as Ctrl (e.g. CapsLock with xkb ctrl:nocaps). When given, character keys pass through while it is held.
#[test]
fn key_as_ctrl_bypasses_nicola() {
    let layout = LayoutDef::parse(NICOLA_J).unwrap();
    let base = nicola_on();
    let input = "d:CAPSLOCK@0 d:A@100 u:A@150 u:CAPSLOCK@200";

    // Default (none given): CapsLock is not a modifier, so A gives う (u).
    let cfg = generate(&layout, &base).unwrap();
    assert_eq!(presses(&cfg, input), ["CapsLock", "U"]);

    // Given: a plain A comes out (xkb makes it Ctrl+A).
    let opt = Options { keys_as_ctrl: vec!["KEY_CAPSLOCK".into()], ..base };
    let cfg = generate(&layout, &opt).unwrap();
    assert_eq!(presses(&cfg, input), ["CapsLock", "A"]);
}

/// Ctrl+thumb (e.g. fcitx5's Ctrl+Muhenkan) delivers the thumb key while Ctrl is held, whatever the release order and timing.
#[test]
fn ctrl_thumb_combo_survives_release_order() {
    let layout = LayoutDef::parse(NICOLA_J).unwrap();
    let opt = Options { keys_as_ctrl: vec!["KEY_CAPSLOCK".into()], ..nicola_on() };
    let cfg = generate(&layout, &opt).unwrap();
    for input in [
        "d:CAPSLOCK@0 d:MUHENKAN@100 u:MUHENKAN@140 u:CAPSLOCK@150",
        "d:CAPSLOCK@0 d:MUHENKAN@100 u:MUHENKAN@140 u:CAPSLOCK@141",
        "d:CAPSLOCK@0 d:MUHENKAN@100 u:CAPSLOCK@135 u:MUHENKAN@140",
        "d:CAPSLOCK@0 d:MUHENKAN@100 u:CAPSLOCK@190 u:MUHENKAN@200",
        "d:LEFTCTRL@0 d:MUHENKAN@100 u:LEFTCTRL@120 u:MUHENKAN@121",
    ] {
        let ev: Vec<String> = simulate_raw(&cfg, input).into_iter().map(|(e, _)| e).collect();
        let ctrl_up = ev.iter().position(|e| e == "out:↑CapsLock" || e == "out:↑LCtrl");
        let mhnk_down = ev.iter().position(|e| e == "out:↓K0xAF");
        assert!(matches!((mhnk_down, ctrl_up), (Some(m), Some(c)) if m < c), "{input}: {ev:?}");
    }
}

/// Expected behavior of the thumb-prefix mode (ADR-18), checked with Unicode (judgment) and romaji (output).
#[test]
fn prefix_mode_cases() {
    let layout = LayoutDef::parse(NICOLA_J).unwrap();
    let uni = generate(&layout, &Options { encoder: Encoder::Unicode, ..nicola_on() }).unwrap();
    let romaji = generate(&layout, &nicola_on()).unwrap();
    let decoder = Decoder::new(&layout);
    let mut failures = Vec::new();
    for case in load(KANATA_PREFIX_CASES) {
        let want = parse_expect(&case.expect);
        // Output times lag by a few ms because of tick processing, so allow ±3 ms.
        if let Err(e) = compare_within(&want, &simulate(&uni, &case.input), 3) {
            failures.push(format!("[unicode] {} {}\n  {e}", case.id, case.desc));
        }
        let want: Vec<String> = want.iter().map(|t| canonical(&t.text)).collect();
        let got = simulate_decoded(&romaji, &case.input, &decoder);
        if got != want {
            failures.push(format!(
                "[romaji] {} {}\n  expected: {}\n  actual:   {}",
                case.id,
                case.desc,
                want.join(" "),
                got.join(" ")
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Thumb-prefix mode with the layout overlays (practical, lag-tolerant, and symbols on character keys).
#[test]
fn prefix_mode_with_overlays() {
    let mut layout = LayoutDef::parse(NICOLA_J).unwrap();
    layout.apply_overlay(include_str!("../../../layouts/overlays/practical.toml")).unwrap();
    layout.apply_overlay(include_str!("../../../layouts/overlays/lag-tolerant.toml")).unwrap();
    layout.apply_overlay(common::SYMBOLS_OVERLAY).unwrap();
    let cfg = generate(&layout, &nicola_on()).unwrap();
    let decoder = Decoder::new(&layout);
    let cases = [
        // かくにん: even if the thumb of に (U + right thumb) is still down when ; is pressed, it gives ん
        (
            "d:W@0 u:W@40 d:I@80 u:I@120 d:HENKAN@150 d:U@170 u:U@200 d:SEMICOLON@220 u:HENKAN@235 u:SEMICOLON@260",
            "か く に ん",
        ),
        // っ is the solo face of ,
        ("d:COMMA@0 u:COMMA@40", "っ"),
        // ぽ is the solo face of Z, ず is Z + right thumb
        ("d:Z@0 u:Z@40 d:HENKAN@80 d:Z@100 u:Z@130 u:HENKAN@150", "ぽ ず"),
        // Symbols: D11 solo →, D01 + right thumb …, D11 + right thumb 『
        (
            "d:LEFTBRACE@0 u:LEFTBRACE@40 d:HENKAN@80 d:Q@100 u:Q@130 d:LEFTBRACE@160 u:LEFTBRACE@190 u:HENKAN@220",
            "→ … 『",
        ),
        // Keys not in the overlays keep the standard layout
        ("d:HENKAN@0 d:W@20 u:W@50 u:HENKAN@80", "が"),
    ];
    for (input, want) in cases {
        let got = simulate_decoded(&cfg, input, &decoder).join(" ");
        assert_eq!(got, want, "{input}");
    }
    // D12 solo (dakuten disabled) gives the bracket key itself.
    let raw: Vec<String> = simulate_raw(&cfg, "d:RIGHTBRACE@0 u:RIGHTBRACE@40").into_iter().map(|(e, _)| e).collect();
    assert_eq!(raw, ["out:↓RBracket", "out:↑RBracket"]);
    // にー: even if the thumb of に is still down when - is pressed, - (E11, out of the layout) gives the key itself
    // (ー in the IME), and the thumb key is not sent.
    assert_eq!(presses(&cfg, "d:HENKAN@0 d:U@20 u:U@50 d:MINUS@70 u:HENKAN@90 u:MINUS@110"), ["N", "I", "Minus"]);
    // 『 is z + the JIS "[" key (kanata `]`), as Mozc spells it z[.
    assert_eq!(presses(&cfg, "d:HENKAN@0 d:LEFTBRACE@20 u:LEFTBRACE@50 u:HENKAN@80"), ["Z", "RBracket"]);
}

/// The name of a kanata key as the simulation shows its press (`Z`, `LShift`, `Comma`).
fn pressed_name(kanata_key: &str) -> String {
    let code = str_to_oscode(kanata_key).unwrap_or_else(|| panic!("no kanata key {kanata_key}"));
    KeyEvent { code, value: KeyValue::Press }.to_string().trim_start_matches('↓').to_string()
}

/// A face written in ASCII is romaji to type: every printable ASCII character, alone and after z, on every
/// face. kanata takes its name (some characters of a name are made full-width), and the key sends the keys
/// of the romaji on a JIS keyboard, those that need Shift after it, and leaves none pressed.
#[test]
fn romaji_faces_send_their_keys() {
    let ascii: Vec<String> = (0x21u8..0x7f).map(|b| (b as char).to_string()).collect();
    let after_z = ascii.iter().filter(|c| !matches!(c.as_str(), "z" | "Z")).map(|c| format!("z{c}"));
    let faces: Vec<String> = ascii.iter().cloned().chain(after_z).collect();
    let layout = LayoutDef::parse(NICOLA_J).unwrap();
    let opt = nicola_on();
    // Three faces per key of the standard layout, as many layouts as it takes.
    let mut faces = faces.iter();
    let mut checked = 0;
    loop {
        let (mut overlay, mut input, mut want, mut t) = (String::new(), Vec::new(), Vec::new(), 0);
        for k in &layout.keys {
            let key = k.name.trim_start_matches("KEY_");
            overlay += &format!("[keys.{}]\n", k.name);
            for (face, thumb) in [("solo", None), ("left", Some("MUHENKAN")), ("right", Some("HENKAN"))] {
                let Some(romaji) = faces.next() else { break };
                overlay += &format!("{face} = \"{}\"\n", romaji.replace('\\', "\\\\").replace('"', "\\\""));
                match thumb {
                    None => input.push(format!("d:{key}@{t} u:{key}@{}", t + 30)),
                    Some(th) => {
                        input.push(format!("d:{th}@{t} d:{key}@{} u:{key}@{} u:{th}@{}", t + 20, t + 50, t + 80))
                    }
                }
                t += 100;
                want.extend(typed_keys(romaji).unwrap().into_iter().map(pressed_name));
                checked += 1;
            }
        }
        if input.is_empty() {
            break;
        }
        let mut layout = LayoutDef::parse(NICOLA_J).unwrap();
        layout.apply_overlay(&overlay).unwrap();
        let cfg = generate(&layout, &opt).unwrap();
        let input = input.join(" ");
        let raw = simulate_raw(&cfg, &input);
        assert!(stuck_keys(&raw).is_empty(), "{input}\n  output keys left pressed: {:?}", stuck_keys(&raw));
        assert_eq!(pressed(&raw), want, "{overlay}");
    }
    assert_eq!(checked, 94 + 92);
}

/// Shift pressed by a romaji face (`z*`) stays pressed until the next tick. A character pressed at nearly the
/// same time (two keys in one keyboard report) waits for it, so that its keys are not typed with Shift.
#[test]
fn character_after_a_shifted_romaji_face_waits_for_shift() {
    let mut layout = LayoutDef::parse(NICOLA_J).unwrap();
    layout.apply_overlay("[keys.KEY_Q]\nsolo = \"z*\"\n[keys.KEY_E]\nsolo = \"*\"").unwrap();
    let cfg = generate(&layout, &nicola_on()).unwrap();
    // Q (z*) or E (*), then W (か) or Q again, 0 to 3 ms later.
    for (first, then) in [("Q", "W"), ("E", "W"), ("Q", "E"), ("E", "Q")] {
        for gap in 0..4 {
            let input = format!("d:{first}@0 d:{then}@{gap} u:{first}@40 u:{then}@60");
            let raw = simulate_raw(&cfg, &input);
            let with_shift = pressed_with_shift(&raw);
            assert!(with_shift.iter().all(|k| k == "Quote"), "{input}: {with_shift:?} pressed with Shift\n  {raw:?}");
            assert!(stuck_keys(&raw).is_empty(), "{input}: left pressed {:?}", stuck_keys(&raw));
        }
    }
    // Without a character that presses Shift (the standard layout without the number row: ？（）), the
    // sending does not wait for Shift.
    let mut plain = LayoutDef::parse(NICOLA_J).unwrap();
    plain.keys.retain(|k| !k.pos.starts_with('E'));
    let plain = generate(&plain, &Options::default()).unwrap();
    assert!(cfg.contains("(switch ($k1 $k2 lsft)") && plain.contains("(switch ($k1 $k2)"));
}

/// The number row (E) of the standard layout: either thumb gives the character of the J-type figure (？
/// and the parentheses are typed with Shift), its stand-ins for ［］ (｛｝) or the additions
/// common to other implementations (〜『』), and the key alone types its digit (or -).
#[test]
fn number_row_thumb_faces() {
    let layout = LayoutDef::parse(NICOLA_J).unwrap();
    let cfg = generate(&layout, &nicola_on()).unwrap();
    let rows: Vec<_> = layout.keys.iter().filter(|k| k.pos.starts_with('E')).collect();
    assert_eq!(
        rows.iter().map(|k| k.pos.as_str()).collect::<Vec<_>>(),
        ["E01", "E02", "E03", "E04", "E05", "E06", "E07", "E08", "E09", "E10", "E11"]
    );
    for k in rows {
        let key = k.name.trim_start_matches("KEY_");
        let face = k.faces.left.as_deref().unwrap();
        assert_eq!(k.faces.right.as_deref(), Some(face), "{}: the same character with either thumb", k.pos);
        let want: Vec<String> = typed_keys(romaji(face).unwrap()).unwrap().into_iter().map(pressed_name).collect();
        for th in ["MUHENKAN", "HENKAN"] {
            let input = format!("d:{th}@0 d:{key}@20 u:{key}@50 u:{th}@80");
            let raw = simulate_raw(&cfg, &input);
            assert!(stuck_keys(&raw).is_empty(), "{input}: {:?}", stuck_keys(&raw));
            assert_eq!(pressed(&raw), want, "{input}");
        }
        let alone = pressed_name(nicola_kbdgen::kanata_key(&k.name).unwrap());
        assert_eq!(presses(&cfg, &format!("d:{key}@0 u:{key}@30")), [alone], "{key} alone");
    }
    // A character pressed right after ？ (in the same keyboard report or nearly) waits for its Shift.
    for gap in 0..4 {
        let input = format!("d:HENKAN@0 d:1@20 d:K@{} u:1@50 u:K@60 u:HENKAN@80", 20 + gap);
        let raw = simulate_raw(&cfg, &input);
        let with_shift = pressed_with_shift(&raw);
        assert!(with_shift.iter().all(|k| k == "Slash"), "{input}: {with_shift:?} with Shift\n  {raw:?}");
    }
}

/// ・ (B10) and ／ (E02 with a thumb) are both sent as `/`, so the romaji output decodes to one token for
/// both (the IME's symbol style decides which appears). The random typing does not use these keys.
#[test]
fn characters_with_the_same_keys_decode_alike() {
    let layout = LayoutDef::parse(NICOLA_J).unwrap();
    let uni = generate(&layout, &Options { encoder: Encoder::Unicode, ..nicola_on() }).unwrap();
    let cfg = generate(&layout, &nicola_on()).unwrap();
    let input = "d:SLASH@0 u:SLASH@30 d:OR@100 d:2@130 u:2@160 u:OR@200 d:COMMA@300 u:COMMA@330";
    let want: Vec<String> = simulate(&uni, input).into_iter().map(|(s, _)| s).collect();
    assert_eq!(want, ["・", "／", "ね"]);
    let got = simulate_decoded(&cfg, input, &Decoder::new(&layout));
    assert_eq!(got, want.iter().map(|s| canonical(s)).collect::<Vec<_>>());
}

/// The combination the user service runs with IME sync (thumb-prefix, no toggle keys, CapsLock treated as
/// Ctrl), on the `nicola` layer that nicola-imesync switches to.
fn resident_options() -> Options {
    Options { toggle_key: None, keys_as_ctrl: vec!["KEY_CAPSLOCK".into()], ..nicola_on() }
}

#[test]
fn resident_service_configuration() {
    let layout = LayoutDef::parse(NICOLA_J).unwrap();
    let cfg = generate(&layout, &resident_options()).unwrap();
    let cases: &[(&str, &[&str])] = &[
        ("d:W@0 u:W@40", &["K", "A"]),
        ("d:HENKAN@0 d:W@20 u:W@50 u:HENKAN@80", &["G", "A"]),
        // CapsLock (Ctrl for xkb) + A passes through: Ctrl+A for the desktop.
        ("d:CAPSLOCK@0 d:A@50 u:A@80 u:CAPSLOCK@120", &["CapsLock", "A"]),
        // Ctrl+Muhenkan (the IME toggle) reaches the IME, with Ctrl or CapsLock.
        ("d:LEFTCTRL@0 d:MUHENKAN@50 u:MUHENKAN@80 u:LEFTCTRL@120", &["LCtrl", "K0xAF"]),
        ("d:CAPSLOCK@0 d:MUHENKAN@50 u:MUHENKAN@80 u:CAPSLOCK@120", &["CapsLock", "K0xAF"]),
        // Hankaku/Zenkaku goes to the IME and does not switch the layer (nicola-imesync does).
        ("d:GRAVE@0 u:GRAVE@30 d:W@60 u:W@90", &["Grave", "K", "A"]),
        // Solo thumb keystroke.
        ("d:HENKAN@0 u:HENKAN@50", &["K0xB0"]),
    ];
    for (input, want) in cases {
        assert_eq!(presses(&cfg, input), *want, "{input}");
    }
}

/// Space as the left thumb key (`NICOLA_THUMB_KEYS=KEY_SPACE:KEY_HENKAN`), on the user service's configuration. It
/// gives the left faces while held, and is sent on release if no character key was pressed meanwhile, once however
/// long it was held (no repeat), as Muhenkan is; with a modifier it passes through (Ctrl+Space and the like).
/// Muhenkan is then a plain key.
#[test]
fn space_as_a_thumb_key() {
    let layout = LayoutDef::parse(NICOLA_J).unwrap();
    let cfg = generate(&layout, &space_thumb_options()).unwrap();
    let cases: &[(&str, &[&str])] = &[
        ("d:SPACE@0 d:W@20 u:W@50 u:SPACE@80", &["E"]),
        ("d:SPACE@0 d:W@200 u:W@230 d:E@250 u:E@280 u:SPACE@300", &["E", "R", "I"]),
        ("d:SPACE@0 u:SPACE@50", &["Space"]),
        ("d:SPACE@0 u:SPACE@600", &["Space"]),
        // A character key first: no chord.
        ("d:W@0 d:SPACE@20 u:W@50 u:SPACE@80", &["K", "A", "Space"]),
        ("d:LEFTCTRL@0 d:SPACE@20 u:SPACE@50 u:LEFTCTRL@80", &["LCtrl", "Space"]),
        ("d:LEFTSHIFT@0 d:SPACE@20 u:SPACE@50 u:LEFTSHIFT@80", &["LShift", "Space"]),
        ("d:HENKAN@0 d:W@20 u:W@50 u:HENKAN@80", &["G", "A"]),
        ("d:MUHENKAN@0 u:MUHENKAN@50", &["K0xAF"]),
        // NICOLA off: Space as it is.
        ("layer:base@0 d:SPACE@20 u:SPACE@50", &["Space"]),
    ];
    for (input, want) in cases {
        assert_eq!(presses(&cfg, input), *want, "{input}");
    }
}

fn space_thumb_options() -> Options {
    Options { thumb_left: "KEY_SPACE".into(), ..resident_options() }
}

/// Random typing with modifiers, CapsLock, Enter and Hankaku/Zenkaku mixed in leaves no output key
/// pressed (docs/design.md §2, stability), in the standard configurations, the user service's one, and with Space as
/// a thumb key.
#[test]
fn random_typing_with_modifiers_leaves_no_key_pressed() {
    let layout = LayoutDef::parse(NICOLA_J).unwrap();
    let configs = [("prefix", nicola_on()), ("resident", resident_options()), ("space-thumb", space_thumb_options())];
    let keys = [
        "W",
        "E",
        "R",
        "A",
        "S",
        "K",
        "L",
        "I",
        "O",
        "SEMICOLON",
        // ？ and （ of the number row, typed with Shift.
        "1",
        "8",
        "OL",
        "OR",
        "OL",
        "OR",
        "SPACE",
        "LEFTSHIFT",
        "LEFTALT",
        "CAPSLOCK",
        "GRAVE",
    ];
    let cases = random_cases() / 5;
    for (name, opt) in configs {
        let cfg = generate(&layout, &opt).unwrap();
        let mut rnd = rng(0x1234_5678_9ABC_DEF1);
        for i in 0..cases {
            let input = random_input(&mut rnd, &keys);
            let stuck = stuck_keys(&simulate_raw(&cfg, &input));
            assert!(stuck.is_empty(), "{name} case {i}: {input}\n  output keys left pressed: {stuck:?}");
        }
    }
}
