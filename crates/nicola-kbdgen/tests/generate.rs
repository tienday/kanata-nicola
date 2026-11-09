//! Generator tests: the romaji table covers every symbol in the layout, kanata can load every
//! generated form, the prebuilt files in `kbd/` are up to date, the docs name only those files and the versions and
//! options of the build, and the examples of editing them by hand and of an extra kanata file load.

mod common;
mod repository;

use std::fs;

use nicola_kbdgen::encoder::{romaji, UNSENDABLE};
use nicola_kbdgen::{generate, generate_parts, Encoder, Options, ENV_FILE};
use nicola_layout::{LayoutDef, NICOLA_J};
use repository::{documents, repo};

const PRACTICAL: &str = include_str!("../../../layouts/overlays/practical.toml");
const LAG_TOLERANT: &str = include_str!("../../../layouts/overlays/lag-tolerant.toml");

fn symbols() -> Vec<String> {
    let layout = LayoutDef::parse(NICOLA_J).unwrap();
    let mut v: Vec<String> =
        layout.keys.iter().flat_map(|k| [&k.faces.solo, &k.faces.left, &k.faces.right]).flatten().cloned().collect();
    v.sort();
    v.dedup();
    v
}

#[test]
fn every_layout_symbol_has_an_encoding() {
    for s in symbols() {
        // Some cannot be sent in romaji (OQ-10).
        if !UNSENDABLE.contains(&s.as_str()) {
            assert!(romaji(&s).is_some(), "romaji: no keys for {s}");
        }
    }
}

fn parses(cfg: &str, what: &str) {
    if let Err(e) = common::load(cfg) {
        panic!("{what}: kanata rejected config: {e}\n{cfg}");
    }
}

#[test]
fn generated_configs_parse_in_kanata() {
    let mut overlaid = LayoutDef::parse(NICOLA_J).unwrap();
    overlaid.apply_overlay(PRACTICAL).unwrap();
    overlaid.apply_overlay(LAG_TOLERANT).unwrap();
    overlaid.apply_overlay(common::SYMBOLS_OVERLAY).unwrap();
    let layouts = [LayoutDef::parse(NICOLA_J).unwrap(), overlaid];
    for layout in &layouts {
        for encoder in [Encoder::Romaji, Encoder::Unicode] {
            for start_enabled in [false, true] {
                let opt = Options { encoder, start_enabled, ..Options::default() };
                parses(&generate(layout, &opt).unwrap(), &format!("{encoder:?}"));
            }
        }
        let opts = [
            // IME sync: no toggle keys.
            Options { toggle_key: None, ..Options::default() },
            // CapsLock treated as Ctrl.
            Options { keys_as_ctrl: vec!["KEY_CAPSLOCK".into()], ..Options::default() },
        ];
        for opt in &opts {
            parses(&generate(layout, opt).unwrap(), &format!("{opt:?}"));
        }
    }
}

/// Romaji ( and ) are named （ and ） in the `.kbd`, like the characters of the number row. They send the same
/// keys (Shift and 8 or 9), so each name is defined once, and kanata takes the configuration (it rejects a name
/// defined twice).
#[test]
fn romaji_parentheses_share_the_definitions_of_the_full_width_ones() {
    let mut layout = LayoutDef::parse(NICOLA_J).unwrap();
    layout.apply_overlay("[keys.KEY_Q]\nright = \"(\"\n[keys.KEY_P]\nright = \")\"").unwrap();
    let cfg = generate(&layout, &Options::default()).unwrap();
    parses(&cfg, "romaji ( and ) with the number row's （ and ）");
    for (cell, definition) in
        [("q (t! shifted q （)", "(t! keys2 （ lsft 8)"), ("p (t! shifted p ）)", "(t! keys2 ） lsft 9)")]
    {
        assert!(cfg.contains(cell), "no {cell}");
        assert_eq!(cfg.matches(definition).count(), 1, "{definition}");
    }
}

/// A key that an overlay adds without its position is shown by its name alone, in the file and in messages.
#[test]
fn keys_added_without_a_position_are_named_alone() {
    let gen = |overlay: &str| {
        let mut layout = LayoutDef::parse(NICOLA_J).unwrap();
        layout.apply_overlay(overlay).unwrap();
        generate(&layout, &Options::default()).map_err(|e| e.to_string())
    };
    let cfg = gen("[keys.KEY_APOSTROPHE]\nsolo = \"→\"").unwrap();
    parses(&cfg, "a key without a position");
    assert!(cfg.contains("\n;; ': → / 未定義 / 未定義\n"), "{cfg}");
    let err = gen("[keys.KEY_APOSTROPHE]\nsolo = \"［\"").unwrap_err();
    assert!(err.starts_with("KEY_APOSTROPHE solo = \"［\": "), "{err}");
}

/// Input that kanata would reject, or that would break the `.kbd`, is an error of the generator itself.
#[test]
fn generator_rejects_faces_on_special_keys_and_bad_names() {
    let gen = |overlay: &str, opt: &Options| {
        let mut layout = LayoutDef::parse(NICOLA_J).unwrap();
        layout.apply_overlay(overlay).unwrap();
        generate(&layout, opt).map_err(|e| e.to_string())
    };
    let default = Options::default();
    let err = gen("[keys.KEY_HENKAN]\nsolo = \"あ\"", &default).unwrap_err();
    assert!(err.contains("KEY_HENKAN is a thumb key"), "{err}");
    assert!(gen("[keys.KEY_GRAVE]\nsolo = \"あ\"", &default).unwrap_err().contains("toggle key"));
    assert!(gen("[keys.KEY_LEFTSHIFT]\nsolo = \"あ\"", &default).unwrap_err().contains("modifier key"));
    let caps_ctrl = Options { keys_as_ctrl: vec!["KEY_CAPSLOCK".into()], ..Options::default() };
    // A key given as an option: the message names the option, as the mistake may be there.
    let err = gen("[keys.KEY_CAPSLOCK]\nsolo = \"あ\"", &caps_ctrl).unwrap_err();
    assert!(err.contains("treated as Ctrl") && err.contains("--key-as-ctrl"), "{err}");

    // A face that cannot be sent as written: the message names the key and the face.
    let err = gen("[keys.KEY_LEFTBRACE]\nright = \"［\"", &default).unwrap_err();
    assert!(err.starts_with("KEY_LEFTBRACE (D11) right = \"［\": \"［\" (U+FF3B) is not in the romaji table"), "{err}");
    let err = gen("[keys.KEY_Q]\nsolo = \"*z\"", &default).unwrap_err();
    assert!(err.starts_with("KEY_Q (D01) solo = \"*z\": ") && err.contains("must come last"), "{err}");

    let quoted = Options { include_device_names: vec!["My \"Board\"".into()], ..Options::default() };
    assert!(gen("", &quoted).unwrap_err().contains("double quote"));
    assert!(gen("[meta]\nname = \"a\\nb\"", &default).unwrap_err().contains("control character"));
}

/// A key has one role among the thumb keys, the toggle key and the keys treated as Ctrl. kanata rejects the
/// first overlaps; a thumb or the toggle key treated as Ctrl would let the character keys through, and so would a
/// modifier as a thumb key.
#[test]
fn special_keys_have_one_role() {
    let layout = LayoutDef::parse(NICOLA_J).unwrap();
    let d = Options::default;
    let keys = |k: &[&str]| k.iter().map(|k| k.to_string()).collect::<Vec<_>>();
    let rejected = [
        (
            Options { thumb_right: "KEY_GRAVE".into(), ..d() },
            "KEY_GRAVE is the right thumb key (--thumbs) and the toggle key (--toggle)",
        ),
        (
            Options { thumb_right: "KEY_MUHENKAN".into(), ..d() },
            "KEY_MUHENKAN is the left thumb key (--thumbs) and the right thumb key",
        ),
        (
            Options { toggle_key: Some("KEY_HENKAN".into()), ..d() },
            "KEY_HENKAN is the right thumb key (--thumbs) and the toggle key",
        ),
        (
            Options { keys_as_ctrl: keys(&["KEY_CAPSLOCK", "KEY_MUHENKAN"]), ..d() },
            "KEY_MUHENKAN is the left thumb key (--thumbs) and treated as Ctrl (--key-as-ctrl)",
        ),
        (
            Options { keys_as_ctrl: keys(&["KEY_GRAVE"]), ..d() },
            "KEY_GRAVE is the toggle key (--toggle) and treated as Ctrl",
        ),
        // While a modifier is held, the character keys pass through: a modifier thumb would give no faces.
        (
            Options { thumb_left: "KEY_LEFTSHIFT".into(), ..d() },
            "KEY_LEFTSHIFT is a modifier key, so it cannot be the left thumb key (--thumbs)",
        ),
        (
            Options { thumb_right: "KEY_RIGHTALT".into(), ..d() },
            "KEY_RIGHTALT is a modifier key, so it cannot be the right thumb key (--thumbs)",
        ),
    ];
    for (opt, want) in &rejected {
        let err = generate(&layout, opt).unwrap_err().to_string();
        assert!(err.starts_with(want), "{err}");
    }
    // Allowed, and read by kanata: a key given twice as Ctrl, Ctrl itself as Ctrl, Space as a thumb key, and the
    // Hankaku/Zenkaku key as Ctrl when there is no toggle key (IME sync).
    let allowed = [
        Options { keys_as_ctrl: keys(&["KEY_CAPSLOCK", "KEY_CAPSLOCK", "KEY_LEFTCTRL"]), ..d() },
        Options { thumb_left: "KEY_SPACE".into(), ..d() },
        Options { toggle_key: None, keys_as_ctrl: keys(&["KEY_GRAVE"]), ..d() },
    ];
    for opt in &allowed {
        parses(&generate(&layout, opt).unwrap(), &format!("{opt:?}"));
    }
}

/// The user's own kanata file is read with include at the end (section 7, also in the list of the header);
/// the path is quoted (it may have spaces), and one that would break the string is an error.
#[test]
fn extra_kbd_is_included_at_the_end() {
    let layout = LayoutDef::parse(NICOLA_J).unwrap();
    let include = "(include \"/home/me/my keys.kbd\")";
    let opt = Options { extra_kbd: Some("/home/me/my keys.kbd".into()), ..Options::default() };
    let cfg = generate(&layout, &opt).unwrap();
    assert!(
        cfg.contains(";;   7. 追加の kanata の設定ファイル")
            && cfg.contains(";; ==== 7. 追加の kanata の設定ファイル ===="),
        "{cfg}"
    );
    assert!(cfg.trim_end().ends_with(include), "{cfg}");
    let plain = generate(&layout, &Options::default()).unwrap();
    assert!(!plain.contains("7. 追加の kanata の設定ファイル") && !plain.contains("(include"));
    let quoted = Options { extra_kbd: Some("/a\"b.kbd".into()), ..Options::default() };
    assert!(generate(&layout, &quoted).unwrap_err().to_string().contains("double quote"));
}

/// The example of the user's own kanata file in docs/settings.md §3.2, read with include at the end, gives a config
/// kanata accepts.
#[test]
fn docs_extra_kbd_example_loads() {
    let doc = fs::read_to_string(repo().join("docs/settings.md")).unwrap();
    let section = doc.split("\n### 3.2 ").nth(1).expect("docs/settings.md has §3.2");
    let example = section
        .split("```lisp\n")
        .nth(1)
        .and_then(|b| b.split("```").next())
        .expect("a ```lisp block in docs/settings.md §3.2");
    let layout = LayoutDef::parse(NICOLA_J).unwrap();
    let include = "(include \"/home/me/user-extra.kbd\")";
    let opt = Options { extra_kbd: Some("/home/me/user-extra.kbd".into()), ..Options::default() };
    let cfg = generate(&layout, &opt).unwrap();
    assert!(cfg.contains(include), "{cfg}");
    // With the file in place of the include (kanata's library does not read include from a string).
    parses(&cfg.replace(include, example), "with the extra file of docs/settings.md §3.2");
}

/// The split form is the single form with the environment section moved to its own file.
#[test]
fn split_form_is_the_single_form() {
    let layout = LayoutDef::parse(NICOLA_J).unwrap();
    let parts = generate_parts(&layout, &Options::default()).unwrap();
    let main = parts.main_file();
    let include = format!("(include {ENV_FILE})");
    assert!(main.contains(&include));
    assert!(parts.env_file().contains("(defcfg"));
    assert!(!main.contains("(defcfg"));
    // Inlining that file in place of the include gives a config kanata accepts.
    parses(&main.replace(&include, &parts.env_file()), "inlined split form");
}

/// `kbd/` holds the generated configuration for people who use kanata-nicola without Rust.
/// Regenerate with (--force also rewrites the environment file, `ENV_FILE`):
///   cargo run --release -p nicola-kbdgen -- --split --force -o kbd/nicola.kbd
///   cargo run --release -p nicola-kbdgen -- --overlay layouts/overlays/practical.toml --overlay layouts/overlays/lag-tolerant.toml --split --force -o kbd/nicola-lag-tolerant.kbd
#[test]
fn prebuilt_files_are_up_to_date() {
    let standard = LayoutDef::parse(NICOLA_J).unwrap();
    let mut lag_tolerant = LayoutDef::parse(NICOLA_J).unwrap();
    lag_tolerant.apply_overlay(PRACTICAL).unwrap();
    lag_tolerant.apply_overlay(LAG_TOLERANT).unwrap();
    let opt = Options::default();
    let a = generate_parts(&standard, &opt).unwrap();
    let b = generate_parts(&lag_tolerant, &opt).unwrap();
    let hint = "kbd/ is out of date; regenerate it (see the comment of this test)";
    assert_eq!(a.main_file(), include_str!("../../../kbd/nicola.kbd"), "{hint}");
    assert_eq!(b.main_file(), include_str!("../../../kbd/nicola-lag-tolerant.kbd"), "{hint}");
    let env_file = fs::read_to_string(repo().join("kbd").join(ENV_FILE)).unwrap();
    assert_eq!(a.env_file(), env_file, "{hint}");
    assert_eq!(b.env_file(), a.env_file());
    // Nothing for one OS (kanata's options for one OS start with its name): Windows uses kbd/ as it is
    // (docs/windows.md).
    for file in [a.main_file(), b.main_file(), a.env_file()] {
        let settings: Vec<&str> = file.lines().filter(|l| !l.trim_start().starts_with(";;")).collect();
        for os in ["linux-", "windows-", "macos-"] {
            assert!(!settings.iter().any(|l| l.contains(os)), "{os}: {file}");
        }
    }
}

/// The list of sections in the header names the section headings of the file, in order.
#[test]
fn header_lists_the_sections() {
    let layout = LayoutDef::parse(NICOLA_J).unwrap();
    for opt in [Options::default(), Options { extra_kbd: Some("/x.kbd".into()), ..Options::default() }] {
        let cfg = generate(&layout, &opt).unwrap();
        // ";;   1. 環境固有設定      対象にするキーボード…": the name ends at two spaces.
        let listed: Vec<&str> = cfg
            .lines()
            .filter_map(|l| l.strip_prefix(";;   "))
            .filter(|l| l.starts_with(|c: char| c.is_ascii_digit()))
            .map(|l| l.split("  ").next().unwrap())
            .collect();
        let headings: Vec<&str> =
            cfg.lines().filter_map(|l| l.strip_prefix(";; ==== ")?.strip_suffix(" ====")).collect();
        assert_eq!(listed, headings, "{opt:?}");
        assert_eq!(headings.len(), if opt.extra_kbd.is_some() { 7 } else { 6 });
    }
}

/// The patched kanata (platform/linux/kanata/build.sh) is the version that the simulation tests run (the kanata
/// dev-dependency), so that the tests check the kanata that the services run.
#[test]
fn patched_kanata_is_the_tested_version() {
    let build = fs::read_to_string(repo().join("platform/linux/kanata/build.sh")).unwrap();
    let built = build.lines().find_map(|l| l.strip_prefix("version=")).expect("version= in build.sh");
    let cargo = fs::read_to_string(repo().join("crates/nicola-kbdgen/Cargo.toml")).unwrap();
    let tested = cargo
        .lines()
        .find_map(|l| l.strip_prefix("kanata = { version = \"=")?.split('"').next())
        .expect("the kanata dev-dependency in crates/nicola-kbdgen/Cargo.toml");
    assert_eq!(built, tested);
}

/// The documents name the versions of the build: the kanata that build.sh patches (docs/design.md §10.1,
/// platform/linux/kanata/README.md) and the Rust of Cargo.toml (README.md, docs/usage.md §1).
#[test]
fn docs_name_the_kanata_and_rust_versions() {
    let read = |file: &str| fs::read_to_string(repo().join(file)).unwrap();
    let build = read("platform/linux/kanata/build.sh");
    let kanata = build.lines().find_map(|l| l.strip_prefix("version=")).expect("version= in build.sh");
    let cargo = read("Cargo.toml");
    let rust = cargo
        .lines()
        .find_map(|l| l.strip_prefix("rust-version = \"")?.strip_suffix('"'))
        .expect("rust-version in Cargo.toml");
    for (doc, text) in [
        ("docs/design.md", format!("| {kanata}（kanata-nicola が使うバージョン） |")),
        ("docs/design.md", format!("kanata {kanata} には")),
        ("platform/linux/kanata/README.md", format!("| 基にするバージョン | kanata {kanata}（")),
        ("README.md", format!("Rust（{rust} 以降）")),
        ("docs/usage.md", format!("Rust {rust} 以降")),
    ] {
        assert!(read(doc).contains(&text), "{doc} does not have {text:?}");
    }
}

/// The options of the generator in docs/implementation.md §2.1 are those of its --help, and the defaults that the
/// table gives for the thumb keys and the toggle key are the generator's.
#[test]
fn docs_list_the_generator_options() {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_nicola-kbdgen")).arg("--help").output().unwrap();
    let help = String::from_utf8(out.stdout).unwrap();
    // An option starts a line after two spaces.
    let mut in_help: Vec<String> = help
        .lines()
        .filter_map(|l| l.strip_prefix("  -"))
        .map(|l| format!("-{}", l.split_whitespace().next().unwrap()))
        .collect();
    in_help.sort();

    let doc = fs::read_to_string(repo().join("docs/implementation.md")).unwrap();
    let section = doc.split("\n### 2.1 ").nth(1).expect("docs/implementation.md has §2.1");
    let section = section.split("\n### ").next().unwrap();
    // "| `--thumbs <L> <R>` | … | `KEY_MUHENKAN KEY_HENKAN` |"; "| `-h`、`--help` | …"
    let rows: Vec<Vec<&str>> =
        section.lines().filter(|l| l.starts_with("| `")).map(|l| l.trim_matches('|').split(" | ").collect()).collect();
    let options = |row: &[&str]| -> Vec<String> {
        row[0].split('`').skip(1).step_by(2).map(|o| o.split(' ').next().unwrap().to_string()).collect()
    };
    let mut in_doc: Vec<String> =
        rows.iter().flat_map(|r| options(r)).filter(|o| !matches!(o.as_str(), "-h" | "--help")).collect();
    in_doc.sort();
    assert_eq!(in_doc, in_help, "the options of docs/implementation.md §2.1");

    let default = |option: &str| rows.iter().find(|r| options(r)[0] == option).map(|r| r[2].trim()).unwrap();
    let d = Options::default();
    let thumbs = default("--thumbs");
    assert!(thumbs.contains(&format!("`{} {}`", d.thumb_left, d.thumb_right)), "--thumbs: {thumbs}");
    let toggle = default("--toggle");
    assert!(toggle.contains(&format!("`{}`", d.toggle_key.as_deref().unwrap())), "--toggle: {toggle}");
}

/// The documents (every `.md` file) name only the files of `kbd/` as `.kbd` files, so that a renamed file of `kbd/`
/// (such as `ENV_FILE`) leaves no old name in them. The code takes the names from the generator.
#[test]
fn docs_name_the_kbd_files_that_exist() {
    let mut known: Vec<String> =
        fs::read_dir(repo().join("kbd")).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
    // The example name of the user's own file (NICOLA_EXTRA_KBD in platform/linux/config.env.example).
    known.push("user-extra.kbd".into());
    let mut unknown = vec![];
    for doc in documents() {
        for (i, line) in fs::read_to_string(repo().join(&doc)).unwrap().lines().enumerate() {
            // A name is the run of name characters before ".kbd" (the part after the last "/" of a path).
            for (at, _) in line.match_indices(".kbd") {
                let after = line[at + 4..].chars().next();
                let len =
                    line[..at].chars().rev().take_while(|c| c.is_ascii_alphanumeric() || "-_".contains(*c)).count();
                let start = at - len; // ASCII, one byte each
                let name = &line[start..at + 4];
                if name != ".kbd"
                    && !after.is_some_and(|c| c.is_ascii_alphanumeric())
                    && !known.iter().any(|k| k == name)
                {
                    unknown.push(format!("{doc}:{}: {name}", i + 1));
                }
            }
        }
    }
    assert!(unknown.is_empty(), "no such file in kbd/ (renamed?):\n{}", unknown.join("\n"));
}

/// The examples of editing by hand in docs/kanata-kbd.md §2.2 (another character in a cell, the definitions of a
/// new character and of a romaji face), applied to `kbd/nicola.kbd`, give a config kanata accepts, and the cell
/// gives the new character.
#[test]
fn docs_editing_examples_of_the_kbd_guide_load() {
    let doc = fs::read_to_string(repo().join("docs/kanata-kbd.md")).unwrap();
    let section = doc.split("\n### 2.2 ").nth(1).expect("docs/kanata-kbd.md has §2.2");
    let section = section.split("\n### ").next().unwrap();
    let mut cell = None;
    let mut definitions = vec![];
    let mut in_code = false;
    // The code blocks are in the steps, indented.
    for line in section.lines().map(str::trim_start) {
        if line.starts_with("```") {
            in_code = !in_code;
        } else if in_code {
            // The cell as "before → after", and the definitions of the new character.
            if let Some((before, after)) = line.split_once('→') {
                cell = Some((before.trim(), after.trim()));
            } else if line.starts_with("(def") {
                definitions.push(line.to_string());
            }
        } else {
            // The character with Shift is in the text, as the two templates to place.
            for span in line.split('`').skip(1).step_by(2) {
                if span.starts_with("(t! keys") {
                    definitions.push(format!("(defvirtualkeys {span})"));
                } else if span.starts_with("(t! send") {
                    definitions.push(format!("(defalias {span})"));
                }
            }
        }
    }
    let (before, after) = cell.expect("a cell as before → after");
    assert_eq!(definitions.len(), 4, "{definitions:?}");
    let main = include_str!("../../../kbd/nicola.kbd");
    assert_eq!(main.matches(before).count(), 1, "{before}");
    let env_file = fs::read_to_string(repo().join("kbd").join(ENV_FILE)).unwrap();
    // The include inlined (kanata's library does not read include from a string); the definitions at the end, in
    // section 6.
    let cfg = format!(
        "{}\n{}\n",
        main.replace(before, after).replace(&format!("(include {ENV_FILE})"), &env_file),
        definitions.join("\n")
    );
    parses(&cfg, "kbd/nicola.kbd with the edits of docs/kanata-kbd.md §2.2");
    assert_eq!(common::presses(&cfg, "layer:nicola@0 d:COMMA@10 u:COMMA@40"), ["X", "T", "U"], "{after}");
}
